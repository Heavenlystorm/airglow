#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Airglow 2.0 (Iced 0.14 + pure-Rust hardware core + Tauri tray-icon).

mod bento_grid;
mod brand;
mod canvas_ambient;
mod canvas_colorwheel;
mod canvas_curves;
mod canvas_stage;
mod canvas_widgets;
mod platform;
mod theme;
mod tray;
mod views;
mod views_lighting;
mod views_palette;
mod views_system;

use std::sync::Mutex;
use std::time::{Duration, Instant};

use airglow_core::automations::{ALL_DAYS_MASK, Action, AutomationRule, Trigger};
use airglow_core::config::{
    ACCENT_SWATCHES, CurvePoint, DesignId, FanChannelMode, OverviewTileKind, RigStyle, TempSource,
    TempUnit, UpdateChannel, WindowState, ZoneConfig, ZoneOverride, format_hex_rgb, parse_hex_rgb,
};
use airglow_core::effects::{EffectId, HARMONIES, Layer, LayerKind, LayerStack, SceneParams};
use airglow_core::engine::{EngineCommand, EngineHandle, EngineSnapshot};
use airglow_core::sensors;
use airglow_core::updates::UpdateStatus;
use iced::widget::{
    Space, button, column, container, image, operation, row, scrollable, stack, text,
};
use iced::{
    Alignment, Element, Event, Length, Point, Size, Subscription, Task, Theme, keyboard, time,
    window,
};

use crate::canvas_ambient::{UiIcon, WindowBackdropCanvas, icon_canvas};
use crate::canvas_stage::StageMessage;
use crate::canvas_widgets::{BrandMarkCanvas, HeaderGlowCanvas};
use crate::platform::{GlobalHotkeyAction, GlobalHotkeyManager, SingleInstanceGuard, SingleInstanceResult};
use crate::theme::DesignTokens;
use crate::tray::{TrayAction, TrayController, TrayMenuState};
use crate::views_palette::{PALETTE_INPUT_ID, filtered_palette_items};

static SINGLE_INSTANCE: Mutex<Option<SingleInstanceGuard>> = Mutex::new(None);

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

    pub fn as_key(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Lighting => "lighting",
            Self::Studio => "studio",
            Self::Fans => "fans",
            Self::Automations => "automations",
            Self::Devices => "devices",
            Self::Settings => "settings",
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSection {
    Appearance,
    Updates,
    General,
    LightingCooling,
    SensorsDevices,
    BackupDiagnostics,
}

impl SettingsSection {
    pub const ALL: [SettingsSection; 6] = [
        Self::Appearance,
        Self::Updates,
        Self::General,
        Self::LightingCooling,
        Self::SensorsDevices,
        Self::BackupDiagnostics,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Updates => "Updates & About",
            Self::General => "General & Telemetry",
            Self::LightingCooling => "Lighting & Cooling",
            Self::SensorsDevices => "Sensors & Devices",
            Self::BackupDiagnostics => "Backup & Diagnostics",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Appearance => "✦",
            Self::Updates => "↻",
            Self::General => "⚙",
            Self::LightingCooling => "☼",
            Self::SensorsDevices => "◈",
            Self::BackupDiagnostics => "⛁",
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            Self::Appearance => "Designs, accents, looks & viewport",
            Self::Updates => "Version, updates & shortcuts",
            Self::General => "Units, tariff, startup & pulse",
            Self::LightingCooling => "Sleep/lock, hysteresis & fans",
            Self::SensorsDevices => "LHM, OpenRGB & conflict guard",
            Self::BackupDiagnostics => "Rig backup, folder & reset",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddTileTab {
    Controls,
    Sensors,
    LiveLhm,
}

#[derive(Debug, Clone)]
pub enum Message {
    Noop,
    Tick,
    WindowOpened(window::Id),
    WindowResized(window::Id, Size),
    WindowMoved(Point),
    WindowFocusChanged(bool),
    CloseRequested(window::Id),
    SelectTab(Tab),
    SetPulseSpan(PulseSpan),
    ClearNotice,
    DismissToast,
    CloseConflictingApp(String),

    // Fullscreen, Viewport, UI Scale & Theater Rig
    ToggleFullscreen,
    ToggleTheaterRig,
    SetFullscreenFillWidth(bool),
    SetUiScalePct(u16),
    CycleUiScale,
    StepUiScale(i8),

    // Overview Tile Workshop & Workspaces
    ToggleEditTilesMode,
    ToggleAddTileCatalog,
    OpenAddTileFromPalette,
    SetAddTileTab(AddTileTab),
    SensorSearchChanged(String),
    AddOverviewTile(OverviewTileKind),
    RemoveOverviewTile(String),
    MoveOverviewTile(String, isize),
    ReorderOverviewTile(String, usize),
    ResizeOverviewTile(String, u8, u8),
    CycleOverviewTileSpan(String),
    ToggleOverviewTileTall(String),
    CycleOverviewTileStyle(String),
    ResetActiveWorkspace,
    ToggleWorkspacesBar,
    SwitchWorkspace(String),
    ToggleWorkspaceCreate,
    WorkspaceNameDraftChanged(String),
    WorkspaceTemplateSelected(Option<String>),
    CreateWorkspace,
    ToggleWorkspaceRename,
    WorkspaceRenameDraftChanged(String),
    ConfirmWorkspaceRename,
    DeleteWorkspace(String),

    // Desk Display Focus Timer
    ToggleFocusTimer,
    ResetFocusTimer,
    SetFocusTimerMins(u16),

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
    ToggleOverviewStageAnimation,
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
    SelectSettingsSection(SettingsSection),
    JumpSettingsSection(SettingsSection),
    CheckForUpdates,
    DownloadAndStageUpdate,
    ApplyStagedUpdateAndRestart,
    SetUpdateChannel(UpdateChannel),
    ToggleAutoCheckUpdates(bool),
    OpenUrl(String),
    OpenConfigFolder,
    ExportRigBundle,
    ImportBackupPathChanged(String),
    ImportRigBundle,
    BrowseBackupFile,
    BackupFilePicked(Option<String>),
    ToggleMinimizeToTray(bool),
    ToggleGlobalHotkeys(bool),
    ToggleLhmFanControl(bool),
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
    single_instance: Option<SingleInstanceGuard>,
    hotkeys: Option<GlobalHotkeyManager>,
    pub main_window: Option<window::Id>,
    pub tab: Tab,
    pub pulse_span: PulseSpan,

    // Live Viewport, Fullscreen, Auto-HiDPI Scale & Theater Rig
    pub window_size: Size,
    window_pos: Option<Point>,
    saved_window_size_phys: Option<(f32, f32)>,
    window_state_dirty_at: Option<Instant>,
    window_focused: bool,
    pub approx_physical_width: f32,
    pub auto_scale: f32,
    pub applied_scale: f32,
    pub is_fullscreen: bool,
    pub theater_rig: bool,
    tray_retry_at: Option<Instant>,
    browsing_backup_file: bool,
    start_fullscreen: bool,
    start_maximized: bool,

    // Adaptive GPU animation clock & cached bundled Firefly artworks
    pub boot_instant: Instant,
    pub anim_t: f32,
    pub moth_art: image::Handle,
    pub dusk_art: image::Handle,
    pub clock_hhmm: String,
    pub clock_date: String,
    pub focus_timer_secs: u32,
    pub focus_timer_running: bool,
    focus_timer_last_tick: Instant,

    // Overview Tile Workshop & Workspaces
    pub edit_tiles_mode: bool,
    pub workspaces_bar_open: bool,
    pub add_tile_open: bool,
    pub add_tile_tab: AddTileTab,
    pub sensor_search_query: String,
    pub workspace_create_open: bool,
    pub workspace_name_draft: String,
    pub workspace_template_draft: Option<String>,
    pub workspace_rename_open: bool,
    pub workspace_rename_draft: String,

    // 2-Pane Settings Navigation & Backup
    pub settings_section: SettingsSection,
    pub import_backup_path_draft: String,

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
    let launch = parse_launch_flags();
    match SingleInstanceGuard::acquire(!launch.background) {
        SingleInstanceResult::Primary(guard) => {
            if let Ok(mut slot) = SINGLE_INSTANCE.lock() {
                *slot = Some(guard);
            }
        }
        SingleInstanceResult::AlreadyRunning => {
            return Ok(());
        }
    }

    iced::daemon(AirglowApp::boot, AirglowApp::update, AirglowApp::view)
        .title(AirglowApp::title)
        .theme(AirglowApp::theme)
        .scale_factor(AirglowApp::scale_factor)
        .subscription(AirglowApp::subscription)
        .run()
}

/// Resolves responsive Auto UI scale factor from `approx_physical_width` with a 120px hysteresis
/// band around each tier threshold so `window::resize_events` never oscillate.
pub fn resolve_auto_scale_with_hysteresis(approx_phys_w: f32, current_auto_scale: f32) -> f32 {
    const BAND: f32 = 120.0;
    if (current_auto_scale - 1.50).abs() < 0.02 && approx_phys_w >= 3250.0 - BAND {
        return 1.50;
    }
    if (current_auto_scale - 1.25).abs() < 0.02
        && (2200.0 - BAND..3250.0).contains(&approx_phys_w)
    {
        return 1.25;
    }
    if (current_auto_scale - 1.10).abs() < 0.02
        && (1820.0 - BAND..2200.0).contains(&approx_phys_w)
    {
        return 1.10;
    }
    airglow_core::config::AppearanceConfig {
        ui_scale_pct: 0,
        ..Default::default()
    }
    .resolve_scale_factor(approx_phys_w)
}

impl AirglowApp {
    fn boot() -> (Self, Task<Message>) {
        let single_instance = SINGLE_INSTANCE.lock().ok().and_then(|mut s| s.take());
        let engine = EngineHandle::start();
        let snapshot = engine.rx.borrow().clone();
        let launch = parse_launch_flags();

        let presets: Vec<(String, String)> = snapshot
            .config
            .presets
            .iter()
            .map(|p| (p.id.clone(), p.name.clone()))
            .collect();
        let tray_state = TrayMenuState {
            lights_on: snapshot.effective_scene.on,
            brightness: snapshot.config.scene.brightness,
            active_preset: snapshot.config.active_preset.as_deref(),
            active_effect: snapshot.effective_scene.effect,
            active_curve: snapshot.effective_curve.as_str(),
            overview_stage_animated: snapshot.config.appearance.overview_stage_animated,
        };
        let tray = TrayController::new(&presets, &tray_state);
        let tray_retry_at = if tray.is_some() {
            None
        } else {
            Some(Instant::now())
        };
        let hotkeys = if snapshot.config.settings.global_hotkeys_enabled {
            Some(GlobalHotkeyManager::start())
        } else {
            None
        };
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

        let initial_tab = if launch.tab_explicit {
            launch.tab
        } else {
            snapshot
                .config
                .window_state
                .last_tab
                .as_deref()
                .and_then(parse_tab_name)
                .unwrap_or(launch.tab)
        };
        if initial_tab == Tab::Studio {
            engine.send(EngineCommand::SetStudioPreviewActive(true));
        }
        if launch.check_updates {
            engine.send(EngineCommand::CheckForUpdates);
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
        let (clock_hhmm, clock_date) = local_clock_strings();

        let ws = snapshot.config.window_state.clone().normalized();
        let saved_window_size_phys = match (ws.width, ws.height) {
            (Some(w), Some(h)) => Some((w, h)),
            _ => None,
        };
        let window_pos = match (ws.x, ws.y) {
            (Some(x), Some(y)) => Some(Point::new(x as f32, y as f32)),
            _ => None,
        };
        let approx_physical_width = saved_window_size_phys.map(|(w, _)| w).unwrap_or(1440.0);
        let approx_physical_height = saved_window_size_phys.map(|(_, h)| h).unwrap_or(900.0);
        let auto_scale = resolve_auto_scale_with_hysteresis(approx_physical_width, 1.0);
        let applied_scale = if compact_window {
            1.0
        } else {
            snapshot.config.appearance.resolve_scale_factor(approx_physical_width)
        };
        let initial_window_size = if compact_window {
            Size::new(430.0, 404.0)
        } else {
            Size::new(
                approx_physical_width / applied_scale,
                approx_physical_height / applied_scale,
            )
        };
        let initial_position = if compact_window {
            window::Position::Default
        } else if let Some(pt) = window_pos {
            window::Position::Specific(pt)
        } else {
            window::Position::Default
        };

        let app = Self {
            engine,
            snapshot,
            tray,
            single_instance,
            hotkeys,
            main_window: None,
            tab: initial_tab,
            pulse_span,
            window_size: initial_window_size,
            window_pos,
            saved_window_size_phys,
            window_state_dirty_at: None,
            window_focused: true,
            approx_physical_width,
            auto_scale,
            applied_scale,
            is_fullscreen: launch.fullscreen,
            theater_rig: launch.theater_rig,
            tray_retry_at,
            browsing_backup_file: false,
            start_fullscreen: launch.fullscreen,
            start_maximized: launch.maximized || ws.maximized,
            boot_instant: Instant::now(),
            anim_t: 0.0,
            moth_art: image::Handle::from_bytes(FIREFLY_MOTH_PNG),
            dusk_art: image::Handle::from_bytes(FIREFLY_DUSK_PNG),
            clock_hhmm,
            clock_date,
            focus_timer_secs: 25 * 60,
            focus_timer_running: false,
            focus_timer_last_tick: Instant::now(),
            edit_tiles_mode: launch.edit_tiles,
            workspaces_bar_open: false,
            add_tile_open: launch.add_tile,
            add_tile_tab: AddTileTab::Controls,
            sensor_search_query: String::new(),
            workspace_create_open: false,
            workspace_name_draft: String::new(),
            workspace_template_draft: None,
            workspace_rename_open: false,
            workspace_rename_draft: String::new(),
            settings_section: launch.settings_section,
            import_backup_path_draft: String::new(),
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

        let startup_task = if launch.background && app.tray.is_some() {
            Task::none()
        } else {
            open_main_window(compact_window, initial_window_size, initial_position)
        };
        (app, startup_task)
    }

    pub fn effective_scale_factor(&self) -> f32 {
        if self.compact_window {
            return 1.0;
        }
        let pct = self.snapshot.config.appearance.ui_scale_pct;
        if pct > 0 {
            (pct as f32 / 100.0).clamp(0.85, 1.50)
        } else {
            self.auto_scale
        }
    }

    fn scale_factor(&self, _window: window::Id) -> f32 {
        self.effective_scale_factor()
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
        let overview_static = self.tab == Tab::Overview
            && !self.theater_rig
            && !self.compact_window
            && !self.snapshot.config.appearance.overview_stage_animated;
        let tick_ms = if self.main_window.is_none() {
            300
        } else if !self.window_focused || overview_static {
            250
        } else if !self.theater_rig
            && !self.compact_window
            && (matches!(self.tab, Tab::Automations | Tab::Devices)
                || (self.tab == Tab::Settings
                    && self.settings_section != SettingsSection::Appearance))
        {
            200
        } else {
            match self.snapshot.config.appearance.ui_fps_cap {
                15 => 66,
                60 => 16,
                _ => 33,
            }
        };
        Subscription::batch([
            time::every(Duration::from_millis(tick_ms)).map(|_| Message::Tick),
            window::close_requests().map(Message::CloseRequested),
            window::resize_events().map(|(id, size)| Message::WindowResized(id, size)),
            iced::event::listen_with(|ev, status, _win| match ev {
                Event::Window(window::Event::Focused) => Some(Message::WindowFocusChanged(true)),
                Event::Window(window::Event::Unfocused) => Some(Message::WindowFocusChanged(false)),
                Event::Window(window::Event::Moved(pos)) => Some(Message::WindowMoved(pos)),
                Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                    match key {
                        keyboard::Key::Named(keyboard::key::Named::F11) => {
                            if modifiers.shift() {
                                Some(Message::ToggleTheaterRig)
                            } else {
                                Some(Message::ToggleFullscreen)
                            }
                        }
                        keyboard::Key::Character(ref ch) if modifiers.command() => {
                            match ch.to_ascii_lowercase().as_str() {
                                "k" => Some(Message::ToggleCommandPalette),
                                "m" => Some(Message::ToggleCompactMode),
                                "/" => Some(Message::ToggleShortcuts),
                                "+" | "=" => Some(Message::StepUiScale(1)),
                                "-" | "_" => Some(Message::StepUiScale(-1)),
                                "0" => Some(Message::StepUiScale(0)),
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
        let caption = if t.is_firefly() {
            [0x1f, 0x19, 0x2c]
        } else {
            theme::hex_to_rgb_arr(t.appbar_mid)
        };
        let ink = theme::hex_to_rgb_arr(t.p.ink);
        let border = if t.is_firefly() {
            [0x2d, 0x25, 0x3f]
        } else {
            theme::hex_to_rgb_arr(t.appbar_border)
        };
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

    fn ensure_tray(&mut self) {
        if self.tray.is_some() {
            self.tray_retry_at = None;
            return;
        }
        let presets: Vec<(String, String)> = self
            .snapshot
            .config
            .presets
            .iter()
            .map(|p| (p.id.clone(), p.name.clone()))
            .collect();
        let tray_state = TrayMenuState {
            lights_on: self.snapshot.effective_scene.on,
            brightness: self.snapshot.config.scene.brightness,
            active_preset: self.snapshot.config.active_preset.as_deref(),
            active_effect: self.snapshot.effective_scene.effect,
            active_curve: self.snapshot.effective_curve.as_str(),
            overview_stage_animated: self.snapshot.config.appearance.overview_stage_animated,
        };
        if let Some(tray) = TrayController::new(&presets, &tray_state) {
            self.tray = Some(tray);
            self.tray_retry_at = None;
        } else {
            self.tray_retry_at = Some(Instant::now());
        }
    }

    fn current_window_position(&self) -> window::Position {
        if self.compact_window {
            window::Position::Default
        } else if let Some(pt) = self.window_pos {
            window::Position::Specific(pt)
        } else {
            window::Position::Default
        }
    }

    fn current_restore_size(&self) -> Size {
        if self.compact_window {
            Size::new(430.0, 404.0)
        } else if let Some((w, h)) = self.saved_window_size_phys {
            let s = self.effective_scale_factor();
            Size::new(w / s, h / s)
        } else {
            let s = self.effective_scale_factor();
            Size::new(1440.0 / s, 900.0 / s)
        }
    }

    fn open_or_restore_main_window(&mut self) -> Task<Message> {
        if let Some(id) = self.main_window {
            return Task::batch([window::minimize(id, false), window::gain_focus(id)]);
        }
        let sz = self.current_restore_size();
        let pos = self.current_window_position();
        open_main_window(self.compact_window, sz, pos)
    }

    fn flush_window_state(&mut self) {
        self.window_state_dirty_at = None;
        let next = WindowState {
            width: self.saved_window_size_phys.map(|(w, _)| w),
            height: self.saved_window_size_phys.map(|(_, h)| h),
            x: self.window_pos.map(|pt| pt.x.round() as i32),
            y: self.window_pos.map(|pt| pt.y.round() as i32),
            maximized: self.snapshot.config.window_state.maximized,
            last_tab: Some(self.tab.as_key().to_string()),
        }
        .normalized();
        if next != self.snapshot.config.window_state {
            self.snapshot.config.window_state = next.clone();
            self.engine.send(EngineCommand::UpdateWindowState(next));
        }
    }

    fn shutdown_and_exit(&mut self) -> Task<Message> {
        self.flush_window_state();
        self.hotkeys = None;
        self.single_instance = None;
        self.engine.shutdown();
        iced::exit()
    }

    fn cycle_profile(&mut self) {
        let presets = &self.snapshot.config.presets;
        if presets.is_empty() {
            return;
        }
        let next_idx = self
            .snapshot
            .config
            .active_preset
            .as_deref()
            .and_then(|cur| presets.iter().position(|p| p.id == cur))
            .map(|i| (i + 1) % presets.len())
            .unwrap_or(0);
        let pid = presets[next_idx].id.clone();
        let pname = presets[next_idx].name.clone();
        self.snapshot.config.active_preset = Some(pid.clone());
        self.engine.send(EngineCommand::ApplyPreset(pid));
        self.show_toast(format!("Profile: \"{pname}\""), self.current_accent_hex());
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Noop => Task::none(),
            Message::Tick => {
                self.anim_t = self.boot_instant.elapsed().as_secs_f32();
                if self.focus_timer_last_tick.elapsed() >= Duration::from_secs(1) {
                    let secs_elapsed = self.focus_timer_last_tick.elapsed().as_secs() as u32;
                    self.focus_timer_last_tick = Instant::now();
                    let (hhmm, date) = local_clock_strings();
                    self.clock_hhmm = hhmm;
                    self.clock_date = date;
                    if self.focus_timer_running {
                        if self.focus_timer_secs <= secs_elapsed {
                            self.focus_timer_secs = 0;
                            self.focus_timer_running = false;
                            self.show_toast("Focus session complete!", self.current_accent_hex());
                        } else {
                            self.focus_timer_secs -= secs_elapsed;
                        }
                    }
                }
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
                    let prev_scale = self.applied_scale.max(0.5);
                    self.snapshot = self.engine.rx.borrow_and_update().clone();
                    let next_scale = self.effective_scale_factor();
                    if (prev_scale - next_scale).abs() > 0.001 && !self.compact_window {
                        self.applied_scale = next_scale;
                        self.window_size = Size::new(
                            self.window_size.width * prev_scale / next_scale,
                            self.window_size.height * prev_scale / next_scale,
                        );
                    }
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
                if self
                    .single_instance
                    .as_ref()
                    .is_some_and(|g| g.poll_show_requested())
                {
                    return self.open_or_restore_main_window();
                }

                let hk_actions = self
                    .hotkeys
                    .as_ref()
                    .map(|h| h.poll_actions())
                    .unwrap_or_default();
                for act in hk_actions {
                    match act {
                        GlobalHotkeyAction::ToggleLights => {
                            let next = !self.snapshot.config.scene.on;
                            self.snapshot.config.scene.on = next;
                            self.engine.send(EngineCommand::SetPower(next));
                        }
                        GlobalHotkeyAction::CycleProfile => {
                            self.cycle_profile();
                        }
                        GlobalHotkeyAction::OpenWindow => {
                            return self.open_or_restore_main_window();
                        }
                    }
                }

                if self
                    .window_state_dirty_at
                    .is_some_and(|t| t.elapsed() >= Duration::from_millis(800))
                {
                    self.flush_window_state();
                }

                if self.tray.is_none()
                    && self
                        .tray_retry_at
                        .is_none_or(|t| t.elapsed() >= Duration::from_secs(2))
                {
                    self.ensure_tray();
                }
                let tray_actions = if let Some(tray) = self.tray.as_mut() {
                    let presets: Vec<(String, String)> = self
                        .snapshot
                        .config
                        .presets
                        .iter()
                        .map(|p| (p.id.clone(), p.name.clone()))
                        .collect();
                    let tray_state = TrayMenuState {
                        lights_on: self.snapshot.effective_scene.on,
                        brightness: self.snapshot.config.scene.brightness,
                        active_preset: self.snapshot.config.active_preset.as_deref(),
                        active_effect: self.snapshot.effective_scene.effect,
                        active_curve: self.snapshot.effective_curve.as_str(),
                        overview_stage_animated: self
                            .snapshot
                            .config
                            .appearance
                            .overview_stage_animated,
                    };
                    tray.sync_state(&presets, &tray_state);

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
                    tray.poll_actions()
                } else {
                    Vec::new()
                };
                for action in tray_actions {
                    match action {
                        TrayAction::OpenWindow => {
                            return self.open_or_restore_main_window();
                        }
                        TrayAction::ToggleCompactMode => {
                            if let Some(id) = self.main_window {
                                let prev_scale = self.applied_scale.max(0.5);
                                self.compact_window = !self.compact_window;
                                self.palette_open = false;
                                let next_scale = self.effective_scale_factor();
                                self.applied_scale = next_scale;
                                let target = if self.compact_window {
                                    Size::new(430.0, 404.0)
                                } else {
                                    self.current_restore_size()
                                };
                                let _ = prev_scale;
                                self.window_size = target;
                                self.titlebar_applied = false;
                                self.sync_window_titlebar();
                                return Task::batch([
                                    window::resize(id, target),
                                    window::gain_focus(id),
                                ]);
                            }
                            self.compact_window = true;
                            self.applied_scale = 1.0;
                            return open_main_window(
                                true,
                                Size::new(430.0, 404.0),
                                window::Position::Default,
                            );
                        }
                        TrayAction::ToggleLights => {
                            let next = !self.snapshot.config.scene.on;
                            self.snapshot.config.scene.on = next;
                            self.engine.send(EngineCommand::SetPower(next));
                        }
                        TrayAction::ToggleOverviewStageAnimation => {
                            let mut appr = self.snapshot.config.appearance.clone();
                            appr.overview_stage_animated = !appr.overview_stage_animated;
                            self.send_appearance(appr);
                        }
                        TrayAction::SetBrightness(val) => {
                            self.engine.send(EngineCommand::SetBrightness(val));
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
                            return self.shutdown_and_exit();
                        }
                    }
                }
                Task::none()
            }
            Message::WindowOpened(id) => {
                self.main_window = Some(id);
                self.titlebar_applied = false;
                self.sync_window_titlebar();
                if self.tray.is_none() {
                    self.ensure_tray();
                }
                if self.start_fullscreen && !self.compact_window {
                    self.start_fullscreen = false;
                    self.is_fullscreen = true;
                    return window::set_mode(id, window::Mode::Fullscreen);
                }
                if self.start_maximized && !self.compact_window {
                    self.start_maximized = false;
                    return window::maximize(id, true);
                }
                Task::none()
            }
            Message::WindowResized(id, size) => {
                if self.main_window == Some(id) || self.main_window.is_none() {
                    if self.compact_window {
                        self.window_size = size;
                        self.applied_scale = 1.0;
                    } else {
                        let prev_scale = self.applied_scale.max(0.5);
                        let phys_w = (size.width * prev_scale).max(400.0);
                        self.approx_physical_width = phys_w;
                        self.auto_scale =
                            resolve_auto_scale_with_hysteresis(phys_w, self.auto_scale);
                        let next_scale = self.effective_scale_factor();
                        self.applied_scale = next_scale;
                        self.window_size = Size::new(
                            size.width * prev_scale / next_scale,
                            size.height * prev_scale / next_scale,
                        );
                        if !self.is_fullscreen && size.width >= 200.0 && size.height >= 150.0 {
                            self.saved_window_size_phys =
                                Some((size.width * next_scale, size.height * next_scale));
                            self.window_state_dirty_at = Some(Instant::now());
                        }
                    }
                }
                Task::none()
            }
            Message::WindowMoved(pos) => {
                if !self.compact_window
                    && !self.is_fullscreen
                    && pos.x > -10000.0
                    && pos.y > -10000.0
                {
                    self.window_pos = Some(pos);
                    self.window_state_dirty_at = Some(Instant::now());
                }
                Task::none()
            }
            Message::WindowFocusChanged(focused) => {
                self.window_focused = focused;
                Task::none()
            }
            Message::CloseRequested(id) => {
                self.flush_window_state();
                if self.snapshot.config.settings.minimize_to_tray_on_close {
                    if self.tray.is_none() {
                        self.ensure_tray();
                    }
                    if self.tray.is_some() {
                        if self.main_window == Some(id) {
                            self.main_window = None;
                        }
                        return window::close(id);
                    }
                }
                if self.main_window == Some(id) {
                    self.main_window = None;
                }
                self.shutdown_and_exit()
            }
            Message::ToggleFullscreen => {
                if self.compact_window {
                    self.compact_window = false;
                }
                self.is_fullscreen = !self.is_fullscreen;
                self.show_toast(
                    if self.is_fullscreen {
                        "Fullscreen mode (F11 to exit)"
                    } else {
                        "Windowed mode"
                    },
                    self.current_accent_hex(),
                );
                if let Some(id) = self.main_window {
                    let mode = if self.is_fullscreen {
                        window::Mode::Fullscreen
                    } else {
                        window::Mode::Windowed
                    };
                    window::set_mode(id, mode)
                } else {
                    Task::none()
                }
            }
            Message::ToggleTheaterRig => {
                if self.compact_window {
                    self.compact_window = false;
                }
                self.theater_rig = !self.theater_rig;
                self.palette_open = false;
                self.show_toast(
                    if self.theater_rig {
                        "Theater Rig Mode (Shift+F11 or Esc to exit)"
                    } else {
                        "Returned to Dashboard"
                    },
                    self.current_accent_hex(),
                );
                Task::none()
            }
            Message::SetFullscreenFillWidth(fill) => {
                self.snapshot.config.fullscreen_fill_width = fill;
                self.engine.send(EngineCommand::SetFullscreenFillWidth(fill));
                self.show_toast(
                    if fill {
                        "Fullscreen edge-to-edge layout enabled"
                    } else {
                        "Centered 2400px max-width layout enabled"
                    },
                    self.current_accent_hex(),
                );
                Task::none()
            }
            Message::SetUiScalePct(pct) => {
                let prev_scale = self.applied_scale.max(0.5);
                let normalized_pct = if pct == 0 { 0 } else { pct.clamp(85, 150) };
                self.snapshot.config.appearance.ui_scale_pct = normalized_pct;
                self.engine.send(EngineCommand::SetUiScalePct(normalized_pct));
                if normalized_pct == 0 {
                    self.auto_scale = resolve_auto_scale_with_hysteresis(
                        self.approx_physical_width,
                        self.auto_scale,
                    );
                }
                let next_scale = self.effective_scale_factor();
                self.applied_scale = next_scale;
                if !self.compact_window && (prev_scale - next_scale).abs() > 0.001 {
                    self.window_size = Size::new(
                        self.window_size.width * prev_scale / next_scale,
                        self.window_size.height * prev_scale / next_scale,
                    );
                }
                let toast_lbl = if normalized_pct == 0 {
                    format!("UI Scale: Auto ({:.0}%)", next_scale * 100.0)
                } else {
                    format!("UI Scale: {normalized_pct}%")
                };
                self.show_toast(toast_lbl, self.current_accent_hex());
                Task::none()
            }
            Message::CycleUiScale => {
                let next = match self.snapshot.config.appearance.ui_scale_pct {
                    0 => 100,
                    100 => 125,
                    _ => 0,
                };
                self.update(Message::SetUiScalePct(next))
            }
            Message::StepUiScale(dir) => {
                if dir == 0 {
                    return self.update(Message::SetUiScalePct(0));
                }
                let cur_pct = if self.snapshot.config.appearance.ui_scale_pct == 0 {
                    (self.effective_scale_factor() * 100.0).round() as u16
                } else {
                    self.snapshot.config.appearance.ui_scale_pct
                };
                const STEPS: &[u16] = &[85, 90, 100, 110, 115, 125, 140, 150];
                let next_pct = if dir > 0 {
                    STEPS.iter().copied().find(|&s| s > cur_pct).unwrap_or(150)
                } else {
                    STEPS.iter().copied().rev().find(|&s| s < cur_pct).unwrap_or(85)
                };
                self.update(Message::SetUiScalePct(next_pct))
            }

            // Overview Tile Workshop & Workspaces
            Message::ToggleEditTilesMode => {
                self.tab = Tab::Overview;
                self.theater_rig = false;
                self.edit_tiles_mode = !self.edit_tiles_mode;
                if !self.edit_tiles_mode {
                    self.add_tile_open = false;
                    self.workspace_create_open = false;
                    self.workspace_rename_open = false;
                }
                Task::none()
            }
            Message::ToggleAddTileCatalog => {
                self.add_tile_open = !self.add_tile_open;
                if self.add_tile_open {
                    self.tab = Tab::Overview;
                    self.theater_rig = false;
                }
                Task::none()
            }
            Message::OpenAddTileFromPalette => {
                self.tab = Tab::Overview;
                self.theater_rig = false;
                self.engine
                    .send(EngineCommand::SetStudioPreviewActive(false));
                self.edit_tiles_mode = true;
                self.add_tile_open = true;
                Task::none()
            }
            Message::SetAddTileTab(tab) => {
                self.add_tile_tab = tab;
                Task::none()
            }
            Message::SensorSearchChanged(q) => {
                self.sensor_search_query = q;
                Task::none()
            }
            Message::AddOverviewTile(kind) => {
                let label = kind.label().to_string();
                self.snapshot.config.add_overview_tile(kind.clone());
                self.engine.send(EngineCommand::AddOverviewTile(kind));
                self.show_toast(format!("Pinned \"{label}\" tile"), self.current_accent_hex());
                Task::none()
            }
            Message::RemoveOverviewTile(id) => {
                self.snapshot.config.remove_overview_tile(&id);
                self.engine.send(EngineCommand::RemoveOverviewTile(id));
                self.show_toast("Removed tile from workspace", self.current_accent_hex());
                Task::none()
            }
            Message::MoveOverviewTile(id, delta) => {
                self.snapshot.config.move_overview_tile(&id, delta);
                self.engine
                    .send(EngineCommand::MoveOverviewTile { id, delta });
                Task::none()
            }
            Message::ReorderOverviewTile(id, to_index) => {
                if self.snapshot.config.reorder_overview_tile_to(&id, to_index) {
                    self.engine
                        .send(EngineCommand::ReorderOverviewTile { id, to_index });
                }
                Task::none()
            }
            Message::ResizeOverviewTile(id, span, rows) => {
                if self.snapshot.config.resize_overview_tile(&id, span, rows) {
                    self.engine
                        .send(EngineCommand::ResizeOverviewTile { id, span, rows });
                }
                Task::none()
            }
            Message::CycleOverviewTileSpan(id) => {
                if let Some(span) = self.snapshot.config.cycle_overview_tile_span(&id) {
                    self.show_toast(format!("Tile width: {span}×"), self.current_accent_hex());
                }
                self.engine.send(EngineCommand::CycleOverviewTileSpan(id));
                Task::none()
            }
            Message::ToggleOverviewTileTall(id) => {
                if let Some(tall) = self.snapshot.config.toggle_overview_tile_tall(&id) {
                    self.show_toast(
                        if tall {
                            "Tile height: Tall"
                        } else {
                            "Tile height: Standard"
                        },
                        self.current_accent_hex(),
                    );
                }
                self.engine.send(EngineCommand::ToggleOverviewTileTall(id));
                Task::none()
            }
            Message::CycleOverviewTileStyle(id) => {
                if let Some(style) = self.snapshot.config.cycle_overview_tile_style(&id) {
                    self.show_toast(
                        format!("Tile style: {}", style.label()),
                        self.current_accent_hex(),
                    );
                }
                self.engine.send(EngineCommand::CycleOverviewTileStyle(id));
                Task::none()
            }
            Message::ResetActiveWorkspace => {
                self.snapshot.config.reset_active_workspace();
                self.engine.send(EngineCommand::ResetActiveWorkspace);
                self.show_toast("Workspace layout reset to default", self.current_accent_hex());
                Task::none()
            }
            Message::ToggleWorkspacesBar => {
                self.workspaces_bar_open = !self.workspaces_bar_open;
                if !self.workspaces_bar_open {
                    self.workspace_create_open = false;
                    self.workspace_rename_open = false;
                }
                Task::none()
            }
            Message::SwitchWorkspace(id) => {
                self.snapshot.config.switch_workspace(&id);
                self.workspace_rename_open = false;
                self.engine.send(EngineCommand::SwitchWorkspace(id));
                Task::none()
            }
            Message::ToggleWorkspaceCreate => {
                self.workspace_create_open = !self.workspace_create_open;
                if self.workspace_create_open {
                    self.workspace_rename_open = false;
                    self.workspace_name_draft.clear();
                    self.workspace_template_draft = None;
                }
                Task::none()
            }
            Message::WorkspaceNameDraftChanged(s) => {
                self.workspace_name_draft = s;
                Task::none()
            }
            Message::WorkspaceTemplateSelected(tpl) => {
                self.workspace_template_draft = tpl;
                Task::none()
            }
            Message::CreateWorkspace => {
                let raw = self.workspace_name_draft.trim();
                let name = if raw.is_empty() {
                    match self.workspace_template_draft.as_deref() {
                        Some("gaming") => "Gaming".to_string(),
                        Some("quiet-studio") => "Quiet Studio".to_string(),
                        Some("thermal-lab") => "Thermal Lab".to_string(),
                        _ => format!(
                            "Workspace {}",
                            self.snapshot.config.overview_workspaces.len() + 1
                        ),
                    }
                } else {
                    raw.to_string()
                };
                let tpl = self.workspace_template_draft.clone();
                self.snapshot
                    .config
                    .create_workspace(&name, tpl.as_deref());
                self.engine.send(EngineCommand::CreateWorkspace {
                    name: name.clone(),
                    template_id: tpl,
                });
                self.workspace_create_open = false;
                self.workspace_name_draft.clear();
                self.show_toast(format!("Created workspace \"{name}\""), self.current_accent_hex());
                Task::none()
            }
            Message::ToggleWorkspaceRename => {
                self.workspace_rename_open = !self.workspace_rename_open;
                if self.workspace_rename_open {
                    self.workspace_create_open = false;
                    self.workspace_rename_draft =
                        self.snapshot.config.active_workspace().name.clone();
                }
                Task::none()
            }
            Message::WorkspaceRenameDraftChanged(s) => {
                self.workspace_rename_draft = s;
                Task::none()
            }
            Message::ConfirmWorkspaceRename => {
                let name = self.workspace_rename_draft.trim().to_string();
                let id = self.snapshot.config.active_workspace_id.clone();
                if !name.is_empty() {
                    self.snapshot.config.rename_workspace(&id, &name);
                    self.engine
                        .send(EngineCommand::RenameWorkspace { id, name: name.clone() });
                    self.show_toast(format!("Renamed workspace to \"{name}\""), self.current_accent_hex());
                }
                self.workspace_rename_open = false;
                Task::none()
            }
            Message::DeleteWorkspace(id) => {
                if self.snapshot.config.overview_workspaces.len() > 1 {
                    self.snapshot.config.delete_workspace(&id);
                    self.engine.send(EngineCommand::DeleteWorkspace(id));
                    self.show_toast("Workspace deleted", self.current_accent_hex());
                }
                Task::none()
            }

            // Desk Display Focus Timer
            Message::ToggleFocusTimer => {
                self.focus_timer_running = !self.focus_timer_running;
                if self.focus_timer_running && self.focus_timer_secs == 0 {
                    self.focus_timer_secs = 25 * 60;
                }
                self.focus_timer_last_tick = Instant::now();
                Task::none()
            }
            Message::ResetFocusTimer => {
                self.focus_timer_running = false;
                self.focus_timer_secs = 25 * 60;
                self.focus_timer_last_tick = Instant::now();
                Task::none()
            }
            Message::SetFocusTimerMins(mins) => {
                self.focus_timer_secs = (mins as u32) * 60;
                self.focus_timer_running = true;
                self.focus_timer_last_tick = Instant::now();
                Task::none()
            }
            Message::SelectTab(tab) => {
                self.tab = tab;
                self.engine
                    .send(EngineCommand::SetStudioPreviewActive(tab == Tab::Studio));
                self.flush_window_state();
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
                } else if self.theater_rig {
                    self.theater_rig = false;
                } else if self.add_tile_open {
                    self.add_tile_open = false;
                } else if self.workspace_create_open || self.workspace_rename_open {
                    self.workspace_create_open = false;
                    self.workspace_rename_open = false;
                } else if self.customize_open {
                    self.customize_open = false;
                } else if self.edit_tiles_mode {
                    self.edit_tiles_mode = false;
                } else if self.workspaces_bar_open {
                    self.workspaces_bar_open = false;
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
                let next_scale = self.effective_scale_factor();
                self.applied_scale = next_scale;
                let target = if self.compact_window {
                    Size::new(430.0, 404.0)
                } else {
                    Size::new(1440.0 / next_scale, 900.0 / next_scale)
                };
                self.window_size = target;
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
            Message::ToggleOverviewStageAnimation => {
                let mut appr = self.snapshot.config.appearance.clone();
                appr.overview_stage_animated = !appr.overview_stage_animated;
                let on = appr.overview_stage_animated;
                self.send_appearance(appr);
                self.show_toast(
                    if on {
                        "Overview lighting canvas: Live"
                    } else {
                        "Overview lighting canvas: Static (low GPU)"
                    },
                    self.current_accent_hex(),
                );
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
            Message::SelectSettingsSection(sec) => {
                self.settings_section = sec;
                Task::none()
            }
            Message::JumpSettingsSection(sec) => {
                self.tab = Tab::Settings;
                self.theater_rig = false;
                self.settings_section = sec;
                self.engine
                    .send(EngineCommand::SetStudioPreviewActive(false));
                Task::none()
            }
            Message::CheckForUpdates => {
                self.snapshot.update_status = UpdateStatus::Checking;
                self.engine.send(EngineCommand::CheckForUpdates);
                self.show_toast("Checking GitHub for Airglow updates…", 0x6bcbf7);
                Task::none()
            }
            Message::DownloadAndStageUpdate => {
                self.engine.send(EngineCommand::DownloadAndStageUpdate);
                self.show_toast("Downloading Airglow update…", 0x6bcbf7);
                Task::none()
            }
            Message::ApplyStagedUpdateAndRestart => {
                self.engine.send(EngineCommand::ApplyStagedUpdateAndRestart);
                self.show_toast("Applying update & restarting Airglow…", self.current_accent_hex());
                self.shutdown_and_exit()
            }
            Message::SetUpdateChannel(ch) => {
                self.snapshot.config.update_channel = ch;
                self.engine.send(EngineCommand::SetUpdateChannel(ch));
                self.show_toast(
                    format!("Update channel: {}", ch.label()),
                    self.current_accent_hex(),
                );
                Task::none()
            }
            Message::ToggleAutoCheckUpdates(enabled) => {
                self.snapshot.config.auto_check_updates = enabled;
                self.engine.send(EngineCommand::SetAutoCheckUpdates(enabled));
                Task::none()
            }
            Message::OpenUrl(url) => {
                self.engine.send(EngineCommand::OpenUrl(url));
                Task::none()
            }
            Message::OpenConfigFolder => {
                self.engine.send(EngineCommand::OpenConfigFolder);
                self.show_toast("Opened Airglow config folder", self.current_accent_hex());
                Task::none()
            }
            Message::ExportRigBundle => {
                self.engine.send(EngineCommand::ExportRigBundle);
                self.show_toast("Exported Airglow rig backup (.json)", 0xa5dece);
                Task::none()
            }
            Message::ImportBackupPathChanged(path) => {
                self.import_backup_path_draft = path;
                Task::none()
            }
            Message::ImportRigBundle => {
                let path = self.import_backup_path_draft.trim().trim_matches('"').to_string();
                if path.is_empty() {
                    self.show_toast("Enter a path to an airglow-rig-backup.json file", 0xf2b94b);
                } else {
                    self.sync_drafts_pending = true;
                    self.engine.send(EngineCommand::ImportRigBundle(path));
                    self.show_toast("Importing Airglow rig backup…", 0xa5dece);
                }
                Task::none()
            }
            Message::BrowseBackupFile => {
                if self.browsing_backup_file {
                    return Task::none();
                }
                self.browsing_backup_file = true;
                Task::perform(
                    async {
                        tokio::task::spawn_blocking(platform::pick_json_backup_file)
                            .await
                            .ok()
                            .flatten()
                    },
                    Message::BackupFilePicked,
                )
            }
            Message::BackupFilePicked(opt) => {
                self.browsing_backup_file = false;
                if let Some(path) = opt {
                    self.import_backup_path_draft = path;
                    self.show_toast(
                        "Backup file selected — click Import to restore",
                        0xa5dece,
                    );
                }
                Task::none()
            }
            Message::ToggleMinimizeToTray(enabled) => {
                self.snapshot.config.settings.minimize_to_tray_on_close = enabled;
                self.engine
                    .send(EngineCommand::UpdateSettings(self.snapshot.config.settings.clone()));
                self.show_toast(
                    if enabled {
                        "Closing window minimizes Airglow to tray"
                    } else {
                        "Closing window exits Airglow"
                    },
                    self.current_accent_hex(),
                );
                Task::none()
            }
            Message::ToggleGlobalHotkeys(enabled) => {
                self.snapshot.config.settings.global_hotkeys_enabled = enabled;
                self.engine
                    .send(EngineCommand::UpdateSettings(self.snapshot.config.settings.clone()));
                self.hotkeys = if enabled {
                    Some(GlobalHotkeyManager::start())
                } else {
                    None
                };
                self.show_toast(
                    if enabled {
                        "Global hotkeys enabled (Ctrl+Alt+Shift+L / P / A)"
                    } else {
                        "Global hotkeys disabled"
                    },
                    self.current_accent_hex(),
                );
                Task::none()
            }
            Message::ToggleLhmFanControl(enable_curve) => {
                let mode = if enable_curve {
                    FanChannelMode::Curve
                } else {
                    FanChannelMode::Auto
                };
                for ch in &self.snapshot.lhm_channels {
                    self.engine.send(EngineCommand::SetFanChannelMode {
                        channel_id: ch.id.clone(),
                        mode,
                    });
                }
                self.show_toast(
                    if enable_curve {
                        "LHM fan channels switched to Airglow Curve"
                    } else {
                        "LHM fan channels returned to BIOS/Auto"
                    },
                    0xa5dece,
                );
                Task::none()
            }
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

        let backdrop = iced::widget::canvas(WindowBackdropCanvas {
            design,
            accent_override: appr.accent_override,
            phase: self.anim_t,
            animate_motes: appr.firefly_particles,
        })
        .width(Length::Fill)
        .height(Length::Fill);

        if self.compact_window {
            let mini = views::view_compact_companion(self, t, font);
            let mut layers: Vec<Element<'_, Message>> = vec![backdrop.into(), mini];
            if let Some((msg, accent, _)) = &self.toast {
                layers.push(views_palette::view_toast_overlay(msg, *accent, t, font));
            }
            if self.palette_open {
                layers.push(views_palette::view_command_palette(self, t, font));
            }
            if self.show_shortcuts {
                layers.push(views_palette::view_shortcuts_modal(t, font));
            }
            return stack(layers).width(Length::Fill).height(Length::Fill).into();
        }

        if self.theater_rig {
            let theater = views::view_theater_rig(self, t, font);
            let mut layers: Vec<Element<'_, Message>> = vec![backdrop.into(), theater];
            if let Some((msg, accent, _)) = &self.toast {
                layers.push(views_palette::view_toast_overlay(msg, *accent, t, font));
            }
            if self.palette_open {
                layers.push(views_palette::view_command_palette(self, t, font));
            }
            if self.show_shortcuts {
                layers.push(views_palette::view_shortcuts_modal(t, font));
            }
            return stack(layers).width(Length::Fill).height(Length::Fill).into();
        }

        let header = self.view_header(t, font);
        let body: Element<'_, Message> = match self.tab {
            Tab::Overview => views::view_overview(self, t, font),
            Tab::Lighting => views_lighting::view_lighting(self, t, font),
            Tab::Studio => views::view_studio(self, t, font),
            Tab::Fans => views_system::view_fans(self, t, font),
            Tab::Automations => views_system::view_automations(self, t, font),
            Tab::Devices => views_system::view_devices(self, t, font),
            Tab::Settings => views_system::view_settings(self, t, font),
        };

        let mut banner_col = column![].spacing(t.sp(8));
        let mut has_banners = false;

        // Trial profile Keep / Revert banner
        if let Some(trial) = &self.snapshot.trial {
            has_banners = true;
            banner_col = banner_col.push(
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
            has_banners = true;
            banner_col = banner_col.push(
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

        let main_stage: Element<'_, Message> = if self.tab == Tab::Overview {
            let mut content_col = column![].spacing(t.sp(12));
            if has_banners {
                content_col = content_col.push(banner_col);
            }
            content_col = content_col.push(body);

            let side_pad: u16 = 28;
            let vert_pad: u16 = 10;
            let responsive_inner: Element<'_, Message> = if self.snapshot.config.fullscreen_fill_width {
                container(content_col)
                    .padding([vert_pad, side_pad])
                    .width(Length::Fill)
                    .into()
            } else {
                container(
                    container(content_col)
                        .max_width(2400.0)
                        .width(Length::Fill),
                )
                .padding([vert_pad, side_pad])
                .center_x(Length::Fill)
                .width(Length::Fill)
                .into()
            };

            scrollable(responsive_inner).height(Length::Fill).into()
        } else {
            let mut stage_col = column![].spacing(0).width(Length::Fill).height(Length::Fill);
            if has_banners {
                stage_col = stage_col.push(
                    container(banner_col)
                        .padding([10, 26])
                        .width(Length::Fill),
                );
            }
            stage_col = stage_col.push(body);
            stage_col.into()
        };

        let base: Element<'_, Message> = container(
            column![header, main_stage]
                .spacing(0)
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::window_ground_style(t))
        .into();

        let mut layers: Vec<Element<'_, Message>> = vec![backdrop.into(), base];

        if let Some((msg, accent, _)) = &self.toast {
            layers.push(views_palette::view_toast_overlay(msg, *accent, t, font));
        }

        if self.palette_open {
            layers.push(views_palette::view_command_palette(self, t, font));
        }

        if self.show_shortcuts {
            layers.push(views_palette::view_shortcuts_modal(t, font));
        }

        stack(layers).width(Length::Fill).height(Length::Fill).into()
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
            .width(Length::Fixed(26.0))
            .height(Length::Fixed(26.0)),
        )
        .padding(4)
        .style(theme::brand_badge_style(t));

        let active_profile = self
            .snapshot
            .config
            .active_preset
            .as_deref()
            .and_then(|pid| self.snapshot.config.presets.iter().find(|pr| pr.id == pid));
        let (prof_name, prof_dot) = if let Some(pr) = active_profile {
            (
                views::truncate_str(&pr.name, 12),
                views::preset_dot_color(pr.effect, pr.params.color),
            )
        } else {
            (
                "Custom".to_string(),
                theme::hex_to_rgb_arr(p.brass_hi),
            )
        };

        let profile_pill = button(
            row![
                views::color_dot(prof_dot, 7.0),
                text(prof_name)
                    .size(11.5)
                    .font(theme::ui_semibold(design))
                    .color(theme::rgb(p.ink_2)),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([4, 10])
        .style(theme::shortcut_badge_btn(t, false))
        .on_press(Message::SelectTab(Tab::Lighting));

        let brand = row![
            mark,
            text("Airglow")
                .size(18)
                .font(theme::ui_bold(design))
                .color(theme::rgb(p.ink)),
            profile_pill,
        ]
        .spacing(9)
        .align_y(Alignment::Center);

        let mut dock_row = row![].spacing(3).align_y(Alignment::Center);
        let update_available = matches!(
            self.snapshot.update_status,
            UpdateStatus::Available { .. } | UpdateStatus::ReadyToRestart { .. }
        );
        for tab_item in Tab::ALL {
            let active = self.tab == tab_item;
            let icon = match tab_item {
                Tab::Overview => UiIcon::Overview,
                Tab::Lighting => UiIcon::Lighting,
                Tab::Studio => UiIcon::Studio,
                Tab::Fans => UiIcon::Fans,
                Tab::Automations => UiIcon::Automations,
                Tab::Devices => UiIcon::Devices,
                Tab::Settings => UiIcon::Settings,
            };
            let icon_col = if active {
                theme::rgb(if p.is_light { p.brass } else { p.brass_hi })
            } else {
                theme::rgb(p.ink_2)
            };
            let tab_font = if active {
                theme::ui_semibold(design)
            } else {
                font
            };
            let show_dot = tab_item == Tab::Settings && update_available;

            let mut tab_content = row![
                icon_canvas(icon, icon_col, 15.0),
                text(tab_item.label()).size(13.5).font(tab_font),
            ]
            .spacing(6)
            .align_y(Alignment::Center);

            if show_dot {
                tab_content = tab_content.push(views::status_dot(p.brass_hi, 6.0));
            }

            dock_row = dock_row.push(
                button(tab_content)
                    .padding([6, 11])
                    .style(theme::nav_tab_style(t, active))
                    .on_press(Message::SelectTab(tab_item)),
            );
        }

        let nav_dock = container(dock_row)
            .padding([3, 4])
            .style(theme::nav_dock_style(t));

        // Quick Command Palette search pill (`Ctrl+K`)
        let cmd_pill = button(
            row![
                icon_canvas(UiIcon::Search, theme::rgb(p.brass_hi), 13.5),
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
                pin_row = pin_row.push(
                    text("·")
                        .size(11)
                        .font(font)
                        .color(theme::rgba(p.ink_3, 0.55)),
                );
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
            .padding([6, 11])
            .style(theme::ambient_pill_style(t));

        // UI Scale badge pill (click cycles Auto / 100% / 125%)
        let scale_pct = (self.effective_scale_factor() * 100.0).round() as u16;
        let scale_is_auto = appr.ui_scale_pct == 0;
        let scale_label = if scale_is_auto {
            format!("Auto {scale_pct}%")
        } else {
            format!("{scale_pct}%")
        };
        let scale_pill = button(
            text(scale_label)
                .size(11)
                .font(mono)
                .color(theme::rgb(if !scale_is_auto || scale_pct != 100 {
                    p.brass_hi
                } else {
                    p.ink_2
                })),
        )
        .padding([6, 10])
        .style(theme::shortcut_badge_btn(t, !scale_is_auto))
        .on_press(Message::CycleUiScale);

        // Fullscreen toggle pill (`F11`)
        let fullscreen_pill = button(icon_canvas(
            UiIcon::Expand,
            theme::rgb(if self.is_fullscreen { p.brass_hi } else { p.ink_2 }),
            14.0,
        ))
        .padding([7, 8])
        .style(theme::shortcut_badge_btn(t, self.is_fullscreen))
        .on_press(Message::ToggleFullscreen);

        // Compact Mini-Mode toggle pill (`Ctrl+M`)
        let compact_pill = button(icon_canvas(
            UiIcon::Minimize,
            theme::rgb(p.ink_2),
            14.0,
        ))
        .padding([7, 8])
        .style(theme::shortcut_badge_btn(t, self.compact_window))
        .on_press(Message::ToggleCompactMode);

        let power_on = self.snapshot.effective_scene.on;
        let power_btn = button(
            container(icon_canvas(
                UiIcon::Power,
                theme::rgb(if power_on { t.primary_ink } else { p.ink_2 }),
                16.0,
            ))
            .center_x(Length::Fill)
            .center_y(Length::Fill),
        )
        .width(Length::Fixed(36.0))
        .height(Length::Fixed(36.0))
        .padding(0)
        .style(theme::power_circle_btn(t, power_on))
        .on_press(Message::TogglePower);

        let right_group = row![
            cmd_pill,
            temp_pill,
            scale_pill,
            fullscreen_pill,
            compact_pill,
            power_btn,
        ]
        .spacing(7)
        .align_y(Alignment::Center);

        let bar = container(
            row![
                brand,
                Space::new().width(Length::Fill),
                nav_dock,
                Space::new().width(Length::Fill),
                right_group,
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .height(Length::Fill),
        )
        .padding([0, 22])
        .width(Length::Fill)
        .height(Length::Fixed(58.0))
        .style(theme::appbar_style(t));

        let glow_line = iced::widget::canvas(HeaderGlowCanvas {
            palette: p,
            accent_override: t.accent_override,
            ambient_rgb: self.snapshot.average_rgb,
            lights_on: power_on,
            anim_t: self.anim_t,
        })
        .width(Length::Fill)
        .height(Length::Fixed(1.0));

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

fn parse_settings_section(raw: &str) -> Option<SettingsSection> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "appearance" | "design" | "theme" => Some(SettingsSection::Appearance),
        "updates" | "about" | "update" => Some(SettingsSection::Updates),
        "general" | "telemetry" => Some(SettingsSection::General),
        "lighting" | "cooling" | "lighting-cooling" => Some(SettingsSection::LightingCooling),
        "sensors" | "devices" | "sensors-devices" => Some(SettingsSection::SensorsDevices),
        "backup" | "diagnostics" | "tools" => Some(SettingsSection::BackupDiagnostics),
        _ => None,
    }
}

struct LaunchFlags {
    tab: Tab,
    tab_explicit: bool,
    palette_open: bool,
    palette_query: String,
    customize_open: bool,
    compact_window: bool,
    show_shortcuts: bool,
    fullscreen: bool,
    maximized: bool,
    theater_rig: bool,
    edit_tiles: bool,
    add_tile: bool,
    settings_section: SettingsSection,
    check_updates: bool,
    background: bool,
}

fn parse_launch_flags_from<I: IntoIterator<Item = String>>(iter: I) -> LaunchFlags {
    let mut flags = LaunchFlags {
        tab: Tab::Overview,
        tab_explicit: false,
        palette_open: false,
        palette_query: String::new(),
        customize_open: false,
        compact_window: false,
        show_shortcuts: false,
        fullscreen: false,
        maximized: false,
        theater_rig: false,
        edit_tiles: false,
        add_tile: false,
        settings_section: SettingsSection::Appearance,
        check_updates: false,
        background: false,
    };
    let mut args = iter.into_iter();
    while let Some(arg) = args.next() {
        if let Some(val) = arg.strip_prefix("--tab=")
            && let Some(tab) = parse_tab_name(val)
        {
            flags.tab = tab;
            flags.tab_explicit = true;
        } else if arg == "--tab"
            && let Some(val) = args.next()
            && let Some(tab) = parse_tab_name(&val)
        {
            flags.tab = tab;
            flags.tab_explicit = true;
        } else if let Some(val) = arg.strip_prefix("--settings-section=")
            && let Some(sec) = parse_settings_section(val)
        {
            flags.tab = Tab::Settings;
            flags.tab_explicit = true;
            flags.settings_section = sec;
        } else if arg == "--settings-section"
            && let Some(val) = args.next()
            && let Some(sec) = parse_settings_section(&val)
        {
            flags.tab = Tab::Settings;
            flags.tab_explicit = true;
            flags.settings_section = sec;
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
        } else if arg == "--fullscreen" {
            flags.fullscreen = true;
        } else if arg == "--maximized" {
            flags.maximized = true;
        } else if arg == "--theater" {
            flags.theater_rig = true;
        } else if arg == "--edit-tiles" {
            flags.edit_tiles = true;
        } else if arg == "--add-tile" {
            flags.edit_tiles = true;
            flags.add_tile = true;
        } else if arg == "--check-updates" {
            flags.check_updates = true;
        } else if arg == "--background" {
            flags.background = true;
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
        flags.tab_explicit = true;
    }
    if let Ok(env_sec) = std::env::var("AIRGLOW_SETTINGS_SECTION")
        && let Some(sec) = parse_settings_section(&env_sec)
    {
        flags.tab = Tab::Settings;
        flags.tab_explicit = true;
        flags.settings_section = sec;
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
    if !flags.fullscreen
        && std::env::var("AIRGLOW_FULLSCREEN")
            .ok()
            .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
    {
        flags.fullscreen = true;
    }
    if !flags.maximized
        && std::env::var("AIRGLOW_MAXIMIZED")
            .ok()
            .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
    {
        flags.maximized = true;
    }
    if !flags.theater_rig
        && std::env::var("AIRGLOW_THEATER")
            .ok()
            .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
    {
        flags.theater_rig = true;
    }
    if !flags.edit_tiles
        && std::env::var("AIRGLOW_EDIT_TILES")
            .ok()
            .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
    {
        flags.edit_tiles = true;
    }
    if !flags.add_tile
        && std::env::var("AIRGLOW_ADD_TILE")
            .ok()
            .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
    {
        flags.edit_tiles = true;
        flags.add_tile = true;
    }
    if !flags.check_updates
        && std::env::var("AIRGLOW_CHECK_UPDATES")
            .ok()
            .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
    {
        flags.check_updates = true;
    }
    if !flags.background
        && std::env::var("AIRGLOW_BACKGROUND")
            .ok()
            .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
    {
        flags.background = true;
    }
    flags
}

#[cfg(windows)]
fn local_clock_strings() -> (String, String) {
    #[repr(C)]
    #[derive(Default)]
    struct SystemTimeWin {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        millis: u16,
    }
    unsafe extern "system" {
        fn GetLocalTime(lp_system_time: *mut SystemTimeWin);
    }
    let mut st = SystemTimeWin::default();
    unsafe { GetLocalTime(&mut st) };
    let dow = match st.day_of_week {
        0 => "Sun",
        1 => "Mon",
        2 => "Tue",
        3 => "Wed",
        4 => "Thu",
        5 => "Fri",
        _ => "Sat",
    };
    let mon = match st.month {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        _ => "Dec",
    };
    (
        format!("{:02}:{:02}", st.hour, st.minute),
        format!("{dow}, {mon} {:02}", st.day),
    )
}

#[cfg(not(windows))]
fn local_clock_strings() -> (String, String) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    (format!("{h:02}:{m:02}"), "Local time".to_string())
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

fn open_main_window(compact: bool, size: Size, position: window::Position) -> Task<Message> {
    let rgba = brand::app_icon_rgba(64);
    let icon = window::icon::from_rgba(rgba, 64, 64).ok();
    let _ = compact;
    let (_id, task) = window::open(window::Settings {
        size,
        position,
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
    fn parses_cli_v21_flags() {
        let f = parse_launch_flags_from(
            [
                "--fullscreen",
                "--edit-tiles",
                "--add-tile",
                "--theater",
                "--settings-section=updates",
                "--check-updates",
            ]
            .into_iter()
            .map(String::from),
        );
        assert!(f.fullscreen);
        assert!(f.edit_tiles);
        assert!(f.add_tile);
        assert!(f.theater_rig);
        assert_eq!(f.tab, Tab::Settings);
        assert!(f.tab_explicit);
        assert_eq!(f.settings_section, super::SettingsSection::Updates);
        assert!(f.check_updates);
        assert!(!f.background);

        let bg = parse_launch_flags_from(["--background"].into_iter().map(String::from));
        assert!(bg.background);
        assert!(!bg.tab_explicit);
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

    #[test]
    fn auto_scale_hysteresis_locks_cleanly_across_resolutions() {
        use super::resolve_auto_scale_with_hysteresis;

        // 1440x900 -> 1.0, 2560x1440 -> 1.25, 3840x2160 -> 1.50
        assert!((resolve_auto_scale_with_hysteresis(1440.0, 1.0) - 1.0).abs() < 0.001);
        assert!((resolve_auto_scale_with_hysteresis(2560.0, 1.0) - 1.25).abs() < 0.001);
        assert!((resolve_auto_scale_with_hysteresis(3840.0, 1.25) - 1.50).abs() < 0.001);

        // Hysteresis 120px band prevents oscillation around 2200px & 3250px thresholds
        assert!((resolve_auto_scale_with_hysteresis(2120.0, 1.25) - 1.25).abs() < 0.001);
        assert!((resolve_auto_scale_with_hysteresis(2050.0, 1.25) - 1.10).abs() < 0.001);
        assert!((resolve_auto_scale_with_hysteresis(3180.0, 1.50) - 1.50).abs() < 0.001);
        assert!((resolve_auto_scale_with_hysteresis(3100.0, 1.50) - 1.25).abs() < 0.001);
    }
}
