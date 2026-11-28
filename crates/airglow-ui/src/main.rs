#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Airglow 2.0 (Iced 0.14 + pure-Rust hardware core + Tauri tray-icon).

mod brand;
mod canvas_ambient;
mod canvas_colorwheel;
mod canvas_curves;
mod canvas_stage;
mod canvas_widgets;
mod theme;
mod tray;
mod views;
mod views_lighting;
mod views_palette;
mod views_system;

use std::time::{Duration, Instant};

use airglow_core::automations::{ALL_DAYS_MASK, Action, AutomationRule, Trigger};
use airglow_core::config::{
    ACCENT_SWATCHES, CurvePoint, DesignId, FanChannelMode, RigStyle, TempSource, TempUnit,
    ZoneConfig, ZoneOverride, format_hex_rgb, parse_hex_rgb,
};
use airglow_core::effects::{EffectId, HARMONIES, Layer, LayerKind, LayerStack, SceneParams};
use airglow_core::engine::{EngineCommand, EngineHandle, EngineSnapshot};
use airglow_core::sensors;
use iced::widget::{
    Space, button, column, container, image, operation, row, scrollable, stack, text,
};
use iced::{
    Alignment, Element, Event, Length, Size, Subscription, Task, Theme, keyboard, time, window,
};

use crate::canvas_stage::StageMessage;
use crate::canvas_widgets::{BrandMarkCanvas, HeaderGlowCanvas};
use crate::theme::DesignTokens;
use crate::tray::{TrayAction, TrayController};
use crate::views_palette::{PALETTE_INPUT_ID, filtered_palette_items};

const MAX_UNDO_DEPTH: usize = 30;
const TOAST_DURATION_SECS: f32 = 3.5;
const FIREFLY_MOTH_PNG: &[u8] = include_bytes!("../assets/firefly.png");
const FIREFLY_DUSK_PNG: &[u8] = include_bytes!("../assets/firefly-dusk.png");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Lighting,
    Studio,
    Fans,
    Automations,
    Devices,
    Settings,
}

impl Tab {
    pub const ALL: [Tab; 7] = [
        Tab::Overview,
        Tab::Lighting,
        Tab::Studio,
        Tab::Fans,
        Tab::Automations,
        Tab::Devices,
        Tab::Settings,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Lighting => "Lighting",
            Self::Studio => "Studio",
            Self::Fans => "Fans",
            Self::Automations => "Automations",
            Self::Devices => "Devices",
            Self::Settings => "Settings",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PulseSpan {
    OneMin,
    FiveMin,
    TenMin,
}

impl PulseSpan {
    pub const ALL: [PulseSpan; 3] = [PulseSpan::OneMin, PulseSpan::FiveMin, PulseSpan::TenMin];

    pub fn from_mins(mins: u8) -> Self {
        match mins {
            1 => Self::OneMin,
            10 => Self::TenMin,
            _ => Self::FiveMin,
        }
    }

    pub fn mins(self) -> u8 {
        match self {
            Self::OneMin => 1,
            Self::FiveMin => 5,
            Self::TenMin => 10,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::OneMin => "1 min",
            Self::FiveMin => "5 min",
            Self::TenMin => "10 min",
        }
    }

    pub fn max_samples(self) -> usize {
        match self {
            Self::OneMin => 30,
            Self::FiveMin => 150,
            Self::TenMin => 300,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoTriggerDraft {
    TimeWindow,
    AppRunning,
    Idle,
    Locked,
    TempAbove,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoActionDraft {
    ApplyPreset,
    LightsOff,
    Dim,
    FanCurve,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StarterIdea {
    SunsetDim,
    GamingBoost,
    QuietWhenIdle,
    ThermalBoost,
}

#[derive(Debug, Clone)]
pub enum Message {
    Noop,
    Tick,
    WindowOpened(window::Id),
    CloseRequested(window::Id),
    SelectTab(Tab),
    SetPulseSpan(PulseSpan),
    ClearNotice,
    DismissToast,
    CloseConflictingApp(String),

    // Command Palette & Global Overlays
    ToggleCommandPalette,
    CloseCommandPalette,
    ToggleShortcuts,
    ShortcutQuestionKey,
    EscapePressed,
    DigitTabShortcut(Tab),
    ToggleCompactMode,
    PaletteQueryChanged(String),
    PaletteMoveSelection(i32),
    PaletteExecuteSelected,
    PaletteExecuteIfOpen,
    PaletteExecuteIndex(usize),
    ToggleCustomizeDrawer,
    OpenCustomizeFromPalette,
    ToggleActivityStrip,

    // Sleep Timer
    CycleSleepTimer,
    StartSleepTimer(u16),
    CancelSleepTimer,

    // Appearance & Customisation
    SetAccentOverride(Option<[u8; 3]>),
    AccentHexDraftChanged(String),
    ApplyAccentHex,
    SetRigStyle(RigStyle),
    SetUiFpsCap(u8),
    ToggleAirflowLines,
    ToggleSpinningFans,
    ToggleAmbientBloom,
    ToggleFireflyParticles,
    ToggleCompactDensity,
    ToggleOverviewTile(String),
    ToggleHeaderPin(String),
    LookNameDraftChanged(String),
    SaveLook,
    ApplyLook(String),
    DeleteLook(String),

    // Lighting & per-zone overrides
    TogglePower,
    SetBrightness(u8),
    SelectEffect(EffectId),
    ApplyHarmony(usize),
    ApplyStudioEffect(String),
    UpdateSceneParams(SceneParams),
    LightingColorPicked([u8; 3]),
    LightingHexDraftChanged(String),
    SwapLightingColors,
    ClearZoneOverride(String),
    ClearAllZoneOverrides,
    SelectWholeRig,
    ToggleArrange,
    Stage(StageMessage),
    IdentifyZone(String),

    // Profiles & trial mode
    ApplyPreset(String),
    TryPreset(String),
    KeepTrial,
    RevertTrial,
    DeletePreset(String),
    PresetNameDraftChanged(String),
    SaveCurrentPreset,

    // Studio workbench
    AddLayer(LayerKind),
    RemoveLayer(u64),
    MoveLayer(u64, bool),
    SelectLayer(u64),
    UpdateSelectedLayer(Layer),
    StudioColorPicked([u8; 3]),
    StudioHexDraftChanged(String),
    SetStudioMasterSpeed(u16),
    StudioSurpriseMe,
    StudioUndo,
    StudioRedo,
    StudioSaveNameChanged(String),
    SaveStudioEffect,
    LoadStudioEffect(String),
    DuplicateStudioEffect(String),
    DeleteStudioEffect(String),
    ToggleStudioTestBeat(bool),
    ToggleStudioPaused,
    SetStudioSpeedMult(f32),

    // Fans, LHM channels & cooling recorder
    SelectFanCurve(String),
    CurveEdited(Vec<CurvePoint>),
    ResetFanCurve(String),
    SelectTempSource(TempSource),
    SetFanHysteresis(f32),
    SetFanChannelMode(String, FanChannelMode),
    ToggleCoolingRecording,
    ExportCoolingCsv,

    // Automations
    SetAutoTriggerDraft(AutoTriggerDraft),
    SetAutoActionDraft(AutoActionDraft),
    SetAutoStartDraft(String),
    SetAutoEndDraft(String),
    ToggleAutoDay(u8),
    SetAutoDaysMask(u8),
    SetAutoExeDraft(String),
    SetAutoIdleDraft(String),
    SetAutoTempSource(TempSource),
    SetAutoTempDraft(String),
    SetAutoPresetDraft(String),
    SetAutoDimDraft(u8),
    SetAutoCurveDraft(String),
    AddAutomation,
    AddStarterAutomation(StarterIdea),
    ToggleAutomation(String),
    DeleteAutomation(String),
    MoveAutomation(String, bool),
    SetAutomationsPaused(bool),

    // Devices & zone setup editor
    ToggleZone(String),
    CycleZoneColorOrder(String),
    ToggleExpandZone(String),
    UpdateZone(ZoneConfig),
    RescanHardware,

    // Settings
    SetDesign(DesignId),
    SetTempUnit(TempUnit),
    ToggleStartWithWindows(bool),
    ToggleLightsOffSleep(bool),
    ToggleLightsOffLock(bool),
    ToggleDemoMode(bool),
    SetLhmUrl(String),
    SetTariffDraft(String),
    ToggleOpenRgb(bool),
    SetOpenRgbHost(String),
    SetOpenRgbPort(String),
    ImportPythonSettings,
    ExportDiagnostics,
}

pub struct AirglowApp {
    pub engine: EngineHandle,
    pub snapshot: EngineSnapshot,
    pub tray: Option<TrayController>,
    pub main_window: Option<window::Id>,
    pub tab: Tab,
    pub pulse_span: PulseSpan,

    // Adaptive GPU animation clock & cached bundled Firefly artworks
    pub boot_instant: Instant,
    pub anim_t: f32,
    pub moth_art: image::Handle,
    pub dusk_art: image::Handle,

    // Command Palette, Shortcuts Modal, Customize Drawer, Compact Mini-Mode, Activity Strip & Toast
    pub compact_window: bool,
    pub titlebar_applied: bool,
    pub show_shortcuts: bool,
    pub palette_open: bool,
    pub palette_query: String,
    pub palette_selected: usize,
    pub customize_open: bool,
    pub activity_open: bool,
    pub toast: Option<(String, u32, Instant)>,

    // Lighting & Devices selection
    pub selected_zone: Option<String>,
    pub arrange_mode: bool,
    pub preset_name_draft: String,
    pub lighting_hex_draft: String,
    pub expanded_zone: Option<String>,

    // Studio state
    pub selected_layer: Option<u64>,
    pub studio_undo: Vec<LayerStack>,
    pub studio_redo: Vec<LayerStack>,
    pub studio_save_name: String,
    pub studio_hex_draft: String,

    // Automations builder drafts
    pub auto_trigger: AutoTriggerDraft,
    pub auto_action: AutoActionDraft,
    pub auto_start_draft: String,
    pub auto_end_draft: String,
    pub auto_days_mask: u8,
    pub auto_exe_draft: String,
    pub auto_idle_draft: String,
    pub auto_temp_source: TempSource,
    pub auto_temp_draft: String,
    pub auto_preset_draft: String,
    pub auto_dim_draft: u8,
    pub auto_curve_draft: String,

    // Settings & Appearance text drafts
    pub look_name_draft: String,
    pub accent_hex_draft: String,
    pub lhm_url_draft: String,
    pub openrgb_host_draft: String,
    pub openrgb_port_draft: String,
    pub tariff_draft: String,
    sync_drafts_pending: bool,
}

fn main() -> iced::Result {
    iced::daemon(AirglowApp::boot, AirglowApp::update, AirglowApp::view)
        .title(AirglowApp::title)
        .theme(AirglowApp::theme)
        .subscription(AirglowApp::subscription)
        .run()
}

impl AirglowApp {
    fn boot() -> (Self, Task<Message>) {
        let engine = EngineHandle::start();
        let snapshot = engine.rx.borrow().clone();
        let presets: Vec<(String, String)> = snapshot
            .config
            .presets
            .iter()
            .map(|p| (p.id.clone(), p.name.clone()))
            .collect();
        let tray = TrayController::new(&presets);
        let selected_layer = snapshot
            .config
            .scene
            .studio_stack
            .layers
            .first()
            .map(|l| l.id);
        let pulse_span = PulseSpan::from_mins(snapshot.config.appearance.pulse_window_mins);
        let lhm_url_draft = snapshot.config.settings.lhm_url.clone();
        let openrgb_host_draft = snapshot.config.settings.openrgb_host.clone();
        let openrgb_port_draft = snapshot.config.settings.openrgb_port.to_string();
        let tariff_draft = format!("{:.4}", snapshot.config.settings.energy_price_eur_kwh);
        let studio_save_name = snapshot.config.scene.studio_stack.name.clone();
        let default_preset = snapshot
            .config
            .presets
            .iter()
            .find(|p| p.id == "gaming")
            .or_else(|| snapshot.config.presets.first())
            .map(|p| p.id.clone())
            .unwrap_or_else(|| "gaming".to_string());

        let launch = parse_launch_flags();
        if launch.tab == Tab::Studio {
            engine.send(EngineCommand::SetStudioPreviewActive(true));
        }
        let lighting_hex_draft = format_hex_rgb(snapshot.config.scene.params.color);
        let studio_hex_draft = snapshot
            .config
            .scene
            .studio_stack
            .layers
            .first()
            .map(|l| format_hex_rgb(l.color))
            .unwrap_or_else(|| "#F48FB1".to_string());
        let accent_hex_draft = format_hex_rgb(
            snapshot
                .config
                .appearance
                .accent_override
                .unwrap_or_else(|| theme::hex_to_rgb_arr(snapshot.config.settings.design.palette().brass)),
        );
        let compact_window = launch.compact_window;

        let app = Self {
            engine,
            snapshot,
            tray,
            main_window: None,
            tab: launch.tab,
            pulse_span,
            boot_instant: Instant::now(),
            anim_t: 0.0,
            moth_art: image::Handle::from_bytes(FIREFLY_MOTH_PNG),
            dusk_art: image::Handle::from_bytes(FIREFLY_DUSK_PNG),
            compact_window,
            titlebar_applied: false,
            show_shortcuts: launch.show_shortcuts,
            palette_open: launch.palette_open,
            palette_query: launch.palette_query,
            palette_selected: 0,
            customize_open: launch.customize_open,
            activity_open: false,
            toast: None,
            selected_zone: None,
            arrange_mode: false,
            preset_name_draft: String::new(),
            lighting_hex_draft,
            expanded_zone: None,
            selected_layer,
            studio_undo: Vec::new(),
            studio_redo: Vec::new(),
            studio_save_name,
            studio_hex_draft,
            auto_trigger: AutoTriggerDraft::AppRunning,
            auto_action: AutoActionDraft::ApplyPreset,
            auto_start_draft: "23:00".to_string(),
            auto_end_draft: "07:00".to_string(),
            auto_days_mask: ALL_DAYS_MASK,
            auto_exe_draft: "Cyberpunk2077.exe".to_string(),
            auto_idle_draft: "10".to_string(),
            auto_temp_source: TempSource::Max,
            auto_temp_draft: "80".to_string(),
            auto_preset_draft: default_preset,
            auto_dim_draft: 15,
            auto_curve_draft: "performance".to_string(),
            look_name_draft: String::new(),
            accent_hex_draft,
            lhm_url_draft,
            openrgb_host_draft,
            openrgb_port_draft,
            tariff_draft,
            sync_drafts_pending: false,
        };

        (app, open_main_window(compact_window))
    }

    fn title(&self, _window: window::Id) -> String {
        format!(
            "Airglow — {}",
            self.snapshot.config.settings.design.label()
        )
    }

    fn theme(&self, _window: window::Id) -> Option<Theme> {
        Some(theme::iced_theme_with_accent(
            self.snapshot.config.settings.design,
            self.snapshot.config.appearance.accent_override,
        ))
    }

    fn subscription(&self) -> Subscription<Message> {
        let tick_ms = if self.main_window.is_some() {
            match self.snapshot.config.appearance.ui_fps_cap {
                15 => 66,
                60 => 16,
                _ => 33,
            }
        } else {
            300
        };
        Subscription::batch([
            time::every(Duration::from_millis(tick_ms)).map(|_| Message::Tick),
            window::close_requests().map(Message::CloseRequested),
            iced::event::listen_with(|ev, status, _win| match ev {
                Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                    match key {
                        keyboard::Key::Character(ref ch) if modifiers.command() => {
                            match ch.to_ascii_lowercase().as_str() {
                                "k" => Some(Message::ToggleCommandPalette),
                                "m" => Some(Message::ToggleCompactMode),
                                "/" => Some(Message::ToggleShortcuts),
                                "z" if modifiers.shift() => Some(Message::StudioRedo),
                                "z" => Some(Message::StudioUndo),
                                "y" => Some(Message::StudioRedo),
                                "1" => Some(Message::SelectTab(Tab::Overview)),
                                "2" => Some(Message::SelectTab(Tab::Lighting)),
                                "3" => Some(Message::SelectTab(Tab::Studio)),
                                "4" => Some(Message::SelectTab(Tab::Fans)),
                                "5" => Some(Message::SelectTab(Tab::Automations)),
                                "6" => Some(Message::SelectTab(Tab::Devices)),
                                "7" => Some(Message::SelectTab(Tab::Settings)),
                                _ => None,
                            }
                        }
                        keyboard::Key::Named(keyboard::key::Named::Escape) => {
                            Some(Message::EscapePressed)
                        }
                        keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
                            Some(Message::PaletteMoveSelection(1))
                        }
                        keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
                            Some(Message::PaletteMoveSelection(-1))
                        }
                        keyboard::Key::Named(keyboard::key::Named::Enter)
                            if status == iced::event::Status::Ignored =>
                        {
                            Some(Message::PaletteExecuteIfOpen)
                        }
                        keyboard::Key::Character(ref ch)
                            if status == iced::event::Status::Ignored
                                && !modifiers.control()
                                && !modifiers.alt()
                                && !modifiers.logo() =>
                        {
                            match ch.as_str() {
                                "?" => Some(Message::ShortcutQuestionKey),
                                "1" => Some(Message::DigitTabShortcut(Tab::Overview)),
                                "2" => Some(Message::DigitTabShortcut(Tab::Lighting)),
                                "3" => Some(Message::DigitTabShortcut(Tab::Studio)),
                                "4" => Some(Message::DigitTabShortcut(Tab::Fans)),
                                "5" => Some(Message::DigitTabShortcut(Tab::Automations)),
                                "6" => Some(Message::DigitTabShortcut(Tab::Devices)),
                                "7" => Some(Message::DigitTabShortcut(Tab::Settings)),
                                _ => None,
                            }
                        }
                        _ => None,
                    }
                }
                _ => None,
            }),
        ])
    }

    fn current_accent_hex(&self) -> u32 {
        DesignTokens::for_appearance(
            self.snapshot.config.settings.design,
            self.snapshot.config.appearance.accent_override,
            self.snapshot.config.appearance.compact_density,
        )
        .p
        .brass_hi
    }

    fn show_toast(&mut self, msg: impl Into<String>, accent: u32) {
        self.toast = Some((msg.into(), accent, Instant::now()));
    }

    fn sync_window_titlebar(&mut self) {
        let t = DesignTokens::for_appearance(
            self.snapshot.config.settings.design,
            self.snapshot.config.appearance.accent_override,
            self.snapshot.config.appearance.compact_density,
        );
        let caption = theme::hex_to_rgb_arr(t.appbar_mid);
        let ink = theme::hex_to_rgb_arr(t.p.ink);
        let border = theme::hex_to_rgb_arr(t.appbar_border);
        self.titlebar_applied =
            airglow_core::system::apply_window_titlebar_theme(caption, ink, border, !t.p.is_light);
    }

    fn push_studio_undo(&mut self) {
        let cur = self.snapshot.config.scene.studio_stack.clone();
        if self.studio_undo.last() != Some(&cur) {
            self.studio_undo.push(cur);
            if self.studio_undo.len() > MAX_UNDO_DEPTH {
                self.studio_undo.remove(0);
            }
        }
        self.studio_redo.clear();
    }

    pub fn selected_zone_cfg(&self) -> Option<&ZoneConfig> {
        let zid = self.selected_zone.as_deref()?;
        self.snapshot.config.zones.iter().find(|z| z.id == zid)
    }

    pub fn active_lighting_target(&self) -> (EffectId, &SceneParams) {
        if self.tab == Tab::Lighting
            && let Some(zid) = self.selected_zone.as_deref()
            && let Some(ov) = self.snapshot.config.scene.zone_overrides.get(zid)
        {
            return (ov.effect, &ov.params);
        }
        (
            self.snapshot.config.scene.effect,
            &self.snapshot.config.scene.params,
        )
    }

    fn send_appearance(&mut self, appr: airglow_core::config::AppearanceConfig) {
        let appr = appr.normalized();
        self.snapshot.config.appearance = appr.clone();
        self.engine.send(EngineCommand::UpdateAppearance(appr));
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Noop => Task::none(),
            Message::Tick => {
                self.anim_t = self.boot_instant.elapsed().as_secs_f32();
                if let Some((_, _, at)) = self.toast
                    && at.elapsed().as_secs_f32() >= TOAST_DURATION_SECS
                {
                    self.toast = None;
                }
                if !self.titlebar_applied && self.main_window.is_some() {
                    self.sync_window_titlebar();
                }
                if self.engine.rx.has_changed().unwrap_or(false) {
                    let prev_design = self.snapshot.config.settings.design;
                    let prev_accent = self.snapshot.config.appearance.accent_override;
                    self.snapshot = self.engine.rx.borrow_and_update().clone();
                    if self.snapshot.config.settings.design != prev_design
                        || self.snapshot.config.appearance.accent_override != prev_accent
                    {
                        self.sync_window_titlebar();
                    }
                    self.pulse_span =
                        PulseSpan::from_mins(self.snapshot.config.appearance.pulse_window_mins);
                    if self.sync_drafts_pending {
                        self.lhm_url_draft = self.snapshot.config.settings.lhm_url.clone();
                        self.openrgb_host_draft = self.snapshot.config.settings.openrgb_host.clone();
                        self.openrgb_port_draft =
                            self.snapshot.config.settings.openrgb_port.to_string();
                        self.tariff_draft =
                            format!("{:.4}", self.snapshot.config.settings.energy_price_eur_kwh);
                        self.sync_drafts_pending = false;
                    }
                    if let Some(sel) = self.selected_layer
                        && !self
                            .snapshot
                            .config
                            .scene
                            .studio_stack
                            .layers
                            .iter()
                            .any(|l| l.id == sel)
                    {
                        self.selected_layer = self
                            .snapshot
                            .config
                            .scene
                            .studio_stack
                            .layers
                            .first()
                            .map(|l| l.id);
                    }
                }
                if let Some(tray) = self.tray.as_mut() {
                    let presets: Vec<(String, String)> = self
                        .snapshot
                        .config
                        .presets
                        .iter()
                        .map(|p| (p.id.clone(), p.name.clone()))
                        .collect();
                    tray.sync_presets(&presets);

                    let unit = self.snapshot.config.settings.temp_unit;
                    let cpu = sensors::format_temp(self.snapshot.sensors.cpu_temp, unit);
                    let gpu = sensors::format_temp(self.snapshot.sensors.gpu_temp, unit);
                    let tip = format!(
                        "Airglow · CPU {cpu} · GPU {gpu} · Fans {}%",
                        self.snapshot.fan_duty
                    );
                    tray.update_status(
                        self.snapshot.average_rgb,
                        self.snapshot.effective_scene.on,
                        &tip,
                    );
                    for action in tray.poll_actions() {
                        match action {
                            TrayAction::OpenWindow => {
                                if let Some(id) = self.main_window {
                                    return window::gain_focus(id);
                                }
                                return open_main_window(self.compact_window);
                            }
                            TrayAction::ToggleCompactMode => {
                                if let Some(id) = self.main_window {
                                    self.compact_window = !self.compact_window;
                                    self.palette_open = false;
                                    let target = if self.compact_window {
                                        Size::new(430.0, 404.0)
                                    } else {
                                        Size::new(1340.0, 920.0)
                                    };
                                    self.titlebar_applied = false;
                                    self.sync_window_titlebar();
                                    return Task::batch([
                                        window::resize(id, target),
                                        window::gain_focus(id),
                                    ]);
                                }
                                self.compact_window = true;
                                return open_main_window(true);
                            }
                            TrayAction::ToggleLights => {
                                let next = !self.snapshot.config.scene.on;
                                self.engine.send(EngineCommand::SetPower(next));
                            }
                            TrayAction::ApplyPreset(pid) => {
                                self.engine.send(EngineCommand::ApplyPreset(pid));
                            }
                            TrayAction::SelectEffect(eff) => {
                                self.engine.send(EngineCommand::SetEffect(eff));
                            }
                            TrayAction::SelectFanCurve(cid) => {
                                self.engine.send(EngineCommand::SelectFanCurve(cid));
                            }
                            TrayAction::Quit => {
                                self.engine.shutdown();
                                return iced::exit();
                            }
                        }
                    }
                }
                Task::none()
            }
            Message::WindowOpened(id) => {
                self.main_window = Some(id);
                self.titlebar_applied = false;
                self.sync_window_titlebar();
                Task::none()
            }
            Message::CloseRequested(id) => {
                if self.main_window == Some(id) {
                    self.main_window = None;
                }
                if self.tray.is_some() {
                    window::close(id)
                } else {
                    self.engine.shutdown();
                    iced::exit()
                }
            }
            Message::SelectTab(tab) => {
                self.tab = tab;
                self.engine
                    .send(EngineCommand::SetStudioPreviewActive(tab == Tab::Studio));
                Task::none()
            }
            Message::SetPulseSpan(span) => {
                self.pulse_span = span;
                let mut appr = self.snapshot.config.appearance.clone();
                appr.pulse_window_mins = span.mins();
                self.send_appearance(appr);
                Task::none()
            }
            Message::ClearNotice => {
                self.engine.send(EngineCommand::ClearNotice);
                Task::none()
            }
            Message::DismissToast => {
                self.toast = None;
                Task::none()
            }
            Message::CloseConflictingApp(name) => {
                self.show_toast(format!("Closing {name}…"), 0xf2b94b);
                self.engine.send(EngineCommand::CloseConflictingApp(name));
                Task::none()
            }

            // Command Palette & Overlays
            Message::ToggleCommandPalette => {
                self.palette_open = !self.palette_open;
                if self.palette_open {
                    self.show_shortcuts = false;
                }
                self.palette_query.clear();
                self.palette_selected = 0;
                if self.palette_open {
                    operation::focus(iced::widget::Id::new(PALETTE_INPUT_ID))
                } else {
                    Task::none()
                }
            }
            Message::CloseCommandPalette => {
                self.palette_open = false;
                self.palette_query.clear();
                self.palette_selected = 0;
                Task::none()
            }
            Message::ToggleShortcuts => {
                self.show_shortcuts = !self.show_shortcuts;
                if self.show_shortcuts {
                    self.palette_open = false;
                }
                Task::none()
            }
            Message::ShortcutQuestionKey => {
                if !self.palette_open {
                    self.show_shortcuts = !self.show_shortcuts;
                }
                Task::none()
            }
            Message::EscapePressed => {
                if self.palette_open {
                    self.palette_open = false;
                    self.palette_query.clear();
                    self.palette_selected = 0;
                } else if self.show_shortcuts {
                    self.show_shortcuts = false;
                } else if self.customize_open {
                    self.customize_open = false;
                }
                Task::none()
            }
            Message::DigitTabShortcut(tab) => {
                if !self.palette_open && !self.compact_window && !self.show_shortcuts {
                    self.tab = tab;
                    self.engine
                        .send(EngineCommand::SetStudioPreviewActive(tab == Tab::Studio));
                }
                Task::none()
            }
            Message::ToggleCompactMode => {
                self.compact_window = !self.compact_window;
                self.palette_open = false;
                self.show_shortcuts = false;
                let target = if self.compact_window {
                    Size::new(430.0, 404.0)
                } else {
                    Size::new(1340.0, 920.0)
                };
                self.show_toast(
                    if self.compact_window {
                        "Compact Mini-Mode (Ctrl+M to expand)"
                    } else {
                        "Expanded full window"
                    },
                    self.current_accent_hex(),
                );
                self.titlebar_applied = false;
                self.sync_window_titlebar();
                if let Some(id) = self.main_window {
                    window::resize(id, target)
                } else {
                    Task::none()
                }
            }
            Message::PaletteQueryChanged(q) => {
                self.palette_query = q;
                self.palette_selected = 0;
                Task::none()
            }
            Message::PaletteMoveSelection(delta) => {
                if !self.palette_open {
                    return Task::none();
                }
                let len = filtered_palette_items(self).len();
                if len > 0 {
                    let cur = self.palette_selected.min(len - 1) as i32;
                    self.palette_selected = (cur + delta).rem_euclid(len as i32) as usize;
                }
                Task::none()
            }
            Message::PaletteExecuteIfOpen => {
                if self.palette_open {
                    self.update(Message::PaletteExecuteSelected)
                } else {
                    Task::none()
                }
            }
            Message::PaletteExecuteSelected => {
                let items = filtered_palette_items(self);
                if let Some(item) = items.get(self.palette_selected).or_else(|| items.first()) {
                    let msg = item.message.clone();
                    self.palette_open = false;
                    self.palette_query.clear();
                    self.palette_selected = 0;
                    return self.update(msg);
                }
                self.palette_open = false;
                Task::none()
            }
            Message::PaletteExecuteIndex(idx) => {
                let items = filtered_palette_items(self);
                if let Some(item) = items.get(idx) {
                    let msg = item.message.clone();
                    self.palette_open = false;
                    self.palette_query.clear();
                    self.palette_selected = 0;
                    return self.update(msg);
                }
                self.palette_open = false;
                Task::none()
            }
            Message::ToggleCustomizeDrawer => {
                self.customize_open = !self.customize_open;
                Task::none()
            }
            Message::OpenCustomizeFromPalette => {
                self.tab = Tab::Overview;
                self.engine
                    .send(EngineCommand::SetStudioPreviewActive(false));
                self.customize_open = !self.customize_open;
                Task::none()
            }
            Message::ToggleActivityStrip => {
                self.activity_open = !self.activity_open;
                Task::none()
            }

            // Sleep Timer
            Message::CycleSleepTimer => {
                let next = match self.snapshot.sleep_timer_remaining_s {
                    None => Some(15u16),
                    Some(rem) if rem <= 20 * 60 => Some(30),
                    Some(rem) if rem <= 45 * 60 => Some(60),
                    Some(_) => None,
                };
                match next {
                    Some(mins) => self.update(Message::StartSleepTimer(mins)),
                    None => self.update(Message::CancelSleepTimer),
                }
            }
            Message::StartSleepTimer(mins) => {
                self.engine.send(EngineCommand::StartSleepTimer(mins));
                self.show_toast(format!("Sleep timer: {mins} min"), 0xd1b9f0);
                Task::none()
            }
            Message::CancelSleepTimer => {
                self.engine.send(EngineCommand::CancelSleepTimer);
                self.show_toast("Sleep timer cancelled", self.current_accent_hex());
                Task::none()
            }

            // Appearance & Customisation
            Message::SetAccentOverride(opt) => {
                let mut appr = self.snapshot.config.appearance.clone();
                appr.accent_override = opt;
                self.send_appearance(appr);
                self.accent_hex_draft = format_hex_rgb(opt.unwrap_or_else(|| {
                    theme::hex_to_rgb_arr(self.snapshot.config.settings.design.palette().brass)
                }));
                let label = match opt {
                    Some(rgb) => ACCENT_SWATCHES
                        .iter()
                        .find(|(_, c)| *c == rgb)
                        .map(|(n, _)| format!("Accent: {n}"))
                        .unwrap_or_else(|| format!("Custom accent {}", format_hex_rgb(rgb))),
                    None => "Accent reset to theme default".to_string(),
                };
                let toast_accent = opt
                    .map(theme::rgb_arr_to_hex)
                    .unwrap_or_else(|| self.current_accent_hex());
                self.show_toast(label, toast_accent);
                self.sync_window_titlebar();
                Task::none()
            }
            Message::AccentHexDraftChanged(s) => {
                self.accent_hex_draft = s.clone();
                let trimmed = s.trim().trim_start_matches('#');
                if trimmed.len() == 6
                    && let Some(rgb) = parse_hex_rgb(&s)
                {
                    let mut appr = self.snapshot.config.appearance.clone();
                    appr.accent_override = Some(rgb);
                    self.send_appearance(appr);
                    self.sync_window_titlebar();
                }
                Task::none()
            }
            Message::ApplyAccentHex => {
                if let Some(rgb) = parse_hex_rgb(&self.accent_hex_draft) {
                    return self.update(Message::SetAccentOverride(Some(rgb)));
                }
                self.show_toast("Enter a valid #RRGGBB hex colour", 0xf2b94b);
                Task::none()
            }
            Message::SetRigStyle(style) => {
                let mut appr = self.snapshot.config.appearance.clone();
                appr.rig_style = style;
                self.send_appearance(appr);
                self.show_toast(
                    format!("Rig style: {}", style.label()),
                    self.current_accent_hex(),
                );
                Task::none()
            }
            Message::SetUiFpsCap(fps) => {
                let mut appr = self.snapshot.config.appearance.clone();
                appr.ui_fps_cap = fps;
                self.send_appearance(appr);
                let mode = match fps {
                    15 => "Eco (15 FPS)",
                    60 => "Smooth (60 FPS)",
                    _ => "Balanced (30 FPS)",
                };
                self.show_toast(format!("GPU Power Mode: {mode}"), 0xa5dece);
                Task::none()
            }
            Message::ToggleAirflowLines => {
                let mut appr = self.snapshot.config.appearance.clone();
                appr.airflow_lines = !appr.airflow_lines;
                self.send_appearance(appr);
                Task::none()
            }
            Message::ToggleSpinningFans => {
                let mut appr = self.snapshot.config.appearance.clone();
                appr.spinning_fans = !appr.spinning_fans;
                self.send_appearance(appr);
                Task::none()
            }
            Message::ToggleAmbientBloom => {
                let mut appr = self.snapshot.config.appearance.clone();
                appr.ambient_bloom = !appr.ambient_bloom;
                self.send_appearance(appr);
                Task::none()
            }
            Message::ToggleFireflyParticles => {
                let mut appr = self.snapshot.config.appearance.clone();
                appr.firefly_particles = !appr.firefly_particles;
                self.send_appearance(appr);
                Task::none()
            }
            Message::ToggleCompactDensity => {
                let mut appr = self.snapshot.config.appearance.clone();
                appr.compact_density = !appr.compact_density;
                self.send_appearance(appr);
                Task::none()
            }
            Message::ToggleOverviewTile(key) => {
                let mut appr = self.snapshot.config.appearance.clone();
                if let Some(pos) = appr.hidden_overview_tiles.iter().position(|k| k == &key) {
                    appr.hidden_overview_tiles.remove(pos);
                } else {
                    appr.hidden_overview_tiles.push(key);
                }
                self.send_appearance(appr);
                Task::none()
            }
            Message::ToggleHeaderPin(key) => {
                let mut appr = self.snapshot.config.appearance.clone();
                if let Some(pos) = appr.header_pins.iter().position(|k| k == &key) {
                    if appr.header_pins.len() > 1 {
                        appr.header_pins.remove(pos);
                    }
                } else {
                    appr.header_pins.push(key);
                }
                self.send_appearance(appr);
                Task::none()
            }
            Message::LookNameDraftChanged(s) => {
                self.look_name_draft = s;
                Task::none()
            }
            Message::SaveLook => {
                let name = self.look_name_draft.trim().to_string();
                if !name.is_empty() {
                    self.engine.send(EngineCommand::SaveLook(name.clone()));
                    self.look_name_draft.clear();
                    self.show_toast(format!("Saved look \"{name}\""), self.current_accent_hex());
                }
                Task::none()
            }
            Message::ApplyLook(id) => {
                if let Some(look) = self
                    .snapshot
                    .config
                    .saved_looks
                    .iter()
                    .find(|l| l.id == id)
                    .cloned()
                {
                    self.snapshot.config.settings.design = look.design;
                    self.snapshot.config.appearance.accent_override = look.accent_override;
                    self.snapshot.config.appearance.rig_style = look.rig_style;
                    self.sync_window_titlebar();
                    let accent = look
                        .accent_override
                        .map(theme::rgb_arr_to_hex)
                        .unwrap_or_else(|| look.design.palette().brass_hi);
                    self.show_toast(format!("Applied look \"{}\"", look.name), accent);
                }
                self.engine.send(EngineCommand::ApplyLook(id));
                Task::none()
            }
            Message::DeleteLook(id) => {
                self.engine.send(EngineCommand::DeleteLook(id));
                self.show_toast("Removed saved look", self.current_accent_hex());
                Task::none()
            }

            // Lighting & per-zone overrides
            Message::TogglePower => {
                let next = !self.snapshot.config.scene.on;
                self.engine.send(EngineCommand::SetPower(next));
                self.show_toast(
                    if next { "Lights turned on" } else { "Lights turned off" },
                    self.current_accent_hex(),
                );
                Task::none()
            }
            Message::SetBrightness(val) => {
                self.engine.send(EngineCommand::SetBrightness(val));
                Task::none()
            }
            Message::SelectEffect(eff) => {
                if self.tab == Tab::Lighting
                    && let Some(zid) = self.selected_zone.clone()
                {
                    let existing = self.snapshot.config.scene.zone_overrides.get(&zid);
                    let params = existing
                        .map(|o| o.params.clone())
                        .unwrap_or_else(|| self.snapshot.config.scene.params.clone());
                    let studio_stack = if eff == EffectId::Studio {
                        existing
                            .and_then(|o| o.studio_stack.clone())
                            .or_else(|| Some(self.snapshot.config.scene.studio_stack.clone()))
                    } else {
                        None
                    };
                    self.engine.send(EngineCommand::SetZoneOverride {
                        zone_id: zid,
                        override_cfg: ZoneOverride {
                            effect: eff,
                            params,
                            studio_stack,
                        },
                    });
                } else {
                    self.engine.send(EngineCommand::SetEffect(eff));
                }
                self.show_toast(format!("Effect: {}", eff.label()), self.current_accent_hex());
                Task::none()
            }
            Message::ApplyHarmony(idx) => {
                if let Some(h) = HARMONIES.get(idx) {
                    self.engine.send(EngineCommand::ApplyHarmony(idx));
                    self.show_toast(
                        format!("Applied {} palette", h.name),
                        theme::rgb_arr_to_hex(h.colors[0]),
                    );
                }
                Task::none()
            }
            Message::ApplyStudioEffect(id) => {
                if self.tab == Tab::Lighting
                    && let Some(zid) = self.selected_zone.clone()
                {
                    if let Some(saved) = self
                        .snapshot
                        .config
                        .studio_effects
                        .iter()
                        .find(|e| e.id == id)
                    {
                        self.engine.send(EngineCommand::SetZoneOverride {
                            zone_id: zid,
                            override_cfg: ZoneOverride {
                                effect: EffectId::Studio,
                                params: self.snapshot.config.scene.params.clone(),
                                studio_stack: Some(saved.stack.clone()),
                            },
                        });
                    }
                } else {
                    self.engine.send(EngineCommand::ApplyStudioEffect(id));
                }
                Task::none()
            }
            Message::UpdateSceneParams(params) => {
                self.lighting_hex_draft = format_hex_rgb(params.color);
                if self.tab == Tab::Lighting
                    && let Some(zid) = self.selected_zone.clone()
                {
                    let (effect, studio_stack) = self
                        .snapshot
                        .config
                        .scene
                        .zone_overrides
                        .get(&zid)
                        .map(|o| (o.effect, o.studio_stack.clone()))
                        .unwrap_or((self.snapshot.config.scene.effect, None));
                    self.engine.send(EngineCommand::SetZoneOverride {
                        zone_id: zid,
                        override_cfg: ZoneOverride {
                            effect,
                            params,
                            studio_stack,
                        },
                    });
                } else {
                    self.engine.send(EngineCommand::SetParams(params));
                }
                Task::none()
            }
            Message::LightingColorPicked(rgb) => {
                let mut next = self.active_lighting_target().1.clone();
                next.color = rgb;
                if let Some(first) = next.colors.first_mut() {
                    *first = rgb;
                }
                self.lighting_hex_draft = format_hex_rgb(rgb);
                self.update(Message::UpdateSceneParams(next))
            }
            Message::LightingHexDraftChanged(s) => {
                self.lighting_hex_draft = s.clone();
                let trimmed = s.trim().trim_start_matches('#');
                if trimmed.len() == 6
                    && let Some(rgb) = parse_hex_rgb(&s)
                {
                    let mut next = self.active_lighting_target().1.clone();
                    next.color = rgb;
                    if let Some(first) = next.colors.first_mut() {
                        *first = rgb;
                    }
                    return self.update(Message::UpdateSceneParams(next));
                }
                Task::none()
            }
            Message::SwapLightingColors => {
                let mut next = self.active_lighting_target().1.clone();
                std::mem::swap(&mut next.color, &mut next.secondary);
                if next.colors.len() >= 2 {
                    next.colors[0] = next.color;
                    next.colors[1] = next.secondary;
                } else if let Some(first) = next.colors.first_mut() {
                    *first = next.color;
                }
                self.show_toast("Swapped primary & secondary colours", theme::rgb_arr_to_hex(next.color));
                self.update(Message::UpdateSceneParams(next))
            }
            Message::ClearZoneOverride(zid) => {
                self.engine.send(EngineCommand::ClearZoneOverride(zid));
                self.show_toast("Cleared zone override", self.current_accent_hex());
                Task::none()
            }
            Message::ClearAllZoneOverrides => {
                self.engine.send(EngineCommand::ClearAllZoneOverrides);
                self.show_toast("Cleared all zone overrides", self.current_accent_hex());
                Task::none()
            }
            Message::SelectWholeRig => {
                self.selected_zone = None;
                Task::none()
            }
            Message::ToggleArrange => {
                self.arrange_mode = !self.arrange_mode;
                Task::none()
            }
            Message::Stage(msg) => {
                match msg {
                    StageMessage::SelectZone(z) => self.selected_zone = z,
                    StageMessage::MoveZone { zone_id, x, y } => {
                        self.engine
                            .send(EngineCommand::UpdateZoneLayout { zone_id, x, y });
                    }
                }
                Task::none()
            }
            Message::IdentifyZone(zid) => {
                self.engine.send(EngineCommand::IdentifyZone(zid));
                Task::none()
            }

            // Profiles & Trial
            Message::ApplyPreset(pid) => {
                if let Some(pr) = self.snapshot.config.presets.iter().find(|p| p.id == pid) {
                    let name = pr.name.clone();
                    self.show_toast(format!("Applied profile \"{name}\""), self.current_accent_hex());
                }
                self.engine.send(EngineCommand::ApplyPreset(pid));
                Task::none()
            }
            Message::TryPreset(pid) => {
                self.engine.send(EngineCommand::TryPreset(pid));
                Task::none()
            }
            Message::KeepTrial => {
                self.engine.send(EngineCommand::KeepTrial);
                self.show_toast("Profile kept", self.current_accent_hex());
                Task::none()
            }
            Message::RevertTrial => {
                self.engine.send(EngineCommand::RevertTrial);
                self.show_toast("Reverted preview", self.current_accent_hex());
                Task::none()
            }
            Message::DeletePreset(pid) => {
                self.engine.send(EngineCommand::DeletePreset(pid));
                self.show_toast("Profile deleted", self.current_accent_hex());
                Task::none()
            }
            Message::PresetNameDraftChanged(s) => {
                self.preset_name_draft = s;
                Task::none()
            }
            Message::SaveCurrentPreset => {
                let name = self.preset_name_draft.trim().to_string();
                if !name.is_empty() {
                    self.engine.send(EngineCommand::SaveCurrentPreset(name.clone()));
                    self.preset_name_draft.clear();
                    self.show_toast(format!("Saved profile \"{name}\""), self.current_accent_hex());
                }
                Task::none()
            }

            // Studio workbench
            Message::AddLayer(kind) => {
                self.push_studio_undo();
                let next_id = self.snapshot.config.scene.studio_stack.next_id;
                self.selected_layer = Some(next_id);
                self.engine.send(EngineCommand::AddStudioLayer(kind));
                Task::none()
            }
            Message::RemoveLayer(id) => {
                self.push_studio_undo();
                if self.selected_layer == Some(id) {
                    self.selected_layer = None;
                }
                self.engine.send(EngineCommand::RemoveStudioLayer(id));
                Task::none()
            }
            Message::MoveLayer(id, up) => {
                self.push_studio_undo();
                self.engine.send(EngineCommand::MoveStudioLayer { id, up });
                Task::none()
            }
            Message::SelectLayer(id) => {
                self.selected_layer = Some(id);
                if let Some(l) = self
                    .snapshot
                    .config
                    .scene
                    .studio_stack
                    .layers
                    .iter()
                    .find(|l| l.id == id)
                {
                    self.studio_hex_draft = format_hex_rgb(l.color);
                }
                Task::none()
            }
            Message::UpdateSelectedLayer(layer) => {
                self.studio_hex_draft = format_hex_rgb(layer.color);
                self.push_studio_undo();
                self.engine.send(EngineCommand::UpdateStudioLayer(layer));
                Task::none()
            }
            Message::StudioColorPicked(rgb) => {
                self.studio_hex_draft = format_hex_rgb(rgb);
                if let Some(mut layer) = self
                    .selected_layer
                    .and_then(|id| {
                        self.snapshot
                            .config
                            .scene
                            .studio_stack
                            .layers
                            .iter()
                            .find(|l| l.id == id)
                    })
                    .or_else(|| self.snapshot.config.scene.studio_stack.layers.first())
                    .cloned()
                {
                    layer.color = rgb;
                    self.push_studio_undo();
                    self.engine.send(EngineCommand::UpdateStudioLayer(layer));
                }
                Task::none()
            }
            Message::StudioHexDraftChanged(s) => {
                self.studio_hex_draft = s.clone();
                let trimmed = s.trim().trim_start_matches('#');
                if trimmed.len() == 6
                    && let Some(rgb) = parse_hex_rgb(&s)
                {
                    return self.update(Message::StudioColorPicked(rgb));
                }
                Task::none()
            }
            Message::SetStudioMasterSpeed(v) => {
                self.push_studio_undo();
                let mut stack = self.snapshot.config.scene.studio_stack.clone();
                stack.master_speed = v.clamp(10, 200) as u8;
                self.engine.send(EngineCommand::SetStudioStack(stack));
                Task::none()
            }
            Message::StudioSurpriseMe => {
                self.push_studio_undo();
                self.selected_layer = Some(1);
                self.engine.send(EngineCommand::StudioSurpriseMe);
                self.show_toast("Generated fresh Studio scene", 0xc59bff);
                Task::none()
            }
            Message::StudioUndo => {
                if let Some(prev) = self.studio_undo.pop() {
                    self.studio_redo
                        .push(self.snapshot.config.scene.studio_stack.clone());
                    self.selected_layer = prev.layers.first().map(|l| l.id);
                    self.engine.send(EngineCommand::SetStudioStack(prev));
                    self.show_toast("Studio undo", self.current_accent_hex());
                }
                Task::none()
            }
            Message::StudioRedo => {
                if let Some(next) = self.studio_redo.pop() {
                    self.studio_undo
                        .push(self.snapshot.config.scene.studio_stack.clone());
                    self.selected_layer = next.layers.first().map(|l| l.id);
                    self.engine.send(EngineCommand::SetStudioStack(next));
                    self.show_toast("Studio redo", self.current_accent_hex());
                }
                Task::none()
            }
            Message::StudioSaveNameChanged(s) => {
                self.studio_save_name = s;
                Task::none()
            }
            Message::SaveStudioEffect => {
                let name = if self.studio_save_name.trim().is_empty() {
                    self.snapshot.config.scene.studio_stack.name.clone()
                } else {
                    self.studio_save_name.clone()
                };
                self.engine.send(EngineCommand::SaveStudioEffect(name.clone()));
                self.show_toast(
                    format!("Saved Studio effect \"{name}\""),
                    self.current_accent_hex(),
                );
                Task::none()
            }
            Message::LoadStudioEffect(id) => {
                self.push_studio_undo();
                if let Some(saved) = self
                    .snapshot
                    .config
                    .studio_effects
                    .iter()
                    .find(|e| e.id == id)
                {
                    self.studio_save_name = saved.name.clone();
                    self.selected_layer = saved.stack.layers.first().map(|l| l.id);
                }
                self.engine.send(EngineCommand::ApplyStudioEffect(id));
                Task::none()
            }
            Message::DuplicateStudioEffect(id) => {
                self.engine.send(EngineCommand::DuplicateStudioEffect(id));
                Task::none()
            }
            Message::DeleteStudioEffect(id) => {
                self.engine.send(EngineCommand::DeleteStudioEffect(id));
                Task::none()
            }
            Message::ToggleStudioTestBeat(on) => {
                self.engine.send(EngineCommand::SetStudioTestBeat(on));
                Task::none()
            }
            Message::ToggleStudioPaused => {
                self.engine
                    .send(EngineCommand::SetStudioPaused(!self.snapshot.studio_paused));
                Task::none()
            }
            Message::SetStudioSpeedMult(mult) => {
                self.engine.send(EngineCommand::SetStudioSpeedMult(mult));
                Task::none()
            }

            // Fans, LHM channels & cooling recorder
            Message::SelectFanCurve(name) => {
                let label = views::cap_first(&name);
                self.engine.send(EngineCommand::SelectFanCurve(name));
                self.show_toast(format!("Fan curve: {label}"), 0xa5dece);
                Task::none()
            }
            Message::CurveEdited(points) => {
                let name = self.snapshot.effective_curve.clone();
                self.engine
                    .send(EngineCommand::UpdateFanCurve { name, points });
                Task::none()
            }
            Message::ResetFanCurve(name) => {
                self.engine.send(EngineCommand::ResetFanCurve(name));
                self.show_toast("Fan curve reset to default", 0xa5dece);
                Task::none()
            }
            Message::SelectTempSource(src) => {
                let mut s = self.snapshot.config.settings.clone();
                s.temp_source = src;
                self.engine.send(EngineCommand::UpdateSettings(s));
                Task::none()
            }
            Message::SetFanHysteresis(val) => {
                let clamped = val.clamp(0.0, 8.0);
                self.snapshot.config.settings.fan_hysteresis_c = clamped;
                self.engine.send(EngineCommand::SetFanHysteresis(clamped));
                Task::none()
            }
            Message::SetFanChannelMode(channel_id, mode) => {
                self.engine
                    .send(EngineCommand::SetFanChannelMode { channel_id, mode });
                Task::none()
            }
            Message::ToggleCoolingRecording => {
                if self.snapshot.cooling_recording {
                    self.engine.send(EngineCommand::StopCoolingRecording);
                    self.show_toast("Cooling session saved", 0xa5dece);
                } else {
                    self.engine.send(EngineCommand::StartCoolingRecording);
                    self.show_toast("Cooling recording started", 0xf48fb1);
                }
                Task::none()
            }
            Message::ExportCoolingCsv => {
                self.engine.send(EngineCommand::ExportCoolingCsv);
                self.show_toast("Exported cooling CSV", 0xa5dece);
                Task::none()
            }

            // Automations
            Message::SetAutoTriggerDraft(t) => {
                self.auto_trigger = t;
                Task::none()
            }
            Message::SetAutoActionDraft(a) => {
                self.auto_action = a;
                Task::none()
            }
            Message::SetAutoStartDraft(s) => {
                self.auto_start_draft = s;
                Task::none()
            }
            Message::SetAutoEndDraft(s) => {
                self.auto_end_draft = s;
                Task::none()
            }
            Message::ToggleAutoDay(day_idx) => {
                let bit = 1u8 << (day_idx % 7);
                let next = self.auto_days_mask ^ bit;
                self.auto_days_mask = if next & ALL_DAYS_MASK == 0 {
                    bit
                } else {
                    next & ALL_DAYS_MASK
                };
                Task::none()
            }
            Message::SetAutoDaysMask(mask) => {
                self.auto_days_mask = mask & ALL_DAYS_MASK;
                Task::none()
            }
            Message::SetAutoExeDraft(s) => {
                self.auto_exe_draft = s;
                Task::none()
            }
            Message::SetAutoIdleDraft(s) => {
                self.auto_idle_draft = s;
                Task::none()
            }
            Message::SetAutoTempSource(src) => {
                self.auto_temp_source = src;
                Task::none()
            }
            Message::SetAutoTempDraft(s) => {
                self.auto_temp_draft = s;
                Task::none()
            }
            Message::SetAutoPresetDraft(pid) => {
                self.auto_preset_draft = pid;
                Task::none()
            }
            Message::SetAutoDimDraft(pct) => {
                self.auto_dim_draft = pct.clamp(5, 95);
                Task::none()
            }
            Message::SetAutoCurveDraft(c) => {
                self.auto_curve_draft = c;
                Task::none()
            }
            Message::AddAutomation => {
                let trigger = match self.auto_trigger {
                    AutoTriggerDraft::TimeWindow => {
                        let (sh, sm) = parse_hhmm(&self.auto_start_draft).unwrap_or((23, 0));
                        let (eh, em) = parse_hhmm(&self.auto_end_draft).unwrap_or((7, 0));
                        Trigger::TimeWindow {
                            start_hour: sh,
                            start_min: sm,
                            end_hour: eh,
                            end_min: em,
                            days_mask: self.auto_days_mask,
                        }
                    }
                    AutoTriggerDraft::AppRunning => Trigger::AppRunning {
                        exe: if self.auto_exe_draft.trim().is_empty() {
                            "Game.exe".to_string()
                        } else {
                            self.auto_exe_draft.trim().to_string()
                        },
                    },
                    AutoTriggerDraft::Idle => {
                        let minutes = self
                            .auto_idle_draft
                            .trim()
                            .parse::<u16>()
                            .unwrap_or(10)
                            .clamp(1, 240);
                        Trigger::Idle { minutes }
                    }
                    AutoTriggerDraft::Locked => Trigger::Locked,
                    AutoTriggerDraft::TempAbove => {
                        let temp_c = self
                            .auto_temp_draft
                            .trim()
                            .parse::<u8>()
                            .unwrap_or(80)
                            .clamp(30, 100);
                        Trigger::TempAbove {
                            source: self.auto_temp_source,
                            temp_c,
                        }
                    }
                };
                let action = match self.auto_action {
                    AutoActionDraft::ApplyPreset => Action::ApplyPreset {
                        preset: self.auto_preset_draft.clone(),
                    },
                    AutoActionDraft::LightsOff => Action::LightsOff,
                    AutoActionDraft::Dim => Action::Dim {
                        brightness: self.auto_dim_draft,
                    },
                    AutoActionDraft::FanCurve => Action::FanCurve {
                        curve: self.auto_curve_draft.clone(),
                    },
                };
                let rule = AutomationRule {
                    id: format!("rule-{}", self.snapshot.tick_count),
                    enabled: true,
                    trigger,
                    action,
                };
                self.engine.send(EngineCommand::AddAutomation(rule));
                self.show_toast("Automation added", self.current_accent_hex());
                Task::none()
            }
            Message::AddStarterAutomation(idea) => {
                let (label, rule) = match idea {
                    StarterIdea::SunsetDim => (
                        "Added Sunset dim at 23:00",
                        AutomationRule {
                            id: format!("sunset-dim-{}", self.snapshot.tick_count),
                            enabled: true,
                            trigger: Trigger::TimeWindow {
                                start_hour: 23,
                                start_min: 0,
                                end_hour: 7,
                                end_min: 0,
                                days_mask: ALL_DAYS_MASK,
                            },
                            action: Action::Dim { brightness: 20 },
                        },
                    ),
                    StarterIdea::GamingBoost => (
                        "Added Gaming boost recipe",
                        AutomationRule {
                            id: format!("gaming-boost-{}", self.snapshot.tick_count),
                            enabled: true,
                            trigger: Trigger::AppRunning {
                                exe: "Cyberpunk2077.exe".to_string(),
                            },
                            action: Action::ApplyPreset {
                                preset: "gaming".to_string(),
                            },
                        },
                    ),
                    StarterIdea::QuietWhenIdle => (
                        "Added Quiet when idle recipe",
                        AutomationRule {
                            id: format!("idle-quiet-{}", self.snapshot.tick_count),
                            enabled: true,
                            trigger: Trigger::Idle { minutes: 15 },
                            action: Action::FanCurve {
                                curve: "silent".to_string(),
                            },
                        },
                    ),
                    StarterIdea::ThermalBoost => (
                        "Added Thermal boost > 75°C recipe",
                        AutomationRule {
                            id: format!("thermal-boost-{}", self.snapshot.tick_count),
                            enabled: true,
                            trigger: Trigger::TempAbove {
                                source: TempSource::Max,
                                temp_c: 75,
                            },
                            action: Action::FanCurve {
                                curve: "performance".to_string(),
                            },
                        },
                    ),
                };
                self.engine.send(EngineCommand::AddAutomation(rule));
                self.show_toast(label, self.current_accent_hex());
                Task::none()
            }
            Message::ToggleAutomation(id) => {
                self.engine.send(EngineCommand::ToggleAutomation(id));
                Task::none()
            }
            Message::DeleteAutomation(id) => {
                self.engine.send(EngineCommand::DeleteAutomation(id));
                Task::none()
            }
            Message::MoveAutomation(id, up) => {
                self.engine.send(EngineCommand::MoveAutomation { id, up });
                Task::none()
            }
            Message::SetAutomationsPaused(paused) => {
                self.engine.send(EngineCommand::SetAutomationsPaused(paused));
                self.show_toast(
                    if paused {
                        "Automations paused"
                    } else {
                        "Automations resumed"
                    },
                    self.current_accent_hex(),
                );
                Task::none()
            }

            // Devices & zone setup editor
            Message::ToggleZone(zid) => {
                self.engine.send(EngineCommand::ToggleZone(zid));
                Task::none()
            }
            Message::CycleZoneColorOrder(zid) => {
                self.engine.send(EngineCommand::CycleZoneColorOrder(zid));
                Task::none()
            }
            Message::ToggleExpandZone(zid) => {
                if self.expanded_zone.as_deref() == Some(zid.as_str()) {
                    self.expanded_zone = None;
                } else {
                    self.expanded_zone = Some(zid);
                }
                Task::none()
            }
            Message::UpdateZone(cfg) => {
                self.engine.send(EngineCommand::UpdateZone(cfg));
                Task::none()
            }
            Message::RescanHardware => {
                self.engine.send(EngineCommand::RescanHardware);
                self.show_toast("Rescanning hardware & sensors…", 0xa5dece);
                Task::none()
            }

            // Settings
            Message::SetDesign(design) => {
                self.snapshot.config.settings.design = design;
                self.sync_window_titlebar();
                self.engine.send(EngineCommand::SetDesign(design));
                self.show_toast(
                    format!("Theme: {}", design.label()),
                    design.palette().brass_hi,
                );
                Task::none()
            }
            Message::SetTempUnit(unit) => {
                self.engine.send(EngineCommand::SetTempUnit(unit));
                Task::none()
            }
            Message::ToggleStartWithWindows(enabled) => {
                self.engine
                    .send(EngineCommand::SetStartWithWindows(enabled));
                Task::none()
            }
            Message::ToggleLightsOffSleep(enabled) => {
                let mut s = self.snapshot.config.settings.clone();
                s.lights_off_on_sleep = enabled;
                self.engine.send(EngineCommand::UpdateSettings(s));
                Task::none()
            }
            Message::ToggleLightsOffLock(enabled) => {
                let mut s = self.snapshot.config.settings.clone();
                s.lights_off_on_lock = enabled;
                self.engine.send(EngineCommand::UpdateSettings(s));
                Task::none()
            }
            Message::ToggleDemoMode(demo) => {
                let mut s = self.snapshot.config.settings.clone();
                s.demo_mode = demo;
                self.engine.send(EngineCommand::UpdateSettings(s));
                Task::none()
            }
            Message::SetLhmUrl(url) => {
                self.lhm_url_draft = url.clone();
                let mut s = self.snapshot.config.settings.clone();
                s.lhm_url = url;
                self.engine.send(EngineCommand::UpdateSettings(s));
                Task::none()
            }
            Message::SetTariffDraft(raw) => {
                self.tariff_draft = raw.clone();
                if let Ok(val) = raw.trim().replace(',', ".").parse::<f32>()
                    && val.is_finite()
                    && (0.0..=10.0).contains(&val)
                {
                    let mut s = self.snapshot.config.settings.clone();
                    s.energy_price_eur_kwh = val;
                    self.engine.send(EngineCommand::UpdateSettings(s));
                }
                Task::none()
            }
            Message::ToggleOpenRgb(enabled) => {
                let mut s = self.snapshot.config.settings.clone();
                s.openrgb_enabled = enabled;
                self.engine.send(EngineCommand::UpdateSettings(s));
                Task::none()
            }
            Message::SetOpenRgbHost(host) => {
                self.openrgb_host_draft = host.clone();
                let mut s = self.snapshot.config.settings.clone();
                s.openrgb_host = host;
                self.engine.send(EngineCommand::UpdateSettings(s));
                Task::none()
            }
            Message::SetOpenRgbPort(raw) => {
                self.openrgb_port_draft = raw.clone();
                if let Ok(port) = raw.trim().parse::<u16>()
                    && port > 0
                {
                    let mut s = self.snapshot.config.settings.clone();
                    s.openrgb_port = port;
                    self.engine.send(EngineCommand::UpdateSettings(s));
                }
                Task::none()
            }
            Message::ImportPythonSettings => {
                self.sync_drafts_pending = true;
                self.engine.send(EngineCommand::ImportPythonSettings);
                self.show_toast("Importing Python Airglow config…", self.current_accent_hex());
                Task::none()
            }
            Message::ExportDiagnostics => {
                self.engine.send(EngineCommand::ExportDiagnostics);
                self.show_toast("Diagnostics report saved", 0xa5dece);
                Task::none()
            }
        }
    }

    fn view(&self, _window: window::Id) -> Element<'_, Message> {
        let design = self.snapshot.config.settings.design;
        let appr = &self.snapshot.config.appearance;
        let t = DesignTokens::for_appearance(design, appr.accent_override, appr.compact_density);
        let p = t.p;
        let font = theme::ui_font(design);

        if self.compact_window {
            let mini = views::view_compact_companion(self, t, font);
            let mut layers: Vec<Element<'_, Message>> = vec![mini];
            if let Some((msg, accent, _)) = &self.toast {
                layers.push(views_palette::view_toast_overlay(msg, *accent, t, font));
            }
            if self.palette_open {
                layers.push(views_palette::view_command_palette(self, t, font));
            }
            if self.show_shortcuts {
                layers.push(views_palette::view_shortcuts_modal(t, font));
            }
            return if layers.len() == 1 {
                layers.pop().unwrap()
            } else {
                stack(layers).width(Length::Fill).height(Length::Fill).into()
            };
        }

        let header = self.view_header(t, font);
        let body: Element<'_, Message> = match self.tab {
            Tab::Overview => views::view_overview(self, t, font),
            Tab::Lighting => views_lighting::view_lighting(self, t, font),
            Tab::Studio => views_lighting::view_studio(self, t, font),
            Tab::Fans => views::view_fans(self, t, font),
            Tab::Automations => views_system::view_automations(self, t, font),
            Tab::Devices => views_system::view_devices(self, t, font),
            Tab::Settings => views_system::view_settings(self, t, font),
        };

        let mut content_col = column![].spacing(t.sp(12));

        // Trial profile Keep / Revert banner
        if let Some(trial) = &self.snapshot.trial {
            content_col = content_col.push(
                container(
                    row![
                        text(format!("Trying profile \"{}\"", trial.preset_name))
                            .size(13)
                            .font(theme::ui_semibold(design))
                            .color(theme::rgb(p.brass_hi)),
                        text("— previewing without overwriting your saved settings.")
                            .size(12)
                            .font(font)
                            .color(theme::rgb(p.ink_2)),
                        Space::new().width(Length::Fill),
                        button(text("Keep").size(12).font(theme::ui_semibold(design)))
                            .padding([6, 14])
                            .style(theme::btn_primary(t))
                            .on_press(Message::KeepTrial),
                        button(text("Revert").size(12).font(font))
                            .padding([6, 14])
                            .style(theme::btn_secondary(t, false))
                            .on_press(Message::RevertTrial),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                )
                .padding([10, 14])
                .style(theme::banner_style(t, p.brass)),
            );
        }

        // Global status notice banner
        if let Some(notice) = &self.snapshot.last_notice {
            content_col = content_col.push(
                container(
                    row![
                        text(notice)
                            .size(12)
                            .font(font)
                            .color(theme::rgb(p.ink))
                            .width(Length::Fill),
                        button(text("Dismiss").size(11).font(font))
                            .padding([4, 10])
                            .style(theme::btn_secondary(t, false))
                            .on_press(Message::ClearNotice),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                )
                .padding([8, 14])
                .style(theme::banner_style(t, p.ok)),
            );
        }

        content_col = content_col.push(body);

        let page_scroll = scrollable(
            container(content_col)
                .padding([t.pad(12), t.pad(24)])
                .width(Length::Fill),
        )
        .height(Length::Fill);

        let base: Element<'_, Message> = container(column![header, page_scroll].spacing(0))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::window_ground_style(t))
            .into();

        let mut layers: Vec<Element<'_, Message>> = vec![base];

        if let Some((msg, accent, _)) = &self.toast {
            layers.push(views_palette::view_toast_overlay(msg, *accent, t, font));
        }

        if self.palette_open {
            layers.push(views_palette::view_command_palette(self, t, font));
        }

        if self.show_shortcuts {
            layers.push(views_palette::view_shortcuts_modal(t, font));
        }

        if layers.len() == 1 {
            layers.pop().unwrap()
        } else {
            stack(layers).width(Length::Fill).height(Length::Fill).into()
        }
    }

    fn view_header(&self, t: DesignTokens, font: iced::Font) -> Element<'_, Message> {
        let p = t.p;
        let design = t.design;
        let mono = theme::mono_font();
        let appr = &self.snapshot.config.appearance;

        let mark = container(
            iced::widget::canvas(BrandMarkCanvas {
                palette: p,
                accent_override: t.accent_override,
            })
            .width(Length::Fixed(22.0))
            .height(Length::Fixed(22.0)),
        )
        .padding(5)
        .style(theme::brand_badge_style(t));

        let brand = row![
            mark,
            text("Airglow")
                .size(19)
                .font(theme::display_semibold(design))
                .color(theme::rgb(p.ink)),
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let mut nav = row![]
            .spacing(if t.pill_tabs { 4 } else { 9 })
            .align_y(Alignment::Center);
        for tab_item in Tab::ALL {
            let active = self.tab == tab_item;
            let tab_font = if active {
                theme::ui_semibold(design)
            } else {
                font
            };

            if t.pill_tabs {
                nav = nav.push(
                    button(text(tab_item.label()).size(13).font(tab_font))
                        .padding([6, 12])
                        .style(theme::nav_tab_btn(t, active))
                        .on_press(Message::SelectTab(tab_item)),
                );
            } else {
                let underline =
                    container(Space::new().width(Length::Fill).height(2)).style(move |_| {
                        iced::widget::container::Style {
                            background: Some(iced::Background::Color(if active {
                                theme::rgb(p.brass)
                            } else {
                                iced::Color::TRANSPARENT
                            })),
                            ..Default::default()
                        }
                    });
                nav = nav.push(
                    button(
                        column![text(tab_item.label()).size(13.5).font(tab_font), underline]
                            .spacing(4)
                            .width(Length::Shrink),
                    )
                    .padding([4, 6])
                    .style(theme::nav_tab_btn(t, active))
                    .on_press(Message::SelectTab(tab_item)),
                );
            }
        }

        // Compact Mini-Mode toggle pill (`Ctrl+M`)
        let compact_pill = button(
            text("⤡")
                .size(12.5)
                .font(font)
                .color(theme::rgb(p.ink_2)),
        )
        .padding([5, 9])
        .style(theme::shortcut_badge_btn(t, self.compact_window))
        .on_press(Message::ToggleCompactMode);

        // Quick Command Palette pill (`Ctrl+K`)
        let cmd_pill = button(
            row![
                text("⌘")
                    .size(11.5)
                    .font(theme::ui_semibold(design))
                    .color(theme::rgb(p.brass_hi)),
                text("Ctrl+K").size(11).font(mono),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
        )
        .padding([6, 10])
        .style(theme::shortcut_badge_btn(t, self.palette_open))
        .on_press(Message::ToggleCommandPalette);

        // Dynamic Pinned Sensors Pill
        let unit = self.snapshot.config.settings.temp_unit;
        let s = &self.snapshot.sensors;
        let pill_muted = p.ink_3;
        let mut pin_row = row![].spacing(5).align_y(Alignment::Center);
        let mut pinned_count = 0usize;

        for pin in &appr.header_pins {
            if pinned_count > 0 {
                pin_row = pin_row.push(Space::new().width(8));
            }
            match pin.as_str() {
                "cpu_temp" => {
                    let normal = if p.is_light { p.ink } else { t.data_primary };
                    let col = views::thermal_temp_color(s.cpu_temp, t, normal);
                    pin_row = pin_row
                        .push(text("CPU").size(10.5).font(font).color(theme::rgb(pill_muted)))
                        .push(
                            text(sensors::format_temp(s.cpu_temp, unit))
                                .size(12)
                                .font(mono)
                                .color(theme::rgb(col)),
                        );
                    pinned_count += 1;
                }
                "gpu_temp" => {
                    let normal = if p.is_light { p.ink_2 } else { t.data_secondary };
                    let col = views::thermal_temp_color(s.gpu_temp, t, normal);
                    pin_row = pin_row
                        .push(text("GPU").size(10.5).font(font).color(theme::rgb(pill_muted)))
                        .push(
                            text(sensors::format_temp(s.gpu_temp, unit))
                                .size(12)
                                .font(mono)
                                .color(theme::rgb(col)),
                        );
                    pinned_count += 1;
                }
                "total_power" => {
                    let w = s
                        .total_power_w
                        .map(|v| format!("{v:.0}W"))
                        .unwrap_or_else(|| "—".to_string());
                    pin_row = pin_row
                        .push(text("PWR").size(10.5).font(font).color(theme::rgb(pill_muted)))
                        .push(text(w).size(12).font(mono).color(theme::rgb(p.brass_hi)));
                    pinned_count += 1;
                }
                "fan_duty" => {
                    pin_row = pin_row
                        .push(text("FAN").size(10.5).font(font).color(theme::rgb(pill_muted)))
                        .push(
                            text(format!("{}%", self.snapshot.fan_duty))
                                .size(12)
                                .font(mono)
                                .color(theme::rgb(t.data_primary)),
                        );
                    pinned_count += 1;
                }
                "ram_load" => {
                    pin_row = pin_row
                        .push(text("RAM").size(10.5).font(font).color(theme::rgb(pill_muted)))
                        .push(
                            text(format!("{:.0}%", s.ram_load))
                                .size(12)
                                .font(mono)
                                .color(theme::rgb(t.pink_rose)),
                        );
                    pinned_count += 1;
                }
                _ => {}
            }
        }
        if pinned_count == 0 {
            pin_row = pin_row
                .push(text("CPU").size(10.5).font(font).color(theme::rgb(pill_muted)))
                .push(
                    text(sensors::format_temp(s.cpu_temp, unit))
                        .size(12)
                        .font(mono)
                        .color(theme::rgb(t.data_primary)),
                );
        }

        let temp_pill = container(pin_row)
            .padding([6, 12])
            .style(theme::ambient_pill_style(t));

        let power_on = self.snapshot.effective_scene.on;
        let power_btn = button(
            container(
                text(if power_on { "⏻" } else { "○" })
                    .size(15)
                    .font(font),
            )
            .center_x(Length::Fill)
            .center_y(Length::Fill),
        )
        .width(Length::Fixed(36.0))
        .height(Length::Fixed(36.0))
        .padding(0)
        .style(theme::power_circle_btn(t, power_on))
        .on_press(Message::TogglePower);

        let bar = container(
            row![
                brand,
                Space::new().width(14),
                nav,
                Space::new().width(Length::Fill),
                compact_pill,
                cmd_pill,
                temp_pill,
                power_btn,
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding([8, 22])
        .width(Length::Fill)
        .style(theme::appbar_style(t));

        let glow_line = iced::widget::canvas(HeaderGlowCanvas {
            palette: p,
            accent_override: t.accent_override,
            ambient_rgb: self.snapshot.average_rgb,
            lights_on: power_on,
            anim_t: self.anim_t,
        })
        .width(Length::Fill)
        .height(Length::Fixed(2.0));

        column![bar, glow_line].spacing(0).into()
    }
}

fn parse_tab_name(raw: &str) -> Option<Tab> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "overview" | "home" => Some(Tab::Overview),
        "lighting" | "lights" => Some(Tab::Lighting),
        "studio" => Some(Tab::Studio),
        "fans" | "cooling" => Some(Tab::Fans),
        "automations" | "rules" => Some(Tab::Automations),
        "devices" | "hardware" => Some(Tab::Devices),
        "settings" | "appearance" => Some(Tab::Settings),
        _ => None,
    }
}

struct LaunchFlags {
    tab: Tab,
    palette_open: bool,
    palette_query: String,
    customize_open: bool,
    compact_window: bool,
    show_shortcuts: bool,
}

fn parse_launch_flags_from<I: IntoIterator<Item = String>>(iter: I) -> LaunchFlags {
    let mut flags = LaunchFlags {
        tab: Tab::Overview,
        palette_open: false,
        palette_query: String::new(),
        customize_open: false,
        compact_window: false,
        show_shortcuts: false,
    };
    let mut args = iter.into_iter();
    while let Some(arg) = args.next() {
        if let Some(val) = arg.strip_prefix("--tab=")
            && let Some(tab) = parse_tab_name(val)
        {
            flags.tab = tab;
        } else if arg == "--tab"
            && let Some(val) = args.next()
            && let Some(tab) = parse_tab_name(&val)
        {
            flags.tab = tab;
        } else if arg == "--palette" {
            flags.palette_open = true;
        } else if let Some(val) = arg.strip_prefix("--palette-query=") {
            flags.palette_open = true;
            flags.palette_query = val.to_string();
        } else if arg == "--palette-query"
            && let Some(val) = args.next()
        {
            flags.palette_open = true;
            flags.palette_query = val;
        } else if arg == "--customize" {
            flags.customize_open = true;
        } else if arg == "--compact" {
            flags.compact_window = true;
        } else if arg == "--shortcuts" {
            flags.show_shortcuts = true;
        }
    }
    flags
}

fn parse_launch_flags() -> LaunchFlags {
    let mut flags = parse_launch_flags_from(std::env::args().skip(1));
    if flags.tab == Tab::Overview
        && let Ok(env_tab) = std::env::var("AIRGLOW_TAB")
        && let Some(tab) = parse_tab_name(&env_tab)
    {
        flags.tab = tab;
    }
    if !flags.palette_open
        && std::env::var("AIRGLOW_PALETTE")
            .ok()
            .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
    {
        flags.palette_open = true;
    }
    if flags.palette_query.is_empty()
        && let Ok(q) = std::env::var("AIRGLOW_PALETTE_QUERY")
        && !q.is_empty()
    {
        flags.palette_open = true;
        flags.palette_query = q;
    }
    if !flags.customize_open
        && std::env::var("AIRGLOW_CUSTOMIZE")
            .ok()
            .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
    {
        flags.customize_open = true;
    }
    if !flags.compact_window
        && std::env::var("AIRGLOW_COMPACT")
            .ok()
            .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
    {
        flags.compact_window = true;
    }
    flags
}

fn parse_hhmm(raw: &str) -> Option<(u8, u8)> {
    let (h, m) = raw.trim().split_once(':')?;
    let hh = h.trim().parse::<u8>().ok()?;
    let mm = m.trim().parse::<u8>().ok()?;
    if hh < 24 && mm < 60 {
        Some((hh, mm))
    } else {
        None
    }
}

fn open_main_window(compact: bool) -> Task<Message> {
    let rgba = brand::app_icon_rgba(64);
    let icon = window::icon::from_rgba(rgba, 64, 64).ok();
    let size = if compact {
        Size::new(430.0, 404.0)
    } else {
        Size::new(1340.0, 920.0)
    };
    let (_id, task) = window::open(window::Settings {
        size,
        min_size: Some(Size::new(390.0, 370.0)),
        icon,
        exit_on_close_request: false,
        ..window::Settings::default()
    });
    task.map(Message::WindowOpened)
}

#[cfg(test)]
mod tests {
    use super::{PulseSpan, Tab, parse_hhmm, parse_launch_flags_from, parse_tab_name};

    #[test]
    fn parses_cli_overlay_flags() {
        let f = parse_launch_flags_from(
            [
                "--tab=settings",
                "--palette",
                "--palette-query=aurora",
                "--customize",
                "--compact",
            ]
            .into_iter()
            .map(String::from),
        );
        assert_eq!(f.tab, Tab::Settings);
        assert!(f.palette_open);
        assert_eq!(f.palette_query, "aurora");
        assert!(f.customize_open);
        assert!(f.compact_window);
    }

    #[test]
    fn parses_valid_and_rejects_invalid_hhmm() {
        assert_eq!(parse_hhmm("23:00"), Some((23, 0)));
        assert_eq!(parse_hhmm("07:45"), Some((7, 45)));
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(parse_hhmm("12:60"), None);
        assert_eq!(parse_hhmm("invalid"), None);
    }

    #[test]
    fn parses_cli_tab_names() {
        assert_eq!(parse_tab_name("overview"), Some(Tab::Overview));
        assert_eq!(parse_tab_name("Lighting"), Some(Tab::Lighting));
        assert_eq!(parse_tab_name("studio"), Some(Tab::Studio));
        assert_eq!(parse_tab_name("fans"), Some(Tab::Fans));
        assert_eq!(parse_tab_name("automations"), Some(Tab::Automations));
        assert_eq!(parse_tab_name("devices"), Some(Tab::Devices));
        assert_eq!(parse_tab_name("settings"), Some(Tab::Settings));
        assert_eq!(parse_tab_name("unknown"), None);
    }

    #[test]
    fn pulse_span_maps_minutes_and_sample_counts() {
        assert_eq!(PulseSpan::from_mins(1), PulseSpan::OneMin);
        assert_eq!(PulseSpan::from_mins(5), PulseSpan::FiveMin);
        assert_eq!(PulseSpan::from_mins(10), PulseSpan::TenMin);
        assert_eq!(PulseSpan::OneMin.max_samples(), 30);
        assert_eq!(PulseSpan::FiveMin.max_samples(), 150);
        assert_eq!(PulseSpan::TenMin.max_samples(), 300);
    }
}
