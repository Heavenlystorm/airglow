//! Import and migrate settings, zones, presets, automations, overview layout, LHM auth,
//! and custom Studio effects from the Python Airglow installation (`%APPDATA%\Airglow`).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Default location of Python Airglow's per-user data directory (`%APPDATA%\Airglow`).
pub fn default_python_airglow_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join("Airglow"))
}

/// Pure-Rust standard Base64 encoder (RFC 4648) used for LHM HTTP Basic Auth.
pub fn encode_base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((n >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(n & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Reads `%APPDATA%\Airglow\lhm-auth.json` if present and builds a `Basic <base64>`
/// `Authorization` header value matching Python Airglow's `tools.lhm_headers()`.
pub fn load_lhm_auth_header() -> Option<String> {
    let dir = default_python_airglow_dir()?;
    load_lhm_auth_header_from(&dir)
}

/// Reads `<src_dir>/lhm-auth.json` if present and builds a `Basic <base64>` header value.
pub fn load_lhm_auth_header_from(src_dir: &Path) -> Option<String> {
    let path = src_dir.join("lhm-auth.json");
    let raw = fs::read_to_string(path).ok()?;
    let val: Value = serde_json::from_str(&raw).ok()?;
    let user = val.get("user")?.as_str()?;
    let password = val.get("password")?.as_str()?;
    if user.is_empty() || password.is_empty() {
        return None;
    }
    let creds = format!("{user}:{password}");
    Some(format!("Basic {}", encode_base64(creds.as_bytes())))
}

/// Parse a CSS hex color (`#rrggbb` or `rrggbb`) into `[r, g, b]`.
pub fn parse_hex_color(hex: &str) -> Option<[u8; 3]> {
    let s = hex.trim().strip_prefix('#').unwrap_or(hex.trim());
    if s.len() != 6 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some([r, g, b])
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MigratedFanChannel {
    /// `"curve"` or `"steady"`.
    pub mode: String,
    /// Duty percentage `20..=100` when `mode == "steady"`.
    pub duty: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct MigratedSceneParams {
    pub color: Option<[u8; 3]>,
    pub background: Option<[u8; 3]>,
    pub colors: Vec<[u8; 3]>,
    pub speed: Option<u8>,
    pub angle: Option<u16>,
    pub size: Option<u8>,
    pub saturation: Option<u8>,
    pub tail: Option<u8>,
    pub reverse: Option<bool>,
    pub cool_temp: Option<u8>,
    pub hot_temp: Option<u8>,
    pub sensitivity: Option<u8>,
    pub floor: Option<u8>,
    pub mirror: Option<bool>,
    pub raw: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MigratedScene {
    pub effect: String,
    pub params: MigratedSceneParams,
    pub brightness: u8,
    pub on: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MigratedPreset {
    pub id: String,
    pub name: String,
    pub effect: String,
    pub params: MigratedSceneParams,
    pub brightness: u8,
    pub fan_curve: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MigratedZoneOverride {
    pub effect: String,
    pub params: MigratedSceneParams,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MigratedZoneLayout {
    Fans {
        fans: usize,
        leds_per_fan: usize,
        x: f32,
        y: f32,
        angle: f32,
        size: f32,
        reverse: bool,
    },
    Strip {
        leds: usize,
        x: f32,
        y: f32,
        angle: f32,
        length: f32,
        reverse: bool,
    },
    Matrix {
        leds: usize,
        rows: usize,
        cols: usize,
        x: f32,
        y: f32,
        angle: f32,
        length: f32,
        reverse: bool,
        map: Vec<i32>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MigratedZone {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    /// Uppercase color order (`RGB`, `RBG`, `BGR`, `BRG`, `GBR`, `GRB`).
    pub color_order: String,
    pub layout: MigratedZoneLayout,
    pub override_effect: Option<MigratedZoneOverride>,
    pub component_id: Option<String>,
    pub component_spot: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MigratedTrigger {
    TimeWindow {
        start_hour: u8,
        start_min: u8,
        end_hour: u8,
        end_min: u8,
        days: Vec<u8>,
    },
    AppRunning {
        exe: String,
    },
    AppFocused {
        exe: String,
    },
    Idle {
        minutes: u16,
    },
    Locked,
    TempAbove {
        source: String,
        temp_c: u8,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MigratedAction {
    ApplyPreset { preset: String },
    LightsOff,
    Dim { brightness: u8 },
    FanCurve { curve: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigratedAutomation {
    pub id: String,
    pub enabled: bool,
    pub trigger: MigratedTrigger,
    pub action: MigratedAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigratedOverviewTile {
    pub id: String,
    pub tile_type: String,
    pub w: u8,
    pub h: u8,
    pub sensors: Vec<String>,
    pub style: Option<String>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MigratedOverview {
    pub tiles: Vec<MigratedOverviewTile>,
    pub pins: Vec<String>,
    pub active_workspace: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MigratedStudioLayer {
    pub kind: String,
    pub enabled: bool,
    pub opacity: u8,
    pub blend: String,
    pub color: Option<[u8; 3]>,
    pub secondary: Option<[u8; 3]>,
    pub colors: Vec<[u8; 3]>,
    pub speed: Option<u8>,
    pub angle: Option<u16>,
    pub scale: Option<u8>,
    pub reverse: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MigratedStudioEffect {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub is_layered: bool,
    pub layers: Vec<MigratedStudioLayer>,
    pub source_path: PathBuf,
}

/// Complete migrated snapshot of a Python Airglow user data directory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MigratedData {
    pub source_dir: PathBuf,
    pub design: String,
    pub lhm_url: String,
    pub lhm_auth_present: bool,
    pub openrgb_enabled: bool,
    pub openrgb_host: String,
    pub openrgb_port: u16,
    pub temp_source: String,
    pub temp_unit: String,
    pub energy_price: f32,
    pub fan_failsafe_duty: u8,
    pub fan_max_step: u8,
    pub lights_off_on_sleep: bool,
    pub lights_off_on_lock: bool,
    pub fan_curves: BTreeMap<String, Vec<[u8; 2]>>,
    pub fan_channels: BTreeMap<String, MigratedFanChannel>,
    pub presets: Vec<MigratedPreset>,
    pub zones: Vec<MigratedZone>,
    pub automations: Vec<MigratedAutomation>,
    pub active_scene: Option<MigratedScene>,
    pub active_curve: String,
    pub active_preset: Option<String>,
    pub overview: MigratedOverview,
    pub studio_effects: Vec<MigratedStudioEffect>,
    pub warnings: Vec<String>,
}

impl Default for MigratedData {
    fn default() -> Self {
        Self {
            source_dir: PathBuf::new(),
            design: "firefly".to_string(),
            lhm_url: "http://127.0.0.1:8085".to_string(),
            lhm_auth_present: false,
            openrgb_enabled: true,
            openrgb_host: "127.0.0.1".to_string(),
            openrgb_port: 6742,
            temp_source: "max".to_string(),
            temp_unit: "C".to_string(),
            energy_price: 0.3797,
            fan_failsafe_duty: 30,
            fan_max_step: 5,
            lights_off_on_sleep: true,
            lights_off_on_lock: false,
            fan_curves: BTreeMap::new(),
            fan_channels: BTreeMap::new(),
            presets: Vec::new(),
            zones: Vec::new(),
            automations: Vec::new(),
            active_scene: None,
            active_curve: "balanced".to_string(),
            active_preset: None,
            overview: MigratedOverview::default(),
            studio_effects: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

/// Imports Python Airglow configuration and user state from `src_dir` (e.g. `%APPDATA%\Airglow`).
///
/// Missing optional files or unknown JSON fields are tolerated gracefully and recorded in
/// `MigratedData::warnings` rather than failing the import.
pub fn import_python_airglow(src_dir: &Path) -> Result<MigratedData> {
    if !src_dir.exists() {
        anyhow::bail!(
            "Python Airglow directory does not exist: {}",
            src_dir.display()
        );
    }

    let mut data = MigratedData {
        source_dir: src_dir.to_path_buf(),
        lhm_auth_present: load_lhm_auth_header_from(src_dir).is_some(),
        ..MigratedData::default()
    };

    if let Some(cfg) = read_optional_json(src_dir, "config.json", &mut data.warnings) {
        parse_config_json(&cfg, &mut data);
    }

    if let Some(presets_val) = read_optional_json(src_dir, "presets.json", &mut data.warnings) {
        parse_presets_json(&presets_val, &mut data);
    }

    if let Some(state_val) = read_optional_json(src_dir, "state.json", &mut data.warnings) {
        parse_state_json(&state_val, &mut data);
    }

    if let Some(auto_val) = read_optional_json(src_dir, "automations.json", &mut data.warnings) {
        parse_automations_json(&auto_val, &mut data);
    }

    if let Some(ov_val) = read_optional_json(src_dir, "overview.json", &mut data.warnings) {
        parse_overview_json(&ov_val, &mut data);
    }

    let effects_dir = src_dir.join("effects");
    if effects_dir.is_dir() {
        collect_studio_effects(&effects_dir, &mut data.studio_effects, &mut data.warnings);
        data.studio_effects.sort_by(|a, b| a.id.cmp(&b.id));
    }

    Ok(data)
}

fn read_optional_json(src_dir: &Path, filename: &str, warnings: &mut Vec<String>) -> Option<Value> {
    let path = src_dir.join(filename);
    if !path.is_file() {
        return None;
    }
    match fs::read_to_string(&path)
        .with_context(|| format!("read {}", path.display()))
        .and_then(|s| serde_json::from_str::<Value>(&s).with_context(|| format!("parse {filename}")))
    {
        Ok(v) => Some(v),
        Err(e) => {
            warnings.push(format!("Skipped {filename}: {e}"));
            None
        }
    }
}

fn normalize_lhm_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    if let Some(rest) = trimmed.strip_prefix("http://localhost")
        && (rest.is_empty() || rest.starts_with(':') || rest.starts_with('/'))
    {
        return format!("http://127.0.0.1{rest}");
    }
    if let Some(rest) = trimmed.strip_prefix("https://localhost")
        && (rest.is_empty() || rest.starts_with(':') || rest.starts_with('/'))
    {
        return format!("https://127.0.0.1{rest}");
    }
    if trimmed.is_empty() {
        "http://127.0.0.1:8085".to_string()
    } else {
        trimmed.to_string()
    }
}

fn as_u8_clamped(v: &Value, lo: u8, hi: u8) -> Option<u8> {
    let n = v
        .as_i64()
        .or_else(|| v.as_f64().map(|f| f.round() as i64))?;
    Some(n.clamp(lo as i64, hi as i64) as u8)
}

fn as_u16_clamped(v: &Value, lo: u16, hi: u16) -> Option<u16> {
    let n = v
        .as_i64()
        .or_else(|| v.as_f64().map(|f| f.round() as i64))?;
    Some(n.clamp(lo as i64, hi as i64) as u16)
}

fn as_f32_finite(v: &Value) -> Option<f32> {
    v.as_f64()
        .map(|f| f as f32)
        .filter(|f| f.is_finite())
}

fn parse_config_json(val: &Value, data: &mut MigratedData) {
    let Some(obj) = val.as_object() else {
        data.warnings
            .push("config.json root is not a JSON object".to_string());
        return;
    };

    if let Some(s) = obj.get("design").and_then(Value::as_str)
        && !s.trim().is_empty()
    {
        data.design = s.trim().to_ascii_lowercase();
    }
    if let Some(s) = obj.get("lhm_url").and_then(Value::as_str) {
        data.lhm_url = normalize_lhm_url(s);
    }
    if let Some(b) = obj.get("openrgb_enabled").and_then(Value::as_bool) {
        data.openrgb_enabled = b;
    }
    if let Some(s) = obj.get("openrgb_host").and_then(Value::as_str)
        && !s.trim().is_empty()
    {
        data.openrgb_host = s.trim().to_string();
    }
    if let Some(p) = obj
        .get("openrgb_port")
        .and_then(|v| as_u16_clamped(v, 1, 65535))
    {
        data.openrgb_port = p;
    }
    if let Some(s) = obj.get("temp_source").and_then(Value::as_str) {
        let lower = s.trim().to_ascii_lowercase();
        if matches!(lower.as_str(), "max" | "cpu" | "gpu" | "liquid") {
            data.temp_source = lower;
        }
    }
    if let Some(s) = obj.get("temp_unit").and_then(Value::as_str) {
        let upper = s.trim().to_ascii_uppercase();
        if matches!(upper.as_str(), "C" | "F") {
            data.temp_unit = upper;
        }
    }
    if let Some(p) = obj.get("energy_price").and_then(as_f32_finite)
        && (0.01..=5.0).contains(&p)
    {
        data.energy_price = p;
    }
    if let Some(d) = obj
        .get("fan_failsafe_duty")
        .and_then(|v| as_u8_clamped(v, 30, 100))
    {
        data.fan_failsafe_duty = d;
    }
    if let Some(s) = obj
        .get("fan_max_step")
        .and_then(|v| as_u8_clamped(v, 1, 25))
    {
        data.fan_max_step = s;
    }
    if let Some(b) = obj.get("lights_off_on_sleep").and_then(Value::as_bool) {
        data.lights_off_on_sleep = b;
    }
    if let Some(b) = obj.get("lights_off_on_lock").and_then(Value::as_bool) {
        data.lights_off_on_lock = b;
    }

    if let Some(curves) = obj.get("fan_curves").and_then(Value::as_object) {
        for (name, pts_val) in curves {
            if let Some(arr) = pts_val.as_array() {
                let mut pts: Vec<[u8; 2]> = arr
                    .iter()
                    .filter_map(|p| {
                        let pair = p.as_array()?;
                        if pair.len() != 2 {
                            return None;
                        }
                        let t = as_u8_clamped(&pair[0], 20, 100)?;
                        let d = as_u8_clamped(&pair[1], 20, 100)?;
                        Some([t, d])
                    })
                    .collect();
                pts.sort_by_key(|p| p[0]);
                pts.dedup_by_key(|p| p[0]);
                if pts.len() >= 2 {
                    data.fan_curves.insert(name.clone(), pts);
                }
            }
        }
    }

    if let Some(channels) = obj.get("fan_channels").and_then(Value::as_object) {
        for (sid, ch_val) in channels.iter().take(32) {
            if !sid.starts_with('/') || sid.len() > 200 {
                continue;
            }
            let Some(ch_obj) = ch_val.as_object() else {
                continue;
            };
            let Some(mode) = ch_obj.get("mode").and_then(Value::as_str) else {
                continue;
            };
            let mode = mode.to_ascii_lowercase();
            if mode == "curve" {
                data.fan_channels.insert(
                    sid.clone(),
                    MigratedFanChannel { mode, duty: None },
                );
            } else if mode == "steady" {
                let duty = ch_obj
                    .get("duty")
                    .and_then(|v| as_u8_clamped(v, 20, 100))
                    .unwrap_or(60);
                data.fan_channels.insert(
                    sid.clone(),
                    MigratedFanChannel {
                        mode,
                        duty: Some(duty),
                    },
                );
            }
        }
    }

    if let Some(zones) = obj.get("zones").and_then(Value::as_object) {
        let mut parsed_zones = Vec::with_capacity(zones.len());
        for (id, zval) in zones {
            if let Some(zone) = parse_zone(id, zval) {
                parsed_zones.push(zone);
            } else {
                data.warnings
                    .push(format!("Skipped malformed zone '{id}' in config.json"));
            }
        }
        parsed_zones.sort_by(|a, b| a.id.cmp(&b.id));
        data.zones = parsed_zones;
    }
}

fn parse_zone(id: &str, val: &Value) -> Option<MigratedZone> {
    let obj = val.as_object()?;
    let layout_obj = obj.get("layout")?.as_object()?;
    let shape = layout_obj.get("shape")?.as_str()?.to_ascii_lowercase();

    let x = layout_obj.get("x").and_then(as_f32_finite).unwrap_or(0.5);
    let y = layout_obj.get("y").and_then(as_f32_finite).unwrap_or(0.5);
    let angle = layout_obj
        .get("angle")
        .and_then(as_f32_finite)
        .unwrap_or(0.0);
    let reverse = layout_obj
        .get("reverse")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let layout = match shape.as_str() {
        "fans" => {
            let fans = layout_obj
                .get("fans")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .clamp(1, 32) as usize;
            let leds_per_fan = layout_obj
                .get("leds_per_fan")
                .and_then(Value::as_u64)
                .unwrap_or(8)
                .clamp(1, 512) as usize;
            let size = layout_obj
                .get("size")
                .and_then(as_f32_finite)
                .unwrap_or(0.18)
                .clamp(0.03, 0.8);
            MigratedZoneLayout::Fans {
                fans,
                leds_per_fan,
                x,
                y,
                angle,
                size,
                reverse,
            }
        }
        "matrix" => {
            let rows = layout_obj
                .get("rows")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .clamp(1, 64) as usize;
            let cols = layout_obj
                .get("cols")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .clamp(1, 64) as usize;
            let map: Vec<i32> = layout_obj
                .get("map")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .map(|v| v.as_i64().unwrap_or(-1) as i32)
                        .collect()
                })
                .unwrap_or_default();
            let leds = layout_obj
                .get("leds")
                .and_then(Value::as_u64)
                .unwrap_or((rows * cols) as u64)
                .clamp(1, 2048) as usize;
            let length = layout_obj
                .get("length")
                .and_then(as_f32_finite)
                .unwrap_or(0.5)
                .clamp(0.05, 1.6);
            MigratedZoneLayout::Matrix {
                leds,
                rows,
                cols,
                x,
                y,
                angle,
                length,
                reverse,
                map,
            }
        }
        _ => {
            let leds = layout_obj
                .get("leds")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .clamp(1, 1024) as usize;
            let length = layout_obj
                .get("length")
                .and_then(as_f32_finite)
                .unwrap_or(0.5)
                .clamp(0.02, 1.6);
            MigratedZoneLayout::Strip {
                leds,
                x,
                y,
                angle,
                length,
                reverse,
            }
        }
    };

    let name = obj
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(id)
        .to_string();
    let enabled = obj
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let color_order = obj
        .get("order")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_ascii_uppercase())
        .filter(|s| matches!(s.as_str(), "RGB" | "RBG" | "BGR" | "BRG" | "GBR" | "GRB"))
        .unwrap_or_else(|| "RGB".to_string());

    let override_effect = obj
        .get("override")
        .and_then(Value::as_object)
        .and_then(|ov| {
            let effect = ov.get("effect")?.as_str()?.trim().to_string();
            if effect.is_empty() {
                return None;
            }
            let params = ov
                .get("params")
                .map(parse_scene_params)
                .unwrap_or_default();
            Some(MigratedZoneOverride { effect, params })
        });

    let component = obj.get("component").and_then(Value::as_object);
    let component_id = component
        .and_then(|c| c.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let component_spot = component
        .and_then(|c| c.get("spot"))
        .and_then(Value::as_str)
        .map(str::to_string);

    Some(MigratedZone {
        id: id.to_string(),
        name,
        enabled,
        color_order,
        layout,
        override_effect,
        component_id,
        component_spot,
    })
}

fn parse_scene_params(val: &Value) -> MigratedSceneParams {
    let mut out = MigratedSceneParams {
        raw: val.clone(),
        ..MigratedSceneParams::default()
    };
    let Some(obj) = val.as_object() else {
        return out;
    };

    out.color = obj
        .get("color")
        .and_then(Value::as_str)
        .and_then(parse_hex_color);
    out.background = obj
        .get("background")
        .or_else(|| obj.get("secondary"))
        .and_then(Value::as_str)
        .and_then(parse_hex_color);
    if let Some(arr) = obj.get("colors").and_then(Value::as_array) {
        out.colors = arr
            .iter()
            .filter_map(|v| v.as_str().and_then(parse_hex_color))
            .collect();
    }
    out.speed = obj.get("speed").and_then(|v| as_u8_clamped(v, 0, 100));
    out.angle = obj.get("angle").and_then(|v| as_u16_clamped(v, 0, 360));
    out.size = obj.get("size").and_then(|v| as_u8_clamped(v, 0, 100));
    out.saturation = obj
        .get("saturation")
        .and_then(|v| as_u8_clamped(v, 0, 100));
    out.tail = obj.get("tail").and_then(|v| as_u8_clamped(v, 0, 100));
    out.reverse = obj.get("reverse").and_then(Value::as_bool);
    out.cool_temp = obj
        .get("cool_temp")
        .or_else(|| obj.get("cool"))
        .and_then(|v| as_u8_clamped(v, 20, 100));
    out.hot_temp = obj
        .get("hot_temp")
        .or_else(|| obj.get("hot"))
        .and_then(|v| as_u8_clamped(v, 20, 100));
    out.sensitivity = obj
        .get("sensitivity")
        .and_then(|v| as_u8_clamped(v, 0, 100));
    out.floor = obj.get("floor").and_then(|v| as_u8_clamped(v, 0, 100));
    out.mirror = obj.get("mirror").and_then(Value::as_bool);
    out
}

fn parse_presets_json(val: &Value, data: &mut MigratedData) {
    let Some(arr) = val.as_array() else {
        data.warnings
            .push("presets.json is not a JSON array".to_string());
        return;
    };

    for item in arr {
        let Some(obj) = item.as_object() else {
            continue;
        };
        let Some(id) = obj
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let name = obj
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(id)
            .to_string();

        // Pre-2.0 presets had only a top-level "color" field without "effect".
        let (effect, params) = if let Some(eff) = obj.get("effect").and_then(Value::as_str) {
            let p = obj
                .get("params")
                .map(parse_scene_params)
                .unwrap_or_default();
            (eff.trim().to_string(), p)
        } else if let Some(col_str) = obj.get("color").and_then(Value::as_str) {
            let mut p = MigratedSceneParams::default();
            p.color = parse_hex_color(col_str);
            ("solid".to_string(), p)
        } else {
            ("solid".to_string(), MigratedSceneParams::default())
        };

        let brightness = obj
            .get("brightness")
            .and_then(|v| as_u8_clamped(v, 0, 100))
            .unwrap_or(100);
        let fan_curve = obj
            .get("fan_curve")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let description = obj
            .get("description")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        data.presets.push(MigratedPreset {
            id: id.to_string(),
            name,
            effect,
            params,
            brightness,
            fan_curve,
            description,
        });
    }
}

fn parse_state_json(val: &Value, data: &mut MigratedData) {
    let Some(obj) = val.as_object() else {
        data.warnings
            .push("state.json is not a JSON object".to_string());
        return;
    };

    if let Some(scene_obj) = obj.get("scene").and_then(Value::as_object) {
        let effect = scene_obj
            .get("effect")
            .and_then(Value::as_str)
            .unwrap_or("solid")
            .trim()
            .to_string();
        let params = scene_obj
            .get("params")
            .map(parse_scene_params)
            .unwrap_or_default();
        let brightness = scene_obj
            .get("brightness")
            .and_then(|v| as_u8_clamped(v, 0, 100))
            .unwrap_or(100);
        let on = scene_obj.get("on").and_then(Value::as_bool).unwrap_or(true);
        data.active_scene = Some(MigratedScene {
            effect,
            params,
            brightness,
            on,
        });
    } else if let Some(col) = obj.get("color").and_then(Value::as_str) {
        // Pre-2.0 state format
        let mut params = MigratedSceneParams::default();
        params.color = parse_hex_color(col);
        data.active_scene = Some(MigratedScene {
            effect: "solid".to_string(),
            params,
            brightness: obj
                .get("brightness")
                .and_then(|v| as_u8_clamped(v, 0, 100))
                .unwrap_or(100),
            on: obj
                .get("lights_on")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        });
    }

    if let Some(curve) = obj
        .get("fan_curve")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        data.active_curve = curve.to_string();
    }

    data.active_preset = obj
        .get("active_preset")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
}

fn parse_hhmm(s: &str) -> Option<(u8, u8)> {
    let (h, m) = s.trim().split_once(':')?;
    let hour: u8 = h.parse().ok()?;
    let min: u8 = m.parse().ok()?;
    if hour <= 23 && min <= 59 {
        Some((hour, min))
    } else {
        None
    }
}

fn parse_automations_json(val: &Value, data: &mut MigratedData) {
    let Some(arr) = val.as_array() else {
        data.warnings
            .push("automations.json is not a JSON array".to_string());
        return;
    };

    for item in arr {
        let Some(obj) = item.as_object() else {
            continue;
        };
        let Some(id) = obj
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let enabled = obj
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let Some(when) = obj.get("when").and_then(Value::as_object) else {
            continue;
        };
        let Some(then) = obj.get("then").and_then(Value::as_object) else {
            continue;
        };

        let trigger = match when.get("type").and_then(Value::as_str) {
            Some("time") => {
                let Some((sh, sm)) = when.get("start").and_then(Value::as_str).and_then(parse_hhmm)
                else {
                    continue;
                };
                let Some((eh, em)) = when.get("end").and_then(Value::as_str).and_then(parse_hhmm)
                else {
                    continue;
                };
                let mut days: Vec<u8> = when
                    .get("days")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_u64().filter(|&d| d <= 6).map(|d| d as u8))
                            .collect()
                    })
                    .unwrap_or_else(|| vec![0, 1, 2, 3, 4, 5, 6]);
                if days.is_empty() {
                    days = vec![0, 1, 2, 3, 4, 5, 6];
                }
                MigratedTrigger::TimeWindow {
                    start_hour: sh,
                    start_min: sm,
                    end_hour: eh,
                    end_min: em,
                    days,
                }
            }
            Some("app_running") => {
                let Some(exe) = when
                    .get("exe")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                else {
                    continue;
                };
                MigratedTrigger::AppRunning {
                    exe: exe.to_string(),
                }
            }
            Some("app_focused") => {
                let Some(exe) = when
                    .get("exe")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                else {
                    continue;
                };
                MigratedTrigger::AppFocused {
                    exe: exe.to_string(),
                }
            }
            Some("idle") => {
                let minutes = when
                    .get("minutes")
                    .and_then(|v| as_u16_clamped(v, 1, 240))
                    .unwrap_or(10);
                MigratedTrigger::Idle { minutes }
            }
            Some("locked") => MigratedTrigger::Locked,
            Some("temp_above") => {
                let temp_c = when
                    .get("temp")
                    .and_then(|v| as_u8_clamped(v, 30, 100))
                    .unwrap_or(80);
                let source = when
                    .get("source")
                    .and_then(Value::as_str)
                    .unwrap_or("max")
                    .to_ascii_lowercase();
                MigratedTrigger::TempAbove { source, temp_c }
            }
            _ => continue,
        };

        let action = match then.get("type").and_then(Value::as_str) {
            Some("preset") => {
                let Some(preset) = then
                    .get("preset")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                else {
                    continue;
                };
                MigratedAction::ApplyPreset {
                    preset: preset.to_string(),
                }
            }
            Some("lights_off") => MigratedAction::LightsOff,
            Some("dim") => {
                let brightness = then
                    .get("brightness")
                    .and_then(|v| as_u8_clamped(v, 0, 100))
                    .unwrap_or(20);
                MigratedAction::Dim { brightness }
            }
            Some("fan_curve") => {
                let Some(curve) = then
                    .get("curve")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                else {
                    continue;
                };
                MigratedAction::FanCurve {
                    curve: curve.to_string(),
                }
            }
            _ => continue,
        };

        data.automations.push(MigratedAutomation {
            id: id.to_string(),
            enabled,
            trigger,
            action,
        });
    }
}

fn parse_overview_json(val: &Value, data: &mut MigratedData) {
    let Some(obj) = val.as_object() else {
        data.warnings
            .push("overview.json is not a JSON object".to_string());
        return;
    };

    if let Some(tiles) = obj.get("tiles").and_then(Value::as_array) {
        for t in tiles {
            let Some(tobj) = t.as_object() else {
                continue;
            };
            let Some(id) = tobj.get("id").and_then(Value::as_str) else {
                continue;
            };
            let Some(tile_type) = tobj.get("type").and_then(Value::as_str) else {
                continue;
            };
            let w = tobj
                .get("w")
                .and_then(|v| as_u8_clamped(v, 1, 4))
                .unwrap_or(1);
            let h = tobj
                .get("h")
                .and_then(|v| as_u8_clamped(v, 1, 3))
                .unwrap_or(1);
            let sensors = tobj
                .get("sensors")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let style = tobj
                .get("style")
                .and_then(Value::as_str)
                .map(str::to_string);
            let label = tobj
                .get("label")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);

            data.overview.tiles.push(MigratedOverviewTile {
                id: id.to_string(),
                tile_type: tile_type.to_string(),
                w,
                h,
                sensors,
                style,
                label,
            });
        }
    }

    if let Some(pins) = obj.get("pins").and_then(Value::as_array) {
        data.overview.pins = pins
            .iter()
            .filter_map(|p| p.as_str().map(str::to_string))
            .collect();
    }

    data.overview.active_workspace = obj
        .get("active_workspace")
        .and_then(Value::as_str)
        .map(str::to_string);
}

fn collect_studio_effects(
    dir: &Path,
    out: &mut Vec<MigratedStudioEffect>,
    warnings: &mut Vec<String>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_studio_effects(&path, out, warnings);
            continue;
        }
        if path
            .extension()
            .and_then(|e| e.to_str())
            .is_none_or(|e| !e.eq_ignore_ascii_case("html"))
        {
            continue;
        }
        match fs::read_to_string(&path) {
            Ok(html) => {
                let id = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("effect")
                    .to_string();
                let name = extract_html_tag(&html, "<title>", "</title>")
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| id.clone());
                let description = extract_meta_content(&html, "description");
                let layers = extract_layers_json(&html)
                    .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
                    .map(|v| parse_studio_layers(&v))
                    .unwrap_or_default();
                let is_layered = !layers.is_empty();
                out.push(MigratedStudioEffect {
                    id,
                    name,
                    description,
                    is_layered,
                    layers,
                    source_path: path,
                });
            }
            Err(e) => warnings.push(format!("Could not read Studio effect {}: {e}", path.display())),
        }
    }
}

fn extract_html_tag<'a>(html: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find(&open.to_ascii_lowercase())? + open.len();
    let end = lower[start..].find(&close.to_ascii_lowercase())? + start;
    Some(&html[start..end])
}

fn extract_meta_content(html: &str, name: &str) -> Option<String> {
    let needle = format!("name=\"{name}\"");
    let lower = html.to_ascii_lowercase();
    let idx = lower.find(&needle.to_ascii_lowercase())?;
    let line_end = html[idx..].find('>').map(|i| idx + i).unwrap_or(html.len());
    let slice = &html[idx..line_end];
    let c_idx = slice.to_ascii_lowercase().find("content=\"")? + "content=\"".len();
    let rest = &slice[c_idx..];
    let end_q = rest.find('"')?;
    let text = rest[..end_q].trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

fn extract_layers_json(html: &str) -> Option<&str> {
    let lower = html.to_ascii_lowercase();
    let marker = lower.find("id=\"layers\"")?;
    let open_end = html[marker..].find('>')? + marker + 1;
    let close_start = lower[open_end..].find("</script>")? + open_end;
    Some(html[open_end..close_start].trim())
}

fn parse_studio_layers(val: &Value) -> Vec<MigratedStudioLayer> {
    let Some(arr) = val.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|item| {
            let obj = item.as_object()?;
            let kind = obj.get("type")?.as_str()?.trim().to_ascii_lowercase();
            let enabled = obj
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let opacity = obj
                .get("opacity")
                .and_then(|v| as_u8_clamped(v, 0, 100))
                .unwrap_or(100);
            let blend = obj
                .get("blend")
                .and_then(Value::as_str)
                .unwrap_or("normal")
                .to_ascii_lowercase();
            let params = obj.get("params").and_then(Value::as_object);
            let colors: Vec<[u8; 3]> = params
                .and_then(|p| p.get("colors"))
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().and_then(parse_hex_color))
                        .collect()
                })
                .unwrap_or_default();
            let color = params
                .and_then(|p| p.get("color"))
                .and_then(Value::as_str)
                .and_then(parse_hex_color)
                .or_else(|| colors.first().copied());
            let secondary = params
                .and_then(|p| p.get("background").or_else(|| p.get("secondary")))
                .and_then(Value::as_str)
                .and_then(parse_hex_color)
                .or_else(|| colors.get(1).copied());
            let speed = params
                .and_then(|p| p.get("speed"))
                .and_then(|v| as_u8_clamped(v, 0, 100));
            let angle = params
                .and_then(|p| p.get("angle"))
                .and_then(|v| as_u16_clamped(v, 0, 360));
            let scale = params
                .and_then(|p| p.get("scale").or_else(|| p.get("density")))
                .and_then(|v| as_u8_clamped(v, 0, 100));
            let reverse = params
                .and_then(|p| p.get("reverse"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            Some(MigratedStudioLayer {
                kind,
                enabled,
                opacity,
                blend,
                color,
                secondary,
                colors,
                speed,
                angle,
                scale,
                reverse,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "airglow_migrate_test_{}_{}",
            tag,
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn base64_encoder_matches_rfc4648_vectors() {
        assert_eq!(encode_base64(b""), "");
        assert_eq!(encode_base64(b"f"), "Zg==");
        assert_eq!(encode_base64(b"fo"), "Zm8=");
        assert_eq!(encode_base64(b"foo"), "Zm9v");
        assert_eq!(encode_base64(b"airglow:secret"), "YWlyZ2xvdzpzZWNyZXQ=");
    }

    #[test]
    fn loads_lhm_auth_header_and_handles_missing() {
        let dir = temp_dir("auth");
        assert_eq!(load_lhm_auth_header_from(&dir), None);

        fs::write(
            dir.join("lhm-auth.json"),
            r#"{"user":"airglow","password":"secret"}"#,
        )
        .expect("write auth");
        assert_eq!(
            load_lhm_auth_header_from(&dir).as_deref(),
            Some("Basic YWlyZ2xvdzpzZWNyZXQ=")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn imports_full_python_airglow_directory_with_matrix_channels_and_studio_effects() {
        let dir = temp_dir("full");
        fs::write(
            dir.join("config.json"),
            r#"{
              "lhm_url": "http://localhost:8085/",
              "design": "kiln",
              "temp_source": "gpu",
              "temp_unit": "F",
              "energy_price": 0.42,
              "fan_failsafe_duty": 45,
              "fan_max_step": 8,
              "openrgb_enabled": true,
              "openrgb_host": "127.0.0.1",
              "openrgb_port": 6743,
              "lights_off_on_sleep": true,
              "lights_off_on_lock": true,
              "unknown_future_field": [1, 2, 3],
              "fan_curves": {
                "silent": [[40, 25], [80, 90]]
              },
              "fan_channels": {
                "/lpc/nct6701d/0/control/0": { "mode": "curve" },
                "/lpc/nct6701d/0/control/1": { "mode": "steady", "duty": 75 }
              },
              "zones": {
                "nzxt-hub/ch1": {
                  "name": "Front Fans",
                  "enabled": true,
                  "order": "GRB",
                  "layout": { "shape": "fans", "fans": 3, "leds_per_fan": 8, "x": 0.2, "y": 0.5, "angle": 90, "size": 0.18, "reverse": false }
                },
                "openrgb:keychron/z0": {
                  "name": "K2 HE",
                  "enabled": true,
                  "order": "RGB",
                  "layout": { "shape": "matrix", "leds": 3, "rows": 2, "cols": 2, "x": 1.4, "y": 0.8, "angle": 0, "length": 0.4, "reverse": false, "map": [0, 1, -1, 2] }
                }
              }
            }"#,
        )
        .unwrap();

        fs::write(
            dir.join("presets.json"),
            r##"[
              { "id": "meteor", "name": "Meteor watch", "effect": "comet", "params": { "color": "#ff9b52", "background": "#16275d", "speed": 21, "tail": 56 }, "brightness": 72, "fan_curve": null }
            ]"##,
        )
        .unwrap();

        fs::write(
            dir.join("state.json"),
            r##"{
              "scene": { "effect": "comet", "params": { "color": "#ff9b52", "speed": 21 }, "brightness": 95, "on": true },
              "fan_curve": "silent",
              "active_preset": "meteor"
            }"##,
        )
        .unwrap();

        fs::write(
            dir.join("automations.json"),
            r#"[
              { "id": "quiet-idle", "enabled": true, "when": { "type": "idle", "minutes": 15 }, "then": { "type": "fan_curve", "curve": "silent" } },
              { "id": "dark-lock", "enabled": true, "when": { "type": "locked" }, "then": { "type": "lights_off" } },
              { "id": "night", "enabled": false, "when": { "type": "time", "start": "23:00", "end": "07:00", "days": [0,1,2,3,4,5,6] }, "then": { "type": "dim", "brightness": 15 } }
            ]"#,
        )
        .unwrap();

        fs::write(
            dir.join("overview.json"),
            r#"{
              "tiles": [{ "id": "cpu-temp", "type": "sensor", "w": 1, "h": 1, "sensors": ["auto/cpu_temp"], "style": "spark" }],
              "pins": ["auto/cpu_temp", "auto/gpu_temp"],
              "active_workspace": "main"
            }"#,
        )
        .unwrap();

        let effects_mine = dir.join("effects").join("mine");
        fs::create_dir_all(&effects_mine).unwrap();
        fs::write(
            effects_mine.join("me.my-aurora.html"),
            r##"<!doctype html><title>My aurora</title><meta name="description" content="Layered glow"><script type="application/json" id="layers">[{"type":"gradient","enabled":true,"opacity":100,"blend":"normal","params":{"colors":["#00ffa3","#2f5bff"],"speed":14,"angle":20,"scale":40}}]</script>"##,
        )
        .unwrap();

        let migrated = import_python_airglow(&dir).expect("import succeeds");
        assert_eq!(migrated.lhm_url, "http://127.0.0.1:8085");
        assert_eq!(migrated.design, "kiln");
        assert_eq!(migrated.temp_source, "gpu");
        assert_eq!(migrated.temp_unit, "F");
        assert!((migrated.energy_price - 0.42).abs() < 0.001);
        assert_eq!(migrated.fan_failsafe_duty, 45);
        assert_eq!(migrated.fan_max_step, 8);
        assert_eq!(migrated.openrgb_port, 6743);
        assert!(migrated.lights_off_on_lock);
        assert_eq!(migrated.fan_channels.len(), 2);
        assert_eq!(
            migrated.fan_channels["/lpc/nct6701d/0/control/1"].duty,
            Some(75)
        );
        assert_eq!(migrated.zones.len(), 2);
        assert!(matches!(
            migrated.zones[1].layout,
            MigratedZoneLayout::Matrix {
                rows: 2,
                cols: 2,
                leds: 3,
                ..
            }
        ));
        assert_eq!(migrated.presets.len(), 1);
        assert_eq!(migrated.presets[0].params.color, Some([255, 155, 82]));
        assert_eq!(migrated.automations.len(), 3);
        assert_eq!(migrated.active_curve, "silent");
        assert_eq!(migrated.active_preset.as_deref(), Some("meteor"));
        assert_eq!(migrated.overview.pins.len(), 2);
        assert_eq!(migrated.studio_effects.len(), 1);
        assert!(migrated.studio_effects[0].is_layered);
        assert_eq!(migrated.studio_effects[0].name, "My aurora");
        assert_eq!(migrated.studio_effects[0].layers.len(), 1);
        assert!(migrated.warnings.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tolerates_missing_optional_files_and_corrupt_json() {
        let dir = temp_dir("partial");
        fs::write(dir.join("config.json"), r#"{"design":"paper"}"#).unwrap();
        fs::write(dir.join("overview.json"), "{broken json").unwrap();

        let migrated = import_python_airglow(&dir).expect("partial import succeeds");
        assert_eq!(migrated.design, "paper");
        assert_eq!(migrated.warnings.len(), 1);
        assert!(migrated.presets.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }
}
