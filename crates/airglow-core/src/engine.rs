//! Threaded lighting, automation, and fan controller engine communicating with the Iced UI.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::watch;

use crate::automations::{self, Action, AutomationContext, AutomationRule, AutomationStatus};
use crate::config::{
    AppConfig, AppSettings, AppearanceConfig, ColorOrder, ConfigStore, CurvePoint, DesignId,
    FanChannelMode, OverviewTileKind, Preset, SavedLook, SceneConfig, TempUnit, UpdateChannel,
    ZoneConfig, ZoneOverride, apply_migrated_data, default_curves, default_data_dir,
    import_preset_json,
};
use crate::cooling::{
    CoolingChannelSample, CoolingCurveMeta, CoolingRecorder, CoolingSampleInput,
    CoolingSessionSummary,
};
use crate::devices::{DiscoveredDevice, HidOutputManager};
use crate::effects::{
    self, EffectId, Layer, LayerKind, LayerStack, SavedStudioEffect, SceneParams,
};
use crate::fans::{self, FanHysteresis};
use crate::geometry::{CANVAS_H, CANVAS_W, ZoneGeometry, ZoneLayout};
use crate::inputs::{AUDIO_BANDS, AudioAnalyzer, AudioFrame, InputContext, ScreenSampler};
use crate::lhm_control::LhmFanController;
use crate::migrate;
use crate::openrgb::{OpenRgbClient, OpenRgbStatus};
use crate::sensors::{SensorPoller, SensorSnapshot};
use crate::system::{self, ConflictingApp};
use crate::updates::{self, UpdateStatus};

const ACTIVE_TICK_MS: u64 = 33; // ~30 FPS
const IDLE_TICK_MS: u64 = 250; // ~4 FPS when static
const HISTORY_LEN: usize = 300; // 10 minutes at 2s sensor poll cadence
const ACTIVITY_LOG_LEN: usize = 30;
const CROSSFADE_DURATION: Duration = Duration::from_millis(350);
const SLEEP_FADE_SECS: f32 = 120.0;
const HW_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(5);
const IDENTIFY_SECS: f32 = 6.0;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityEvent {
    pub time_hm: String,
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct ZoneFrame {
    pub zone_id: String,
    pub zone_name: String,
    pub enabled: bool,
    pub layout: ZoneLayout,
    pub canvas_pos: Vec<[f32; 2]>,
    pub fan_rings: Vec<([f32; 2], f32)>,
    pub colors: Vec<[u8; 3]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrialInfo {
    pub preset_id: String,
    pub preset_name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LhmChannelStatus {
    pub id: String,
    pub name: String,
    pub hardware: String,
    pub rpm: Option<u16>,
    pub duty: Option<f32>,
    pub is_pump: bool,
    pub mode: FanChannelMode,
    pub target_duty: Option<u8>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EngineSnapshot {
    pub config: AppConfig,
    pub effective_scene: SceneConfig,
    pub effective_curve: String,
    pub trial: Option<TrialInfo>,
    pub automation_status: AutomationStatus,
    pub identify_zone: Option<String>,
    pub frames: Vec<ZoneFrame>,
    pub studio_preview_frames: Vec<ZoneFrame>,
    pub average_rgb: [u8; 3],
    pub sensors: SensorSnapshot,
    pub cpu_temp_history: Vec<f32>,
    pub gpu_temp_history: Vec<f32>,
    pub ssd_temp_history: Vec<f32>,
    pub net_down_history: Vec<f32>,
    pub cpu_power_history: Vec<f32>,
    pub gpu_power_history: Vec<f32>,
    pub cost_history: Vec<f32>,
    pub fan_temp: Option<f32>,
    pub fan_target: f32,
    pub fan_duty: u8,
    pub fan_rpms: Vec<(String, u16)>,
    pub lhm_channels: Vec<LhmChannelStatus>,
    pub lhm_auth_configured: bool,
    pub conflicts: Vec<ConflictingApp>,
    pub idle_secs: u64,
    pub workstation_locked: bool,
    pub studio_test_beat: bool,
    pub studio_paused: bool,
    pub studio_speed_mult: f32,
    pub audio_frame: AudioFrame,
    pub audio_bands: [f32; AUDIO_BANDS],
    pub audio_bass: f32,
    pub cpu_temp_peak: f32,
    pub gpu_temp_peak: f32,
    pub power_peak_w: f32,
    pub sleep_timer_remaining_s: Option<u32>,
    pub activity_log: Vec<ActivityEvent>,
    pub cooling_recording: bool,
    pub cooling_sample_count: usize,
    pub cooling_session_count: usize,
    pub cooling_sessions: Vec<CoolingSessionSummary>,
    pub last_notice: Option<String>,
    pub update_status: UpdateStatus,
    pub hid_devices: Vec<DiscoveredDevice>,
    pub openrgb: OpenRgbStatus,
    pub fps: f32,
    pub tick_count: u64,
}

#[derive(Debug, Clone)]
pub enum EngineCommand {
    SetPower(bool),
    SetBrightness(u8),
    SetEffect(EffectId),
    SetParams(SceneParams),
    ApplyHarmony(usize),
    SetZoneOverride {
        zone_id: String,
        override_cfg: ZoneOverride,
    },
    ClearZoneOverride(String),
    ClearAllZoneOverrides,
    SetStudioStack(LayerStack),
    AddStudioLayer(LayerKind),
    RemoveStudioLayer(u64),
    MoveStudioLayer { id: u64, up: bool },
    UpdateStudioLayer(Layer),
    StudioSurpriseMe,
    SaveStudioEffect(String),
    ApplyStudioEffect(String),
    DuplicateStudioEffect(String),
    DeleteStudioEffect(String),
    SetStudioTestBeat(bool),
    SetStudioPaused(bool),
    SetStudioSpeedMult(f32),
    SetStudioPreviewActive(bool),
    ApplyPreset(String),
    TryPreset(String),
    KeepTrial,
    RevertTrial,
    SaveCurrentPreset(String),
    DeletePreset(String),
    ImportPresetJson(String),
    SelectFanCurve(String),
    UpdateFanCurve { name: String, points: Vec<CurvePoint> },
    ResetFanCurve(String),
    SetFanChannelMode {
        channel_id: String,
        mode: FanChannelMode,
    },
    SetFanHysteresis(f32),
    StartSleepTimer(u16),
    CancelSleepTimer,
    StartCoolingRecording,
    StopCoolingRecording,
    ExportCoolingCsv,
    UpdateSettings(AppSettings),
    UpdateAppearance(AppearanceConfig),
    UpdateWindowState(crate::config::WindowState),
    SaveLook(String),
    ApplyLook(String),
    DeleteLook(String),
    SetDesign(DesignId),
    SetTempUnit(TempUnit),
    SetStartWithWindows(bool),
    ToggleZone(String),
    IdentifyZone(String),
    UpdateZone(ZoneConfig),
    UpdateZoneLayout { zone_id: String, x: f32, y: f32 },
    CycleZoneColorOrder(String),
    AddAutomation(AutomationRule),
    UpdateAutomation(AutomationRule),
    ToggleAutomation(String),
    DeleteAutomation(String),
    MoveAutomation { id: String, up: bool },
    SetAutomationsPaused(bool),
    CloseConflictingApp(String),
    ImportPythonSettings,
    ExportDiagnostics,
    ExportRigBundle,
    ImportRigBundle(String),
    OpenUrl(String),
    OpenConfigFolder,
    CheckForUpdates,
    DownloadAndStageUpdate,
    ApplyStagedUpdateAndRestart,
    SetUpdateChannel(UpdateChannel),
    SetAutoCheckUpdates(bool),
    SetFullscreenFillWidth(bool),
    SetUiScalePct(u16),
    AddOverviewTile(OverviewTileKind),
    RemoveOverviewTile(String),
    MoveOverviewTile { id: String, delta: isize },
    ReorderOverviewTile { id: String, to_index: usize },
    ResizeOverviewTile { id: String, span: u8, rows: u8 },
    CycleOverviewTileSpan(String),
    ToggleOverviewTileTall(String),
    CycleOverviewTileStyle(String),
    ResetActiveWorkspace,
    SwitchWorkspace(String),
    CreateWorkspace { name: String, template_id: Option<String> },
    RenameWorkspace { id: String, name: String },
    DeleteWorkspace(String),
    ClearNotice,
    RescanHardware,
    Shutdown,
}

struct TrialState {
    info: TrialInfo,
    before_scene: SceneConfig,
    before_curve: String,
    before_preset: Option<String>,
}

/// Smooth per-LED cubic-smoothstep RGB crossfader (350ms default) for seamless visual
/// and hardware transitions whenever the active scene, effect, harmony, or power state changes.
pub struct RgbCrossfader {
    prev_colors: Vec<Vec<[u8; 3]>>,
    from_colors: Vec<Vec<[u8; 3]>>,
    started_at: Option<Instant>,
    duration: Duration,
}

impl RgbCrossfader {
    pub fn new(duration: Duration) -> Self {
        Self {
            prev_colors: Vec::new(),
            from_colors: Vec::new(),
            started_at: None,
            duration,
        }
    }

    pub fn trigger(&mut self, now: Instant) {
        if !self.prev_colors.is_empty() {
            self.from_colors = self.prev_colors.clone();
            self.started_at = Some(now);
        }
    }

    pub fn is_active(&self, now: Instant) -> bool {
        self.started_at
            .is_some_and(|start| now.saturating_duration_since(start) < self.duration)
    }

    /// Blends `frames` in place if a crossfade is running, records the blended output as the
    /// latest frame, and returns `true` while the crossfade is still in progress.
    pub fn apply(&mut self, frames: &mut [ZoneFrame], now: Instant) -> bool {
        let mut active = false;
        if let Some(start) = self.started_at {
            let elapsed = now.saturating_duration_since(start).as_secs_f32();
            let total = self.duration.as_secs_f32().max(1e-4);
            let u = (elapsed / total).clamp(0.0, 1.0);
            if u < 1.0 {
                // Cubic smoothstep for an organic ease-in-out glide
                let k = u * u * (3.0 - 2.0 * u);
                for (zi, frame) in frames.iter_mut().enumerate() {
                    if let Some(from_zone) = self.from_colors.get(zi)
                        && from_zone.len() == frame.colors.len()
                    {
                        for (dst, &src) in frame.colors.iter_mut().zip(from_zone.iter()) {
                            dst[0] = (src[0] as f32 + (dst[0] as f32 - src[0] as f32) * k)
                                .round()
                                .clamp(0.0, 255.0) as u8;
                            dst[1] = (src[1] as f32 + (dst[1] as f32 - src[1] as f32) * k)
                                .round()
                                .clamp(0.0, 255.0) as u8;
                            dst[2] = (src[2] as f32 + (dst[2] as f32 - src[2] as f32) * k)
                                .round()
                                .clamp(0.0, 255.0) as u8;
                        }
                    }
                }
                active = true;
            } else {
                self.started_at = None;
            }
        }
        if self.prev_colors.len() != frames.len() {
            self.prev_colors = frames.iter().map(|f| f.colors.clone()).collect();
        } else {
            for (slot, frame) in self.prev_colors.iter_mut().zip(frames.iter()) {
                slot.clear();
                slot.extend_from_slice(&frame.colors);
            }
        }
        active
    }
}

/// Sunset / Sleep timer that dims lights smoothly over the final 2 minutes before powering off.
#[derive(Debug, Clone, Copy)]
pub struct SleepTimer {
    ends_at: Instant,
    fade_secs: f32,
}

impl SleepTimer {
    pub fn start(now: Instant, minutes: u16) -> Self {
        let total_secs = (minutes.max(1) as u64) * 60;
        let fade_secs = (total_secs as f32).min(SLEEP_FADE_SECS);
        Self {
            ends_at: now + Duration::from_secs(total_secs),
            fade_secs,
        }
    }

    pub fn remaining_secs(&self, now: Instant) -> u32 {
        if now >= self.ends_at {
            0
        } else {
            let rem = self.ends_at.saturating_duration_since(now).as_secs_f32();
            rem.ceil().max(0.0) as u32
        }
    }

    pub fn is_expired(&self, now: Instant) -> bool {
        now >= self.ends_at
    }

    /// Returns a brightness multiplier in `0.0..=1.0`, staying at `1.0` until the final
    /// `fade_secs` (up to 120s), then smoothly dimming to `0.0`.
    pub fn brightness_scale(&self, now: Instant) -> f32 {
        if now >= self.ends_at {
            return 0.0;
        }
        let rem = self.ends_at.saturating_duration_since(now).as_secs_f32();
        if rem >= self.fade_secs || self.fade_secs <= 0.0 {
            1.0
        } else {
            let u = (rem / self.fade_secs).clamp(0.0, 1.0);
            // Smoothstep sunset ramp
            u * u * (3.0 - 2.0 * u)
        }
    }
}

fn push_activity(log: &mut VecDeque<ActivityEvent>, kind: &str, text: impl Into<String>) {
    let (h, m, _) = automations::local_time_hm_wday();
    push_activity_at(log, format!("{h:02}:{m:02}"), kind, text);
}

fn push_activity_at(
    log: &mut VecDeque<ActivityEvent>,
    time_hm: String,
    kind: &str,
    text: impl Into<String>,
) {
    if log.len() >= ACTIVITY_LOG_LEN {
        log.pop_front();
    }
    log.push_back(ActivityEvent {
        time_hm,
        kind: kind.to_string(),
        text: text.into(),
    });
}

fn frames_colors_equal(prev: &[Vec<[u8; 3]>], frames: &[ZoneFrame]) -> bool {
    prev.len() == frames.len()
        && prev
            .iter()
            .zip(frames.iter())
            .all(|(p, f)| p.as_slice() == f.colors.as_slice())
}

struct SensorWorkerReport {
    sensors: SensorSnapshot,
    exes: HashSet<String>,
    conflicts: Vec<ConflictingApp>,
    lhm_errors: HashMap<String, String>,
}

#[derive(Clone)]
struct WorkerSharedConfig {
    lhm_url: String,
    demo_mode: bool,
    lhm_auth: Option<String>,
    lhm_targets: BTreeMap<String, u8>,
}

struct HwDesiredState {
    frames: Vec<ZoneFrame>,
    zones: Vec<ZoneConfig>,
    frame_gen: u64,
    fan_duty: Option<u8>,
    openrgb_enabled: bool,
    openrgb_host: String,
    openrgb_port: u16,
    rescan_gen: u64,
    shutdown: bool,
}

struct HwStatusReport {
    hid_devices: Vec<DiscoveredDevice>,
    openrgb: OpenRgbStatus,
    fan_rpms: Vec<(String, u16)>,
    scan_gen: u64,
}

pub struct EngineHandle {
    tx: mpsc::Sender<EngineCommand>,
    pub rx: watch::Receiver<EngineSnapshot>,
    join: Option<thread::JoinHandle<()>>,
}

impl EngineHandle {
    pub fn start() -> Self {
        let store = ConfigStore::new(default_data_dir());
        let (mut config, auto_migrated) = store.load_or_migrate();
        config.settings.start_with_windows = system::is_autostart_enabled();
        let initial_notice = auto_migrated
            .then(|| "Imported your existing settings from Python Airglow.".to_string());
        let lhm_auth = migrate::load_lhm_auth_header();
        let initial = build_initial_snapshot(&config, lhm_auth.is_some(), initial_notice.clone());

        let (watch_tx, watch_rx) = watch::channel(initial);
        let (cmd_tx, cmd_rx) = mpsc::channel::<EngineCommand>();

        let join = thread::Builder::new()
            .name("airglow-engine".to_string())
            .spawn(move || run_engine(store, config, lhm_auth, initial_notice, cmd_rx, watch_tx))
            .expect("spawn engine thread");

        Self {
            tx: cmd_tx,
            rx: watch_rx,
            join: Some(join),
        }
    }

    pub fn send(&self, cmd: EngineCommand) {
        let _ = self.tx.send(cmd);
    }

    pub fn shutdown(&mut self) {
        let _ = self.tx.send(EngineCommand::Shutdown);
        if let Some(handle) = self.join.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for EngineHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn build_initial_snapshot(
    config: &AppConfig,
    lhm_auth_configured: bool,
    last_notice: Option<String>,
) -> EngineSnapshot {
    let frames = render_all_frames(
        &config.zones,
        &config.scene,
        0.0,
        &SensorSnapshot::default(),
        &InputContext::empty(),
        None,
    );
    EngineSnapshot {
        config: config.clone(),
        effective_scene: config.scene.clone(),
        effective_curve: config.active_curve.clone(),
        trial: None,
        automation_status: AutomationStatus {
            paused: config.automations_paused,
            ..AutomationStatus::default()
        },
        identify_zone: None,
        average_rgb: avg_rgb(&frames),
        frames,
        studio_preview_frames: Vec::new(),
        sensors: SensorSnapshot::default(),
        cpu_temp_history: Vec::new(),
        gpu_temp_history: Vec::new(),
        ssd_temp_history: Vec::new(),
        net_down_history: Vec::new(),
        cpu_power_history: Vec::new(),
        gpu_power_history: Vec::new(),
        cost_history: Vec::new(),
        fan_temp: None,
        fan_target: config.settings.fan_failsafe_duty as f32,
        fan_duty: config.settings.fan_failsafe_duty,
        fan_rpms: Vec::new(),
        lhm_channels: Vec::new(),
        lhm_auth_configured,
        conflicts: Vec::new(),
        idle_secs: 0,
        workstation_locked: false,
        studio_test_beat: false,
        studio_paused: false,
        studio_speed_mult: 1.0,
        audio_frame: AudioFrame::default(),
        audio_bands: [0.0; AUDIO_BANDS],
        audio_bass: 0.0,
        cpu_temp_peak: 0.0,
        gpu_temp_peak: 0.0,
        power_peak_w: 0.0,
        sleep_timer_remaining_s: None,
        activity_log: Vec::new(),
        cooling_recording: false,
        cooling_sample_count: 0,
        cooling_session_count: 0,
        cooling_sessions: Vec::new(),
        last_notice,
        update_status: UpdateStatus::Idle,
        hid_devices: Vec::new(),
        openrgb: OpenRgbStatus {
            connected: false,
            controller_count: 0,
            message: "Scanning…".to_string(),
            controllers: Vec::new(),
        },
        fps: 0.0,
        tick_count: 0,
    }
}

fn scene_needs_inputs(zones: &[ZoneConfig], scene: &SceneConfig) -> (bool, u8, bool) {
    if !scene.on || scene.brightness == 0 {
        return (false, 1, false);
    }
    let any_default_zone = zones
        .iter()
        .any(|z| z.enabled && !scene.zone_overrides.contains_key(&z.id));
    let mut need_screen = any_default_zone && scene.effect.needs_screen();
    let mut display = scene.params.display;
    let mut need_audio = any_default_zone
        && (scene.effect.needs_audio()
            || (scene.effect == EffectId::Studio && scene.studio_stack.uses_audio()));

    for (zid, ov) in &scene.zone_overrides {
        if !zones.iter().any(|z| z.enabled && z.id == *zid) {
            continue;
        }
        if ov.effect.needs_screen() {
            need_screen = true;
            display = ov.params.display;
        }
        if ov.effect.needs_audio() {
            need_audio = true;
        }
        if ov.effect == EffectId::Studio {
            let stack = ov.studio_stack.as_ref().unwrap_or(&scene.studio_stack);
            if stack.uses_audio() {
                need_audio = true;
            }
        }
    }
    (need_screen, display, need_audio)
}

fn scene_is_animated(scene: &SceneConfig) -> bool {
    if !scene.on || scene.brightness == 0 {
        return false;
    }
    if scene.effect.is_animated() {
        return true;
    }
    scene
        .zone_overrides
        .values()
        .any(|ov| ov.effect.is_animated())
}

fn render_all_frames(
    zones: &[ZoneConfig],
    scene: &SceneConfig,
    t: f32,
    sensors: &SensorSnapshot,
    inputs: &InputContext,
    identify_zone: Option<&str>,
) -> Vec<ZoneFrame> {
    let geoms: Vec<ZoneGeometry> = zones.iter().map(ZoneGeometry::from_layout).collect();
    render_all_frames_cached(zones, &geoms, scene, t, sensors, inputs, identify_zone)
}

fn render_all_frames_cached(
    zones: &[ZoneConfig],
    geoms: &[ZoneGeometry],
    scene: &SceneConfig,
    t: f32,
    sensors: &SensorSnapshot,
    inputs: &InputContext,
    identify_zone: Option<&str>,
) -> Vec<ZoneFrame> {
    zones
        .iter()
        .zip(geoms.iter())
        .map(|(z, geom)| {
            let colors = if identify_zone == Some(z.id.as_str()) {
                let pulse = if (t * 4.0).sin() >= 0.0 { 255 } else { 40 };
                vec![[pulse, pulse, pulse]; geom.len()]
            } else if z.enabled {
                if let Some(ov) = scene.zone_overrides.get(&z.id) {
                    let stack = ov.studio_stack.as_ref().unwrap_or(&scene.studio_stack);
                    effects::render_zone_with_inputs(
                        ov.effect,
                        &ov.params,
                        stack,
                        geom,
                        t,
                        scene.brightness,
                        scene.on,
                        sensors,
                        inputs,
                    )
                } else {
                    effects::render_zone_with_inputs(
                        scene.effect,
                        &scene.params,
                        &scene.studio_stack,
                        geom,
                        t,
                        scene.brightness,
                        scene.on,
                        sensors,
                        inputs,
                    )
                }
            } else {
                vec![[0, 0, 0]; geom.len()]
            };
            ZoneFrame {
                zone_id: geom.zone_id.clone(),
                zone_name: geom.zone_name.clone(),
                enabled: z.enabled,
                layout: z.layout.clone(),
                canvas_pos: geom.canvas_pos.clone(),
                fan_rings: geom.fan_rings.clone(),
                colors,
            }
        })
        .collect()
}

/// Evaluates the Studio `LayerStack` across all configured rig zones at full brightness for
/// real-rig live preview on the Studio tab (without altering live hardware lighting output).
pub fn render_studio_preview_frames(
    zones: &[ZoneConfig],
    geoms: &[ZoneGeometry],
    stack: &LayerStack,
    t: f32,
    sensors: &SensorSnapshot,
    inputs: &InputContext,
) -> Vec<ZoneFrame> {
    let default_params = SceneParams::default();
    zones
        .iter()
        .zip(geoms.iter())
        .map(|(z, geom)| {
            let colors = if z.enabled {
                effects::render_zone_with_inputs(
                    EffectId::Studio,
                    &default_params,
                    stack,
                    geom,
                    t,
                    100,
                    true,
                    sensors,
                    inputs,
                )
            } else {
                vec![[0, 0, 0]; geom.len()]
            };
            ZoneFrame {
                zone_id: geom.zone_id.clone(),
                zone_name: geom.zone_name.clone(),
                enabled: z.enabled,
                layout: z.layout.clone(),
                canvas_pos: geom.canvas_pos.clone(),
                fan_rings: geom.fan_rings.clone(),
                colors,
            }
        })
        .collect()
}

fn avg_rgb(frames: &[ZoneFrame]) -> [u8; 3] {
    let (mut r, mut g, mut b, mut count) = (0u64, 0u64, 0u64, 0u64);
    for frame in frames {
        for &c in &frame.colors {
            r += c[0] as u64;
            g += c[1] as u64;
            b += c[2] as u64;
            count += 1;
        }
    }
    if count == 0 {
        return [0, 0, 0];
    }
    [(r / count) as u8, (g / count) as u8, (b / count) as u8]
}

fn push_ring(buf: &mut VecDeque<f32>, val: f32) {
    if buf.len() >= HISTORY_LEN {
        buf.pop_front();
    }
    buf.push_back(val);
}

fn cycle_order(cur: ColorOrder) -> ColorOrder {
    match cur {
        ColorOrder::Rgb => ColorOrder::Rbg,
        ColorOrder::Rbg => ColorOrder::Grb,
        ColorOrder::Grb => ColorOrder::Gbr,
        ColorOrder::Gbr => ColorOrder::Bgr,
        ColorOrder::Bgr => ColorOrder::Brg,
        ColorOrder::Brg => ColorOrder::Rgb,
    }
}

fn slugify(name: &str, fallback: &str) -> String {
    let s: String = name
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let trimmed = s.trim_matches('-').to_string();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed
    }
}

fn compute_effective(
    config: &AppConfig,
    status: &AutomationStatus,
    locked: bool,
) -> (SceneConfig, String) {
    let mut scene = config.scene.clone();
    let mut curve = config.active_curve.clone();

    if let Some(winner_id) = &status.light_winner
        && let Some(rule) = config.automations.iter().find(|r| &r.id == winner_id)
    {
        match &rule.action {
            Action::LightsOff => {
                scene.on = false;
            }
            Action::Dim { brightness } => {
                scene.brightness = scene.brightness.min(*brightness);
            }
            Action::ApplyPreset { preset } => {
                if let Some(p) = config.presets.iter().find(|p| &p.id == preset) {
                    scene.on = true;
                    scene.effect = p.effect;
                    scene.params = p.params.clone();
                    scene.brightness = p.brightness;
                    if let Some(stack) = &p.studio_stack {
                        scene.studio_stack = stack.clone();
                    }
                    scene.zone_overrides = p.zone_overrides.clone();
                }
            }
            Action::FanCurve { .. } => {}
        }
    }

    if let Some(winner_id) = &status.fan_winner
        && let Some(rule) = config.automations.iter().find(|r| &r.id == winner_id)
    {
        match &rule.action {
            Action::FanCurve { curve: c } if config.fan_curves.contains_key(c) => {
                curve = c.clone();
            }
            Action::ApplyPreset { preset } => {
                if let Some(p) = config.presets.iter().find(|p| &p.id == preset)
                    && config.fan_curves.contains_key(&p.fan_curve)
                {
                    curve = p.fan_curve.clone();
                }
            }
            _ => {}
        }
    }

    if locked && config.settings.lights_off_on_lock {
        scene.on = false;
    }

    (scene, curve)
}

fn apply_preset_to_scene(scene: &mut SceneConfig, preset: &Preset) {
    scene.on = true;
    scene.effect = preset.effect;
    scene.params = preset.params.clone();
    scene.brightness = preset.brightness;
    if let Some(stack) = &preset.studio_stack {
        scene.studio_stack = stack.clone();
    }
    scene.zone_overrides = preset.zone_overrides.clone();
    scene.active_studio_effect = None;
}

fn spawn_update_check(channel: UpdateChannel, tx: mpsc::Sender<UpdateStatus>) {
    let _ = thread::Builder::new()
        .name("airglow-updater".to_string())
        .spawn(move || {
            let status = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt.block_on(updates::check_github_updates_async(channel)),
                Err(e) => UpdateStatus::Error(format!("Update runtime error: {e}")),
            };
            let _ = tx.send(status);
        });
}

fn spawn_update_download(
    exe_url: String,
    sha256_url: Option<String>,
    latest: String,
    tx: mpsc::Sender<UpdateStatus>,
) {
    let _ = thread::Builder::new()
        .name("airglow-updater-dl".to_string())
        .spawn(move || {
            let progress_tx = tx.clone();
            let progress_latest = latest.clone();
            let status = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt.block_on(updates::download_update_with_progress_async(
                    exe_url,
                    sha256_url,
                    latest,
                    move |pct| {
                        let _ = progress_tx.send(UpdateStatus::Downloading {
                            latest: progress_latest.clone(),
                            progress_pct: pct,
                        });
                    },
                )),
                Err(e) => UpdateStatus::Error(format!("Update runtime error: {e}")),
            };
            let _ = tx.send(status);
        });
}

fn apply_set_ui_scale_pct(
    config: &mut AppConfig,
    pct: u16,
    store: &ConfigStore,
    in_trial: bool,
    dirty_save: &mut bool,
    last_save: &mut Instant,
    watch_tx: &watch::Sender<EngineSnapshot>,
) {
    config.appearance.ui_scale_pct = pct;
    config.appearance.normalize();
    if !in_trial {
        let _ = store.save(config);
        *dirty_save = false;
        *last_save = Instant::now();
    } else {
        *dirty_save = true;
    }
    watch_tx.send_modify(|snap| {
        snap.config.appearance = config.appearance.clone();
    });
}

fn run_engine(
    store: ConfigStore,
    mut config: AppConfig,
    mut lhm_auth: Option<String>,
    mut last_notice: Option<String>,
    cmd_rx: mpsc::Receiver<EngineCommand>,
    watch_tx: watch::Sender<EngineSnapshot>,
) {
    let started = Instant::now();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let shared_cfg = Arc::new(Mutex::new(WorkerSharedConfig {
        lhm_url: config.settings.lhm_url.clone(),
        demo_mode: config.settings.demo_mode,
        lhm_auth: lhm_auth.clone(),
        lhm_targets: BTreeMap::new(),
    }));
    let (sensor_tx, sensor_rx) = mpsc::channel::<SensorWorkerReport>();

    // Offload LHM HTTP sensor polling, LHM fan POST writes, sysinfo process scans,
    // and conflict detection onto a dedicated worker so the 30 FPS render loop never stalls.
    let worker_stop = Arc::clone(&stop_flag);
    let worker_cfg = Arc::clone(&shared_cfg);
    let sensor_worker = thread::Builder::new()
        .name("airglow-sensors".to_string())
        .spawn(move || {
            let mut poller = SensorPoller::new();
            let mut lhm_ctrl = LhmFanController::new();
            let mut polls = 0u64;
            while !worker_stop.load(Ordering::Relaxed) {
                let cfg_snap = worker_cfg
                    .lock()
                    .map(|g| g.clone())
                    .unwrap_or_else(|_| WorkerSharedConfig {
                        lhm_url: "http://127.0.0.1:8085".to_string(),
                        demo_mode: true,
                        lhm_auth: None,
                        lhm_targets: BTreeMap::new(),
                    });
                let elapsed = started.elapsed().as_secs_f32();
                let snap = poller.poll(
                    &cfg_snap.lhm_url,
                    cfg_snap.lhm_auth.as_deref(),
                    cfg_snap.demo_mode,
                    elapsed,
                );
                if snap.lhm_connected {
                    lhm_ctrl.apply(
                        &cfg_snap.lhm_url,
                        cfg_snap.lhm_auth.as_deref(),
                        &cfg_snap.lhm_targets,
                    );
                } else {
                    lhm_ctrl.resend_all();
                }

                let exes = poller.running_exe_names();
                let conflicts = if polls % 3 == 0 {
                    system::detect_conflicting_apps(None)
                } else {
                    let list: Vec<(String, Option<std::path::PathBuf>)> =
                        exes.iter().map(|e| (e.clone(), None)).collect();
                    system::detect_conflicting_apps_from_list(&list, None)
                };

                let report = SensorWorkerReport {
                    sensors: snap,
                    exes,
                    conflicts,
                    lhm_errors: lhm_ctrl.errors().clone(),
                };
                if sensor_tx.send(report).is_err() {
                    break;
                }
                polls += 1;
                let wait = if polls < 3 {
                    Duration::from_millis(800)
                } else {
                    Duration::from_secs(2)
                };
                let slept_until = Instant::now() + wait;
                while Instant::now() < slept_until && !worker_stop.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(100));
                }
            }
            // Hand every LHM fan channel cleanly back to BIOS/firmware control on shutdown.
            if let Ok(cfg_snap) = worker_cfg.lock() {
                lhm_ctrl.release_all(&cfg_snap.lhm_url, cfg_snap.lhm_auth.as_deref());
            }
        })
        .ok();

    let mut sensors = SensorSnapshot::default();
    let mut running_exes: HashSet<String> = HashSet::new();
    let mut conflicts: Vec<ConflictingApp> = Vec::new();
    let mut lhm_errors: HashMap<String, String> = HashMap::new();

    // Offload all blocking USB HID (rescan, direct RGB writes, fan duty/RPM) and OpenRGB SDK
    // TCP (connect_timeout, UpdateLEDs) I/O onto a dedicated worker thread so the render loop
    // maintains a silky-smooth ~30 FPS even with multiple real USB + OpenRGB controllers attached.
    let hw_desired = Arc::new((
        Mutex::new(HwDesiredState {
            frames: Vec::new(),
            zones: config.zones.clone(),
            frame_gen: 0,
            fan_duty: None,
            openrgb_enabled: config.settings.openrgb_enabled,
            openrgb_host: config.settings.openrgb_host.clone(),
            openrgb_port: config.settings.openrgb_port,
            rescan_gen: 1,
            shutdown: false,
        }),
        Condvar::new(),
    ));
    let hw_status = Arc::new(Mutex::new(HwStatusReport {
        hid_devices: Vec::new(),
        openrgb: OpenRgbStatus {
            connected: false,
            controller_count: 0,
            message: "Scanning…".to_string(),
            controllers: Vec::new(),
        },
        fan_rpms: Vec::new(),
        scan_gen: 0,
    }));
    let hw_worker = {
        let desired = Arc::clone(&hw_desired);
        let status = Arc::clone(&hw_status);
        thread::Builder::new()
            .name("airglow-hw".to_string())
            .spawn(move || {
                let mut hid_mgr = HidOutputManager::new();
                let mut openrgb_client = OpenRgbClient::new();
                let mut openrgb_status = OpenRgbStatus {
                    connected: false,
                    controller_count: 0,
                    message: "Scanning…".to_string(),
                    controllers: Vec::new(),
                };
                let mut hid_devices: Vec<DiscoveredDevice> = Vec::new();
                let mut last_frame_gen = 0u64;
                let mut last_rescan_gen = 0u64;
                let mut next_periodic_scan = Instant::now() + Duration::from_millis(120);
                let mut last_pushed_colors: Vec<Vec<[u8; 3]>> = Vec::new();
                let mut last_push_time = Instant::now() - HW_KEEPALIVE_INTERVAL;
                let mut scan_gen = 0u64;

                loop {
                    let (frames, zones, frame_gen, fan_duty, orgb_en, orgb_host, orgb_port, rescan_gen, shutdown) = {
                        let (lock, cvar) = &*desired;
                        let mut guard = match lock.lock() {
                            Ok(g) => g,
                            Err(p) => p.into_inner(),
                        };
                        if !guard.shutdown
                            && guard.frame_gen == last_frame_gen
                            && guard.rescan_gen == last_rescan_gen
                        {
                            let wait = next_periodic_scan
                                .saturating_duration_since(Instant::now())
                                .min(Duration::from_millis(200));
                            let (g, _) = cvar
                                .wait_timeout(guard, wait)
                                .unwrap_or_else(|p| p.into_inner());
                            guard = g;
                        }
                        (
                            if guard.frame_gen != last_frame_gen {
                                Some(guard.frames.clone())
                            } else {
                                None
                            },
                            guard.zones.clone(),
                            guard.frame_gen,
                            guard.fan_duty,
                            guard.openrgb_enabled,
                            guard.openrgb_host.clone(),
                            guard.openrgb_port,
                            guard.rescan_gen,
                            guard.shutdown,
                        )
                    };

                    if shutdown {
                        hid_mgr.blackout();
                        openrgb_client.disconnect();
                        break;
                    }

                    let now = Instant::now();
                    let force_rescan = rescan_gen != last_rescan_gen;
                    if force_rescan || now >= next_periodic_scan {
                        if force_rescan {
                            openrgb_client.disconnect();
                            last_rescan_gen = rescan_gen;
                        }
                        hid_devices = hid_mgr.rescan();
                        openrgb_status = if orgb_en {
                            openrgb_client.ensure_connected(&orgb_host, orgb_port)
                        } else {
                            openrgb_client.disconnect();
                            OpenRgbStatus {
                                connected: false,
                                controller_count: 0,
                                message: "OpenRGB disabled".to_string(),
                                controllers: Vec::new(),
                            }
                        };
                        next_periodic_scan = Instant::now() + Duration::from_secs(10);
                        scan_gen += 1;
                    }

                    if let Some(duty) = fan_duty {
                        hid_mgr.write_fan_duty(duty);
                    }
                    let fan_rpms = hid_mgr.poll_fan_rpms();

                    if let Some(ref cur_frames) = frames {
                        last_frame_gen = frame_gen;
                        let hw_changed = !frames_colors_equal(&last_pushed_colors, cur_frames);
                        let keepalive_due =
                            now.duration_since(last_push_time) >= HW_KEEPALIVE_INTERVAL;
                        if hw_changed || keepalive_due {
                            hid_mgr.write_zone_frames(cur_frames, &zones);
                            if orgb_en && openrgb_status.connected {
                                let flat_colors: Vec<[u8; 3]> = cur_frames
                                    .iter()
                                    .flat_map(|f| f.colors.iter().copied())
                                    .collect();
                                openrgb_client.push_colors(&flat_colors, ColorOrder::Rgb);
                            }
                            if last_pushed_colors.len() != cur_frames.len() {
                                last_pushed_colors =
                                    cur_frames.iter().map(|f| f.colors.clone()).collect();
                            } else {
                                for (slot, f) in
                                    last_pushed_colors.iter_mut().zip(cur_frames.iter())
                                {
                                    slot.clear();
                                    slot.extend_from_slice(&f.colors);
                                }
                            }
                            last_push_time = now;
                        }
                    }

                    if let Ok(mut st) = status.lock() {
                        st.hid_devices = hid_devices.clone();
                        st.openrgb = openrgb_status.clone();
                        st.fan_rpms = fan_rpms;
                        st.scan_gen = scan_gen;
                    }
                }
            })
            .ok()
    };

    let mut hid_devices: Vec<DiscoveredDevice> = Vec::new();
    let mut openrgb_status = OpenRgbStatus {
        connected: false,
        controller_count: 0,
        message: "Scanning…".to_string(),
        controllers: Vec::new(),
    };
    let mut hw_fan_rpms: Vec<(String, u16)> = Vec::new();
    let mut last_seen_scan_gen: u64 = 0;

    let mut screen_sampler = ScreenSampler::new();
    let mut audio_analyzer = AudioAnalyzer::new();
    let mut cooling_recorder = CoolingRecorder::new(store.dir());

    let mut cpu_temp_hist = VecDeque::with_capacity(HISTORY_LEN);
    let mut gpu_temp_hist = VecDeque::with_capacity(HISTORY_LEN);
    let mut ssd_temp_hist = VecDeque::with_capacity(HISTORY_LEN);
    let mut net_down_hist = VecDeque::with_capacity(HISTORY_LEN);
    let mut cpu_power_hist = VecDeque::with_capacity(HISTORY_LEN);
    let mut gpu_power_hist = VecDeque::with_capacity(HISTORY_LEN);
    let mut cost_hist = VecDeque::with_capacity(HISTORY_LEN);
    let mut seen_lhm_live = false;

    let mut prev_active_rules: HashSet<String> = HashSet::new();
    let mut identify_target: Option<(String, Instant)> = None;
    let mut trial: Option<TrialState> = None;

    let mut studio_test_beat = false;
    let mut studio_paused = false;
    let mut studio_speed_mult: f32 = 1.0;
    let mut studio_preview_active = false;
    let mut anim_time: f32 = 0.0;
    let mut surprise_counter: u64 = 1;

    let mut idle_secs: u64 = 0;
    let mut workstation_locked = false;
    let mut last_sys_signal = Instant::now() - Duration::from_secs(2);

    let mut fan_duty: Option<u8> = None;
    let mut fan_hysteresis = FanHysteresis::new();
    let mut crossfader = RgbCrossfader::new(CROSSFADE_DURATION);
    let mut sleep_timer: Option<SleepTimer> = None;
    let mut activity_log: VecDeque<ActivityEvent> = VecDeque::with_capacity(ACTIVITY_LOG_LEN);
    push_activity(&mut activity_log, "system", "Airglow started");

    let mut cpu_temp_peak: f32 = 0.0;
    let mut gpu_temp_peak: f32 = 0.0;
    let mut power_peak_w: f32 = 0.0;

    let mut prev_light_winner: Option<String> = None;
    let mut prev_fan_winner: Option<String> = None;
    let mut prev_thermal_override = false;
    let mut prev_hid_online: HashSet<String> = HashSet::new();
    let mut prev_openrgb_connected = false;
    let mut prev_scene_sig: Option<(bool, EffectId, Option<String>, usize)> = None;

    let mut cached_geoms: Vec<ZoneGeometry> =
        config.zones.iter().map(ZoneGeometry::from_layout).collect();
    let mut zones_dirty = false;
    let mut last_hw_colors: Vec<Vec<[u8; 3]>> = Vec::new();
    let mut last_hw_push = Instant::now() - HW_KEEPALIVE_INTERVAL;

    let mut last_frame_time = Instant::now();
    let mut next_tick_deadline = Instant::now();
    let mut smoothed_fps: f32 = 30.0;
    let mut dirty_save = false;
    let mut last_save = Instant::now();
    let mut tick_count: u64 = 0;

    let (update_tx, update_rx) = mpsc::channel::<UpdateStatus>();
    let mut update_status = UpdateStatus::Idle;
    let mut auto_update_checked = false;
    let mut exit_after_shutdown = false;

    loop {
        let mut shutdown = false;
        let cmd_now = Instant::now();
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                EngineCommand::Shutdown => shutdown = true,
                EngineCommand::SetPower(on) => {
                    if config.scene.on != on {
                        config.scene.on = on;
                        crossfader.trigger(cmd_now);
                        if !on {
                            sleep_timer = None;
                        }
                        push_activity(
                            &mut activity_log,
                            "lighting",
                            if on { "Lights turned on" } else { "Lights turned off" },
                        );
                    }
                    dirty_save = trial.is_none();
                }
                EngineCommand::SetBrightness(b) => {
                    config.scene.brightness = b.min(100);
                    config.active_preset = None;
                    dirty_save = trial.is_none();
                }
                EngineCommand::SetEffect(eff) => {
                    if config.scene.effect != eff {
                        crossfader.trigger(cmd_now);
                        push_activity(
                            &mut activity_log,
                            "lighting",
                            format!("Effect: {}", eff.label()),
                        );
                    }
                    config.scene.effect = eff;
                    config.scene.active_studio_effect = None;
                    config.active_preset = None;
                    dirty_save = trial.is_none();
                }
                EngineCommand::SetParams(p) => {
                    config.scene.params = p;
                    config.active_preset = None;
                    dirty_save = trial.is_none();
                }
                EngineCommand::ApplyHarmony(idx) => {
                    if let Some(h) = effects::HARMONIES.get(idx) {
                        crossfader.trigger(cmd_now);
                        config.scene.params.colors = h.colors.to_vec();
                        config.scene.params.color = h.colors[0];
                        config.scene.params.secondary = h.colors[1];
                        config.scene.params.hot_color = h.colors[2];
                        if config.scene.effect == EffectId::Studio {
                            for (i, layer) in config
                                .scene
                                .studio_stack
                                .layers
                                .iter_mut()
                                .enumerate()
                            {
                                layer.color = h.colors[i % 3];
                                layer.secondary = h.colors[(i + 1) % 3];
                            }
                        }
                        config.active_preset = None;
                        last_notice = Some(format!("Applied {} palette", h.name));
                        push_activity(
                            &mut activity_log,
                            "lighting",
                            format!("Applied {} palette", h.name),
                        );
                        dirty_save = trial.is_none();
                    }
                }
                EngineCommand::SetZoneOverride {
                    zone_id,
                    override_cfg,
                } => {
                    crossfader.trigger(cmd_now);
                    config.scene.zone_overrides.insert(zone_id, override_cfg);
                    config.active_preset = None;
                    dirty_save = trial.is_none();
                }
                EngineCommand::ClearZoneOverride(zone_id) => {
                    crossfader.trigger(cmd_now);
                    config.scene.zone_overrides.remove(&zone_id);
                    dirty_save = trial.is_none();
                }
                EngineCommand::ClearAllZoneOverrides => {
                    crossfader.trigger(cmd_now);
                    config.scene.zone_overrides.clear();
                    dirty_save = trial.is_none();
                }
                EngineCommand::SetStudioStack(stack) => {
                    if config.scene.effect != EffectId::Studio {
                        crossfader.trigger(cmd_now);
                    }
                    config.scene.studio_stack = stack;
                    config.scene.effect = EffectId::Studio;
                    config.active_preset = None;
                    dirty_save = trial.is_none();
                }
                EngineCommand::AddStudioLayer(kind) => {
                    let id = config.scene.studio_stack.next_id;
                    config.scene.studio_stack.next_id += 1;
                    config
                        .scene
                        .studio_stack
                        .layers
                        .push(Layer::preset(id, kind));
                    config.scene.effect = EffectId::Studio;
                    dirty_save = trial.is_none();
                }
                EngineCommand::RemoveStudioLayer(id) => {
                    config.scene.studio_stack.layers.retain(|l| l.id != id);
                    dirty_save = trial.is_none();
                }
                EngineCommand::MoveStudioLayer { id, up } => {
                    let layers = &mut config.scene.studio_stack.layers;
                    if let Some(pos) = layers.iter().position(|l| l.id == id) {
                        if up && pos > 0 {
                            layers.swap(pos, pos - 1);
                        } else if !up && pos + 1 < layers.len() {
                            layers.swap(pos, pos + 1);
                        }
                    }
                    dirty_save = trial.is_none();
                }
                EngineCommand::UpdateStudioLayer(updated) => {
                    if let Some(slot) = config
                        .scene
                        .studio_stack
                        .layers
                        .iter_mut()
                        .find(|l| l.id == updated.id)
                    {
                        *slot = updated;
                    }
                    dirty_save = trial.is_none();
                }
                EngineCommand::StudioSurpriseMe => {
                    crossfader.trigger(cmd_now);
                    surprise_counter = surprise_counter.wrapping_add(1);
                    let seed = started.elapsed().as_nanos() as u64 ^ (surprise_counter * 0x9E37);
                    config.scene.studio_stack = effects::surprise_me_stack(seed);
                    config.scene.effect = EffectId::Studio;
                    config.scene.active_studio_effect = None;
                    config.active_preset = None;
                    dirty_save = trial.is_none();
                }
                EngineCommand::SaveStudioEffect(name) => {
                    let trimmed = name.trim();
                    if !trimmed.is_empty() {
                        let id = slugify(trimmed, &format!("studio-{tick_count}"));
                        let mut stack = config.scene.studio_stack.clone();
                        stack.name = trimmed.to_string();
                        let saved = SavedStudioEffect {
                            id: id.clone(),
                            name: trimmed.to_string(),
                            stack,
                        };
                        if let Some(slot) = config.studio_effects.iter_mut().find(|e| e.id == id) {
                            *slot = saved;
                        } else if config.studio_effects.len() < 32 {
                            config.studio_effects.push(saved);
                        }
                        config.scene.active_studio_effect = Some(id);
                        last_notice = Some(format!("Saved Studio effect \"{trimmed}\""));
                        dirty_save = true;
                    }
                }
                EngineCommand::ApplyStudioEffect(id) => {
                    if let Some(found) = config.studio_effects.iter().find(|e| e.id == id).cloned()
                    {
                        crossfader.trigger(cmd_now);
                        let ename = found.name.clone();
                        config.scene.studio_stack = found.stack;
                        config.scene.effect = EffectId::Studio;
                        config.scene.on = true;
                        config.scene.active_studio_effect = Some(found.id);
                        config.active_preset = None;
                        push_activity(
                            &mut activity_log,
                            "lighting",
                            format!("Studio effect: {ename}"),
                        );
                        dirty_save = trial.is_none();
                    }
                }
                EngineCommand::DuplicateStudioEffect(id) => {
                    if let Some(found) = config.studio_effects.iter().find(|e| e.id == id).cloned()
                    {
                        let new_name = format!("{} copy", found.name);
                        let new_id = format!("{}-copy-{}", found.id, tick_count % 1000);
                        let mut stack = found.stack;
                        stack.name = new_name.clone();
                        if config.studio_effects.len() < 32 {
                            config.studio_effects.push(SavedStudioEffect {
                                id: new_id,
                                name: new_name,
                                stack,
                            });
                            dirty_save = true;
                        }
                    }
                }
                EngineCommand::DeleteStudioEffect(id) => {
                    config.studio_effects.retain(|e| e.id != id);
                    if config.scene.active_studio_effect.as_deref() == Some(id.as_str()) {
                        config.scene.active_studio_effect = None;
                    }
                    dirty_save = true;
                }
                EngineCommand::SetStudioTestBeat(on) => {
                    studio_test_beat = on;
                    audio_analyzer.set_test_beat(on);
                }
                EngineCommand::SetStudioPaused(paused) => {
                    studio_paused = paused;
                }
                EngineCommand::SetStudioSpeedMult(mult) => {
                    studio_speed_mult = mult.clamp(0.1, 4.0);
                }
                EngineCommand::SetStudioPreviewActive(active) => {
                    studio_preview_active = active;
                }
                EngineCommand::ApplyPreset(pid) => {
                    if let Some(preset) = config.presets.iter().find(|p| p.id == pid).cloned() {
                        trial = None;
                        crossfader.trigger(cmd_now);
                        let pname = preset.name.clone();
                        apply_preset_to_scene(&mut config.scene, &preset);
                        if config.fan_curves.contains_key(&preset.fan_curve) {
                            config.active_curve = preset.fan_curve;
                        }
                        config.active_preset = Some(preset.id);
                        push_activity(
                            &mut activity_log,
                            "preset",
                            format!("Applied profile \"{pname}\""),
                        );
                        dirty_save = true;
                    }
                }
                EngineCommand::TryPreset(pid) => {
                    if let Some(preset) = config.presets.iter().find(|p| p.id == pid).cloned() {
                        crossfader.trigger(cmd_now);
                        if trial.is_none() {
                            trial = Some(TrialState {
                                info: TrialInfo {
                                    preset_id: preset.id.clone(),
                                    preset_name: preset.name.clone(),
                                },
                                before_scene: config.scene.clone(),
                                before_curve: config.active_curve.clone(),
                                before_preset: config.active_preset.clone(),
                            });
                        } else if let Some(t_state) = &mut trial {
                            t_state.info = TrialInfo {
                                preset_id: preset.id.clone(),
                                preset_name: preset.name.clone(),
                            };
                        }
                        apply_preset_to_scene(&mut config.scene, &preset);
                        if config.fan_curves.contains_key(&preset.fan_curve) {
                            config.active_curve = preset.fan_curve;
                        }
                        config.active_preset = None;
                    }
                }
                EngineCommand::KeepTrial => {
                    if let Some(t_state) = trial.take() {
                        config.active_preset = Some(t_state.info.preset_id);
                        last_notice = Some(format!("Kept profile \"{}\"", t_state.info.preset_name));
                        push_activity(
                            &mut activity_log,
                            "preset",
                            format!("Kept profile \"{}\"", t_state.info.preset_name),
                        );
                        dirty_save = true;
                    }
                }
                EngineCommand::RevertTrial => {
                    if let Some(t_state) = trial.take() {
                        crossfader.trigger(cmd_now);
                        config.scene = t_state.before_scene;
                        config.active_curve = t_state.before_curve;
                        config.active_preset = t_state.before_preset;
                    }
                }
                EngineCommand::SaveCurrentPreset(name) => {
                    let trimmed = name.trim();
                    if !trimmed.is_empty() {
                        let id = slugify(trimmed, &format!("preset-{tick_count}"));
                        let preset = Preset {
                            id: id.clone(),
                            name: trimmed.to_string(),
                            description: String::new(),
                            effect: config.scene.effect,
                            params: config.scene.params.clone(),
                            brightness: config.scene.brightness,
                            fan_curve: config.active_curve.clone(),
                            studio_stack: (config.scene.effect == EffectId::Studio)
                                .then(|| config.scene.studio_stack.clone()),
                            zone_overrides: config.scene.zone_overrides.clone(),
                        };
                        if let Some(slot) = config.presets.iter_mut().find(|p| p.id == id) {
                            *slot = preset;
                        } else if config.presets.len() < 24 {
                            config.presets.push(preset);
                        }
                        config.active_preset = Some(id);
                        trial = None;
                        dirty_save = true;
                    }
                }
                EngineCommand::DeletePreset(pid) => {
                    if !matches!(pid.as_str(), "silent" | "balanced" | "gaming" | "night") {
                        config.presets.retain(|p| p.id != pid);
                        if config.active_preset.as_deref() == Some(pid.as_str()) {
                            config.active_preset = None;
                        }
                        dirty_save = true;
                    }
                }
                EngineCommand::ImportPresetJson(raw) => match import_preset_json(&raw) {
                    Ok(mut preset) => {
                        if !config.fan_curves.contains_key(&preset.fan_curve) {
                            preset.fan_curve = "balanced".to_string();
                        }
                        let name = preset.name.clone();
                        if let Some(slot) = config.presets.iter_mut().find(|p| p.id == preset.id) {
                            *slot = preset;
                        } else if config.presets.len() < 24 {
                            config.presets.push(preset);
                        }
                        last_notice = Some(format!("Imported profile \"{name}\""));
                        dirty_save = true;
                    }
                    Err(e) => {
                        last_notice = Some(format!("Profile import failed: {e}"));
                    }
                },
                EngineCommand::SelectFanCurve(name) => {
                    if config.fan_curves.contains_key(&name) {
                        config.active_curve = name;
                        config.active_preset = None;
                        dirty_save = trial.is_none();
                    }
                }
                EngineCommand::UpdateFanCurve { name, mut points } => {
                    points.sort_by_key(|p| p.temp);
                    config.fan_curves.insert(name, points);
                    dirty_save = true;
                }
                EngineCommand::ResetFanCurve(name) => {
                    let defaults = default_curves();
                    if let Some(pts) = defaults.get(&name) {
                        config.fan_curves.insert(name, pts.clone());
                        dirty_save = true;
                    }
                }
                EngineCommand::SetFanChannelMode { channel_id, mode } => {
                    if mode == FanChannelMode::Auto {
                        config.fan_channels.remove(&channel_id);
                    } else {
                        config.fan_channels.insert(channel_id, mode);
                    }
                    dirty_save = true;
                }
                EngineCommand::SetFanHysteresis(hyst_c) => {
                    config.settings.fan_hysteresis_c = hyst_c.clamp(0.0, 10.0);
                    dirty_save = true;
                }
                EngineCommand::StartSleepTimer(minutes) => {
                    if minutes == 0 {
                        if sleep_timer.take().is_some() {
                            last_notice = Some("Sleep timer cancelled.".to_string());
                            push_activity(&mut activity_log, "sleep", "Sleep timer cancelled");
                        }
                    } else {
                        let mins = minutes.min(480);
                        sleep_timer = Some(SleepTimer::start(cmd_now, mins));
                        last_notice = Some(format!("Sleep timer: {mins} min"));
                        push_activity(
                            &mut activity_log,
                            "sleep",
                            format!("Sleep timer set for {mins} min"),
                        );
                    }
                }
                EngineCommand::CancelSleepTimer => {
                    if sleep_timer.take().is_some() {
                        last_notice = Some("Sleep timer cancelled.".to_string());
                        push_activity(&mut activity_log, "sleep", "Sleep timer cancelled");
                    }
                }
                EngineCommand::StartCoolingRecording => {
                    let note = format!("Curve: {}", config.active_curve);
                    match cooling_recorder.start(note) {
                        Ok(_) => {
                            last_notice = Some("Cooling recording started.".to_string());
                        }
                        Err(e) => {
                            last_notice = Some(format!("Could not start recording: {e}"));
                        }
                    }
                }
                EngineCommand::StopCoolingRecording => match cooling_recorder.stop() {
                    Ok(sess) => {
                        last_notice = Some(format!(
                            "Saved cooling recording ({} samples).",
                            sess.samples.len()
                        ));
                    }
                    Err(e) => {
                        last_notice = Some(format!("Could not stop recording: {e}"));
                    }
                },
                EngineCommand::ExportCoolingCsv => {
                    let target_id = cooling_recorder
                        .active_session()
                        .map(|s| s.id.clone())
                        .or_else(|| cooling_recorder.sessions().last().map(|s| s.id.clone()));
                    match target_id {
                        Some(id) => match cooling_recorder.export_csv_to_file(&id, store.dir()) {
                            Ok(path) => {
                                last_notice =
                                    Some(format!("Exported cooling CSV to {}", path.display()));
                            }
                            Err(e) => {
                                last_notice = Some(format!("CSV export failed: {e}"));
                            }
                        },
                        None => {
                            last_notice =
                                Some("No cooling recording to export yet.".to_string());
                        }
                    }
                }
                EngineCommand::UpdateSettings(settings) => {
                    let rescan = settings.openrgb_enabled != config.settings.openrgb_enabled
                        || settings.openrgb_host != config.settings.openrgb_host
                        || settings.openrgb_port != config.settings.openrgb_port;
                    if settings.start_with_windows != config.settings.start_with_windows
                        && let Err(e) = system::set_autostart(settings.start_with_windows)
                    {
                        last_notice = Some(format!("Autostart update failed: {e}"));
                    }
                    if let Ok(mut cfg) = shared_cfg.lock() {
                        cfg.lhm_url = settings.lhm_url.clone();
                        cfg.demo_mode = settings.demo_mode;
                    }
                    config.settings = settings;
                    config.settings.fan_hysteresis_c =
                        config.settings.fan_hysteresis_c.clamp(0.0, 10.0);
                    config.settings.start_with_windows = system::is_autostart_enabled();
                    if rescan {
                        let (lock, cvar) = &*hw_desired;
                        if let Ok(mut hw) = lock.lock() {
                            hw.openrgb_enabled = config.settings.openrgb_enabled;
                            hw.openrgb_host = config.settings.openrgb_host.clone();
                            hw.openrgb_port = config.settings.openrgb_port;
                            hw.rescan_gen = hw.rescan_gen.wrapping_add(1);
                            cvar.notify_one();
                        }
                    }
                    dirty_save = true;
                }
                EngineCommand::UpdateAppearance(appearance) => {
                    config.appearance = appearance.normalized();
                    dirty_save = true;
                }
                EngineCommand::UpdateWindowState(ws) => {
                    config.window_state = ws.normalized();
                    dirty_save = true;
                }
                EngineCommand::SaveLook(name) => {
                    let trimmed = name.trim();
                    if !trimmed.is_empty() {
                        let id = slugify(trimmed, &format!("look-{tick_count}"));
                        let look = SavedLook {
                            id: id.clone(),
                            name: trimmed.to_string(),
                            design: config.settings.design,
                            accent_override: config.appearance.accent_override,
                            rig_style: config.appearance.rig_style,
                            firefly_particles: config.appearance.firefly_particles,
                        };
                        if let Some(slot) = config.saved_looks.iter_mut().find(|l| l.id == id) {
                            *slot = look;
                        } else if config.saved_looks.len() < 24 {
                            config.saved_looks.push(look);
                        }
                        last_notice = Some(format!("Saved look \"{trimmed}\""));
                        push_activity(
                            &mut activity_log,
                            "appearance",
                            format!("Saved look \"{trimmed}\""),
                        );
                        dirty_save = true;
                    }
                }
                EngineCommand::ApplyLook(id) => {
                    if let Some(look) = config.saved_looks.iter().find(|l| l.id == id).cloned() {
                        config.settings.design = look.design;
                        config.appearance.accent_override = look.accent_override;
                        config.appearance.rig_style = look.rig_style;
                        config.appearance.firefly_particles = look.firefly_particles;
                        last_notice = Some(format!("Applied look \"{}\"", look.name));
                        push_activity(
                            &mut activity_log,
                            "appearance",
                            format!("Applied look \"{}\"", look.name),
                        );
                        dirty_save = true;
                    }
                }
                EngineCommand::DeleteLook(id) => {
                    let before = config.saved_looks.len();
                    config.saved_looks.retain(|l| l.id != id);
                    if config.saved_looks.len() != before {
                        last_notice = Some("Deleted saved look.".to_string());
                        dirty_save = true;
                    }
                }
                EngineCommand::SetDesign(design) => {
                    config.settings.design = design;
                    dirty_save = true;
                }
                EngineCommand::SetTempUnit(unit) => {
                    config.settings.temp_unit = unit;
                    dirty_save = true;
                }
                EngineCommand::SetStartWithWindows(enabled) => {
                    match system::set_autostart(enabled) {
                        Ok(()) => {
                            config.settings.start_with_windows = system::is_autostart_enabled();
                            last_notice = Some(if enabled {
                                "Airglow will start with Windows.".to_string()
                            } else {
                                "Removed Airglow from Windows startup.".to_string()
                            });
                            dirty_save = true;
                        }
                        Err(e) => {
                            last_notice = Some(format!("Autostart update failed: {e}"));
                        }
                    }
                }
                EngineCommand::ToggleZone(zid) => {
                    if let Some(z) = config.zones.iter_mut().find(|z| z.id == zid) {
                        z.enabled = !z.enabled;
                        crossfader.trigger(cmd_now);
                        dirty_save = true;
                    }
                }
                EngineCommand::IdentifyZone(zid) => {
                    identify_target =
                        Some((zid, Instant::now() + Duration::from_secs_f32(IDENTIFY_SECS)));
                }
                EngineCommand::UpdateZone(updated) => {
                    if let Some(slot) = config.zones.iter_mut().find(|z| z.id == updated.id) {
                        *slot = updated;
                        zones_dirty = true;
                        dirty_save = true;
                    }
                }
                EngineCommand::UpdateZoneLayout { zone_id, x, y } => {
                    if let Some(z) = config.zones.iter_mut().find(|z| z.id == zone_id) {
                        let cx = x.clamp(0.08, CANVAS_W - 0.08);
                        let cy = y.clamp(0.08, CANVAS_H - 0.08);
                        z.layout.set_center(cx, cy);
                        zones_dirty = true;
                        dirty_save = true;
                    }
                }
                EngineCommand::CycleZoneColorOrder(zid) => {
                    if let Some(z) = config.zones.iter_mut().find(|z| z.id == zid) {
                        z.color_order = cycle_order(z.color_order);
                        dirty_save = true;
                    }
                }
                EngineCommand::AddAutomation(rule) => {
                    config.automations.push(rule);
                    dirty_save = true;
                }
                EngineCommand::UpdateAutomation(updated) => {
                    if let Some(slot) = config.automations.iter_mut().find(|r| r.id == updated.id) {
                        *slot = updated;
                        dirty_save = true;
                    }
                }
                EngineCommand::ToggleAutomation(id) => {
                    if let Some(r) = config.automations.iter_mut().find(|r| r.id == id) {
                        r.enabled = !r.enabled;
                        dirty_save = true;
                    }
                }
                EngineCommand::DeleteAutomation(id) => {
                    config.automations.retain(|r| r.id != id);
                    dirty_save = true;
                }
                EngineCommand::MoveAutomation { id, up } => {
                    if let Some(pos) = config.automations.iter().position(|r| r.id == id) {
                        if up && pos > 0 {
                            config.automations.swap(pos, pos - 1);
                        } else if !up && pos + 1 < config.automations.len() {
                            config.automations.swap(pos, pos + 1);
                        }
                        dirty_save = true;
                    }
                }
                EngineCommand::SetAutomationsPaused(paused) => {
                    config.automations_paused = paused;
                    dirty_save = true;
                }
                EngineCommand::CloseConflictingApp(name) => {
                    match system::close_conflicting_process(&name) {
                        Ok(res) => {
                            if !res.closed.is_empty() {
                                last_notice = Some(format!(
                                    "Closed {name} ({}).",
                                    res.closed.join(", ")
                                ));
                                conflicts.retain(|c| !c.name.eq_ignore_ascii_case(&name));
                            } else if !res.services.is_empty() {
                                last_notice = Some(format!(
                                    "{name} runs as a Windows service ({}); stop it in Services.",
                                    res.services.join(", ")
                                ));
                            } else if !res.failed.is_empty() {
                                last_notice = Some(format!(
                                    "Could not close {name} (try running as administrator)."
                                ));
                            } else {
                                last_notice = Some(format!("{name} is no longer running."));
                            }
                        }
                        Err(e) => {
                            last_notice = Some(format!("Could not close {name}: {e}"));
                        }
                    }
                }
                EngineCommand::ImportPythonSettings => {
                    match migrate::default_python_airglow_dir() {
                        Some(dir) if dir.is_dir() => match migrate::import_python_airglow(&dir) {
                            Ok(migrated) => {
                                let zc = migrated.zones.len();
                                let pc = migrated.presets.len();
                                let fc = migrated.fan_channels.len();
                                apply_migrated_data(&mut config, &migrated);
                                zones_dirty = true;
                                crossfader.trigger(cmd_now);
                                lhm_auth = migrate::load_lhm_auth_header();
                                if let Ok(mut wcfg) = shared_cfg.lock() {
                                    wcfg.lhm_url = config.settings.lhm_url.clone();
                                    wcfg.demo_mode = config.settings.demo_mode;
                                    wcfg.lhm_auth = lhm_auth.clone();
                                }
                                let _ = store.save(&config);
                                dirty_save = false;
                                last_save = Instant::now();
                                last_notice = Some(format!(
                                    "Imported Python Airglow settings ({zc} zones, {pc} presets, {fc} fan channels)."
                                ));
                            }
                            Err(e) => {
                                last_notice = Some(format!("Import failed: {e}"));
                            }
                        },
                        _ => {
                            last_notice =
                                Some("No Python Airglow folder found in %APPDATA%\\Airglow.".to_string());
                        }
                    }
                }
                EngineCommand::ExportDiagnostics => {
                    let summary = serde_json::json!({
                        "version": env!("CARGO_PKG_VERSION"),
                        "settings": &config.settings,
                        "active_curve": &config.active_curve,
                        "active_preset": &config.active_preset,
                        "zones_count": config.zones.len(),
                        "presets_count": config.presets.len(),
                        "automations_count": config.automations.len(),
                        "lhm_connected": sensors.lhm_connected,
                        "lhm_auth_configured": lhm_auth.is_some(),
                        "lhm_fan_channels": sensors.lhm_fan_channels.len(),
                        "cpu_name": &sensors.cpu_name,
                        "gpu_name": &sensors.gpu_name,
                        "cpu_temp": sensors.cpu_temp,
                        "gpu_temp": sensors.gpu_temp,
                        "openrgb": {
                            "connected": openrgb_status.connected,
                            "controller_count": openrgb_status.controller_count,
                            "message": &openrgb_status.message,
                        },
                        "hid_devices": hid_devices.iter().map(|d| serde_json::json!({
                            "id": d.id, "name": d.name, "online": d.online, "note": d.note
                        })).collect::<Vec<_>>(),
                        "conflicts": &conflicts,
                    });
                    match system::export_diagnostics_report(store.dir(), &summary) {
                        Ok(path) => {
                            last_notice = Some(format!(
                                "Saved diagnostics report to {}",
                                path.display()
                            ));
                        }
                        Err(e) => {
                            last_notice = Some(format!("Diagnostics export failed: {e}"));
                        }
                    }
                }
                EngineCommand::ExportRigBundle => match system::export_rig_bundle(&config) {
                    Ok(path) => {
                        last_notice =
                            Some(format!("Exported rig backup to {}", path.display()));
                        push_activity(&mut activity_log, "system", "Exported rig backup");
                    }
                    Err(e) => {
                        last_notice = Some(format!("Rig backup export failed: {e}"));
                    }
                },
                EngineCommand::ImportRigBundle(raw_path) => {
                    let trimmed = raw_path.trim().trim_matches('"').trim();
                    if trimmed.is_empty() {
                        last_notice =
                            Some("Enter a path to an Airglow rig backup JSON file.".to_string());
                    } else {
                        match system::import_rig_bundle(std::path::Path::new(trimmed)) {
                            Ok(mut imported) => {
                                let rescan = imported.settings.openrgb_enabled
                                    != config.settings.openrgb_enabled
                                    || imported.settings.openrgb_host
                                        != config.settings.openrgb_host
                                    || imported.settings.openrgb_port
                                        != config.settings.openrgb_port;
                                imported.settings.start_with_windows =
                                    system::is_autostart_enabled();
                                config = imported;
                                trial = None;
                                zones_dirty = true;
                                crossfader.trigger(cmd_now);
                                if let Ok(mut wcfg) = shared_cfg.lock() {
                                    wcfg.lhm_url = config.settings.lhm_url.clone();
                                    wcfg.demo_mode = config.settings.demo_mode;
                                }
                                if rescan {
                                    let (lock, cvar) = &*hw_desired;
                                    if let Ok(mut hw) = lock.lock() {
                                        hw.openrgb_enabled = config.settings.openrgb_enabled;
                                        hw.openrgb_host = config.settings.openrgb_host.clone();
                                        hw.openrgb_port = config.settings.openrgb_port;
                                        hw.rescan_gen = hw.rescan_gen.wrapping_add(1);
                                        cvar.notify_one();
                                    }
                                }
                                let _ = store.save(&config);
                                dirty_save = false;
                                last_save = Instant::now();
                                last_notice =
                                    Some(format!("Restored rig backup from {trimmed}"));
                                push_activity(&mut activity_log, "system", "Restored rig backup");
                            }
                            Err(e) => {
                                last_notice = Some(format!("Rig backup import failed: {e}"));
                            }
                        }
                    }
                }
                EngineCommand::OpenUrl(url) => {
                    if let Err(e) = system::open_url(&url) {
                        last_notice = Some(format!("Could not open URL: {e}"));
                    }
                }
                EngineCommand::OpenConfigFolder => {
                    if let Err(e) = system::open_config_folder() {
                        last_notice = Some(format!("Could not open config folder: {e}"));
                    }
                }
                EngineCommand::CheckForUpdates => {
                    if !matches!(
                        update_status,
                        UpdateStatus::Checking | UpdateStatus::Downloading { .. }
                    ) {
                        auto_update_checked = true;
                        update_status = UpdateStatus::Checking;
                        spawn_update_check(config.update_channel, update_tx.clone());
                    }
                }
                EngineCommand::DownloadAndStageUpdate => {
                    if let UpdateStatus::Available {
                        latest,
                        exe_url,
                        sha256_url,
                        ..
                    } = update_status.clone()
                    {
                        if let Some(url) = exe_url {
                            update_status = UpdateStatus::Downloading {
                                latest: latest.clone(),
                                progress_pct: 0,
                            };
                            spawn_update_download(url, sha256_url, latest, update_tx.clone());
                        } else {
                            let msg = "This release has no Airglow.exe asset — open GitHub Releases to download manually.".to_string();
                            update_status = UpdateStatus::Error(msg.clone());
                            last_notice = Some(msg);
                        }
                    }
                }
                EngineCommand::ApplyStagedUpdateAndRestart => {
                    if let UpdateStatus::ReadyToRestart { staged_exe, .. } = update_status.clone() {
                        match updates::apply_staged_update_and_restart(&staged_exe) {
                            Ok(()) => {
                                last_notice = Some("Restarting to apply update…".to_string());
                                dirty_save = true;
                                shutdown = true;
                                exit_after_shutdown = true;
                            }
                            Err(e) => {
                                let msg = format!("Could not launch updater: {e}");
                                update_status = UpdateStatus::Error(msg.clone());
                                last_notice = Some(msg);
                            }
                        }
                    }
                }
                EngineCommand::SetUpdateChannel(channel) => {
                    if config.update_channel != channel {
                        config.update_channel = channel;
                        dirty_save = true;
                    }
                }
                EngineCommand::SetAutoCheckUpdates(enabled) => {
                    if config.auto_check_updates != enabled {
                        config.auto_check_updates = enabled;
                        dirty_save = true;
                    }
                }
                EngineCommand::SetFullscreenFillWidth(fill) => {
                    if config.fullscreen_fill_width != fill {
                        config.fullscreen_fill_width = fill;
                        dirty_save = true;
                    }
                }
                EngineCommand::SetUiScalePct(pct) => {
                    apply_set_ui_scale_pct(
                        &mut config,
                        pct,
                        &store,
                        trial.is_some(),
                        &mut dirty_save,
                        &mut last_save,
                        &watch_tx,
                    );
                }
                EngineCommand::AddOverviewTile(kind) => {
                    let label = kind.label().to_string();
                    config.add_overview_tile(kind);
                    last_notice = Some(format!("Added \"{label}\" tile to Overview."));
                    dirty_save = true;
                }
                EngineCommand::RemoveOverviewTile(id) => {
                    if config.remove_overview_tile(&id) {
                        dirty_save = true;
                    }
                }
                EngineCommand::MoveOverviewTile { id, delta } => {
                    if config.move_overview_tile(&id, delta) {
                        dirty_save = true;
                    }
                }
                EngineCommand::ReorderOverviewTile { id, to_index } => {
                    if config.reorder_overview_tile_to(&id, to_index) {
                        dirty_save = true;
                    }
                }
                EngineCommand::ResizeOverviewTile { id, span, rows } => {
                    if config.resize_overview_tile(&id, span, rows) {
                        dirty_save = true;
                    }
                }
                EngineCommand::CycleOverviewTileSpan(id) => {
                    if config.cycle_overview_tile_span(&id).is_some() {
                        dirty_save = true;
                    }
                }
                EngineCommand::ToggleOverviewTileTall(id) => {
                    if config.toggle_overview_tile_tall(&id).is_some() {
                        dirty_save = true;
                    }
                }
                EngineCommand::CycleOverviewTileStyle(id) => {
                    if config.cycle_overview_tile_style(&id).is_some() {
                        dirty_save = true;
                    }
                }
                EngineCommand::ResetActiveWorkspace => {
                    config.reset_active_workspace();
                    last_notice = Some(format!(
                        "Reset \"{}\" layout to defaults.",
                        config.active_workspace().name
                    ));
                    dirty_save = true;
                }
                EngineCommand::SwitchWorkspace(id) => {
                    if config.switch_workspace(&id) {
                        dirty_save = true;
                    }
                }
                EngineCommand::CreateWorkspace { name, template_id } => {
                    config.create_workspace(&name, template_id.as_deref());
                    last_notice = Some(format!(
                        "Created workspace \"{}\".",
                        config.active_workspace().name
                    ));
                    dirty_save = true;
                }
                EngineCommand::RenameWorkspace { id, name } => {
                    if config.rename_workspace(&id, &name) {
                        dirty_save = true;
                    }
                }
                EngineCommand::DeleteWorkspace(id) => {
                    if config.delete_workspace(&id) {
                        last_notice = Some("Deleted workspace.".to_string());
                        dirty_save = true;
                    }
                }
                EngineCommand::ClearNotice => {
                    last_notice = None;
                }
                EngineCommand::RescanHardware => {
                    let (lock, cvar) = &*hw_desired;
                    if let Ok(mut hw) = lock.lock() {
                        hw.rescan_gen = hw.rescan_gen.wrapping_add(1);
                        cvar.notify_one();
                    }
                }
            }
        }

        if shutdown {
            stop_flag.store(true, Ordering::Relaxed);
            audio_analyzer.stop();
            if cooling_recorder.is_recording() {
                let _ = cooling_recorder.stop();
            }
            {
                let (lock, cvar) = &*hw_desired;
                if let Ok(mut hw) = lock.lock() {
                    hw.shutdown = true;
                    cvar.notify_one();
                }
            }
            if dirty_save && trial.is_none() {
                let _ = store.save(&config);
            }
            if let Some(worker) = hw_worker {
                let _ = worker.join();
            }
            if let Some(worker) = sensor_worker {
                let _ = worker.join();
            }
            if exit_after_shutdown {
                std::process::exit(0);
            }
            break;
        }

        while let Ok(next_status) = update_rx.try_recv() {
            match &next_status {
                UpdateStatus::Available { latest, .. } => {
                    push_activity(
                        &mut activity_log,
                        "update",
                        format!("Airglow v{latest} available"),
                    );
                }
                UpdateStatus::ReadyToRestart { latest, .. } => {
                    last_notice = Some(format!(
                        "Airglow v{latest} downloaded — restart to apply."
                    ));
                    push_activity(
                        &mut activity_log,
                        "update",
                        format!("Airglow v{latest} staged and ready to restart"),
                    );
                }
                UpdateStatus::Error(msg) => {
                    push_activity(&mut activity_log, "update", format!("Update error: {msg}"));
                }
                _ => {}
            }
            update_status = next_status;
        }

        if !auto_update_checked
            && config.auto_check_updates
            && started.elapsed() >= Duration::from_secs(10)
            && matches!(update_status, UpdateStatus::Idle)
        {
            auto_update_checked = true;
            update_status = UpdateStatus::Checking;
            spawn_update_check(config.update_channel, update_tx.clone());
        }

        let now = Instant::now();
        let dt = now.duration_since(last_frame_time).as_secs_f32().max(0.001);
        last_frame_time = now;
        let instant_fps = (1.0 / dt).clamp(1.0, 120.0);
        smoothed_fps = smoothed_fps * 0.8 + instant_fps * 0.2;

        let mut got_sensor_tick = false;
        while let Ok(report) = sensor_rx.try_recv() {
            sensors = report.sensors;
            running_exes = report.exes;
            conflicts = report.conflicts;
            lhm_errors = report.lhm_errors;
            got_sensor_tick = true;

            // The first time real LHM telemetry connects, discard any prior synthetic demo
            // samples so live graphs and peaks never mix fake sine waves with real LHM data.
            if sensors.lhm_connected && !seen_lhm_live {
                seen_lhm_live = true;
                cpu_temp_hist.clear();
                gpu_temp_hist.clear();
                ssd_temp_hist.clear();
                net_down_hist.clear();
                cpu_power_hist.clear();
                gpu_power_hist.clear();
                cost_hist.clear();
                cpu_temp_peak = 0.0;
                gpu_temp_peak = 0.0;
                power_peak_w = 0.0;
            }

            // Once real LHM is live, only record genuine LHM samples into telemetry history.
            if !seen_lhm_live || sensors.lhm_connected {
                if let Some(t) = sensors.cpu_temp {
                    cpu_temp_peak = cpu_temp_peak.max(t);
                    push_ring(&mut cpu_temp_hist, t);
                }
                if let Some(t) = sensors.gpu_temp {
                    gpu_temp_peak = gpu_temp_peak.max(t);
                    push_ring(&mut gpu_temp_hist, t);
                }
                if let Some(t) = sensors.ssd_temp {
                    push_ring(&mut ssd_temp_hist, t);
                }
                if let Some(bps) = sensors.net_down_bps {
                    push_ring(&mut net_down_hist, bps);
                }
                if let Some(w) = sensors.cpu_power_w {
                    push_ring(&mut cpu_power_hist, w);
                }
                if let Some(w) = sensors.gpu_power_w {
                    push_ring(&mut gpu_power_hist, w);
                }
                if let Some(total_w) = sensors.total_power_w {
                    power_peak_w = power_peak_w.max(total_w);
                    let cost = (total_w / 1000.0) * config.settings.energy_price_eur_kwh;
                    push_ring(&mut cost_hist, cost);
                }
            }
        }

        if now.duration_since(last_sys_signal) >= Duration::from_millis(500) {
            idle_secs = system::idle_seconds().max(0.0) as u64;
            workstation_locked = system::is_workstation_locked();
            last_sys_signal = now;
        }

        // Poll latest non-blocking hardware discovery & fan RPM snapshot from airglow-hw worker
        if let Ok(st) = hw_status.try_lock() {
            hw_fan_rpms = st.fan_rpms.clone();
            if st.scan_gen != last_seen_scan_gen {
                last_seen_scan_gen = st.scan_gen;
                hid_devices = st.hid_devices.clone();
                let cur_hid_online: HashSet<String> = hid_devices
                    .iter()
                    .filter(|d| d.online)
                    .map(|d| d.name.clone())
                    .collect();
                for added in cur_hid_online.difference(&prev_hid_online) {
                    push_activity(&mut activity_log, "hardware", format!("Connected: {added}"));
                }
                for removed in prev_hid_online.difference(&cur_hid_online) {
                    push_activity(
                        &mut activity_log,
                        "hardware",
                        format!("Disconnected: {removed}"),
                    );
                }
                prev_hid_online = cur_hid_online;

                openrgb_status = st.openrgb.clone();
                if openrgb_status.connected != prev_openrgb_connected {
                    if openrgb_status.connected {
                        push_activity(
                            &mut activity_log,
                            "hardware",
                            format!(
                                "OpenRGB connected ({} controllers)",
                                openrgb_status.controller_count
                            ),
                        );
                    } else {
                        push_activity(&mut activity_log, "hardware", "OpenRGB disconnected");
                    }
                    prev_openrgb_connected = openrgb_status.connected;
                }
            }
        }

        let (hour, min, weekday_mon0) = automations::local_time_hm_wday();
        let auto_ctx = AutomationContext {
            running_exes: &running_exes,
            local_hm: (hour, min),
            weekday_mon0,
            idle_secs,
            locked: workstation_locked,
            sensors: &sensors,
        };
        let auto_status = automations::evaluate_rules(
            &config.automations,
            config.automations_paused,
            &auto_ctx,
            &prev_active_rules,
            &config.presets,
        );
        prev_active_rules = auto_status.active_ids.iter().cloned().collect();

        if auto_status.light_winner != prev_light_winner {
            match &auto_status.light_winner {
                Some(id) => {
                    let summary = config
                        .automations
                        .iter()
                        .find(|r| &r.id == id)
                        .map(|r| r.action.describe(&config.presets))
                        .unwrap_or_else(|| format!("Rule {id}"));
                    push_activity(
                        &mut activity_log,
                        "automation",
                        format!("Automation active: {summary}"),
                    );
                }
                None => {
                    push_activity(
                        &mut activity_log,
                        "automation",
                        "Lighting automation ended — restored manual scene",
                    );
                }
            }
            prev_light_winner = auto_status.light_winner.clone();
        }
        if auto_status.fan_winner != prev_fan_winner {
            if let Some(id) = &auto_status.fan_winner {
                if auto_status.light_winner.as_ref() != Some(id) {
                    let summary = config
                        .automations
                        .iter()
                        .find(|r| &r.id == id)
                        .map(|r| r.action.describe(&config.presets))
                        .unwrap_or_else(|| id.clone());
                    push_activity(
                        &mut activity_log,
                        "automation",
                        format!("Fan automation active: {summary}"),
                    );
                }
            } else if prev_fan_winner.is_some() {
                push_activity(
                    &mut activity_log,
                    "automation",
                    "Fan automation ended — restored manual curve",
                );
            }
            prev_fan_winner = auto_status.fan_winner.clone();
        }

        // Handle Sleep / Sunset timer expiration and smooth final-2-minute dimming
        let mut sleep_brightness_scale = 1.0f32;
        let mut sleep_timer_remaining_s: Option<u32> = None;
        if let Some(timer) = sleep_timer {
            if timer.is_expired(now) {
                sleep_timer = None;
                if config.scene.on {
                    config.scene.on = false;
                    crossfader.trigger(now);
                    dirty_save = trial.is_none();
                }
                last_notice = Some("Sleep timer finished — lights turned off.".to_string());
                push_activity(
                    &mut activity_log,
                    "sleep",
                    "Sleep timer finished — lights off",
                );
            } else {
                sleep_timer_remaining_s = Some(timer.remaining_secs(now));
                sleep_brightness_scale = timer.brightness_scale(now);
            }
        }

        let (mut effective_scene, effective_curve) =
            compute_effective(&config, &auto_status, workstation_locked);
        if sleep_brightness_scale < 1.0 && effective_scene.on {
            effective_scene.brightness = ((effective_scene.brightness as f32)
                * sleep_brightness_scale)
                .round()
                .clamp(0.0, 100.0) as u8;
        }

        // Trigger smooth 350ms crossfade whenever automation/lock/command alters the active scene signature
        let cur_scene_sig = (
            effective_scene.on,
            effective_scene.effect,
            effective_scene.active_studio_effect.clone(),
            effective_scene.zone_overrides.len(),
        );
        if let Some(prev_sig) = &prev_scene_sig
            && *prev_sig != cur_scene_sig
            && !crossfader.is_active(now)
        {
            crossfader.trigger(now);
        }
        prev_scene_sig = Some(cur_scene_sig);

        let fan_temp = fans::source_temp(&sensors, config.settings.temp_source);
        let effective_fan_temp =
            fan_hysteresis.update(fan_temp, config.settings.fan_hysteresis_c);
        let curve_pts = config
            .fan_curves
            .get(&effective_curve)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let fan_target = fans::target_duty(
            curve_pts,
            effective_fan_temp,
            config.settings.fan_failsafe_duty,
        );
        let stepped = fans::step_toward(fan_duty, fan_target, config.settings.fan_max_step);
        fan_duty = Some(stepped);

        // Compute LHM motherboard/AIO channel targets and publish to sensor worker
        let mut lhm_targets: BTreeMap<String, u8> = BTreeMap::new();
        let hottest_temp = fans::source_temp(&sensors, crate::config::TempSource::Max);
        let thermal_override = hottest_temp.is_some_and(|t| t >= crate::config::SAFETY_TEMP);
        if thermal_override != prev_thermal_override {
            if thermal_override {
                push_activity(
                    &mut activity_log,
                    "safety",
                    "Thermal safety override (≥ 90°C) — fans at 100%",
                );
            } else {
                push_activity(
                    &mut activity_log,
                    "safety",
                    "Thermal safety cleared — returning to fan curve",
                );
            }
            prev_thermal_override = thermal_override;
        }
        let lhm_channels: Vec<LhmChannelStatus> = sensors
            .lhm_fan_channels
            .iter()
            .map(|ch| {
                let mode = config
                    .fan_channels
                    .get(&ch.id)
                    .copied()
                    .unwrap_or(FanChannelMode::Auto);
                let target_duty = match mode {
                    FanChannelMode::Auto => None,
                    FanChannelMode::Curve => {
                        let d = if thermal_override { 100 } else { stepped };
                        lhm_targets.insert(ch.id.clone(), d);
                        Some(d)
                    }
                    FanChannelMode::Steady { duty } => {
                        let d = if thermal_override {
                            100
                        } else {
                            duty.clamp(crate::config::MIN_FAN_DUTY, 100)
                        };
                        lhm_targets.insert(ch.id.clone(), d);
                        Some(d)
                    }
                };
                LhmChannelStatus {
                    id: ch.id.clone(),
                    name: ch.name.clone(),
                    hardware: ch.hardware.clone(),
                    rpm: ch.rpm,
                    duty: ch.duty,
                    is_pump: ch.is_pump,
                    mode,
                    target_duty,
                    error: lhm_errors.get(&ch.id).cloned(),
                }
            })
            .collect();

        if let Ok(mut wcfg) = shared_cfg.lock() {
            wcfg.lhm_targets = lhm_targets;
        }

        let mut fan_rpms = hw_fan_rpms.clone();
        if fan_rpms.is_empty() && !sensors.lhm_fan_channels.is_empty() {
            for ch in &sensors.lhm_fan_channels {
                if let Some(rpm) = ch.rpm
                    && rpm > 0
                {
                    fan_rpms.push((ch.name.clone(), rpm));
                }
            }
        }
        if fan_rpms.is_empty() && config.settings.demo_mode {
            let base = 420 + (stepped as u16) * 14;
            fan_rpms = vec![
                ("Front Intake".to_string(), base),
                ("Top Radiator".to_string(), base.saturating_sub(45)),
                ("Rear Exhaust".to_string(), base + 60),
            ];
        }

        // Record cooling sample when active
        if got_sensor_tick && cooling_recorder.is_recording() {
            let avg_rpm = if fan_rpms.is_empty() {
                None
            } else {
                let sum: u32 = fan_rpms.iter().map(|(_, r)| *r as u32).sum();
                Some(sum as f32 / fan_rpms.len() as f32)
            };
            let curve_meta = CoolingCurveMeta {
                name: effective_curve.clone(),
                source: format!("{:?}", config.settings.temp_source).to_ascii_lowercase(),
                failsafe: config.settings.fan_failsafe_duty,
                points: curve_pts.iter().map(|p| [p.temp, p.duty]).collect(),
            };
            let ch_samples = lhm_channels
                .iter()
                .map(|c| CoolingChannelSample {
                    id: c.id.clone(),
                    name: c.name.clone(),
                    mode: match c.mode {
                        FanChannelMode::Auto => None,
                        FanChannelMode::Curve => Some("curve".to_string()),
                        FanChannelMode::Steady { .. } => Some("steady".to_string()),
                    },
                    rpm: c.rpm.map(|r| r as f64),
                    target: c.target_duty.map(|d| d as f64),
                    duty: c.duty.map(|d| d as f64),
                })
                .collect();
            cooling_recorder.record_sample(CoolingSampleInput {
                cpu: sensors.cpu_temp,
                gpu: sensors.gpu_temp,
                liquid: sensors.liquid_temp,
                temp: fan_temp,
                duty: Some(stepped as f32),
                rpm: avg_rpm,
                curve: Some(curve_meta),
                channels: ch_samples,
            });
        }

        let active_identify = match &identify_target {
            Some((zid, until)) if *until > now => Some(zid.clone()),
            _ => {
                identify_target = None;
                None
            }
        };

        if !studio_paused {
            anim_time += dt * studio_speed_mult;
        }
        let render_t = if effective_scene.effect == EffectId::Studio {
            anim_time
        } else {
            started.elapsed().as_secs_f32()
        };

        let (need_screen, display, need_audio_scene) =
            scene_needs_inputs(&config.zones, &effective_scene);
        let render_studio_preview =
            studio_preview_active && effective_scene.effect != EffectId::Studio;
        let need_audio = need_audio_scene
            || studio_test_beat
            || (render_studio_preview && config.scene.studio_stack.uses_audio());
        let input_ctx = InputContext::capture(
            &mut screen_sampler,
            need_screen,
            display,
            &mut audio_analyzer,
            need_audio,
            if render_studio_preview || effective_scene.effect == EffectId::Studio {
                anim_time
            } else {
                render_t
            },
        );
        let audio_frame = input_ctx.audio_frame();

        if zones_dirty || cached_geoms.len() != config.zones.len() {
            cached_geoms = config.zones.iter().map(ZoneGeometry::from_layout).collect();
            zones_dirty = false;
        }
        let mut frames = render_all_frames_cached(
            &config.zones,
            &cached_geoms,
            &effective_scene,
            render_t,
            &sensors,
            &input_ctx,
            active_identify.as_deref(),
        );
        let crossfade_active = crossfader.apply(&mut frames, now);

        let studio_preview_frames = if render_studio_preview {
            render_studio_preview_frames(
                &config.zones,
                &cached_geoms,
                &config.scene.studio_stack,
                anim_time,
                &sensors,
                &input_ctx,
            )
        } else {
            Vec::new()
        };

        // Hand off changed frames & fan duty non-blockingly to airglow-hw worker
        let hw_changed = !frames_colors_equal(&last_hw_colors, &frames);
        let keepalive_due = now.duration_since(last_hw_push) >= HW_KEEPALIVE_INTERVAL;
        if hw_changed || keepalive_due {
            let (lock, cvar) = &*hw_desired;
            if let Ok(mut hw) = lock.lock() {
                hw.frames = frames.clone();
                hw.zones = config.zones.clone();
                hw.fan_duty = Some(stepped);
                hw.frame_gen = hw.frame_gen.wrapping_add(1);
                cvar.notify_one();
            }
            if last_hw_colors.len() != frames.len() {
                last_hw_colors = frames.iter().map(|f| f.colors.clone()).collect();
            } else {
                for (slot, f) in last_hw_colors.iter_mut().zip(frames.iter()) {
                    slot.clear();
                    slot.extend_from_slice(&f.colors);
                }
            }
            last_hw_push = now;
        } else {
            let (lock, cvar) = &*hw_desired;
            if let Ok(mut hw) = lock.try_lock()
                && hw.fan_duty != Some(stepped)
            {
                hw.fan_duty = Some(stepped);
                cvar.notify_one();
            }
        }

        let average_rgb = avg_rgb(&frames);
        tick_count += 1;

        if dirty_save && trial.is_none() && now.duration_since(last_save) >= Duration::from_millis(600) {
            let _ = store.save(&config);
            dirty_save = false;
            last_save = now;
        }

        let _ = watch_tx.send(EngineSnapshot {
            config: config.clone(),
            effective_scene: effective_scene.clone(),
            effective_curve,
            trial: trial.as_ref().map(|t| t.info.clone()),
            automation_status: auto_status,
            identify_zone: active_identify.clone(),
            frames,
            studio_preview_frames,
            average_rgb,
            sensors: sensors.clone(),
            cpu_temp_history: cpu_temp_hist.iter().copied().collect(),
            gpu_temp_history: gpu_temp_hist.iter().copied().collect(),
            ssd_temp_history: ssd_temp_hist.iter().copied().collect(),
            net_down_history: net_down_hist.iter().copied().collect(),
            cpu_power_history: cpu_power_hist.iter().copied().collect(),
            gpu_power_history: gpu_power_hist.iter().copied().collect(),
            cost_history: cost_hist.iter().copied().collect(),
            fan_temp,
            fan_target,
            fan_duty: stepped,
            fan_rpms,
            lhm_channels,
            lhm_auth_configured: lhm_auth.is_some(),
            conflicts: conflicts.clone(),
            idle_secs,
            workstation_locked,
            studio_test_beat,
            studio_paused,
            studio_speed_mult,
            audio_frame,
            audio_bands: audio_frame.bands,
            audio_bass: audio_frame.bass,
            cpu_temp_peak,
            gpu_temp_peak,
            power_peak_w,
            sleep_timer_remaining_s,
            activity_log: activity_log.iter().cloned().collect(),
            cooling_recording: cooling_recorder.is_recording(),
            cooling_sample_count: cooling_recorder.active_sample_count(),
            cooling_session_count: cooling_recorder.sessions().len(),
            cooling_sessions: cooling_recorder.summaries(),
            last_notice: last_notice.clone(),
            update_status: update_status.clone(),
            hid_devices: hid_devices.clone(),
            openrgb: openrgb_status.clone(),
            fps: smoothed_fps,
            tick_count,
        });

        let wait_ms = if crossfade_active
            || active_identify.is_some()
            || studio_test_beat
            || (render_studio_preview && !studio_paused)
            || (scene_is_animated(&effective_scene)
                && !(effective_scene.effect == EffectId::Studio && studio_paused))
        {
            ACTIVE_TICK_MS
        } else {
            IDLE_TICK_MS
        };
        let period = Duration::from_millis(wait_ms);
        next_tick_deadline += period;
        let after_work = Instant::now();
        if next_tick_deadline < after_work {
            // Fell behind (e.g., sleep/wake or mode switch from idle): snap deadline cleanly
            next_tick_deadline = after_work;
        }
        let sleep_dur = next_tick_deadline.saturating_duration_since(after_work);
        if !sleep_dur.is_zero() {
            // Compensate slightly for Windows default ~15.6ms timer granularity on 33ms active ticks
            let adjusted = if wait_ms == ACTIVE_TICK_MS && sleep_dur > Duration::from_millis(16) {
                sleep_dur.saturating_sub(Duration::from_millis(8))
            } else {
                sleep_dur
            };
            if !adjusted.is_zero() {
                thread::sleep(adjusted);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_override_renders_independently_and_clears_on_preset() {
        let mut cfg = AppConfig::default();
        cfg.scene.effect = EffectId::Solid;
        cfg.scene.params.color = [0, 0, 255];
        cfg.scene.zone_overrides.insert(
            cfg.zones[0].id.clone(),
            ZoneOverride {
                effect: EffectId::Solid,
                params: SceneParams {
                    color: [255, 0, 0],
                    ..SceneParams::default()
                },
                studio_stack: None,
            },
        );
        let frames = render_all_frames(
            &cfg.zones,
            &cfg.scene,
            0.0,
            &SensorSnapshot::default(),
            &InputContext::empty(),
            None,
        );
        assert_eq!(frames[0].colors[0], [255, 0, 0]);
        assert_eq!(frames[1].colors[0], [0, 0, 255]);

        let preset = cfg.presets[0].clone();
        apply_preset_to_scene(&mut cfg.scene, &preset);
        assert!(cfg.scene.zone_overrides.is_empty());
    }

    #[test]
    fn lights_off_on_lock_blacks_out_effective_scene() {
        let mut cfg = AppConfig::default();
        cfg.settings.lights_off_on_lock = true;
        cfg.scene.on = true;
        let (eff_unlocked, _) = compute_effective(&cfg, &AutomationStatus::default(), false);
        assert!(eff_unlocked.on);
        let (eff_locked, _) = compute_effective(&cfg, &AutomationStatus::default(), true);
        assert!(!eff_locked.on);
    }

    #[test]
    fn applies_migrated_python_data_cleanly() {
        let mut cfg = AppConfig::default();
        let migrated = migrate::MigratedData {
            design: "firefly".to_string(),
            temp_unit: "F".to_string(),
            lights_off_on_lock: true,
            ..migrate::MigratedData::default()
        };
        apply_migrated_data(&mut cfg, &migrated);
        assert_eq!(cfg.settings.design, DesignId::Firefly);
        assert_eq!(cfg.settings.temp_unit, TempUnit::F);
        assert!(cfg.settings.lights_off_on_lock);
    }

    #[test]
    fn rgb_crossfader_interpolates_smoothly_over_350ms() {
        let mut cfg = AppConfig::default();
        cfg.scene.effect = EffectId::Solid;
        cfg.scene.params.color = [200, 0, 0];
        let mut frame_red = render_all_frames(
            &cfg.zones,
            &cfg.scene,
            0.0,
            &SensorSnapshot::default(),
            &InputContext::empty(),
            None,
        );

        let t0 = Instant::now();
        let mut xf = RgbCrossfader::new(CROSSFADE_DURATION);
        // Seed initial frame
        assert!(!xf.apply(&mut frame_red, t0));
        // Trigger transition to blue
        xf.trigger(t0);
        assert!(xf.is_active(t0));

        cfg.scene.params.color = [0, 0, 200];
        let mut mid = render_all_frames(
            &cfg.zones,
            &cfg.scene,
            0.0,
            &SensorSnapshot::default(),
            &InputContext::empty(),
            None,
        );
        // Halfway at 175ms: smoothstep(0.5) = 0.5 -> [100, 0, 100]
        assert!(xf.apply(&mut mid, t0 + Duration::from_millis(175)));
        assert_eq!(mid[0].colors[0], [100, 0, 100]);

        // After 350ms: fully settled at [0, 0, 200]
        let mut done = render_all_frames(
            &cfg.zones,
            &cfg.scene,
            0.0,
            &SensorSnapshot::default(),
            &InputContext::empty(),
            None,
        );
        assert!(!xf.apply(&mut done, t0 + Duration::from_millis(360)));
        assert_eq!(done[0].colors[0], [0, 0, 200]);
    }

    #[test]
    fn sleep_timer_dims_final_two_minutes_and_expires() {
        let t0 = Instant::now();
        let timer = SleepTimer::start(t0, 15);
        assert!(!timer.is_expired(t0));
        assert_eq!(timer.remaining_secs(t0), 900);
        // Outside final 2 minutes: full brightness
        assert!((timer.brightness_scale(t0 + Duration::from_secs(300)) - 1.0).abs() < 1e-4);
        // Halfway through final 2 minutes (60s left): smoothstep(0.5) = 0.5
        let mid_scale = timer.brightness_scale(t0 + Duration::from_secs(840));
        assert!((mid_scale - 0.5).abs() < 0.02);
        // At 15 min: expired and 0 brightness
        assert!(timer.is_expired(t0 + Duration::from_secs(900)));
        assert_eq!(timer.remaining_secs(t0 + Duration::from_secs(900)), 0);
        assert_eq!(timer.brightness_scale(t0 + Duration::from_secs(900)), 0.0);
    }

    #[test]
    fn activity_log_caps_at_thirty_entries_and_inputs_gate_on_enabled_zones() {
        let mut log = VecDeque::with_capacity(ACTIVITY_LOG_LEN);
        for i in 0..45 {
            push_activity_at(&mut log, format!("12:{:02}", i % 60), "test", format!("evt {i}"));
        }
        assert_eq!(log.len(), 30);
        assert_eq!(log.front().unwrap().text, "evt 15");
        assert_eq!(log.back().unwrap().text, "evt 44");

        // Disabled zones should not spin up screen or audio capture
        let mut cfg = AppConfig::default();
        cfg.scene.effect = EffectId::Screen;
        let (need_screen, _, need_audio) = scene_needs_inputs(&cfg.zones, &cfg.scene);
        assert!(need_screen);
        assert!(!need_audio);

        for z in &mut cfg.zones {
            z.enabled = false;
        }
        let (need_screen_off, _, need_audio_off) = scene_needs_inputs(&cfg.zones, &cfg.scene);
        assert!(!need_screen_off);
        assert!(!need_audio_off);
    }

    #[test]
    fn studio_preview_frames_render_all_real_zones_independently_of_active_scene() {
        let mut cfg = AppConfig::default();
        // Active scene is off / solid black, yet Studio preview renders the full stack live
        cfg.scene.on = false;
        cfg.scene.effect = EffectId::Solid;
        let geoms: Vec<ZoneGeometry> = cfg.zones.iter().map(ZoneGeometry::from_layout).collect();
        let preview_a = render_studio_preview_frames(
            &cfg.zones,
            &geoms,
            &cfg.scene.studio_stack,
            0.4,
            &SensorSnapshot::default(),
            &InputContext::empty(),
        );
        let preview_b = render_studio_preview_frames(
            &cfg.zones,
            &geoms,
            &cfg.scene.studio_stack,
            1.9,
            &SensorSnapshot::default(),
            &InputContext::empty(),
        );
        assert_eq!(preview_a.len(), cfg.zones.len());
        for ((frame, zone), geom) in preview_a.iter().zip(cfg.zones.iter()).zip(geoms.iter()) {
            assert_eq!(frame.zone_id, zone.id);
            assert_eq!(frame.colors.len(), geom.len());
            assert!(frame.colors.iter().any(|c| c[0] > 0 || c[1] > 0 || c[2] > 0));
        }
        assert_ne!(preview_a[0].colors, preview_b[0].colors);
    }

    #[test]
    fn set_ui_scale_pct_command_normalizes_persists_and_broadcasts() {
        let dir = std::env::temp_dir().join(format!(
            "airglow_engine_ui_scale_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let store = ConfigStore::new(&dir);
        let mut cfg = AppConfig::default();
        let initial = build_initial_snapshot(&cfg, false, None);
        let (watch_tx, mut watch_rx) = watch::channel(initial);
        let mut dirty_save = false;
        let mut last_save = Instant::now();

        let dispatch = |cmd: EngineCommand,
                        cfg: &mut AppConfig,
                        in_trial: bool,
                        dirty_save: &mut bool,
                        last_save: &mut Instant| {
            if let EngineCommand::SetUiScalePct(pct) = cmd {
                apply_set_ui_scale_pct(
                    cfg, pct, &store, in_trial, dirty_save, last_save, &watch_tx,
                );
            } else {
                panic!("expected SetUiScalePct");
            }
        };

        // Valid preset 125% saves to disk and broadcasts snapshot immediately
        let _ = watch_rx.borrow_and_update();
        dispatch(
            EngineCommand::SetUiScalePct(125),
            &mut cfg,
            false,
            &mut dirty_save,
            &mut last_save,
        );
        assert_eq!(cfg.appearance.ui_scale_pct, 125);
        assert!(!dirty_save);
        assert!(watch_rx.has_changed().expect("watch channel open"));
        assert_eq!(watch_rx.borrow_and_update().config.appearance.ui_scale_pct, 125);
        assert_eq!(store.load_or_default().appearance.ui_scale_pct, 125);

        // Out-of-bounds low (50 -> 85) and high (220 -> 150) clamp on normalize, save, and broadcast
        dispatch(
            EngineCommand::SetUiScalePct(50),
            &mut cfg,
            false,
            &mut dirty_save,
            &mut last_save,
        );
        assert_eq!(cfg.appearance.ui_scale_pct, 85);
        assert!(watch_rx.has_changed().expect("watch channel open"));
        assert_eq!(watch_rx.borrow_and_update().config.appearance.ui_scale_pct, 85);
        assert_eq!(store.load_or_default().appearance.ui_scale_pct, 85);

        dispatch(
            EngineCommand::SetUiScalePct(220),
            &mut cfg,
            false,
            &mut dirty_save,
            &mut last_save,
        );
        assert_eq!(cfg.appearance.ui_scale_pct, 150);
        assert_eq!(watch_rx.borrow_and_update().config.appearance.ui_scale_pct, 150);
        assert_eq!(store.load_or_default().appearance.ui_scale_pct, 150);

        // 0 resets back to Auto
        dispatch(
            EngineCommand::SetUiScalePct(0),
            &mut cfg,
            false,
            &mut dirty_save,
            &mut last_save,
        );
        assert_eq!(cfg.appearance.ui_scale_pct, 0);
        assert_eq!(watch_rx.borrow_and_update().config.appearance.ui_scale_pct, 0);
        assert_eq!(store.load_or_default().appearance.ui_scale_pct, 0);

        // During trial preview, broadcasts snapshot and marks dirty_save without overwriting disk
        dispatch(
            EngineCommand::SetUiScalePct(140),
            &mut cfg,
            true,
            &mut dirty_save,
            &mut last_save,
        );
        assert_eq!(cfg.appearance.ui_scale_pct, 140);
        assert!(dirty_save);
        assert_eq!(watch_rx.borrow_and_update().config.appearance.ui_scale_pct, 140);
        assert_eq!(store.load_or_default().appearance.ui_scale_pct, 0);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
