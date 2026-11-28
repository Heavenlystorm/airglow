//! Settings, 13 interface design palettes, presets, fan curves, and JSON persistence.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::automations::{ALL_DAYS_MASK, Action, AutomationRule, Trigger, default_rules};
use crate::effects::{
    BlendMode, EffectId, Layer, LayerKind, LayerStack, SavedStudioEffect, SceneParams,
    default_studio_effects,
};
use crate::geometry::ZoneLayout;
use crate::migrate::{
    MigratedAction, MigratedData, MigratedSceneParams, MigratedTrigger, MigratedZoneLayout,
    default_python_airglow_dir, import_python_airglow,
};

pub const APP_NAME: &str = "Airglow";
pub const MIN_FAN_DUTY: u8 = 20;
pub const SAFETY_TEMP: f32 = 90.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum TempUnit {
    #[default]
    C,
    F,
}

impl TempUnit {
    pub fn suffix(self) -> &'static str {
        match self {
            Self::C => "°C",
            Self::F => "°F",
        }
    }

    pub fn convert(self, temp_c: f32) -> f32 {
        match self {
            Self::C => temp_c,
            Self::F => temp_c * 9.0 / 5.0 + 32.0,
        }
    }

    pub fn format_temp(self, temp_c: f32) -> String {
        format!("{:.0}{}", self.convert(temp_c), self.suffix())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DesignId {
    #[default]
    Firefly,
    Graphite,
    Ascii,
    Paper,
    Amber,
    Contrast,
    Neon,
    Kiln,
    Lumen,
    Midnight,
    Spore,
    Rim,
    Darkroom,
}

impl DesignId {
    pub const ALL: [DesignId; 13] = [
        DesignId::Firefly,
        DesignId::Graphite,
        DesignId::Ascii,
        DesignId::Paper,
        DesignId::Amber,
        DesignId::Contrast,
        DesignId::Neon,
        DesignId::Kiln,
        DesignId::Lumen,
        DesignId::Midnight,
        DesignId::Spore,
        DesignId::Rim,
        DesignId::Darkroom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Firefly => "Firefly",
            Self::Graphite => "Graphite",
            Self::Ascii => "ASCII",
            Self::Paper => "Paper",
            Self::Amber => "Amber CRT",
            Self::Contrast => "High contrast",
            Self::Neon => "Neon",
            Self::Kiln => "Kiln",
            Self::Lumen => "Lumen",
            Self::Midnight => "Midnight",
            Self::Spore => "Spore",
            Self::Rim => "Rim",
            Self::Darkroom => "Darkroom",
        }
    }

    pub fn tagline(self) -> &'static str {
        match self {
            Self::Firefly => "Dusk plum, glowing lantern moth, apricot and mint",
            Self::Graphite => "Charcoal workshop, warm ivory and brass",
            Self::Ascii => "P1 phosphor serial console",
            Self::Paper => "Daylight paper and dark ink",
            Self::Amber => "Warm amber phosphor CRT",
            Self::Contrast => "Pure black, white and signal yellow",
            Self::Neon => "Deep indigo and magenta signal",
            Self::Kiln => "Near-black studio and ember orange",
            Self::Lumen => "Violet-black and blueprint brass",
            Self::Midnight => "Deep navy and azure signal",
            Self::Spore => "Warm espresso and oyster pink",
            Self::Rim => "Warm black hairlines with corner ticks",
            Self::Darkroom => "Print-black with amber safelight",
        }
    }

    pub fn palette(self) -> DesignPalette {
        match self {
            Self::Firefly => DesignPalette {
                is_light: false,
                square_corners: false,
                monospaced: false,
                ground: 0x221d30,
                panel: 0x30293e,
                raised: 0x40364e,
                well: 0x241f31,
                line: 0x62536d,
                ink: 0xfff4e4,
                ink_2: 0xe2d4df,
                ink_3: 0xc2b2c7,
                brass: 0xffc38b,
                brass_hi: 0xffdbb2,
                hot: 0xffa19b,
                warm: 0xffd37e,
                ok: 0xa5dece,
                radius_panel: 16.0,
                radius_ctl: 9.0,
            },
            Self::Graphite => DesignPalette {
                is_light: false,
                square_corners: false,
                monospaced: false,
                ground: 0x17181a,
                panel: 0x1f2023,
                raised: 0x28292d,
                well: 0x121314,
                line: 0x36373c,
                ink: 0xe9e5da,
                ink_2: 0xb9b5aa,
                ink_3: 0x8e8a80,
                brass: 0xc9a25f,
                brass_hi: 0xdab673,
                hot: 0xef5a48,
                warm: 0xe39540,
                ok: 0x7cc58e,
                radius_panel: 6.0,
                radius_ctl: 4.0,
            },
            Self::Ascii => DesignPalette {
                is_light: false,
                square_corners: true,
                monospaced: true,
                ground: 0x060806,
                panel: 0x0b0e0b,
                raised: 0x111611,
                well: 0x040604,
                line: 0x2c3a2c,
                ink: 0xd9ffd9,
                ink_2: 0xa9d6a9,
                ink_3: 0x7ba57b,
                brass: 0x77ff8f,
                brass_hi: 0xb6ffc4,
                hot: 0xff7a5c,
                warm: 0xffd166,
                ok: 0x77ff8f,
                radius_panel: 0.0,
                radius_ctl: 0.0,
            },
            Self::Paper => DesignPalette {
                is_light: true,
                square_corners: false,
                monospaced: false,
                ground: 0xe8e4db,
                panel: 0xf5f2eb,
                raised: 0xfcfaf4,
                well: 0xded9cf,
                line: 0xc9c2b4,
                ink: 0x1c1d19,
                ink_2: 0x43443d,
                ink_3: 0x5e5f57,
                brass: 0x8a6119,
                brass_hi: 0x7a5410,
                hot: 0xb23a2b,
                warm: 0x9a5b12,
                ok: 0x2f6e45,
                radius_panel: 6.0,
                radius_ctl: 4.0,
            },
            Self::Amber => DesignPalette {
                is_light: false,
                square_corners: false,
                monospaced: true,
                ground: 0x0c0903,
                panel: 0x141006,
                raised: 0x1c1608,
                well: 0x080601,
                line: 0x3d3014,
                ink: 0xffd39a,
                ink_2: 0xe6b473,
                ink_3: 0xb98f52,
                brass: 0xffb347,
                brass_hi: 0xffd08a,
                hot: 0xff7a55,
                warm: 0xffb347,
                ok: 0xb6e08a,
                radius_panel: 4.0,
                radius_ctl: 3.0,
            },
            Self::Contrast => DesignPalette {
                is_light: false,
                square_corners: true,
                monospaced: false,
                ground: 0x000000,
                panel: 0x000000,
                raised: 0x141414,
                well: 0x000000,
                line: 0xffffff,
                ink: 0xffffff,
                ink_2: 0xffffff,
                ink_3: 0xd0d0d0,
                brass: 0xffe000,
                brass_hi: 0xfff45c,
                hot: 0xff6b6b,
                warm: 0xffb000,
                ok: 0x66ff99,
                radius_panel: 0.0,
                radius_ctl: 0.0,
            },
            Self::Neon => DesignPalette {
                is_light: false,
                square_corners: false,
                monospaced: false,
                ground: 0x07070d,
                panel: 0x0d0d17,
                raised: 0x151524,
                well: 0x050509,
                line: 0x2e2147,
                ink: 0xf1edff,
                ink_2: 0xc3bad9,
                ink_3: 0x9186ab,
                brass: 0xff2fa8,
                brass_hi: 0xff74c5,
                hot: 0xff5470,
                warm: 0xffb454,
                ok: 0x42e8b4,
                radius_panel: 8.0,
                radius_ctl: 5.0,
            },
            Self::Kiln => DesignPalette {
                is_light: false,
                square_corners: false,
                monospaced: false,
                ground: 0x0b0605,
                panel: 0x170e0b,
                raised: 0x221814,
                well: 0x060302,
                line: 0x272220,
                ink: 0xf2eadd,
                ink_2: 0xd5c8b9,
                ink_3: 0xb3a192,
                brass: 0xfe6a00,
                brass_hi: 0xffcf66,
                hot: 0xff6a2b,
                warm: 0xffa04a,
                ok: 0xd8cbb4,
                radius_panel: 6.0,
                radius_ctl: 4.0,
            },
            Self::Lumen => DesignPalette {
                is_light: false,
                square_corners: false,
                monospaced: false,
                ground: 0x060510,
                panel: 0x0d0b19,
                raised: 0x161324,
                well: 0x030208,
                line: 0x262436,
                ink: 0xf3f0f8,
                ink_2: 0xcfccd8,
                ink_3: 0x9c98aa,
                brass: 0xffa54b,
                brass_hi: 0xffd9a8,
                hot: 0xf3798b,
                warm: 0xffc27a,
                ok: 0x72d699,
                radius_panel: 6.0,
                radius_ctl: 4.0,
            },
            Self::Midnight => DesignPalette {
                is_light: false,
                square_corners: true,
                monospaced: false,
                ground: 0x02071d,
                panel: 0x071129,
                raised: 0x0f1c37,
                well: 0x01040f,
                line: 0x1c2a48,
                ink: 0xe7f0f9,
                ink_2: 0xbddcf7,
                ink_3: 0x98a9c1,
                brass: 0x6bcbf7,
                brass_hi: 0xa5e4ff,
                hot: 0x3483ad,
                warm: 0x8fd8ff,
                ok: 0x6bcbf7,
                radius_panel: 0.0,
                radius_ctl: 0.0,
            },
            Self::Spore => DesignPalette {
                is_light: false,
                square_corners: false,
                monospaced: false,
                ground: 0x0f0a06,
                panel: 0x19120e,
                raised: 0x271d17,
                well: 0x0a0604,
                line: 0x372f28,
                ink: 0xefe7d8,
                ink_2: 0xd3c9bb,
                ink_3: 0x9d9388,
                brass: 0xe89f92,
                brass_hi: 0xf6c3ba,
                hot: 0xc16d45,
                warm: 0xe8b98f,
                ok: 0xc9c0a4,
                radius_panel: 12.0,
                radius_ctl: 999.0,
            },
            Self::Rim => DesignPalette {
                is_light: false,
                square_corners: true,
                monospaced: true,
                ground: 0x030201,
                panel: 0x080604,
                raised: 0x100d09,
                well: 0x010100,
                line: 0x24211c,
                ink: 0xefebe4,
                ink_2: 0xcecac3,
                ink_3: 0x9c9890,
                brass: 0xf97c3d,
                brass_hi: 0xffb08a,
                hot: 0xf97c3d,
                warm: 0xffb08a,
                ok: 0x6db0f4,
                radius_panel: 0.0,
                radius_ctl: 0.0,
            },
            Self::Darkroom => DesignPalette {
                is_light: false,
                square_corners: true,
                monospaced: false,
                ground: 0x090604,
                panel: 0x100c0a,
                raised: 0x1b1613,
                well: 0x050302,
                line: 0x2f2a27,
                ink: 0xe9e4dc,
                ink_2: 0xc2bdb5,
                ink_3: 0x918b85,
                brass: 0xec8d38,
                brass_hi: 0xffc07e,
                hot: 0xec8d38,
                warm: 0xc06e41,
                ok: 0xd8d2c8,
                radius_panel: 0.0,
                radius_ctl: 0.0,
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DesignPalette {
    pub is_light: bool,
    pub square_corners: bool,
    pub monospaced: bool,
    pub ground: u32,
    pub panel: u32,
    pub raised: u32,
    pub well: u32,
    pub line: u32,
    pub ink: u32,
    pub ink_2: u32,
    pub ink_3: u32,
    pub brass: u32,
    pub brass_hi: u32,
    pub hot: u32,
    pub warm: u32,
    pub ok: u32,
    pub radius_panel: f32,
    pub radius_ctl: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum ColorOrder {
    #[default]
    Rgb,
    Rbg,
    Bgr,
    Brg,
    Gbr,
    Grb,
}

impl ColorOrder {
    pub fn reorder(self, rgb: [u8; 3]) -> [u8; 3] {
        match self {
            Self::Rgb => [rgb[0], rgb[1], rgb[2]],
            Self::Rbg => [rgb[0], rgb[2], rgb[1]],
            Self::Bgr => [rgb[2], rgb[1], rgb[0]],
            Self::Brg => [rgb[2], rgb[0], rgb[1]],
            Self::Gbr => [rgb[1], rgb[2], rgb[0]],
            Self::Grb => [rgb[1], rgb[0], rgb[2]],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TempSource {
    #[default]
    Max,
    Cpu,
    Gpu,
    Liquid,
}

impl TempSource {
    pub const ALL: [TempSource; 4] = [Self::Max, Self::Cpu, Self::Gpu, Self::Liquid];

    pub fn label(self) -> &'static str {
        match self {
            Self::Max => "Hotter of CPU / GPU",
            Self::Cpu => "Processor (CPU)",
            Self::Gpu => "Graphics card (GPU)",
            Self::Liquid => "Liquid coolant",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum FanChannelMode {
    #[default]
    Auto,
    Curve,
    Steady { duty: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurvePoint {
    pub temp: u8,
    pub duty: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZoneConfig {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub color_order: ColorOrder,
    pub layout: ZoneLayout,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZoneOverride {
    pub effect: EffectId,
    #[serde(default)]
    pub params: SceneParams,
    #[serde(default)]
    pub studio_stack: Option<LayerStack>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneConfig {
    pub on: bool,
    pub brightness: u8,
    pub effect: EffectId,
    pub params: SceneParams,
    pub studio_stack: LayerStack,
    #[serde(default)]
    pub zone_overrides: BTreeMap<String, ZoneOverride>,
    #[serde(default)]
    pub active_studio_effect: Option<String>,
}

impl Default for SceneConfig {
    fn default() -> Self {
        Self {
            on: true,
            brightness: 100,
            effect: EffectId::Gradient,
            params: SceneParams::default(),
            studio_stack: LayerStack::default(),
            zone_overrides: BTreeMap::new(),
            active_studio_effect: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub effect: EffectId,
    pub params: SceneParams,
    pub brightness: u8,
    pub fan_curve: String,
    #[serde(default)]
    pub studio_stack: Option<LayerStack>,
    #[serde(default)]
    pub zone_overrides: BTreeMap<String, ZoneOverride>,
}

/// Export a preset as formatted JSON for sharing / backup.
pub fn export_preset_json(preset: &Preset) -> Result<String> {
    serde_json::to_string_pretty(preset).context("serialize preset to JSON")
}

/// Import a preset from JSON (supports native `Preset` JSON and Python Airglow `airglow_profile: 1` JSON).
pub fn import_preset_json(raw: &str) -> Result<Preset> {
    if let Ok(preset) = serde_json::from_str::<Preset>(raw) {
        return Ok(preset);
    }
    let val: serde_json::Value = serde_json::from_str(raw).context("parse preset JSON")?;
    let name = val
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .context("preset JSON missing 'name'")?
        .to_string();
    let id = val
        .get("id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            name.to_ascii_lowercase()
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect()
        });
    let lighting = val.get("lighting").unwrap_or(&val);
    let effect_str = lighting
        .get("effect")
        .and_then(|v| v.as_str())
        .unwrap_or("solid");
    let effect = parse_effect_id(effect_str);
    let brightness = lighting
        .get("brightness")
        .and_then(|v| v.as_u64())
        .unwrap_or(100)
        .min(100) as u8;
    let params = lighting
        .get("params")
        .map(parse_python_scene_params)
        .unwrap_or_default();
    let fan_curve = val
        .get("fan_curve")
        .and_then(|v| v.as_str())
        .unwrap_or("balanced")
        .to_string();
    let description = val
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Ok(Preset {
        id,
        name,
        description,
        effect,
        params,
        brightness,
        fan_curve,
        studio_stack: None,
        zone_overrides: BTreeMap::new(),
    })
}

pub fn parse_effect_id(s: &str) -> EffectId {
    match s.trim().to_ascii_lowercase().as_str() {
        "solid" => EffectId::Solid,
        "breathing" => EffectId::Breathing,
        "cycle" => EffectId::Cycle,
        "rainbow" => EffectId::Rainbow,
        "gradient" => EffectId::Gradient,
        "comet" => EffectId::Comet,
        "aurora" => EffectId::Aurora,
        "fireflyswarm" | "firefly_swarm" | "firefly-swarm" | "firefly" => EffectId::FireflySwarm,
        "meteorshower" | "meteor_shower" | "meteor-shower" | "meteor" => EffectId::MeteorShower,
        "heatmap" | "heat_map" | "heat-map" => EffectId::Heatmap,
        "temperature" => EffectId::Temperature,
        "screen" => EffectId::Screen,
        "spectrum" => EffectId::Spectrum,
        "pulse" => EffectId::Pulse,
        "off" => EffectId::Off,
        s if s.starts_with("fx:") || s == "studio" => EffectId::Studio,
        _ => EffectId::Solid,
    }
}

pub fn parse_hex_color(s: &str) -> Option<[u8; 3]> {
    parse_hex_rgb(s)
}

/// Parses a hex RGB color string such as `"#f48fb1"`, `"f48fb1"`, `"#F48FB1"`, or 3-digit `"#f8b"`.
pub fn parse_hex_rgb(input: &str) -> Option<[u8; 3]> {
    let trimmed = input.trim();
    let hex = trimmed.strip_prefix('#').unwrap_or(trimmed);
    match hex.len() {
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some([r, g, b])
        }
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
            Some([r * 17, g * 17, b * 17])
        }
        _ => None,
    }
}

/// Formats an RGB triple as an uppercase `#RRGGBB` string (e.g. `"#F48FB1"`).
pub fn format_hex_rgb(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

pub fn parse_python_scene_params(val: &serde_json::Value) -> SceneParams {
    let mut p = SceneParams::default();
    if let Some(obj) = val.as_object() {
        if let Some(c) = obj
            .get("color")
            .or_else(|| obj.get("cool"))
            .and_then(|v| v.as_str())
            .and_then(parse_hex_color)
        {
            p.color = c;
        }
        if let Some(c) = obj
            .get("background")
            .or_else(|| obj.get("secondary"))
            .and_then(|v| v.as_str())
            .and_then(parse_hex_color)
        {
            p.secondary = c;
        }
        if let Some(c) = obj
            .get("hot")
            .and_then(|v| v.as_str())
            .and_then(parse_hex_color)
        {
            p.hot_color = c;
        }
        if let Some(arr) = obj.get("colors").and_then(|v| v.as_array()) {
            let parsed: Vec<[u8; 3]> = arr
                .iter()
                .filter_map(|v| v.as_str().and_then(parse_hex_color))
                .collect();
            if parsed.len() >= 2 {
                p.colors = parsed;
            }
        }
        if let Some(v) = obj.get("speed").and_then(|v| v.as_u64()) {
            p.speed = v.min(100) as u8;
        }
        if let Some(v) = obj.get("angle").and_then(|v| v.as_u64()) {
            p.angle = (v % 360) as u16;
        }
        if let Some(v) = obj.get("size").and_then(|v| v.as_u64()) {
            p.size = v.min(100) as u8;
        }
        if let Some(v) = obj.get("saturation").and_then(|v| v.as_u64()) {
            p.saturation = v.min(100) as u8;
        }
        if let Some(v) = obj.get("tail").and_then(|v| v.as_u64()) {
            p.tail = v.min(100) as u8;
        }
        if let Some(v) = obj.get("reverse").and_then(|v| v.as_bool()) {
            p.reverse = v;
        }
        if let Some(v) = obj.get("mirror").and_then(|v| v.as_bool()) {
            p.mirror = v;
        }
        if let Some(v) = obj.get("low").or_else(|| obj.get("cool_temp")).and_then(|v| v.as_u64()) {
            p.cool_temp = v.clamp(20, 90) as u8;
        }
        if let Some(v) = obj.get("high").or_else(|| obj.get("hot_temp")).and_then(|v| v.as_u64()) {
            p.hot_temp = v.clamp(30, 100) as u8;
        }
        if let Some(v) = obj.get("sensitivity").and_then(|v| v.as_u64()) {
            p.sensitivity = v.min(100) as u8;
        }
        if let Some(v) = obj.get("hue").or_else(|| obj.get("hue_shift")).and_then(|v| v.as_u64()) {
            p.hue_shift = (v % 360) as u16;
        }
        if let Some(v) = obj.get("floor").and_then(|v| v.as_u64()) {
            p.floor = v.min(100) as u8;
        }
        if let Some(src) = obj.get("source").and_then(|v| v.as_str()) {
            p.temp_source = match src.to_ascii_lowercase().as_str() {
                "cpu" => TempSource::Cpu,
                "gpu" => TempSource::Gpu,
                "liquid" => TempSource::Liquid,
                _ => TempSource::Max,
            };
        }
    }
    p
}

fn default_true() -> bool {
    true
}

fn default_ui_fps_cap() -> u8 {
    30
}

fn default_pulse_window_mins() -> u8 {
    5
}

fn default_header_pins() -> Vec<String> {
    vec!["cpu_temp".to_string(), "gpu_temp".to_string()]
}

fn default_fan_hysteresis_c() -> f32 {
    2.0
}

/// Curated accent swatches that work across all 13 Airglow designs.
pub const ACCENT_SWATCHES: &[(&str, [u8; 3])] = &[
    ("Rose Pink", [0xf4, 0x8f, 0xb1]),
    ("Cherry", [0xff, 0x5c, 0x8a]),
    ("Apricot", [0xff, 0xc3, 0x8b]),
    ("Lavender", [0xc5, 0x9b, 0xff]),
    ("Mint", [0x7c, 0xe8, 0xc8]),
    ("Coral", [0xff, 0x8a, 0x75]),
    ("Sky", [0x6b, 0xcb, 0xf7]),
    ("Gold", [0xff, 0xd1, 0x66]),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RigStyle {
    #[default]
    Living,
    Architectural,
    Minimal,
}

impl RigStyle {
    pub const ALL: [RigStyle; 3] = [Self::Living, Self::Architectural, Self::Minimal];

    pub fn label(self) -> &'static str {
        match self {
            Self::Living => "Living",
            Self::Architectural => "Architectural",
            Self::Minimal => "Minimal",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Living => "Glass chassis, live AIO temp screen, VRM pulses & soft bloom.",
            Self::Architectural => "Precision drafting blueprint with crisp technical dimensions.",
            Self::Minimal => "Clean distraction-free LED array and fan silhouettes.",
        }
    }
}

pub const UI_SCALE_PRESETS: &[(u16, &str)] = &[
    (0, "Auto"),
    (100, "100%"),
    (115, "115%"),
    (125, "125%"),
    (140, "140%"),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppearanceConfig {
    #[serde(default)]
    pub accent_override: Option<[u8; 3]>,
    #[serde(default)]
    pub rig_style: RigStyle,
    #[serde(default = "default_true")]
    pub airflow_lines: bool,
    #[serde(default = "default_true")]
    pub spinning_fans: bool,
    #[serde(default = "default_true")]
    pub ambient_bloom: bool,
    #[serde(default = "default_true")]
    pub firefly_particles: bool,
    #[serde(default)]
    pub compact_density: bool,
    #[serde(default)]
    pub ui_scale_pct: u16,
    #[serde(default = "default_ui_fps_cap")]
    pub ui_fps_cap: u8,
    #[serde(default = "default_pulse_window_mins")]
    pub pulse_window_mins: u8,
    #[serde(default)]
    pub hidden_overview_tiles: Vec<String>,
    #[serde(default = "default_header_pins")]
    pub header_pins: Vec<String>,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            accent_override: None,
            rig_style: RigStyle::Living,
            airflow_lines: true,
            spinning_fans: true,
            ambient_bloom: true,
            firefly_particles: true,
            compact_density: false,
            ui_scale_pct: 0,
            ui_fps_cap: 30,
            pulse_window_mins: 5,
            hidden_overview_tiles: Vec::new(),
            header_pins: default_header_pins(),
        }
    }
}

impl AppearanceConfig {
    pub const UI_SCALE_PRESETS: &'static [(u16, &'static str)] = UI_SCALE_PRESETS;

    pub fn normalize(&mut self) {
        self.ui_fps_cap = match self.ui_fps_cap {
            0..=22 => 15,
            23..=45 => 30,
            _ => 60,
        };
        self.pulse_window_mins = match self.pulse_window_mins {
            0..=2 => 1,
            3..=7 => 5,
            _ => 10,
        };
        if self.ui_scale_pct != 0 {
            self.ui_scale_pct = self.ui_scale_pct.clamp(85, 150);
        }
    }

    pub fn normalized(mut self) -> Self {
        self.normalize();
        self
    }

    pub fn resolve_scale_factor(&self, approx_physical_width: f32) -> f32 {
        if self.ui_scale_pct > 0 {
            (self.ui_scale_pct as f32 / 100.0).clamp(0.85, 1.50)
        } else if approx_physical_width >= 3250.0 {
            1.50
        } else if approx_physical_width >= 2200.0 {
            1.25
        } else if approx_physical_width >= 1820.0 {
            1.10
        } else {
            1.0
        }
    }

    pub fn is_tile_visible(&self, tile_id: &str) -> bool {
        !self.hidden_overview_tiles.iter().any(|t| t == tile_id)
    }

    pub fn is_header_pinned(&self, sensor_id: &str) -> bool {
        self.header_pins.iter().any(|p| p == sensor_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedLook {
    pub id: String,
    pub name: String,
    pub design: DesignId,
    #[serde(default)]
    pub accent_override: Option<[u8; 3]>,
    #[serde(default)]
    pub rig_style: RigStyle,
    #[serde(default = "default_true")]
    pub firefly_particles: bool,
}

pub fn default_saved_looks() -> Vec<SavedLook> {
    vec![
        SavedLook {
            id: "firefly-pink".to_string(),
            name: "Firefly Pink".to_string(),
            design: DesignId::Firefly,
            accent_override: Some([0xf4, 0x8f, 0xb1]),
            rig_style: RigStyle::Living,
            firefly_particles: true,
        },
        SavedLook {
            id: "firefly-classic".to_string(),
            name: "Firefly Classic".to_string(),
            design: DesignId::Firefly,
            accent_override: None,
            rig_style: RigStyle::Living,
            firefly_particles: true,
        },
        SavedLook {
            id: "midnight-blueprint".to_string(),
            name: "Midnight Blueprint".to_string(),
            design: DesignId::Midnight,
            accent_override: Some([0x6b, 0xcb, 0xf7]),
            rig_style: RigStyle::Architectural,
            firefly_particles: false,
        },
        SavedLook {
            id: "kiln-ember".to_string(),
            name: "Kiln Ember".to_string(),
            design: DesignId::Kiln,
            accent_override: Some([0xff, 0x8a, 0x75]),
            rig_style: RigStyle::Living,
            firefly_particles: true,
        },
    ]
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppSettings {
    pub design: DesignId,
    pub lhm_url: String,
    pub openrgb_enabled: bool,
    pub openrgb_host: String,
    pub openrgb_port: u16,
    pub temp_source: TempSource,
    #[serde(default)]
    pub temp_unit: TempUnit,
    pub fan_failsafe_duty: u8,
    pub fan_max_step: u8,
    #[serde(default = "default_fan_hysteresis_c")]
    pub fan_hysteresis_c: f32,
    pub energy_price_eur_kwh: f32,
    pub demo_mode: bool,
    pub lights_off_on_sleep: bool,
    #[serde(default)]
    pub lights_off_on_lock: bool,
    #[serde(default)]
    pub start_with_windows: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            design: DesignId::Firefly,
            lhm_url: "http://127.0.0.1:8085".to_string(),
            openrgb_enabled: true,
            openrgb_host: "127.0.0.1".to_string(),
            openrgb_port: 6742,
            temp_source: TempSource::Max,
            temp_unit: TempUnit::C,
            fan_failsafe_duty: 30,
            fan_max_step: 5,
            fan_hysteresis_c: 2.0,
            energy_price_eur_kwh: 0.3797,
            demo_mode: true,
            lights_off_on_sleep: true,
            lights_off_on_lock: false,
            start_with_windows: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    #[default]
    Stable,
    Beta,
}

impl UpdateChannel {
    pub const ALL: [UpdateChannel; 2] = [Self::Stable, Self::Beta];

    pub fn label(self) -> &'static str {
        match self {
            Self::Stable => "Stable",
            Self::Beta => "Beta",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TileStyle {
    #[default]
    Auto,
    Number,
    Gauge,
    Graph,
    Bar,
}

impl TileStyle {
    pub const ALL: [TileStyle; 5] = [
        Self::Auto,
        Self::Number,
        Self::Gauge,
        Self::Graph,
        Self::Bar,
    ];

    pub fn next(self) -> Self {
        match self {
            Self::Auto => Self::Number,
            Self::Number => Self::Gauge,
            Self::Gauge => Self::Graph,
            Self::Graph => Self::Bar,
            Self::Bar => Self::Auto,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Instrument",
            Self::Number => "Number",
            Self::Gauge => "Gauge",
            Self::Graph => "Graph",
            Self::Bar => "Bar",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OverviewTileKind {
    Lighting,
    Profiles,
    Fans,
    Hardware,
    DeskDisplay,
    Lookbook,
    CpuTemp,
    GpuTemp,
    GpuHotspot,
    LiquidTemp,
    CpuLoad,
    GpuLoad,
    RamUsage,
    DriveTemp,
    PowerDraw,
    CostEfficiency,
    NetDown,
    NetUp,
    CustomSensor {
        sensor_id: String,
        label: String,
        unit: String,
    },
}

impl OverviewTileKind {
    pub fn builtin_controls() -> Vec<Self> {
        vec![
            Self::Lighting,
            Self::Profiles,
            Self::Fans,
            Self::Hardware,
            Self::DeskDisplay,
            Self::Lookbook,
        ]
    }

    pub fn builtin_sensors() -> Vec<Self> {
        vec![
            Self::CpuTemp,
            Self::GpuTemp,
            Self::GpuHotspot,
            Self::LiquidTemp,
            Self::CpuLoad,
            Self::GpuLoad,
            Self::RamUsage,
            Self::DriveTemp,
            Self::PowerDraw,
            Self::CostEfficiency,
            Self::NetDown,
            Self::NetUp,
        ]
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Lighting => "Living Rig",
            Self::Profiles => "Profiles",
            Self::Fans => "Case Fans",
            Self::Hardware => "Hardware Status",
            Self::DeskDisplay => "Desk Display",
            Self::Lookbook => "Lighting Lookbook",
            Self::CpuTemp => "CPU Temp",
            Self::GpuTemp => "GPU Temp",
            Self::GpuHotspot => "GPU Hotspot",
            Self::LiquidTemp => "Liquid Temp",
            Self::CpuLoad => "CPU Load",
            Self::GpuLoad => "GPU Load",
            Self::RamUsage => "RAM Usage",
            Self::DriveTemp => "Drive Temp",
            Self::PowerDraw => "Power Draw",
            Self::CostEfficiency => "Energy Cost",
            Self::NetDown => "Net Down",
            Self::NetUp => "Net Up",
            Self::CustomSensor {
                sensor_id, label, ..
            } => {
                let trimmed = label.trim();
                if !trimmed.is_empty() {
                    trimmed
                } else {
                    sensor_id.as_str()
                }
            }
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Lighting => "Live rig stage with quick brightness, power, and expand controls.",
            Self::Profiles => "One-click lighting and cooling profiles with live trial preview.",
            Self::Fans => "Live cooling curve selector, fan RPM telemetry, and duty readout.",
            Self::Hardware => "USB HID controllers, OpenRGB bridge, and LHM sensor status.",
            Self::DeskDisplay => "Ambient desk clock, focus timer, and compact mini-mode launcher.",
            Self::Lookbook => "Curated lighting harmonies and favorite effects at a glance.",
            Self::CpuTemp => "Processor package temperature and recent thermal trend.",
            Self::GpuTemp => "Graphics core temperature and recent thermal trend.",
            Self::GpuHotspot => "Graphics hotspot peak temperature readout.",
            Self::LiquidTemp => "AIO coolant temperature readout.",
            Self::CpuLoad => "Processor utilization percentage.",
            Self::GpuLoad => "Graphics card utilization percentage.",
            Self::RamUsage => "System memory usage and capacity.",
            Self::DriveTemp => "Primary NVMe / SSD thermal reading.",
            Self::PowerDraw => "Combined CPU and GPU package power draw in watts.",
            Self::CostEfficiency => "Estimated hourly electricity cost from live wattage.",
            Self::NetDown => "Network download throughput.",
            Self::NetUp => "Network upload throughput.",
            Self::CustomSensor { .. } => "Pinned live LibreHardwareMonitor sensor tile.",
        }
    }

    pub fn category(&self) -> &'static str {
        match self {
            Self::Lighting
            | Self::Profiles
            | Self::Fans
            | Self::Hardware
            | Self::DeskDisplay
            | Self::Lookbook => "Controls",
            Self::CustomSensor { .. } => "Live Sensors",
            _ => "Sensors",
        }
    }

    pub fn default_span(&self) -> u8 {
        match self {
            Self::Lighting
            | Self::Profiles
            | Self::PowerDraw
            | Self::DeskDisplay
            | Self::Lookbook
            | Self::Hardware
            | Self::CostEfficiency => 2,
            Self::CpuTemp
            | Self::GpuTemp
            | Self::CpuLoad
            | Self::GpuLoad
            | Self::Fans
            | Self::RamUsage
            | Self::DriveTemp
            | Self::NetDown
            | Self::NetUp
            | Self::GpuHotspot
            | Self::LiquidTemp
            | Self::CustomSensor { .. } => 1,
        }
    }

    pub fn default_rows(&self) -> u8 {
        match self {
            Self::Lighting => 2,
            _ => 1,
        }
    }

    pub fn default_tall(&self) -> bool {
        self.default_rows() >= 2
    }

    pub fn is_sensor(&self) -> bool {
        !matches!(
            self,
            Self::Lighting
                | Self::Profiles
                | Self::Fans
                | Self::Hardware
                | Self::DeskDisplay
                | Self::Lookbook
        )
    }

    fn id_prefix(&self) -> &'static str {
        match self {
            Self::Lighting => "lighting",
            Self::Profiles => "profiles",
            Self::Fans => "fans",
            Self::Hardware => "hardware",
            Self::DeskDisplay => "desk-display",
            Self::Lookbook => "lookbook",
            Self::CpuTemp => "cpu-temp",
            Self::GpuTemp => "gpu-temp",
            Self::GpuHotspot => "gpu-hotspot",
            Self::LiquidTemp => "liquid-temp",
            Self::CpuLoad => "cpu-load",
            Self::GpuLoad => "gpu-load",
            Self::RamUsage => "ram-usage",
            Self::DriveTemp => "drive-temp",
            Self::PowerDraw => "power-draw",
            Self::CostEfficiency => "cost",
            Self::NetDown => "net-down",
            Self::NetUp => "net-up",
            Self::CustomSensor { .. } => "sensor",
        }
    }
}

fn default_tile_span() -> u8 {
    1
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "OverviewTileSerde")]
pub struct OverviewTile {
    pub id: String,
    pub kind: OverviewTileKind,
    #[serde(default = "default_tile_span")]
    pub span: u8,
    #[serde(default)]
    pub rows: u8,
    #[serde(default)]
    pub tall: bool,
    #[serde(default)]
    pub style: TileStyle,
    #[serde(default)]
    pub accent: Option<[u8; 3]>,
}

#[derive(Deserialize)]
struct OverviewTileSerde {
    id: String,
    kind: OverviewTileKind,
    #[serde(default = "default_tile_span")]
    span: u8,
    #[serde(default)]
    rows: u8,
    #[serde(default)]
    tall: bool,
    #[serde(default)]
    style: TileStyle,
    #[serde(default)]
    accent: Option<[u8; 3]>,
}

impl From<OverviewTileSerde> for OverviewTile {
    fn from(raw: OverviewTileSerde) -> Self {
        let rows = if raw.rows == 0 {
            if raw.tall { 2 } else { 1 }
        } else {
            raw.rows.clamp(1, 3)
        };
        let tall = rows >= 2;
        Self {
            id: raw.id,
            kind: raw.kind,
            span: raw.span.clamp(1, 4),
            rows,
            tall,
            style: raw.style,
            accent: raw.accent,
        }
    }
}

impl OverviewTile {
    pub fn new(id: impl Into<String>, kind: OverviewTileKind) -> Self {
        let span = kind.default_span();
        let rows = kind.default_rows().clamp(1, 3);
        let tall = rows >= 2;
        Self {
            id: id.into(),
            kind,
            span,
            rows,
            tall,
            style: TileStyle::Auto,
            accent: None,
        }
    }

    pub fn with_dims(
        id: impl Into<String>,
        kind: OverviewTileKind,
        span: u8,
        rows: u8,
        style: TileStyle,
    ) -> Self {
        let clamped_rows = rows.clamp(1, 3);
        Self {
            id: id.into(),
            kind,
            span: span.clamp(1, 4),
            rows: clamped_rows,
            tall: clamped_rows >= 2,
            style,
            accent: None,
        }
    }

    fn with_layout(
        id: impl Into<String>,
        kind: OverviewTileKind,
        span: u8,
        tall: bool,
        style: TileStyle,
    ) -> Self {
        Self::with_dims(id, kind, span, if tall { 2 } else { 1 }, style)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverviewWorkspace {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub tiles: Vec<OverviewTile>,
}

impl OverviewWorkspace {
    pub fn default_workspace() -> Self {
        Self {
            id: "default".to_string(),
            name: "My workspace".to_string(),
            tiles: vec![
                OverviewTile::with_dims(
                    "lighting-1",
                    OverviewTileKind::Lighting,
                    2,
                    2,
                    TileStyle::Auto,
                ),
                OverviewTile::with_dims(
                    "cpu-temp-1",
                    OverviewTileKind::CpuTemp,
                    1,
                    1,
                    TileStyle::Number,
                ),
                OverviewTile::with_dims(
                    "gpu-temp-1",
                    OverviewTileKind::GpuTemp,
                    1,
                    1,
                    TileStyle::Number,
                ),
                OverviewTile::with_dims(
                    "cpu-load-1",
                    OverviewTileKind::CpuLoad,
                    1,
                    1,
                    TileStyle::Gauge,
                ),
                OverviewTile::with_dims(
                    "gpu-load-1",
                    OverviewTileKind::GpuLoad,
                    1,
                    1,
                    TileStyle::Gauge,
                ),
                OverviewTile::with_dims(
                    "profiles-1",
                    OverviewTileKind::Profiles,
                    2,
                    1,
                    TileStyle::Auto,
                ),
                OverviewTile::with_dims(
                    "power-draw-1",
                    OverviewTileKind::PowerDraw,
                    2,
                    1,
                    TileStyle::Auto,
                ),
                OverviewTile::with_dims(
                    "fans-1",
                    OverviewTileKind::Fans,
                    1,
                    1,
                    TileStyle::Auto,
                ),
                OverviewTile::with_dims(
                    "ram-usage-1",
                    OverviewTileKind::RamUsage,
                    1,
                    1,
                    TileStyle::Bar,
                ),
                OverviewTile::with_dims(
                    "drive-temp-1",
                    OverviewTileKind::DriveTemp,
                    1,
                    1,
                    TileStyle::Number,
                ),
                OverviewTile::with_dims(
                    "net-down-1",
                    OverviewTileKind::NetDown,
                    1,
                    1,
                    TileStyle::Number,
                ),
            ],
        }
    }

    pub fn starter_templates() -> Vec<Self> {
        vec![
            Self {
                id: "gaming".to_string(),
                name: "Gaming".to_string(),
                tiles: vec![
                    OverviewTile::with_layout(
                        "lighting-1",
                        OverviewTileKind::Lighting,
                        2,
                        true,
                        TileStyle::Auto,
                    ),
                    OverviewTile::with_layout(
                        "profiles-1",
                        OverviewTileKind::Profiles,
                        2,
                        true,
                        TileStyle::Auto,
                    ),
                    OverviewTile::with_layout(
                        "gpu-temp-1",
                        OverviewTileKind::GpuTemp,
                        1,
                        false,
                        TileStyle::Gauge,
                    ),
                    OverviewTile::with_layout(
                        "gpu-hotspot-1",
                        OverviewTileKind::GpuHotspot,
                        1,
                        false,
                        TileStyle::Number,
                    ),
                    OverviewTile::with_layout(
                        "gpu-load-1",
                        OverviewTileKind::GpuLoad,
                        1,
                        false,
                        TileStyle::Bar,
                    ),
                    OverviewTile::with_layout(
                        "cpu-temp-1",
                        OverviewTileKind::CpuTemp,
                        1,
                        false,
                        TileStyle::Gauge,
                    ),
                    OverviewTile::with_layout(
                        "fans-1",
                        OverviewTileKind::Fans,
                        2,
                        false,
                        TileStyle::Auto,
                    ),
                    OverviewTile::with_layout(
                        "power-draw-1",
                        OverviewTileKind::PowerDraw,
                        1,
                        false,
                        TileStyle::Graph,
                    ),
                    OverviewTile::with_layout(
                        "net-down-1",
                        OverviewTileKind::NetDown,
                        1,
                        false,
                        TileStyle::Graph,
                    ),
                ],
            },
            Self {
                id: "quiet-studio".to_string(),
                name: "Quiet Studio".to_string(),
                tiles: vec![
                    OverviewTile::with_layout(
                        "lighting-1",
                        OverviewTileKind::Lighting,
                        2,
                        true,
                        TileStyle::Auto,
                    ),
                    OverviewTile::with_layout(
                        "desk-display-1",
                        OverviewTileKind::DeskDisplay,
                        2,
                        true,
                        TileStyle::Auto,
                    ),
                    OverviewTile::with_layout(
                        "lookbook-1",
                        OverviewTileKind::Lookbook,
                        2,
                        false,
                        TileStyle::Auto,
                    ),
                    OverviewTile::with_layout(
                        "fans-1",
                        OverviewTileKind::Fans,
                        2,
                        false,
                        TileStyle::Auto,
                    ),
                    OverviewTile::with_layout(
                        "cpu-temp-1",
                        OverviewTileKind::CpuTemp,
                        1,
                        false,
                        TileStyle::Number,
                    ),
                    OverviewTile::with_layout(
                        "gpu-temp-1",
                        OverviewTileKind::GpuTemp,
                        1,
                        false,
                        TileStyle::Number,
                    ),
                    OverviewTile::with_layout(
                        "power-draw-1",
                        OverviewTileKind::PowerDraw,
                        1,
                        false,
                        TileStyle::Number,
                    ),
                    OverviewTile::with_layout(
                        "cost-1",
                        OverviewTileKind::CostEfficiency,
                        1,
                        false,
                        TileStyle::Number,
                    ),
                ],
            },
            Self {
                id: "thermal-lab".to_string(),
                name: "Thermal Lab".to_string(),
                tiles: vec![
                    OverviewTile::with_layout(
                        "lighting-1",
                        OverviewTileKind::Lighting,
                        2,
                        true,
                        TileStyle::Auto,
                    ),
                    OverviewTile::with_layout(
                        "fans-1",
                        OverviewTileKind::Fans,
                        2,
                        true,
                        TileStyle::Auto,
                    ),
                    OverviewTile::with_layout(
                        "cpu-temp-1",
                        OverviewTileKind::CpuTemp,
                        1,
                        false,
                        TileStyle::Graph,
                    ),
                    OverviewTile::with_layout(
                        "gpu-temp-1",
                        OverviewTileKind::GpuTemp,
                        1,
                        false,
                        TileStyle::Graph,
                    ),
                    OverviewTile::with_layout(
                        "gpu-hotspot-1",
                        OverviewTileKind::GpuHotspot,
                        1,
                        false,
                        TileStyle::Graph,
                    ),
                    OverviewTile::with_layout(
                        "liquid-temp-1",
                        OverviewTileKind::LiquidTemp,
                        1,
                        false,
                        TileStyle::Graph,
                    ),
                    OverviewTile::with_layout(
                        "drive-temp-1",
                        OverviewTileKind::DriveTemp,
                        1,
                        false,
                        TileStyle::Gauge,
                    ),
                    OverviewTile::with_layout(
                        "cpu-load-1",
                        OverviewTileKind::CpuLoad,
                        1,
                        false,
                        TileStyle::Bar,
                    ),
                    OverviewTile::with_layout(
                        "gpu-load-1",
                        OverviewTileKind::GpuLoad,
                        1,
                        false,
                        TileStyle::Bar,
                    ),
                    OverviewTile::with_layout(
                        "power-draw-1",
                        OverviewTileKind::PowerDraw,
                        1,
                        false,
                        TileStyle::Graph,
                    ),
                    OverviewTile::with_layout(
                        "hardware-1",
                        OverviewTileKind::Hardware,
                        2,
                        false,
                        TileStyle::Auto,
                    ),
                    OverviewTile::with_layout(
                        "ram-usage-1",
                        OverviewTileKind::RamUsage,
                        1,
                        false,
                        TileStyle::Bar,
                    ),
                    OverviewTile::with_layout(
                        "cost-1",
                        OverviewTileKind::CostEfficiency,
                        1,
                        false,
                        TileStyle::Number,
                    ),
                ],
            },
        ]
    }
}

fn default_overview_workspaces() -> Vec<OverviewWorkspace> {
    vec![OverviewWorkspace::default_workspace()]
}

fn default_active_workspace_id() -> String {
    "default".to_string()
}

fn slugify_id(raw: &str, fallback: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in raw.trim().to_ascii_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    pub settings: AppSettings,
    #[serde(default)]
    pub appearance: AppearanceConfig,
    #[serde(default = "default_saved_looks")]
    pub saved_looks: Vec<SavedLook>,
    pub scene: SceneConfig,
    pub active_curve: String,
    pub active_preset: Option<String>,
    pub fan_curves: BTreeMap<String, Vec<CurvePoint>>,
    #[serde(default)]
    pub fan_channels: BTreeMap<String, FanChannelMode>,
    pub presets: Vec<Preset>,
    pub zones: Vec<ZoneConfig>,
    #[serde(default = "default_rules")]
    pub automations: Vec<AutomationRule>,
    #[serde(default)]
    pub automations_paused: bool,
    #[serde(default = "default_studio_effects")]
    pub studio_effects: Vec<SavedStudioEffect>,
    #[serde(default = "default_overview_workspaces")]
    pub overview_workspaces: Vec<OverviewWorkspace>,
    #[serde(default = "default_active_workspace_id")]
    pub active_workspace_id: String,
    #[serde(default)]
    pub update_channel: UpdateChannel,
    #[serde(default = "default_true")]
    pub auto_check_updates: bool,
    #[serde(default)]
    pub fullscreen_fill_width: bool,
}

impl AppConfig {
    /// Normalizes config invariants: valid appearance bounds, hysteresis range, at least one
    /// Overview workspace, valid tile spans/IDs, and a valid `active_workspace_id`.
    pub fn normalize(&mut self) {
        self.appearance.normalize();
        self.settings.fan_hysteresis_c = self.settings.fan_hysteresis_c.clamp(0.0, 10.0);

        if self.overview_workspaces.is_empty() {
            self.overview_workspaces = default_overview_workspaces();
        }
        let mut seen_ws_ids: Vec<String> = Vec::with_capacity(self.overview_workspaces.len());
        for (idx, ws) in self.overview_workspaces.iter_mut().enumerate() {
            if ws.name.trim().is_empty() {
                ws.name = format!("Workspace {}", idx + 1);
            }
            let base_id = if ws.id.trim().is_empty() {
                slugify_id(&ws.name, &format!("workspace-{}", idx + 1))
            } else {
                ws.id.trim().to_string()
            };
            let mut candidate = base_id.clone();
            let mut suffix = 2usize;
            while seen_ws_ids.iter().any(|existing| existing == &candidate) {
                candidate = format!("{base_id}-{suffix}");
                suffix += 1;
            }
            ws.id = candidate.clone();
            seen_ws_ids.push(candidate);

            let is_default_ws = ws.id == "default"
                || ws.id == "my-workspace"
                || ws.name.eq_ignore_ascii_case("my workspace");
            let missing_gpu_load = !ws
                .tiles
                .iter()
                .any(|t| matches!(t.kind, OverviewTileKind::GpuLoad));
            let has_custom_sensor = ws
                .tiles
                .iter()
                .any(|t| matches!(t.kind, OverviewTileKind::CustomSensor { .. }));
            let has_legacy_tall_profiles = ws
                .tiles
                .iter()
                .any(|t| matches!(t.kind, OverviewTileKind::Profiles) && (t.tall || t.rows >= 2));
            let has_legacy_v21_spans = !ws.tiles.is_empty()
                && ws.tiles.iter().all(|t| t.style == TileStyle::Auto)
                && ws
                    .tiles
                    .iter()
                    .any(|t| matches!(t.kind, OverviewTileKind::PowerDraw) && t.span == 1)
                && ws
                    .tiles
                    .iter()
                    .any(|t| matches!(t.kind, OverviewTileKind::Fans) && t.span == 2);
            if is_default_ws
                && missing_gpu_load
                && !has_custom_sensor
                && (has_legacy_tall_profiles || has_legacy_v21_spans)
            {
                ws.tiles = OverviewWorkspace::default_workspace().tiles;
            }

            let mut seen_tile_ids: Vec<String> = Vec::with_capacity(ws.tiles.len());
            for (tidx, tile) in ws.tiles.iter_mut().enumerate() {
                tile.span = tile.span.clamp(1, 4);
                if tile.rows == 0 {
                    tile.rows = if tile.tall { 2 } else { 1 };
                }
                tile.rows = tile.rows.clamp(1, 3);
                tile.tall = tile.rows >= 2;
                let base_tid = if tile.id.trim().is_empty() {
                    format!("{}-{}", tile.kind.id_prefix(), tidx + 1)
                } else {
                    tile.id.trim().to_string()
                };
                let mut tcand = base_tid.clone();
                let mut tsuf = 2usize;
                while seen_tile_ids.iter().any(|existing| existing == &tcand) {
                    tcand = format!("{base_tid}-{tsuf}");
                    tsuf += 1;
                }
                tile.id = tcand.clone();
                seen_tile_ids.push(tcand);
            }
        }

        if !self
            .overview_workspaces
            .iter()
            .any(|w| w.id == self.active_workspace_id)
        {
            self.active_workspace_id = self.overview_workspaces[0].id.clone();
        }
    }

    pub fn normalized(mut self) -> Self {
        self.normalize();
        self
    }

    pub fn active_workspace(&self) -> &OverviewWorkspace {
        self.overview_workspaces
            .iter()
            .find(|w| w.id == self.active_workspace_id)
            .or_else(|| self.overview_workspaces.first())
            .expect("AppConfig always has at least one OverviewWorkspace")
    }

    pub fn active_workspace_mut(&mut self) -> &mut OverviewWorkspace {
        if self.overview_workspaces.is_empty() {
            self.overview_workspaces = default_overview_workspaces();
        }
        let idx = self
            .overview_workspaces
            .iter()
            .position(|w| w.id == self.active_workspace_id)
            .unwrap_or_else(|| {
                self.active_workspace_id = self.overview_workspaces[0].id.clone();
                0
            });
        &mut self.overview_workspaces[idx]
    }

    pub fn add_overview_tile(&mut self, kind: OverviewTileKind) -> String {
        let ws = self.active_workspace_mut();
        let prefix = kind.id_prefix();
        let mut n = 1usize;
        let id = loop {
            let candidate = format!("{prefix}-{n}");
            if !ws.tiles.iter().any(|t| t.id == candidate) {
                break candidate;
            }
            n += 1;
        };
        if ws.tiles.len() < 36 {
            ws.tiles.push(OverviewTile::new(id.clone(), kind));
        }
        id
    }

    pub fn remove_overview_tile(&mut self, id: &str) -> bool {
        let ws = self.active_workspace_mut();
        let before = ws.tiles.len();
        ws.tiles.retain(|t| t.id != id);
        ws.tiles.len() != before
    }

    pub fn move_overview_tile(&mut self, id: &str, delta: isize) -> bool {
        let ws = self.active_workspace_mut();
        let Some(pos) = ws.tiles.iter().position(|t| t.id == id) else {
            return false;
        };
        if ws.tiles.len() <= 1 || delta == 0 {
            return false;
        }
        let max_idx = (ws.tiles.len() - 1) as isize;
        let target = (pos as isize + delta).clamp(0, max_idx) as usize;
        if target == pos {
            return false;
        }
        let tile = ws.tiles.remove(pos);
        ws.tiles.insert(target, tile);
        true
    }

    pub fn reorder_overview_tile_to(&mut self, id: &str, to_index: usize) -> bool {
        let ws = self.active_workspace_mut();
        let Some(from_index) = ws.tiles.iter().position(|t| t.id == id) else {
            return false;
        };
        if ws.tiles.len() <= 1 {
            return false;
        }
        let target = to_index.min(ws.tiles.len() - 1);
        if target == from_index {
            return false;
        }
        let tile = ws.tiles.remove(from_index);
        ws.tiles.insert(target, tile);
        true
    }

    pub fn resize_overview_tile(&mut self, id: &str, span: u8, rows: u8) -> bool {
        let ws = self.active_workspace_mut();
        let Some(tile) = ws.tiles.iter_mut().find(|t| t.id == id) else {
            return false;
        };
        let new_span = span.clamp(1, 4);
        let new_rows = rows.clamp(1, 3);
        let new_tall = new_rows >= 2;
        if tile.span == new_span && tile.rows == new_rows && tile.tall == new_tall {
            return false;
        }
        tile.span = new_span;
        tile.rows = new_rows;
        tile.tall = new_tall;
        true
    }

    pub fn cycle_overview_tile_span(&mut self, id: &str) -> Option<u8> {
        let ws = self.active_workspace_mut();
        let tile = ws.tiles.iter_mut().find(|t| t.id == id)?;
        tile.span = (tile.span.clamp(1, 4) % 4) + 1;
        Some(tile.span)
    }

    pub fn toggle_overview_tile_tall(&mut self, id: &str) -> Option<bool> {
        let ws = self.active_workspace_mut();
        let tile = ws.tiles.iter_mut().find(|t| t.id == id)?;
        tile.tall = !tile.tall;
        tile.rows = if tile.tall { 2 } else { 1 };
        Some(tile.tall)
    }

    pub fn cycle_overview_tile_style(&mut self, id: &str) -> Option<TileStyle> {
        let ws = self.active_workspace_mut();
        let tile = ws.tiles.iter_mut().find(|t| t.id == id)?;
        tile.style = tile.style.next();
        Some(tile.style)
    }

    pub fn reset_active_workspace(&mut self) {
        let active_id = self.active_workspace().id.clone();
        let template_tiles = OverviewWorkspace::starter_templates()
            .into_iter()
            .find(|t| t.id == active_id)
            .map(|t| t.tiles)
            .unwrap_or_else(|| OverviewWorkspace::default_workspace().tiles);
        self.active_workspace_mut().tiles = template_tiles;
    }

    pub fn switch_workspace(&mut self, id: &str) -> bool {
        if self.overview_workspaces.iter().any(|w| w.id == id) {
            self.active_workspace_id = id.to_string();
            true
        } else {
            false
        }
    }

    pub fn create_workspace(&mut self, name: &str, template_id: Option<&str>) -> String {
        self.normalize();
        if self.overview_workspaces.len() >= 12 {
            return self.active_workspace_id.clone();
        }
        let templates = OverviewWorkspace::starter_templates();
        let matched_template = template_id.and_then(|tid| {
            let needle = tid.trim().to_ascii_lowercase();
            templates
                .iter()
                .find(|t| t.id == needle || t.name.to_ascii_lowercase() == needle)
        });

        let trimmed_name = name.trim();
        let final_name = if !trimmed_name.is_empty() {
            trimmed_name.to_string()
        } else if let Some(tpl) = matched_template {
            tpl.name.clone()
        } else {
            format!("Workspace {}", self.overview_workspaces.len() + 1)
        };

        let base_id = slugify_id(
            &final_name,
            &format!("workspace-{}", self.overview_workspaces.len() + 1),
        );
        let mut id = base_id.clone();
        let mut suffix = 2usize;
        while self.overview_workspaces.iter().any(|w| w.id == id) {
            id = format!("{base_id}-{suffix}");
            suffix += 1;
        }

        let tiles = matched_template
            .map(|t| t.tiles.clone())
            .unwrap_or_else(|| OverviewWorkspace::default_workspace().tiles);

        self.overview_workspaces.push(OverviewWorkspace {
            id: id.clone(),
            name: final_name,
            tiles,
        });
        self.active_workspace_id = id.clone();
        id
    }

    pub fn rename_workspace(&mut self, id: &str, name: &str) -> bool {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return false;
        }
        if let Some(ws) = self.overview_workspaces.iter_mut().find(|w| w.id == id) {
            ws.name = trimmed.to_string();
            true
        } else {
            false
        }
    }

    pub fn delete_workspace(&mut self, id: &str) -> bool {
        if self.overview_workspaces.len() <= 1 {
            return false;
        }
        let before = self.overview_workspaces.len();
        self.overview_workspaces.retain(|w| w.id != id);
        if self.overview_workspaces.len() == before {
            return false;
        }
        if self.active_workspace_id == id {
            self.active_workspace_id = self.overview_workspaces[0].id.clone();
        }
        true
    }
}

pub fn default_curves() -> BTreeMap<String, Vec<CurvePoint>> {
    let mut fan_curves = BTreeMap::new();
    fan_curves.insert(
        "silent".to_string(),
        vec![
            CurvePoint { temp: 40, duty: 25 },
            CurvePoint { temp: 55, duty: 30 },
            CurvePoint { temp: 68, duty: 45 },
            CurvePoint { temp: 78, duty: 65 },
            CurvePoint { temp: 88, duty: 100 },
        ],
    );
    fan_curves.insert(
        "balanced".to_string(),
        vec![
            CurvePoint { temp: 40, duty: 30 },
            CurvePoint { temp: 55, duty: 40 },
            CurvePoint { temp: 68, duty: 60 },
            CurvePoint { temp: 78, duty: 80 },
            CurvePoint { temp: 88, duty: 100 },
        ],
    );
    fan_curves.insert(
        "performance".to_string(),
        vec![
            CurvePoint { temp: 40, duty: 45 },
            CurvePoint { temp: 55, duty: 60 },
            CurvePoint { temp: 68, duty: 80 },
            CurvePoint { temp: 75, duty: 100 },
        ],
    );
    fan_curves
}

impl Default for AppConfig {
    fn default() -> Self {
        let fan_curves = default_curves();

        let presets = vec![
            Preset {
                id: "silent".to_string(),
                name: "Silent".to_string(),
                description: "Warm amber glow with quiet fans.".to_string(),
                effect: EffectId::Solid,
                params: SceneParams {
                    color: [255, 122, 26],
                    ..SceneParams::default()
                },
                brightness: 35,
                fan_curve: "silent".to_string(),
                studio_stack: None,
                zone_overrides: BTreeMap::new(),
            },
            Preset {
                id: "balanced".to_string(),
                name: "Balanced".to_string(),
                description: "Cool azure gradient with balanced cooling.".to_string(),
                effect: EffectId::Gradient,
                params: SceneParams {
                    colors: vec![[0, 180, 255], [47, 91, 255]],
                    speed: 25,
                    ..SceneParams::default()
                },
                brightness: 80,
                fan_curve: "balanced".to_string(),
                studio_stack: None,
                zone_overrides: BTreeMap::new(),
            },
            Preset {
                id: "gaming".to_string(),
                name: "Gaming".to_string(),
                description: "Vivid rainbow wave and performance fan curve.".to_string(),
                effect: EffectId::Rainbow,
                params: SceneParams {
                    speed: 55,
                    ..SceneParams::default()
                },
                brightness: 100,
                fan_curve: "performance".to_string(),
                studio_stack: None,
                zone_overrides: BTreeMap::new(),
            },
            Preset {
                id: "night".to_string(),
                name: "Night".to_string(),
                description: "Dim ember breathing for late-night sessions.".to_string(),
                effect: EffectId::Breathing,
                params: SceneParams {
                    color: [255, 90, 31],
                    speed: 15,
                    ..SceneParams::default()
                },
                brightness: 15,
                fan_curve: "silent".to_string(),
                studio_stack: None,
                zone_overrides: BTreeMap::new(),
            },
            Preset {
                id: "heatmap".to_string(),
                name: "Heat Map".to_string(),
                description: "Shifts from cool blue to red as temps climb.".to_string(),
                effect: EffectId::Temperature,
                params: SceneParams {
                    color: [0, 180, 255],
                    ..SceneParams::default()
                },
                brightness: 90,
                fan_curve: "balanced".to_string(),
                studio_stack: None,
                zone_overrides: BTreeMap::new(),
            },
        ];

        Self {
            settings: AppSettings::default(),
            appearance: AppearanceConfig::default(),
            saved_looks: default_saved_looks(),
            scene: SceneConfig::default(),
            active_curve: "balanced".to_string(),
            active_preset: Some("balanced".to_string()),
            fan_curves,
            fan_channels: BTreeMap::new(),
            presets,
            zones: crate::geometry::sample_case_zones(),
            automations: default_rules(),
            automations_paused: false,
            studio_effects: default_studio_effects(),
            overview_workspaces: default_overview_workspaces(),
            active_workspace_id: default_active_workspace_id(),
            update_channel: UpdateChannel::Stable,
            auto_check_updates: true,
            fullscreen_fill_width: false,
        }
    }
}

pub fn default_data_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Airglow-rs")
}

pub fn python_airglow_data_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Airglow")
}

pub struct ConfigStore {
    dir: PathBuf,
    path: PathBuf,
}

impl ConfigStore {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref().to_path_buf();
        let path = dir.join("config.json");
        Self { dir, path }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    pub fn load_or_default(&self) -> AppConfig {
        let mut cfg = match fs::read_to_string(&self.path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
            Err(_) => AppConfig::default(),
        };
        cfg.normalize();
        cfg
    }

    /// Loads `%APPDATA%\Airglow-rs\config.json` if present; otherwise on first launch,
    /// attempts to auto-migrate from `%APPDATA%\Airglow` and persists the migrated config.
    /// Returns `(config, did_auto_migrate)`.
    pub fn load_or_migrate(&self) -> (AppConfig, bool) {
        if self.exists() {
            return (self.load_or_default(), false);
        }
        let mut cfg = AppConfig::default();
        if let Some(py_dir) = default_python_airglow_dir()
            && py_dir.is_dir()
            && let Ok(migrated) = import_python_airglow(&py_dir)
        {
            apply_migrated_data(&mut cfg, &migrated);
            cfg.normalize();
            let _ = self.save(&cfg);
            return (cfg, true);
        }
        cfg.normalize();
        (cfg, false)
    }

    pub fn save(&self, cfg: &AppConfig) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).context("create config directory")?;
        }
        let tmp = self.path.with_extension("json.tmp");
        let payload = serde_json::to_string_pretty(cfg)?;
        fs::write(&tmp, payload).context("write temp config")?;
        fs::rename(&tmp, &self.path).context("replace config file")?;
        Ok(())
    }
}

pub fn parse_design_id(s: &str) -> DesignId {
    match s.trim().to_ascii_lowercase().as_str() {
        "firefly" => DesignId::Firefly,
        "graphite" => DesignId::Graphite,
        "ascii" => DesignId::Ascii,
        "paper" => DesignId::Paper,
        "amber" => DesignId::Amber,
        "contrast" => DesignId::Contrast,
        "neon" => DesignId::Neon,
        "kiln" => DesignId::Kiln,
        "lumen" => DesignId::Lumen,
        "midnight" => DesignId::Midnight,
        "spore" => DesignId::Spore,
        "rim" => DesignId::Rim,
        "darkroom" => DesignId::Darkroom,
        _ => DesignId::Firefly,
    }
}

pub fn parse_color_order(s: &str) -> ColorOrder {
    match s.trim().to_ascii_uppercase().as_str() {
        "RBG" => ColorOrder::Rbg,
        "BGR" => ColorOrder::Bgr,
        "BRG" => ColorOrder::Brg,
        "GBR" => ColorOrder::Gbr,
        "GRB" => ColorOrder::Grb,
        _ => ColorOrder::Rgb,
    }
}

pub fn parse_temp_source(s: &str) -> TempSource {
    match s.trim().to_ascii_lowercase().as_str() {
        "cpu" => TempSource::Cpu,
        "gpu" => TempSource::Gpu,
        "liquid" => TempSource::Liquid,
        _ => TempSource::Max,
    }
}

fn convert_migrated_params(src: &MigratedSceneParams) -> SceneParams {
    let mut p = if src.raw.is_object() {
        parse_python_scene_params(&src.raw)
    } else {
        SceneParams::default()
    };
    if let Some(c) = src.color {
        p.color = c;
    }
    if let Some(bg) = src.background {
        p.secondary = bg;
    }
    if src.colors.len() >= 2 {
        p.colors = src.colors.clone();
    }
    if let Some(v) = src.speed {
        p.speed = v;
    }
    if let Some(v) = src.angle {
        p.angle = v % 360;
    }
    if let Some(v) = src.size {
        p.size = v;
    }
    if let Some(v) = src.saturation {
        p.saturation = v;
    }
    if let Some(v) = src.tail {
        p.tail = v;
    }
    if let Some(v) = src.reverse {
        p.reverse = v;
    }
    if let Some(v) = src.cool_temp {
        p.cool_temp = v;
    }
    if let Some(v) = src.hot_temp {
        p.hot_temp = v;
    }
    if let Some(v) = src.sensitivity {
        p.sensitivity = v;
    }
    if let Some(v) = src.floor {
        p.floor = v;
    }
    if let Some(v) = src.mirror {
        p.mirror = v;
    }
    p
}

/// Applies migrated Python Airglow settings, zones, presets, curves, fan channels,
/// automations, active scene, and layered Studio effects onto `cfg`.
pub fn apply_migrated_data(cfg: &mut AppConfig, data: &MigratedData) {
    cfg.settings.design = parse_design_id(&data.design);
    cfg.settings.lhm_url = data.lhm_url.clone();
    cfg.settings.openrgb_enabled = data.openrgb_enabled;
    cfg.settings.openrgb_host = data.openrgb_host.clone();
    cfg.settings.openrgb_port = data.openrgb_port;
    cfg.settings.temp_source = parse_temp_source(&data.temp_source);
    cfg.settings.temp_unit = if data.temp_unit.eq_ignore_ascii_case("F") {
        TempUnit::F
    } else {
        TempUnit::C
    };
    cfg.settings.energy_price_eur_kwh = data.energy_price;
    cfg.settings.fan_failsafe_duty = data.fan_failsafe_duty;
    cfg.settings.fan_max_step = data.fan_max_step;
    cfg.settings.lights_off_on_sleep = data.lights_off_on_sleep;
    cfg.settings.lights_off_on_lock = data.lights_off_on_lock;

    for (name, pts) in &data.fan_curves {
        let converted: Vec<CurvePoint> = pts
            .iter()
            .map(|&[temp, duty]| CurvePoint { temp, duty })
            .collect();
        if converted.len() >= 2 {
            cfg.fan_curves.insert(name.clone(), converted);
        }
    }

    if !data.fan_channels.is_empty() {
        cfg.fan_channels.clear();
        for (sid, ch) in &data.fan_channels {
            let mode = match ch.mode.as_str() {
                "curve" => FanChannelMode::Curve,
                "steady" => FanChannelMode::Steady {
                    duty: ch.duty.unwrap_or(60).clamp(MIN_FAN_DUTY, 100),
                },
                _ => FanChannelMode::Auto,
            };
            if mode != FanChannelMode::Auto {
                cfg.fan_channels.insert(sid.clone(), mode);
            }
        }
    }

    if !data.zones.is_empty() {
        cfg.zones.clear();
        cfg.scene.zone_overrides.clear();
        for z in &data.zones {
            let layout = match &z.layout {
                MigratedZoneLayout::Fans {
                    fans,
                    leds_per_fan,
                    x,
                    y,
                    angle,
                    size,
                    reverse,
                } => ZoneLayout::Fans {
                    fans: *fans,
                    leds_per_fan: *leds_per_fan,
                    x: *x,
                    y: *y,
                    angle: *angle,
                    size: *size,
                    reverse: *reverse,
                },
                MigratedZoneLayout::Strip {
                    leds,
                    x,
                    y,
                    angle,
                    length,
                    reverse,
                } => ZoneLayout::Strip {
                    leds: *leds,
                    x: *x,
                    y: *y,
                    angle: *angle,
                    length: *length,
                    reverse: *reverse,
                },
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
                } => ZoneLayout::Matrix {
                    leds: *leds,
                    rows: *rows,
                    cols: *cols,
                    x: *x,
                    y: *y,
                    angle: *angle,
                    length: *length,
                    reverse: *reverse,
                    map: map.clone(),
                },
            };
            cfg.zones.push(ZoneConfig {
                id: z.id.clone(),
                name: z.name.clone(),
                enabled: z.enabled,
                color_order: parse_color_order(&z.color_order),
                layout,
            });
            if let Some(ov) = &z.override_effect {
                cfg.scene.zone_overrides.insert(
                    z.id.clone(),
                    ZoneOverride {
                        effect: parse_effect_id(&ov.effect),
                        params: convert_migrated_params(&ov.params),
                        studio_stack: None,
                    },
                );
            }
        }
    }

    if !data.presets.is_empty() {
        cfg.presets = data
            .presets
            .iter()
            .map(|p| {
                let fan_curve = p
                    .fan_curve
                    .as_deref()
                    .filter(|c| cfg.fan_curves.contains_key(*c))
                    .unwrap_or("balanced")
                    .to_string();
                Preset {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    description: p.description.clone().unwrap_or_default(),
                    effect: parse_effect_id(&p.effect),
                    params: convert_migrated_params(&p.params),
                    brightness: p.brightness.min(100),
                    fan_curve,
                    studio_stack: None,
                    zone_overrides: BTreeMap::new(),
                }
            })
            .collect();
    }

    if !data.automations.is_empty() {
        cfg.automations = data
            .automations
            .iter()
            .map(|a| {
                let trigger = match &a.trigger {
                    MigratedTrigger::TimeWindow {
                        start_hour,
                        start_min,
                        end_hour,
                        end_min,
                        days,
                    } => {
                        let mut mask = 0u8;
                        for &d in days {
                            if d < 7 {
                                mask |= 1u8 << d;
                            }
                        }
                        if mask == 0 {
                            mask = ALL_DAYS_MASK;
                        }
                        Trigger::TimeWindow {
                            start_hour: *start_hour,
                            start_min: *start_min,
                            end_hour: *end_hour,
                            end_min: *end_min,
                            days_mask: mask,
                        }
                    }
                    MigratedTrigger::AppRunning { exe } | MigratedTrigger::AppFocused { exe } => {
                        Trigger::AppRunning { exe: exe.clone() }
                    }
                    MigratedTrigger::Idle { minutes } => Trigger::Idle { minutes: *minutes },
                    MigratedTrigger::Locked => Trigger::Locked,
                    MigratedTrigger::TempAbove { source, temp_c } => Trigger::TempAbove {
                        source: parse_temp_source(source),
                        temp_c: *temp_c,
                    },
                };
                let action = match &a.action {
                    MigratedAction::ApplyPreset { preset } => Action::ApplyPreset {
                        preset: preset.clone(),
                    },
                    MigratedAction::LightsOff => Action::LightsOff,
                    MigratedAction::Dim { brightness } => Action::Dim {
                        brightness: *brightness,
                    },
                    MigratedAction::FanCurve { curve } => Action::FanCurve {
                        curve: curve.clone(),
                    },
                };
                AutomationRule {
                    id: a.id.clone(),
                    enabled: a.enabled,
                    trigger,
                    action,
                }
            })
            .collect();
    }

    if let Some(sc) = &data.active_scene {
        cfg.scene.on = sc.on;
        cfg.scene.brightness = sc.brightness.min(100);
        cfg.scene.effect = parse_effect_id(&sc.effect);
        cfg.scene.params = convert_migrated_params(&sc.params);
    }

    if cfg.fan_curves.contains_key(&data.active_curve) {
        cfg.active_curve = data.active_curve.clone();
    }
    cfg.active_preset = data.active_preset.clone();

    for fx in &data.studio_effects {
        if !fx.is_layered || fx.layers.is_empty() {
            continue;
        }
        let mut layers = Vec::new();
        let mut next_id = 1u64;
        for ml in &fx.layers {
            let kind = match ml.kind.to_ascii_lowercase().as_str() {
                "solid" => LayerKind::Solid,
                "gradient" => LayerKind::Gradient,
                "rainbow" | "cycle" => LayerKind::Rainbow,
                "plasma" => LayerKind::Plasma,
                "wave" => LayerKind::Wave,
                "scanner" => LayerKind::Scanner,
                "comet" => LayerKind::Comet,
                "chase" => LayerKind::Chase,
                "sparkles" => LayerKind::Sparkles,
                "pulse" | "breathing" => LayerKind::Breathing,
                "beat" => LayerKind::Beat,
                "spectrum" => LayerKind::Spectrum,
                "bass" => LayerKind::Bass,
                _ => LayerKind::Gradient,
            };
            let blend = match ml.blend.to_ascii_lowercase().as_str() {
                "add" => BlendMode::Add,
                "screen" => BlendMode::Screen,
                "multiply" => BlendMode::Multiply,
                _ => BlendMode::Normal,
            };
            let mut layer = Layer::preset(next_id, kind);
            next_id += 1;
            layer.enabled = ml.enabled;
            layer.opacity = ml.opacity.min(100);
            layer.blend = blend;
            if let Some(c) = ml.color.or_else(|| ml.colors.first().copied()) {
                layer.color = c;
            }
            if let Some(s) = ml.secondary.or_else(|| ml.colors.get(1).copied()) {
                layer.secondary = s;
            }
            if let Some(sp) = ml.speed {
                layer.speed = sp;
            }
            if let Some(ang) = ml.angle {
                layer.angle = ang % 360;
            }
            if let Some(sc) = ml.scale {
                layer.scale = sc;
            }
            layer.reverse = ml.reverse;
            layers.push(layer);
        }
        if layers.is_empty() {
            continue;
        }
        let saved = SavedStudioEffect {
            id: fx.id.clone(),
            name: fx.name.clone(),
            stack: LayerStack {
                name: fx.name.clone(),
                master_speed: 100,
                layers,
                next_id,
            },
        };
        if let Some(slot) = cfg.studio_effects.iter_mut().find(|e| e.id == saved.id) {
            *slot = saved;
        } else if cfg.studio_effects.len() < 32 {
            cfg.studio_effects.push(saved);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_thirteen_designs_have_distinct_names_and_valid_color_order() {
        assert_eq!(DesignId::ALL.len(), 13);
        assert_eq!(DesignId::default(), DesignId::Firefly);
        assert_eq!(DesignId::ALL[0], DesignId::Firefly);
        assert_eq!(parse_design_id("firefly"), DesignId::Firefly);
        let mut labels: Vec<&str> = DesignId::ALL.iter().map(|d| d.label()).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), 13);
        assert_eq!(ColorOrder::Grb.reorder([10, 20, 30]), [20, 10, 30]);
        assert_eq!(TempUnit::C.format_temp(50.0), "50°C");
        assert_eq!(TempUnit::F.format_temp(50.0), "122°F");
    }

    #[test]
    fn config_roundtrip_temp_dir() {
        let dir = std::env::temp_dir().join(format!("airglow_cfg_test_{}", std::process::id()));
        let store = ConfigStore::new(&dir);
        let cfg = AppConfig::default();
        store.save(&cfg).expect("save");
        let loaded = store.load_or_default();
        assert_eq!(loaded, cfg);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn preset_export_and_python_import_roundtrip() {
        let cfg = AppConfig::default();
        let json = export_preset_json(&cfg.presets[1]).expect("export");
        let imported = import_preset_json(&json).expect("import native");
        assert_eq!(imported, cfg.presets[1]);

        let py_json = r##"{
            "airglow_profile": 1,
            "id": "neon-dusk",
            "name": "Neon Dusk",
            "description": "Synthwave glow",
            "lighting": {
                "effect": "gradient",
                "brightness": 85,
                "params": { "colors": ["#ff2fb4", "#2f5bff"], "speed": 30, "angle": 45 }
            }
        }"##;
        let py_preset = import_preset_json(py_json).expect("import python");
        assert_eq!(py_preset.id, "neon-dusk");
        assert_eq!(py_preset.effect, EffectId::Gradient);
        assert_eq!(py_preset.brightness, 85);
        assert_eq!(py_preset.params.angle, 45);
    }

    #[test]
    fn legacy_config_json_deserializes_with_new_defaults() {
        // Simulate an older %APPDATA%\Airglow-rs\config.json missing appearance, saved_looks, and fan_hysteresis_c
        let mut legacy = serde_json::to_value(AppConfig::default()).expect("to_value");
        let obj = legacy.as_object_mut().expect("root object");
        obj.remove("appearance");
        obj.remove("saved_looks");
        obj.get_mut("settings")
            .and_then(|s| s.as_object_mut())
            .expect("settings")
            .remove("fan_hysteresis_c");

        let raw = serde_json::to_string(&legacy).expect("serialize legacy");
        let loaded: AppConfig = serde_json::from_str(&raw).expect("deserialize legacy config");
        assert_eq!(loaded.appearance, AppearanceConfig::default());
        assert_eq!(loaded.appearance.ui_fps_cap, 30);
        assert_eq!(loaded.appearance.pulse_window_mins, 5);
        assert_eq!(loaded.appearance.rig_style, RigStyle::Living);
        assert!(loaded.appearance.airflow_lines);
        assert!(loaded.appearance.spinning_fans);
        assert!(loaded.appearance.ambient_bloom);
        assert!(loaded.appearance.firefly_particles);
        assert!(!loaded.appearance.compact_density);
        assert_eq!(loaded.appearance.header_pins, vec!["cpu_temp", "gpu_temp"]);
        assert_eq!(loaded.saved_looks.len(), 4);
        assert!((loaded.settings.fan_hysteresis_c - 2.0).abs() < 1e-4);
    }

    #[test]
    fn appearance_normalization_and_effect_parsing() {
        let a = AppearanceConfig {
            ui_fps_cap: 120,
            pulse_window_mins: 9,
            ..AppearanceConfig::default()
        }
        .normalized();
        assert_eq!(a.ui_fps_cap, 60);
        assert_eq!(a.pulse_window_mins, 10);

        let b = AppearanceConfig {
            ui_fps_cap: 10,
            pulse_window_mins: 1,
            ..AppearanceConfig::default()
        }
        .normalized();
        assert_eq!(b.ui_fps_cap, 15);
        assert_eq!(b.pulse_window_mins, 1);

        assert_eq!(parse_effect_id("aurora"), EffectId::Aurora);
        assert_eq!(parse_effect_id("firefly_swarm"), EffectId::FireflySwarm);
        assert_eq!(parse_effect_id("meteor_shower"), EffectId::MeteorShower);
        assert_eq!(parse_effect_id("heatmap"), EffectId::Heatmap);
    }

    #[test]
    fn parse_and_format_hex_rgb_roundtrip() {
        assert_eq!(parse_hex_rgb("#f48fb1"), Some([0xf4, 0x8f, 0xb1]));
        assert_eq!(parse_hex_rgb("f48fb1"), Some([0xf4, 0x8f, 0xb1]));
        assert_eq!(parse_hex_rgb("  #F48FB1 "), Some([0xf4, 0x8f, 0xb1]));
        assert_eq!(parse_hex_rgb("#f8b"), Some([0xff, 0x88, 0xbb]));
        assert_eq!(parse_hex_rgb("zzzzzz"), None);
        assert_eq!(parse_hex_rgb("#12345"), None);
        assert_eq!(parse_hex_rgb(""), None);
        assert_eq!(format_hex_rgb([0xf4, 0x8f, 0xb1]), "#F48FB1");
        assert_eq!(format_hex_rgb([0, 15, 255]), "#000FFF");
        assert_eq!(
            parse_hex_rgb(&format_hex_rgb([0x7c, 0xe8, 0xc8])),
            Some([0x7c, 0xe8, 0xc8])
        );
    }

    #[test]
    fn overview_tile_crud_reorder_resize_and_style_cycling() {
        let mut cfg = AppConfig::default();
        let initial_len = cfg.active_workspace().tiles.len();
        assert!(initial_len >= 8);

        // Add built-in and custom sensors
        let net_id = cfg.add_overview_tile(OverviewTileKind::NetUp);
        let custom_id = cfg.add_overview_tile(OverviewTileKind::CustomSensor {
            sensor_id: "/amdcpu/0/temperature/2".to_string(),
            label: "Tctl / Tdie".to_string(),
            unit: "°C".to_string(),
        });
        assert_eq!(cfg.active_workspace().tiles.len(), initial_len + 2);
        let custom_tile = cfg
            .active_workspace()
            .tiles
            .iter()
            .find(|t| t.id == custom_id)
            .expect("custom tile exists");
        assert_eq!(custom_tile.kind.label(), "Tctl / Tdie");
        assert_eq!(custom_tile.kind.category(), "Live Sensors");
        assert!(custom_tile.kind.is_sensor());
        assert!(!OverviewTileKind::Lighting.is_sensor());
        assert_eq!(OverviewTileKind::DeskDisplay.category(), "Controls");

        // Cycle span: 1 -> 2 -> 3 -> 4 -> 1
        assert_eq!(cfg.cycle_overview_tile_span(&net_id), Some(2));
        assert_eq!(cfg.cycle_overview_tile_span(&net_id), Some(3));
        assert_eq!(cfg.cycle_overview_tile_span(&net_id), Some(4));
        assert_eq!(cfg.cycle_overview_tile_span(&net_id), Some(1));

        // Toggle tall
        assert_eq!(cfg.toggle_overview_tile_tall(&net_id), Some(true));
        assert_eq!(cfg.toggle_overview_tile_tall(&net_id), Some(false));

        // Cycle style: Auto -> Number -> Gauge -> Graph -> Bar -> Auto
        assert_eq!(cfg.cycle_overview_tile_style(&net_id), Some(TileStyle::Number));
        assert_eq!(cfg.cycle_overview_tile_style(&net_id), Some(TileStyle::Gauge));
        assert_eq!(cfg.cycle_overview_tile_style(&net_id), Some(TileStyle::Graph));
        assert_eq!(cfg.cycle_overview_tile_style(&net_id), Some(TileStyle::Bar));
        assert_eq!(cfg.cycle_overview_tile_style(&net_id), Some(TileStyle::Auto));
        assert_eq!(TileStyle::Auto.label(), "Instrument");

        // Reorder tile backward and forward
        let pos_before = cfg
            .active_workspace()
            .tiles
            .iter()
            .position(|t| t.id == custom_id)
            .unwrap();
        assert!(cfg.move_overview_tile(&custom_id, -2));
        let pos_after = cfg
            .active_workspace()
            .tiles
            .iter()
            .position(|t| t.id == custom_id)
            .unwrap();
        assert_eq!(pos_after, pos_before - 2);
        assert!(cfg.move_overview_tile(&custom_id, 1));
        let pos_fwd = cfg
            .active_workspace()
            .tiles
            .iter()
            .position(|t| t.id == custom_id)
            .unwrap();
        assert_eq!(pos_fwd, pos_before - 1);

        // Remove tile
        assert!(cfg.remove_overview_tile(&custom_id));
        assert!(!cfg.remove_overview_tile(&custom_id));
        assert_eq!(cfg.active_workspace().tiles.len(), initial_len + 1);
    }

    #[test]
    fn default_bento_workspace_is_4x4_sixteen_cells_and_upgrades_v21_default() {
        let ws = OverviewWorkspace::default_workspace();
        assert_eq!(ws.tiles.len(), 11);
        let total_cells: usize = ws
            .tiles
            .iter()
            .map(|t| (t.span as usize) * (t.rows as usize))
            .sum();
        assert_eq!(total_cells, 16);

        // Verify key default dimensions and styles
        assert_eq!(ws.tiles[0].kind, OverviewTileKind::Lighting);
        assert_eq!((ws.tiles[0].span, ws.tiles[0].rows, ws.tiles[0].tall), (2, 2, true));
        assert_eq!(ws.tiles[1].kind, OverviewTileKind::CpuTemp);
        assert_eq!(ws.tiles[1].style, TileStyle::Number);
        assert_eq!(ws.tiles[3].kind, OverviewTileKind::CpuLoad);
        assert_eq!(ws.tiles[3].style, TileStyle::Gauge);
        assert_eq!(ws.tiles[4].kind, OverviewTileKind::GpuLoad);
        assert_eq!(ws.tiles[4].style, TileStyle::Gauge);
        assert_eq!(ws.tiles[5].kind, OverviewTileKind::Profiles);
        assert_eq!((ws.tiles[5].span, ws.tiles[5].rows, ws.tiles[5].tall), (2, 1, false));
        assert_eq!(ws.tiles[6].kind, OverviewTileKind::PowerDraw);
        assert_eq!((ws.tiles[6].span, ws.tiles[6].rows), (2, 1));
        assert_eq!(ws.tiles[7].kind, OverviewTileKind::Fans);
        assert_eq!((ws.tiles[7].span, ws.tiles[7].rows), (1, 1));

        assert_eq!(OverviewTileKind::Lighting.default_rows(), 2);
        assert_eq!(OverviewTileKind::Lighting.default_span(), 2);
        assert_eq!(OverviewTileKind::Profiles.default_rows(), 1);
        assert_eq!(OverviewTileKind::Profiles.default_span(), 2);
        assert_eq!(OverviewTileKind::PowerDraw.default_span(), 2);
        assert_eq!(OverviewTileKind::CostEfficiency.default_span(), 2);
        assert_eq!(OverviewTileKind::Fans.default_span(), 1);
        assert_eq!(OverviewTileKind::GpuLoad.default_span(), 1);

        // Legacy tile JSON without `rows` initializes rows from `tall`
        let raw_tall = r#"{"id":"lighting-1","kind":{"type":"lighting"},"span":2,"tall":true}"#;
        let parsed_tall: OverviewTile = serde_json::from_str(raw_tall).expect("parse tall tile");
        assert_eq!(parsed_tall.rows, 2);
        assert!(parsed_tall.tall);

        let raw_short = r#"{"id":"fans-1","kind":{"type":"fans"},"span":1,"tall":false}"#;
        let parsed_short: OverviewTile = serde_json::from_str(raw_short).expect("parse short tile");
        assert_eq!(parsed_short.rows, 1);
        assert!(!parsed_short.tall);

        // Unedited v2.1 "my-workspace" (Profiles tall == true, missing GpuLoad) upgrades cleanly
        let mut legacy_cfg = AppConfig::default();
        legacy_cfg.overview_workspaces = vec![OverviewWorkspace {
            id: "my-workspace".to_string(),
            name: "My workspace".to_string(),
            tiles: vec![
                OverviewTile::with_layout(
                    "lighting-1",
                    OverviewTileKind::Lighting,
                    2,
                    true,
                    TileStyle::Auto,
                ),
                OverviewTile::with_layout(
                    "profiles-1",
                    OverviewTileKind::Profiles,
                    2,
                    true,
                    TileStyle::Auto,
                ),
                OverviewTile::with_layout(
                    "cpu-temp-1",
                    OverviewTileKind::CpuTemp,
                    1,
                    false,
                    TileStyle::Auto,
                ),
            ],
        }];
        legacy_cfg.active_workspace_id = "my-workspace".to_string();
        legacy_cfg.normalize();
        assert_eq!(legacy_cfg.active_workspace().tiles.len(), 11);
        let upgraded_cells: usize = legacy_cfg
            .active_workspace()
            .tiles
            .iter()
            .map(|t| (t.span as usize) * (t.rows as usize))
            .sum();
        assert_eq!(upgraded_cells, 16);
    }

    #[test]
    fn reorder_overview_tile_to_and_2d_resize_clamp_and_sync_tall() {
        let mut cfg = AppConfig::default();
        let first_id = cfg.active_workspace().tiles[0].id.clone();
        let last_idx = cfg.active_workspace().tiles.len() - 1;

        // Same index and unknown ID return false
        assert!(!cfg.reorder_overview_tile_to(&first_id, 0));
        assert!(!cfg.reorder_overview_tile_to("nonexistent", 2));

        // Splice first tile to index 3
        assert!(cfg.reorder_overview_tile_to(&first_id, 3));
        assert_eq!(cfg.active_workspace().tiles[3].id, first_id);

        // Out-of-bounds target index clamps cleanly to last index
        assert!(cfg.reorder_overview_tile_to(&first_id, 999));
        assert_eq!(cfg.active_workspace().tiles[last_idx].id, first_id);

        // Splice back to index 0
        assert!(cfg.reorder_overview_tile_to(&first_id, 0));
        assert_eq!(cfg.active_workspace().tiles[0].id, first_id);

        // 2D resize clamps span to 1..=4, rows to 1..=3, and syncs tall = rows >= 2
        assert!(!cfg.resize_overview_tile("nonexistent", 2, 2));
        assert!(cfg.resize_overview_tile("cpu-temp-1", 10, 9));
        let resized = cfg
            .active_workspace()
            .tiles
            .iter()
            .find(|t| t.id == "cpu-temp-1")
            .unwrap();
        assert_eq!((resized.span, resized.rows, resized.tall), (4, 3, true));
        // Redundant identical resize returns false
        assert!(!cfg.resize_overview_tile("cpu-temp-1", 4, 3));

        // Shrinking to 0 clamps to 1x1 and clears tall
        assert!(cfg.resize_overview_tile("cpu-temp-1", 0, 0));
        let shrunk = cfg
            .active_workspace()
            .tiles
            .iter()
            .find(|t| t.id == "cpu-temp-1")
            .unwrap();
        assert_eq!((shrunk.span, shrunk.rows, shrunk.tall), (1, 1, false));
    }

    #[test]
    fn workspace_lifecycle_templates_reset_and_normalization() {
        let mut cfg = AppConfig::default();
        assert_eq!(cfg.overview_workspaces.len(), 1);
        assert_eq!(cfg.active_workspace_id, "default");

        // Cannot delete last workspace
        assert!(!cfg.delete_workspace("default"));

        // Starter templates exist
        let templates = OverviewWorkspace::starter_templates();
        let names: Vec<&str> = templates.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["Gaming", "Quiet Studio", "Thermal Lab"]);

        // Create workspace from template
        let gaming_id = cfg.create_workspace("Night Gaming", Some("gaming"));
        assert_eq!(cfg.active_workspace_id, gaming_id);
        assert_eq!(cfg.active_workspace().name, "Night Gaming");
        assert!(
            cfg.active_workspace()
                .tiles
                .iter()
                .any(|t| matches!(t.kind, OverviewTileKind::GpuHotspot))
        );

        // Rename workspace
        assert!(cfg.rename_workspace(&gaming_id, "FPS Rig"));
        assert_eq!(cfg.active_workspace().name, "FPS Rig");
        assert!(!cfg.rename_workspace(&gaming_id, "   "));

        // Modify & reset active workspace
        cfg.active_workspace_mut().tiles.clear();
        assert!(cfg.active_workspace().tiles.is_empty());
        cfg.reset_active_workspace();
        assert!(!cfg.active_workspace().tiles.is_empty());

        // Switch & delete
        assert!(cfg.switch_workspace("default"));
        assert_eq!(cfg.active_workspace_id, "default");
        assert!(cfg.delete_workspace(&gaming_id));
        assert_eq!(cfg.overview_workspaces.len(), 1);

        // Normalization repairs empty workspaces & invalid active_workspace_id
        cfg.overview_workspaces.clear();
        cfg.active_workspace_id = "nonexistent".to_string();
        cfg.normalize();
        assert_eq!(cfg.overview_workspaces.len(), 1);
        assert_eq!(cfg.active_workspace_id, "default");
        assert_eq!(cfg.update_channel, UpdateChannel::Stable);
        assert!(cfg.auto_check_updates);
        assert!(!cfg.fullscreen_fill_width);
    }

    #[test]
    fn ui_scale_presets_normalization_and_responsive_scale_factor() {
        assert_eq!(
            UI_SCALE_PRESETS,
            &[
                (0, "Auto"),
                (100, "100%"),
                (115, "115%"),
                (125, "125%"),
                (140, "140%"),
            ]
        );
        assert_eq!(AppearanceConfig::UI_SCALE_PRESETS, UI_SCALE_PRESETS);

        let mut auto_cfg = AppearanceConfig::default();
        assert_eq!(auto_cfg.ui_scale_pct, 0);
        auto_cfg.normalize();
        assert_eq!(auto_cfg.ui_scale_pct, 0);

        // Auto responsive HiDPI scaling across standard & widescreen physical widths
        assert!((auto_cfg.resolve_scale_factor(1440.0) - 1.0).abs() < 1e-6); // 1440x900
        assert!((auto_cfg.resolve_scale_factor(1819.0) - 1.0).abs() < 1e-6);
        assert!((auto_cfg.resolve_scale_factor(1820.0) - 1.10).abs() < 1e-6);
        assert!((auto_cfg.resolve_scale_factor(1920.0) - 1.10).abs() < 1e-6); // 1920x1080
        assert!((auto_cfg.resolve_scale_factor(2199.0) - 1.10).abs() < 1e-6);
        assert!((auto_cfg.resolve_scale_factor(2200.0) - 1.25).abs() < 1e-6);
        assert!((auto_cfg.resolve_scale_factor(2560.0) - 1.25).abs() < 1e-6); // 2560x1440 -> 2048x1152 logical
        assert!((auto_cfg.resolve_scale_factor(3249.0) - 1.25).abs() < 1e-6);
        assert!((auto_cfg.resolve_scale_factor(3250.0) - 1.50).abs() < 1e-6);
        assert!((auto_cfg.resolve_scale_factor(3840.0) - 1.50).abs() < 1e-6); // 3840x2160 (4K)

        // Manual overrides ignore physical width and clamp to 0.85..=1.50
        for &(pct, expected) in &[
            (100u16, 1.00f32),
            (115, 1.15),
            (125, 1.25),
            (140, 1.40),
            (50, 0.85),
            (200, 1.50),
        ] {
            let manual = AppearanceConfig {
                ui_scale_pct: pct,
                ..AppearanceConfig::default()
            };
            assert!((manual.resolve_scale_factor(1440.0) - expected).abs() < 1e-6);
            assert!((manual.resolve_scale_factor(3840.0) - expected).abs() < 1e-6);
        }

        // Normalization preserves 0 (Auto) and clamps non-zero to 85..=150
        let mut low = AppearanceConfig {
            ui_scale_pct: 40,
            ..AppearanceConfig::default()
        };
        low.normalize();
        assert_eq!(low.ui_scale_pct, 85);

        let mut high = AppearanceConfig {
            ui_scale_pct: 250,
            ..AppearanceConfig::default()
        };
        high.normalize();
        assert_eq!(high.ui_scale_pct, 150);

        let mut app_cfg = AppConfig::default();
        app_cfg.appearance.ui_scale_pct = 180;
        app_cfg.normalize();
        assert_eq!(app_cfg.appearance.ui_scale_pct, 150);
    }
}
