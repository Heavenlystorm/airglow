//! Built-in lighting effects and pure-Rust Studio Layer Mode compositor.

use serde::{Deserialize, Serialize};
use std::f32::consts::PI;

use crate::config::TempSource;
use crate::fans;
use crate::geometry::ZoneGeometry;
use crate::inputs::{AudioFrame, InputContext};
use crate::sensors::SensorSnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum EffectId {
    Solid,
    Breathing,
    Cycle,
    Rainbow,
    #[default]
    Gradient,
    Comet,
    Aurora,
    FireflySwarm,
    MeteorShower,
    Heatmap,
    Temperature,
    Screen,
    Spectrum,
    Pulse,
    Studio,
    Off,
}

impl EffectId {
    pub const ALL: [EffectId; 16] = [
        Self::Solid,
        Self::Breathing,
        Self::Cycle,
        Self::Rainbow,
        Self::Gradient,
        Self::Comet,
        Self::Aurora,
        Self::FireflySwarm,
        Self::MeteorShower,
        Self::Heatmap,
        Self::Temperature,
        Self::Screen,
        Self::Spectrum,
        Self::Pulse,
        Self::Studio,
        Self::Off,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Solid => "Solid",
            Self::Breathing => "Breathing",
            Self::Cycle => "Colour cycle",
            Self::Rainbow => "Rainbow wave",
            Self::Gradient => "Gradient",
            Self::Comet => "Comet",
            Self::Aurora => "Aurora",
            Self::FireflySwarm => "Firefly swarm",
            Self::MeteorShower => "Meteor shower",
            Self::Heatmap => "Heat map",
            Self::Temperature => "Temperature",
            Self::Screen => "Screen ambience",
            Self::Spectrum => "Music spectrum",
            Self::Pulse => "Music pulse",
            Self::Studio => "Studio layers",
            Self::Off => "Off",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Solid => "One steady colour everywhere.",
            Self::Breathing => "Fades slowly in and out.",
            Self::Cycle => "The whole rig drifts through every colour together.",
            Self::Rainbow => "Bands of colour sweep across every device.",
            Self::Gradient => "Your colours blend across the rig and drift.",
            Self::Comet => "A bright head with a fading tail races around each fan.",
            Self::Aurora => "Silky multi-ribbon northern lights drifting with twinkling stars.",
            Self::FireflySwarm => "Warm lantern fireflies drifting and pulsing softly across the rig.",
            Self::MeteorShower => "Diagonal shooting stars with glowing ember tails across the rig.",
            Self::Heatmap => "Spatial thermal gradient shifting with live CPU and GPU temperatures.",
            Self::Temperature => "Shifts from cool blue to hot red with PC temperature.",
            Self::Screen => "Each LED takes the colour of the part of your screen it sits next to.",
            Self::Spectrum => "Bass on the left to treble on the right, glowing with your music.",
            Self::Pulse => "The whole rig flashes with the beat in your chosen colour.",
            Self::Studio => "Multi-layer composited effect built in Studio.",
            Self::Off => "All LEDs dark.",
        }
    }

    pub fn is_animated(self) -> bool {
        !matches!(self, Self::Solid | Self::Off)
    }

    pub fn needs_screen(self) -> bool {
        matches!(self, Self::Screen)
    }

    pub fn needs_audio(self) -> bool {
        matches!(self, Self::Spectrum | Self::Pulse)
    }
}

/// Curated 3-stop RGB color harmony palette usable in Lighting and Studio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorHarmony {
    pub name: &'static str,
    pub colors: [[u8; 3]; 3],
}

pub const HARMONIES: &[ColorHarmony] = &[
    ColorHarmony {
        name: "Firefly Dusk",
        colors: [[244, 143, 177], [255, 195, 139], [165, 222, 206]],
    },
    ColorHarmony {
        name: "Cherry Blossom",
        colors: [[255, 92, 138], [255, 179, 209], [206, 147, 216]],
    },
    ColorHarmony {
        name: "Northern Lights",
        colors: [[66, 245, 185], [107, 203, 247], [164, 110, 255]],
    },
    ColorHarmony {
        name: "Rose Quartz",
        colors: [[244, 143, 177], [232, 165, 152], [197, 155, 255]],
    },
    ColorHarmony {
        name: "Sunset Coral",
        colors: [[255, 107, 107], [255, 166, 107], [255, 217, 125]],
    },
    ColorHarmony {
        name: "Cyber Vapor",
        colors: [[255, 47, 180], [138, 61, 255], [0, 229, 255]],
    },
    ColorHarmony {
        name: "Mint Aurora",
        colors: [[124, 232, 200], [72, 202, 228], [199, 249, 204]],
    },
    ColorHarmony {
        name: "Amber Workshop",
        colors: [[255, 179, 71], [236, 122, 46], [255, 224, 158]],
    },
    ColorHarmony {
        name: "Deep Ocean",
        colors: [[0, 150, 255], [0, 212, 190], [67, 97, 238]],
    },
    ColorHarmony {
        name: "Tokyo Neon",
        colors: [[255, 42, 133], [0, 245, 212], [254, 228, 64]],
    },
];

fn default_sensitivity() -> u8 {
    60
}

fn default_floor() -> u8 {
    10
}

fn default_hot_color() -> [u8; 3] {
    [255, 34, 0]
}

fn default_display() -> u8 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneParams {
    pub color: [u8; 3],
    pub secondary: [u8; 3],
    pub colors: Vec<[u8; 3]>,
    pub speed: u8,
    pub angle: u16,
    pub size: u8,
    pub saturation: u8,
    pub tail: u8,
    pub reverse: bool,
    pub cool_temp: u8,
    pub hot_temp: u8,
    #[serde(default)]
    pub mirror: bool,
    #[serde(default = "default_display")]
    pub display: u8,
    #[serde(default = "default_sensitivity")]
    pub sensitivity: u8,
    #[serde(default)]
    pub hue_shift: u16,
    #[serde(default = "default_floor")]
    pub floor: u8,
    #[serde(default)]
    pub temp_source: TempSource,
    #[serde(default = "default_hot_color")]
    pub hot_color: [u8; 3],
}

impl Default for SceneParams {
    fn default() -> Self {
        Self {
            color: [0, 155, 222],
            secondary: [6, 18, 28],
            colors: vec![[255, 47, 180], [47, 91, 255]],
            speed: 40,
            angle: 0,
            size: 50,
            saturation: 100,
            tail: 45,
            reverse: false,
            cool_temp: 40,
            hot_temp: 80,
            mirror: false,
            display: 1,
            sensitivity: 60,
            hue_shift: 0,
            floor: 10,
            temp_source: TempSource::Max,
            hot_color: [255, 34, 0],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BlendMode {
    #[default]
    Normal,
    Add,
    Screen,
    Multiply,
}

impl BlendMode {
    pub const ALL: [BlendMode; 4] = [Self::Normal, Self::Add, Self::Screen, Self::Multiply];

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Add => "Add",
            Self::Screen => "Screen",
            Self::Multiply => "Multiply",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LayerKind {
    Solid,
    #[default]
    Gradient,
    Rainbow,
    Plasma,
    Wave,
    Scanner,
    Comet,
    Chase,
    Sparkles,
    Breathing,
    Beat,
    Spectrum,
    Bass,
}

impl LayerKind {
    pub const ALL: [LayerKind; 13] = [
        Self::Solid,
        Self::Gradient,
        Self::Rainbow,
        Self::Plasma,
        Self::Wave,
        Self::Scanner,
        Self::Comet,
        Self::Chase,
        Self::Sparkles,
        Self::Breathing,
        Self::Beat,
        Self::Spectrum,
        Self::Bass,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Solid => "Solid",
            Self::Gradient => "Gradient",
            Self::Rainbow => "Rainbow",
            Self::Plasma => "Plasma",
            Self::Wave => "Wave",
            Self::Scanner => "Scanner",
            Self::Comet => "Comet",
            Self::Chase => "Chase",
            Self::Sparkles => "Sparkles",
            Self::Breathing => "Breathing",
            Self::Beat => "Beat pulse",
            Self::Spectrum => "Spectrum",
            Self::Bass => "Bass glow",
        }
    }

    pub fn is_music(self) -> bool {
        matches!(self, Self::Beat | Self::Spectrum | Self::Bass)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: u64,
    pub kind: LayerKind,
    pub enabled: bool,
    pub blend: BlendMode,
    pub opacity: u8,
    pub color: [u8; 3],
    pub secondary: [u8; 3],
    pub speed: u8,
    pub angle: u16,
    pub scale: u8,
    pub reverse: bool,
    /// Per-layer music reactivity (0..=100).
    #[serde(default)]
    pub react: u8,
}

impl Layer {
    pub fn preset(id: u64, kind: LayerKind) -> Self {
        let (blend, opacity, color, secondary, speed, scale, react) = match kind {
            LayerKind::Solid => (BlendMode::Normal, 100, [0, 155, 222], [12, 18, 28], 30, 50, 0),
            LayerKind::Gradient => (BlendMode::Normal, 100, [255, 47, 180], [47, 91, 255], 25, 55, 0),
            LayerKind::Rainbow => (BlendMode::Normal, 100, [255, 255, 255], [0, 0, 0], 40, 50, 0),
            LayerKind::Plasma => (BlendMode::Normal, 100, [0, 224, 200], [138, 61, 255], 35, 50, 0),
            LayerKind::Wave => (BlendMode::Screen, 70, [255, 255, 255], [0, 0, 0], 45, 50, 0),
            LayerKind::Scanner => (BlendMode::Add, 90, [255, 32, 80], [0, 0, 0], 40, 28, 0),
            LayerKind::Comet => (BlendMode::Add, 95, [255, 235, 190], [6, 18, 28], 45, 45, 0),
            LayerKind::Chase => (BlendMode::Screen, 85, [0, 224, 200], [0, 0, 0], 40, 50, 0),
            LayerKind::Sparkles => (BlendMode::Add, 85, [255, 255, 255], [0, 0, 0], 55, 35, 0),
            LayerKind::Breathing => (BlendMode::Multiply, 100, [255, 90, 31], [0, 0, 0], 30, 50, 0),
            LayerKind::Beat => (BlendMode::Add, 75, [138, 61, 255], [0, 0, 0], 60, 50, 0),
            LayerKind::Spectrum => (BlendMode::Screen, 85, [0, 224, 200], [0, 0, 0], 60, 60, 0),
            LayerKind::Bass => (BlendMode::Add, 70, [255, 47, 180], [0, 0, 0], 60, 60, 0),
        };
        Self {
            id,
            kind,
            enabled: true,
            blend,
            opacity,
            color,
            secondary,
            speed,
            angle: 0,
            scale,
            reverse: false,
            react,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerStack {
    pub name: String,
    pub master_speed: u8,
    pub layers: Vec<Layer>,
    pub next_id: u64,
}

impl Default for LayerStack {
    fn default() -> Self {
        Self {
            name: "Aurora Drift".to_string(),
            master_speed: 100,
            layers: vec![
                Layer::preset(1, LayerKind::Plasma),
                Layer::preset(2, LayerKind::Comet),
            ],
            next_id: 3,
        }
    }
}

impl LayerStack {
    pub fn uses_audio(&self) -> bool {
        self.layers
            .iter()
            .any(|l| l.enabled && (l.kind.is_music() || l.react > 0))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedStudioEffect {
    pub id: String,
    pub name: String,
    pub stack: LayerStack,
}

pub fn default_studio_effects() -> Vec<SavedStudioEffect> {
    vec![
        SavedStudioEffect {
            id: "aurora-drift".to_string(),
            name: "Aurora Drift".to_string(),
            stack: LayerStack {
                name: "Aurora Drift".to_string(),
                master_speed: 100,
                layers: vec![
                    Layer {
                        id: 1,
                        kind: LayerKind::Gradient,
                        enabled: true,
                        blend: BlendMode::Normal,
                        opacity: 100,
                        color: [0, 255, 163],
                        secondary: [47, 91, 255],
                        speed: 18,
                        angle: 20,
                        scale: 45,
                        reverse: false,
                        react: 0,
                    },
                    Layer {
                        id: 2,
                        kind: LayerKind::Plasma,
                        enabled: true,
                        blend: BlendMode::Screen,
                        opacity: 50,
                        color: [0, 224, 200],
                        secondary: [138, 61, 255],
                        speed: 22,
                        angle: 0,
                        scale: 60,
                        reverse: false,
                        react: 0,
                    },
                    Layer {
                        id: 3,
                        kind: LayerKind::Sparkles,
                        enabled: true,
                        blend: BlendMode::Add,
                        opacity: 70,
                        color: [232, 255, 246],
                        secondary: [0, 0, 0],
                        speed: 30,
                        angle: 0,
                        scale: 20,
                        reverse: false,
                        react: 0,
                    },
                ],
                next_id: 4,
            },
        },
        SavedStudioEffect {
            id: "ember-coals".to_string(),
            name: "Ember".to_string(),
            stack: LayerStack {
                name: "Ember".to_string(),
                master_speed: 100,
                layers: vec![
                    Layer {
                        id: 1,
                        kind: LayerKind::Plasma,
                        enabled: true,
                        blend: BlendMode::Normal,
                        opacity: 100,
                        color: [26, 4, 0],
                        secondary: [255, 58, 0],
                        speed: 35,
                        angle: 0,
                        scale: 55,
                        reverse: false,
                        react: 0,
                    },
                    Layer {
                        id: 2,
                        kind: LayerKind::Sparkles,
                        enabled: true,
                        blend: BlendMode::Add,
                        opacity: 80,
                        color: [255, 207, 122],
                        secondary: [0, 0, 0],
                        speed: 60,
                        angle: 0,
                        scale: 25,
                        reverse: false,
                        react: 0,
                    },
                ],
                next_id: 3,
            },
        },
        SavedStudioEffect {
            id: "party-beat".to_string(),
            name: "Party Beat".to_string(),
            stack: LayerStack {
                name: "Party Beat".to_string(),
                master_speed: 100,
                layers: vec![
                    Layer {
                        id: 1,
                        kind: LayerKind::Spectrum,
                        enabled: true,
                        blend: BlendMode::Normal,
                        opacity: 100,
                        color: [138, 61, 255],
                        secondary: [0, 0, 0],
                        speed: 65,
                        angle: 0,
                        scale: 65,
                        reverse: false,
                        react: 0,
                    },
                    Layer {
                        id: 2,
                        kind: LayerKind::Beat,
                        enabled: true,
                        blend: BlendMode::Add,
                        opacity: 60,
                        color: [255, 255, 255],
                        secondary: [0, 0, 0],
                        speed: 60,
                        angle: 0,
                        scale: 45,
                        reverse: false,
                        react: 0,
                    },
                ],
                next_id: 3,
            },
        },
    ]
}

/// Deterministic "Surprise me" Studio layer generator using a splitmix64 PRNG seeded by `seed`.
pub fn surprise_me_stack(seed: u64) -> LayerStack {
    struct Rng(u64);
    impl Rng {
        fn next_u64(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }
        fn range(&mut self, lo: u64, hi: u64) -> u64 {
            if hi <= lo {
                return lo;
            }
            lo + (self.next_u64() % (hi - lo + 1))
        }
    }

    const PALETTES: &[([u8; 3], [u8; 3], [u8; 3], &str)] = &[
        ([0, 255, 163], [47, 91, 255], [138, 61, 255], "Northern Pulse"),
        ([255, 47, 180], [255, 178, 26], [255, 90, 31], "Solar Flare"),
        ([0, 224, 200], [0, 180, 255], [216, 244, 255], "Arctic Current"),
        ([255, 16, 64], [138, 61, 255], [47, 91, 255], "Neon Overdrive"),
        ([200, 255, 42], [34, 224, 107], [0, 224, 200], "Phosphor Bloom"),
        ([255, 207, 122], [255, 58, 0], [26, 4, 0], "Kiln Ember"),
        ([232, 255, 246], [138, 61, 255], [47, 91, 255], "Starlight Drift"),
        ([255, 178, 26], [255, 90, 31], [255, 47, 180], "Sunset Horizon"),
    ];

    let mut rng = Rng(seed ^ 0xA1B2_C3D4_E5F6_0718);
    let (c0, c1, c2, name) = PALETTES[rng.range(0, (PALETTES.len() - 1) as u64) as usize];

    let base_kinds = [LayerKind::Gradient, LayerKind::Plasma, LayerKind::Rainbow];
    let base_kind = base_kinds[rng.range(0, (base_kinds.len() - 1) as u64) as usize];
    let mut base = Layer::preset(1, base_kind);
    base.color = c0;
    base.secondary = c1;
    base.speed = rng.range(14, 48) as u8;
    base.angle = rng.range(0, 359) as u16;
    base.scale = rng.range(35, 78) as u8;

    let motion_kinds = [
        LayerKind::Wave,
        LayerKind::Scanner,
        LayerKind::Sparkles,
        LayerKind::Comet,
        LayerKind::Chase,
    ];
    let motion_kind = motion_kinds[rng.range(0, (motion_kinds.len() - 1) as u64) as usize];
    let mut motion = Layer::preset(2, motion_kind);
    motion.blend = if rng.range(0, 1) == 0 {
        BlendMode::Add
    } else {
        BlendMode::Screen
    };
    motion.opacity = rng.range(40, 80) as u8;
    motion.color = c2;
    motion.secondary = [5, 7, 13];
    motion.speed = rng.range(20, 62) as u8;
    motion.angle = rng.range(0, 359) as u16;
    motion.scale = rng.range(20, 72) as u8;
    motion.reverse = rng.range(0, 1) == 1;

    let mut layers = vec![base, motion];
    let mut next_id = 3;

    if rng.range(0, 9) < 4 {
        let music_kind = match rng.range(0, 2) {
            0 => LayerKind::Beat,
            1 => LayerKind::Spectrum,
            _ => LayerKind::Bass,
        };
        let mut music = Layer::preset(next_id, music_kind);
        next_id += 1;
        music.color = c0;
        music.opacity = rng.range(45, 75) as u8;
        music.blend = if music_kind == LayerKind::Spectrum {
            BlendMode::Screen
        } else {
            BlendMode::Add
        };
        music.speed = rng.range(50, 80) as u8;
        music.angle = rng.range(0, 359) as u16;
        layers.push(music);
    }

    LayerStack {
        name: name.to_string(),
        master_speed: 100,
        layers,
        next_id,
    }
}

pub fn speed_hz(speed: u8, lo: f32, hi: f32) -> f32 {
    lo * (hi / lo).powf(speed as f32 / 100.0)
}

#[inline]
fn rgb_f(c: [u8; 3]) -> [f32; 3] {
    [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0]
}

#[inline]
fn to_u8(c: [f32; 3]) -> [u8; 3] {
    [
        (c[0].clamp(0.0, 1.0) * 255.0).round() as u8,
        (c[1].clamp(0.0, 1.0) * 255.0).round() as u8,
        (c[2].clamp(0.0, 1.0) * 255.0).round() as u8,
    ]
}

#[inline]
fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let f = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * f,
        a[1] + (b[1] - a[1]) * f,
        a[2] + (b[2] - a[2]) * f,
    ]
}

#[inline]
fn scale3(a: [f32; 3], k: f32) -> [f32; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

#[inline]
fn along(pos: [f32; 2], angle_deg: u16) -> f32 {
    let a = (angle_deg as f32).to_radians();
    pos[0] * a.cos() + pos[1] * a.sin()
}

#[inline]
fn along_norm(pos: [f32; 2], angle_deg: u16) -> f32 {
    let a = (angle_deg as f32).to_radians();
    let (c, s) = (a.cos(), a.sin());
    let corners = [0.0f32, c, s, c + s];
    let lo = corners.into_iter().fold(f32::INFINITY, f32::min);
    let hi = corners.into_iter().fold(f32::NEG_INFINITY, f32::max);
    let span = (hi - lo).max(1e-5);
    ((pos[0] * c + pos[1] * s - lo) / span).clamp(0.0, 1.0)
}

pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h = h.rem_euclid(1.0);
    let sector = (h * 6.0).floor() as i32;
    let f = h * 6.0 - sector as f32;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    match sector.rem_euclid(6) {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

fn blend_pixel(dst: [f32; 3], src: [f32; 3], mode: BlendMode, alpha: f32) -> [f32; 3] {
    let mixed = match mode {
        BlendMode::Normal => src,
        BlendMode::Add => [
            (dst[0] + src[0]).min(1.0),
            (dst[1] + src[1]).min(1.0),
            (dst[2] + src[2]).min(1.0),
        ],
        BlendMode::Screen => [
            1.0 - (1.0 - dst[0]) * (1.0 - src[0]),
            1.0 - (1.0 - dst[1]) * (1.0 - src[1]),
            1.0 - (1.0 - dst[2]) * (1.0 - src[2]),
        ],
        BlendMode::Multiply => [dst[0] * src[0], dst[1] * src[1], dst[2] * src[2]],
    };
    lerp3(dst, mixed, alpha)
}

#[allow(clippy::too_many_arguments)]
pub fn render_zone(
    effect: EffectId,
    params: &SceneParams,
    studio: &LayerStack,
    geom: &ZoneGeometry,
    t: f32,
    brightness: u8,
    on: bool,
    sensors: &SensorSnapshot,
) -> Vec<[u8; 3]> {
    render_zone_with_inputs(
        effect,
        params,
        studio,
        geom,
        t,
        brightness,
        on,
        sensors,
        &InputContext::empty(),
    )
}

#[allow(clippy::too_many_arguments)]
pub fn render_zone_with_inputs(
    effect: EffectId,
    params: &SceneParams,
    studio: &LayerStack,
    geom: &ZoneGeometry,
    t: f32,
    brightness: u8,
    on: bool,
    sensors: &SensorSnapshot,
    inputs: &InputContext,
) -> Vec<[u8; 3]> {
    let n = geom.len();
    if !on || brightness == 0 || effect == EffectId::Off {
        return vec![[0, 0, 0]; n];
    }

    let bright = brightness as f32 / 100.0;
    let mut out = Vec::with_capacity(n);

    match effect {
        EffectId::Off => return vec![[0, 0, 0]; n],
        EffectId::Solid => {
            let c = scale3(rgb_f(params.color), bright);
            out.resize(n, to_u8(c));
        }
        EffectId::Breathing => {
            let k = 0.5 - 0.5 * (2.0 * PI * speed_hz(params.speed, 0.05, 0.6) * t).cos();
            let factor = (0.06 + 0.94 * k * k) * bright;
            let c = scale3(rgb_f(params.color), factor);
            out.resize(n, to_u8(c));
        }
        EffectId::Cycle => {
            let h = speed_hz(params.speed, 0.005, 0.25) * t;
            let c = scale3(hsv_to_rgb(h, params.saturation as f32 / 100.0, 1.0), bright);
            out.resize(n, to_u8(c));
        }
        EffectId::Rainbow => {
            let scale = 0.4 + 2.6 * (1.0 - params.size as f32 / 100.0);
            let sat = params.saturation as f32 / 100.0;
            let shift = speed_hz(params.speed, 0.02, 0.8) * t;
            for &pos in &geom.norm_pos {
                let h = along(pos, params.angle) * scale - shift;
                out.push(to_u8(scale3(hsv_to_rgb(h, sat, 1.0), bright)));
            }
        }
        EffectId::Gradient => {
            let stops: Vec<[f32; 3]> = if params.colors.len() >= 2 {
                params.colors.iter().copied().map(rgb_f).collect()
            } else {
                vec![rgb_f(params.color), rgb_f(params.secondary)]
            };
            let drift = speed_hz(params.speed, 0.005, 0.2) * t;
            for &pos in &geom.norm_pos {
                let mut s = (along(pos, params.angle) * 0.8 + drift).rem_euclid(1.0);
                s = 1.0 - (2.0 * s - 1.0).abs();
                let x = s * (stops.len() - 1) as f32;
                let idx = (x.floor() as usize).min(stops.len() - 2);
                let c = lerp3(stops[idx], stops[idx + 1], x - idx as f32);
                out.push(to_u8(scale3(c, bright)));
            }
        }
        EffectId::Comet => {
            let head = (speed_hz(params.speed, 0.08, 2.0) * t).rem_euclid(1.0);
            let tail = 0.04 + 0.9 * (params.tail as f32 / 100.0);
            let fg = rgb_f(params.color);
            let bg = rgb_f(params.secondary);
            for &loc in &geom.local {
                let d = if params.reverse {
                    (loc - head).rem_euclid(1.0)
                } else {
                    (head - loc).rem_euclid(1.0)
                };
                let k = (1.0 - d / tail).clamp(0.0, 1.0).powi(2);
                out.push(to_u8(scale3(lerp3(bg, fg, k), bright)));
            }
        }
        EffectId::Aurora => {
            let (c0, c1, c2) = if params.colors.len() >= 3 {
                (
                    rgb_f(params.colors[0]),
                    rgb_f(params.colors[1]),
                    rgb_f(params.colors[2]),
                )
            } else if params.colors.len() == 2 {
                (
                    rgb_f(params.colors[0]),
                    [0.24, 0.95, 0.72],
                    rgb_f(params.colors[1]),
                )
            } else {
                ([0.26, 0.96, 0.72], [0.96, 0.56, 0.70], [0.56, 0.36, 1.0])
            };
            let sp = speed_hz(params.speed, 0.08, 0.95) * t;
            let sky = scale3(c2, 0.14);
            for (i, &pos) in geom.norm_pos.iter().enumerate() {
                let u = along_norm(pos, params.angle);
                let v = pos[1];
                let w1 = 0.5 + 0.5 * (u * 4.2 + sp * 1.1 + (v * 3.1 - sp * 0.6).sin() * 1.4).sin();
                let w2 = 0.5
                    + 0.5
                        * (u * 6.5 - sp * 0.85 + (pos[0] * 2.7 + v * 4.0 + sp * 0.5).cos() * 1.2)
                            .sin();
                let ridge = 0.36 + 0.24 * (u * 3.0 + sp * 0.4).sin();
                let curtain = 0.32 + 0.68 * (1.0 - (v - ridge).abs() * 1.35).clamp(0.0, 1.0);
                let mut col = sky;
                let a1 = w1.powf(1.6) * curtain;
                let a2 = w2.powf(1.8) * 0.82;
                col = [
                    (col[0] + c0[0] * a1 + c1[0] * a2).min(1.0),
                    (col[1] + c0[1] * a1 + c1[1] * a2).min(1.0),
                    (col[2] + c0[2] * a1 + c1[2] * a2).min(1.0),
                ];
                let star_hash = ((i as f32 * 12.9898 + 78.233).sin() * 43758.5453)
                    .fract()
                    .abs();
                if star_hash > 0.78 {
                    let twinkle =
                        (0.5 + 0.5 * (t * (1.2 + star_hash * 2.4) + i as f32).sin()).powi(4) * 0.65;
                    col = [
                        (col[0] + twinkle).min(1.0),
                        (col[1] + twinkle * 0.96).min(1.0),
                        (col[2] + twinkle * 0.90).min(1.0),
                    ];
                }
                out.push(to_u8(scale3(col, bright)));
            }
        }
        EffectId::FireflySwarm => {
            let base_raw = rgb_f(params.secondary);
            let base = [
                (base_raw[0] * 0.25).max(0.06),
                (base_raw[1] * 0.22).max(0.04),
                (base_raw[2] * 0.30).max(0.10),
            ];
            let palette: [[f32; 3]; 3] = if params.colors.len() >= 3 {
                [
                    rgb_f(params.colors[0]),
                    rgb_f(params.colors[1]),
                    rgb_f(params.colors[2]),
                ]
            } else {
                [
                    [1.0, 0.77, 0.48],  // warm apricot lantern
                    [0.96, 0.56, 0.69], // rose pink bloom
                    [0.65, 0.92, 0.80], // soft bioluminescent mint
                ]
            };
            let rate = speed_hz(params.speed, 0.12, 1.05);
            let rad = 0.15 + 0.24 * (params.size as f32 / 100.0);
            let mut flies = [([0.0f32; 2], 0.0f32, [0.0f32; 3]); 6];
            for (k, slot) in flies.iter_mut().enumerate() {
                let kf = k as f32;
                let fx = 0.5 + 0.44 * (t * rate * (0.62 + kf * 0.13) + kf * 1.73).sin();
                let fy = 0.5 + 0.42 * (t * rate * (0.49 + kf * 0.17) + kf * 2.41).cos();
                let pulse = 0.24
                    + 0.76 * (0.5 + 0.5 * (t * (0.85 + kf * 0.27) + kf * 1.15).sin()).powi(2);
                *slot = ([fx, fy], pulse, palette[k % 3]);
            }
            for &pos in &geom.norm_pos {
                let mut col = base;
                for &(fpos, pulse, fcol) in &flies {
                    let dx = pos[0] - fpos[0];
                    let dy = pos[1] - fpos[1];
                    let dist = (dx * dx + dy * dy).sqrt();
                    if dist < rad {
                        let glow = (1.0 - dist / rad).powf(2.1) * pulse;
                        col[0] = (col[0] + fcol[0] * glow).min(1.0);
                        col[1] = (col[1] + fcol[1] * glow).min(1.0);
                        col[2] = (col[2] + fcol[2] * glow).min(1.0);
                    }
                }
                out.push(to_u8(scale3(col, bright)));
            }
        }
        EffectId::MeteorShower => {
            let eff_angle = if params.angle == 0 { 35 } else { params.angle };
            let rad = (eff_angle as f32).to_radians();
            let (dx, dy) = (rad.cos(), rad.sin());
            let (px, py) = (-dy, dx);
            let dir_sign = if params.reverse { -1.0 } else { 1.0 };
            let tail_len = 0.20 + 0.55 * (params.tail as f32 / 100.0);
            let width = 0.10 + 0.14 * (params.size as f32 / 100.0);
            let base_raw = rgb_f(params.secondary);
            let bg = [
                (base_raw[0] * 0.20).max(0.03),
                (base_raw[1] * 0.18).max(0.03),
                (base_raw[2] * 0.26).max(0.07),
            ];
            let head_col = if params.colors.len() >= 2 {
                rgb_f(params.colors[0])
            } else {
                rgb_f(params.color)
            };
            let tail_col = if params.colors.len() >= 2 {
                rgb_f(params.colors[1])
            } else {
                rgb_f(params.hot_color)
            };
            let base_rate = speed_hz(params.speed, 0.24, 1.85);

            // Precompute 5 diagonal meteor streaks for this frame
            let mut meteors = [(0.0f32, 0.0f32); 5];
            for (m, slot) in meteors.iter_mut().enumerate() {
                let mf = m as f32;
                let lane = -0.30 + 0.22 * mf;
                let rate = base_rate * (0.82 + 0.12 * mf);
                let head = ((t * rate + mf * 0.41).rem_euclid(1.85) - 0.25) * dir_sign;
                *slot = (head, lane);
            }

            for (i, &pos) in geom.norm_pos.iter().enumerate() {
                let mut col = bg;
                let proj = (pos[0] * dx + pos[1] * dy) * dir_sign;
                let perp = pos[0] * px + pos[1] * py;
                for &(head, lane) in &meteors {
                    let perp_d = (perp - lane).abs();
                    if perp_d >= width {
                        continue;
                    }
                    let behind = head - proj;
                    if behind >= -0.05 && behind <= tail_len {
                        let along_k = if behind < 0.0 {
                            (1.0 + behind / 0.05).clamp(0.0, 1.0)
                        } else {
                            (1.0 - behind / tail_len).clamp(0.0, 1.0).powf(1.8)
                        };
                        let perp_k = (1.0 - perp_d / width).powi(2);
                        let intensity = along_k * perp_k;
                        let core = (1.0 - (behind.abs() / 0.08).min(1.0)).powi(2);
                        let ember = lerp3(tail_col, head_col, along_k);
                        let streak_col = lerp3(ember, [1.0, 0.97, 0.92], core * 0.85);
                        col[0] = (col[0] + streak_col[0] * intensity).min(1.0);
                        col[1] = (col[1] + streak_col[1] * intensity).min(1.0);
                        col[2] = (col[2] + streak_col[2] * intensity).min(1.0);
                    }
                }
                let star_hash = ((i as f32 * 19.31 + 31.7).sin() * 14753.17).fract().abs();
                if star_hash > 0.84 {
                    let glint = (0.5 + 0.5 * (t * 2.2 + i as f32 * 0.7).sin()).powi(4) * 0.35;
                    col[0] = (col[0] + glint).min(1.0);
                    col[1] = (col[1] + glint * 0.95).min(1.0);
                    col[2] = (col[2] + glint).min(1.0);
                }
                out.push(to_u8(scale3(col, bright)));
            }
        }
        EffectId::Heatmap => {
            let cpu_t = sensors.cpu_temp.unwrap_or(46.0);
            let gpu_t = sensors.gpu_temp.unwrap_or(42.0);
            let lo = params.cool_temp as f32;
            let hi = (params.hot_temp as f32).max(lo + 1.0);
            let wave_sp = speed_hz(params.speed, 0.08, 0.75) * t;
            let stops: [[f32; 3]; 4] = [
                [0.36, 0.88, 0.78], // cool mint
                [0.38, 0.70, 1.00], // cool sky blue
                [1.00, 0.75, 0.46], // warm apricot
                [1.00, 0.33, 0.31], // hot coral-crimson
            ];
            for &pos in &geom.norm_pos {
                let w_gpu = (pos[0] * 0.45
                    + pos[1] * 0.55
                    + 0.07 * (wave_sp + pos[0] * 3.2 - pos[1] * 2.4).sin())
                .clamp(0.0, 1.0);
                let local_temp = cpu_t * (1.0 - w_gpu) + gpu_t * w_gpu;
                let frac = ((local_temp - lo) / (hi - lo)).clamp(0.0, 1.0);
                let x = frac * (stops.len() - 1) as f32;
                let idx = (x.floor() as usize).min(stops.len() - 2);
                let mut col = lerp3(stops[idx], stops[idx + 1], x - idx as f32);
                if local_temp >= hi {
                    col = scale3(col, 0.78 + 0.22 * (2.0 * PI * 1.1 * t).cos());
                }
                out.push(to_u8(scale3(col, bright)));
            }
        }
        EffectId::Temperature => {
            let temp = fans::source_temp(sensors, params.temp_source);
            let lo = params.cool_temp as f32;
            let hi = (params.hot_temp as f32).max(lo + 1.0);
            let frac = temp
                .map(|v| ((v - lo) / (hi - lo)).clamp(0.0, 1.0))
                .unwrap_or(0.0);
            let mut c = lerp3(rgb_f(params.color), rgb_f(params.hot_color), frac);
            if temp.is_some_and(|v| v >= hi) {
                c = scale3(c, 0.75 + 0.25 * (2.0 * PI * 1.2 * t).cos());
            }
            out.resize(n, to_u8(scale3(c, bright)));
        }
        EffectId::Screen => {
            for &pos in &geom.norm_pos {
                let rgb = inputs.sample_screen_boosted(pos, params.mirror, params.saturation);
                out.push(to_u8(scale3(rgb, bright)));
            }
        }
        EffectId::Spectrum => {
            let gain = 0.4 + 1.6 * params.sensitivity as f32 / 100.0;
            let hue_off = params.hue_shift as f32 / 360.0;
            for &pos in &geom.norm_pos {
                let mut x = along(pos, params.angle);
                if x >= 1.0 {
                    x = 1.0 - 1e-6;
                }
                x = x.rem_euclid(1.0);
                let level = (inputs.audio_band(x) * gain).clamp(0.0, 1.0);
                let h = (hue_off + 0.66 - x * 0.66).rem_euclid(1.0);
                out.push(to_u8(scale3(hsv_to_rgb(h, 1.0, level.powf(1.4)), bright)));
            }
        }
        EffectId::Pulse => {
            let bass = inputs.audio_bass();
            let gain = 0.4 + 1.6 * params.sensitivity as f32 / 100.0;
            let k = (bass * gain).clamp(0.0, 1.0);
            let floor = params.floor as f32 / 100.0;
            let c = scale3(rgb_f(params.color), (floor + (1.0 - floor) * k) * bright);
            out.resize(n, to_u8(c));
        }
        EffectId::Studio => {
            let master_rate = studio.master_speed as f32 / 100.0;
            let st = t * master_rate;
            let aud = inputs.audio_frame();
            let mut buf = vec![[0.0f32; 3]; n];
            for layer in &studio.layers {
                if !layer.enabled || layer.opacity == 0 {
                    continue;
                }
                let base_alpha = layer.opacity as f32 / 100.0;
                let alpha = if layer.react > 0 {
                    let r_mod = layer.react as f32 / 100.0;
                    let energy = if aud.beat {
                        1.0
                    } else {
                        (aud.bass * 1.4).clamp(0.0, 1.0)
                    };
                    (base_alpha * ((1.0 - 0.65 * r_mod) + 0.65 * r_mod * energy)).clamp(0.0, 1.0)
                } else {
                    base_alpha
                };
                for i in 0..n {
                    let src = eval_layer(layer, geom.norm_pos[i], geom.local[i], i, st, &aud);
                    buf[i] = blend_pixel(buf[i], src, layer.blend, alpha);
                }
            }
            for c in buf {
                out.push(to_u8(scale3(c, bright)));
            }
        }
    }

    out
}

fn eval_layer(
    layer: &Layer,
    pos: [f32; 2],
    local: f32,
    idx: usize,
    t: f32,
    audio: &AudioFrame,
) -> [f32; 3] {
    let fg = rgb_f(layer.color);
    let bg = rgb_f(layer.secondary);
    match layer.kind {
        LayerKind::Solid => fg,
        LayerKind::Gradient => {
            let scale = 0.3 + 1.4 * (layer.scale as f32 / 100.0);
            let drift = speed_hz(layer.speed, 0.005, 0.25) * t;
            let mut s = (along(pos, layer.angle) * scale + drift).rem_euclid(1.0);
            s = 1.0 - (2.0 * s - 1.0).abs();
            lerp3(fg, bg, s)
        }
        LayerKind::Rainbow => {
            let scale = 0.4 + 2.4 * (1.0 - layer.scale as f32 / 100.0);
            let h = along(pos, layer.angle) * scale - speed_hz(layer.speed, 0.02, 0.8) * t;
            hsv_to_rgb(h, 1.0, 1.0)
        }
        LayerKind::Plasma => {
            let sp = speed_hz(layer.speed, 0.2, 1.8) * t;
            let sc = 2.0 + 6.0 * (layer.scale as f32 / 100.0);
            let v1 = (pos[0] * sc + sp).sin();
            let v2 = (pos[1] * sc - sp * 0.7).cos();
            let v3 = ((pos[0] + pos[1]) * sc * 0.7 + sp * 1.1).sin();
            let mix = ((v1 + v2 + v3) / 3.0) * 0.5 + 0.5;
            lerp3(fg, bg, mix)
        }
        LayerKind::Wave => {
            let freq = 2.0 + 10.0 * (1.0 - layer.scale as f32 / 100.0);
            let phase = along(pos, layer.angle) * freq - speed_hz(layer.speed, 0.2, 2.5) * t;
            let wave = (0.5 + 0.5 * (phase * 2.0 * PI).sin()).powi(2);
            scale3(fg, wave)
        }
        LayerKind::Scanner => {
            let p = (speed_hz(layer.speed, 0.1, 1.0) * t).rem_euclid(1.0);
            let bar = 1.0 - (2.0 * p - 1.0).abs();
            let width = (layer.scale as f32 / 100.0).clamp(0.05, 0.6);
            let dist = (along_norm(pos, layer.angle) - bar).abs();
            let k = (1.0 - dist / width).clamp(0.0, 1.0).powi(2);
            scale3(fg, k)
        }
        LayerKind::Comet => {
            let head = (speed_hz(layer.speed, 0.08, 2.0) * t).rem_euclid(1.0);
            let d = if layer.reverse {
                (local - head).rem_euclid(1.0)
            } else {
                (head - local).rem_euclid(1.0)
            };
            let tail = 0.06 + 0.8 * (layer.scale as f32 / 100.0);
            let k = (1.0 - d / tail).clamp(0.0, 1.0).powi(2);
            lerp3(bg, fg, k)
        }
        LayerKind::Chase => {
            let head = (speed_hz(layer.speed, 0.08, 1.8) * t).rem_euclid(1.0);
            let phase = if layer.reverse { local + head } else { local - head };
            let bands = (phase * 3.0).rem_euclid(1.0);
            let k = (0.5 + 0.5 * (bands * 2.0 * PI).cos()).powi(2);
            scale3(fg, k)
        }
        LayerKind::Sparkles => {
            let rate = speed_hz(layer.speed, 0.5, 5.0);
            let bucket = (t * rate + (idx as f32) * 0.37).floor();
            let frac = (t * rate + (idx as f32) * 0.37).fract();
            let hash = ((idx as f32 * 12.9898 + bucket * 78.233).sin() * 43758.5453)
                .fract()
                .abs();
            let threshold = 1.0 - (layer.scale as f32 / 120.0).clamp(0.05, 0.75);
            if hash > threshold {
                scale3(fg, (1.0 - frac).powi(2))
            } else {
                [0.0, 0.0, 0.0]
            }
        }
        LayerKind::Breathing => {
            let k = 0.5 - 0.5 * (2.0 * PI * speed_hz(layer.speed, 0.05, 0.7) * t).cos();
            scale3(fg, 0.1 + 0.9 * k * k)
        }
        LayerKind::Beat => {
            let gain = 0.4 + 1.6 * (layer.speed as f32 / 100.0);
            let hit = if audio.beat {
                1.0
            } else {
                (audio.bass * gain).clamp(0.0, 1.0)
            };
            scale3(fg, hit)
        }
        LayerKind::Spectrum => {
            let gain = 0.4 + 1.6 * (layer.speed as f32 / 100.0);
            let s = along_norm(pos, layer.angle).min(0.999_999);
            let level = (audio.band_at(s) * gain).clamp(0.0, 1.0);
            let hue = (layer.scale as f32 / 100.0 + 0.66 - s * 0.66).rem_euclid(1.0);
            hsv_to_rgb(hue, 1.0, level.powf(1.4))
        }
        LayerKind::Bass => {
            let gain = 0.4 + 1.6 * (layer.speed as f32 / 100.0);
            let r = 0.05 + (layer.scale as f32 / 100.0) * 0.9 * (audio.bass * gain).clamp(0.0, 1.0);
            let dx = (pos[0] - 0.5) * 1.6;
            let dy = pos[1] - 0.5;
            let dist = (dx * dx + dy * dy).sqrt();
            let k = (1.0 - dist / r.max(0.01)).clamp(0.0, 1.0).powf(1.5);
            scale3(fg, k)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{ZoneGeometry, sample_case_zones};
    use crate::inputs::{ScreenFrame, synthetic_test_beat};

    #[test]
    fn studio_layers_compose_without_webview() {
        let geom = ZoneGeometry::from_layout(&sample_case_zones()[0]);
        let stack = LayerStack::default();
        let sensors = SensorSnapshot::default();
        let frame = render_zone(
            EffectId::Studio,
            &SceneParams::default(),
            &stack,
            &geom,
            1.25,
            100,
            true,
            &sensors,
        );
        assert_eq!(frame.len(), geom.len());
        assert!(frame.iter().any(|rgb| rgb[0] > 0 || rgb[1] > 0 || rgb[2] > 0));
    }

    #[test]
    fn off_or_zero_brightness_renders_black() {
        let geom = ZoneGeometry::from_layout(&sample_case_zones()[0]);
        let frame = render_zone(
            EffectId::Rainbow,
            &SceneParams::default(),
            &LayerStack::default(),
            &geom,
            2.0,
            100,
            false,
            &SensorSnapshot::default(),
        );
        assert!(frame.iter().all(|&c| c == [0, 0, 0]));
    }

    #[test]
    fn screen_spectrum_pulse_and_music_layers_render() {
        let geom = ZoneGeometry::from_layout(&sample_case_zones()[3]);
        let screen = ScreenFrame::from_grid(2, 2, vec![[1.0, 0.2, 0.0], [0.0, 0.4, 1.0], [0.2, 0.8, 0.2], [0.9, 0.1, 0.9]]);
        let audio = synthetic_test_beat(0.02);
        let ctx = InputContext::new(Some(screen), Some(audio));
        let sensors = SensorSnapshot::default();

        let screen_frame = render_zone_with_inputs(
            EffectId::Screen,
            &SceneParams {
                saturation: 40,
                mirror: true,
                ..SceneParams::default()
            },
            &LayerStack::default(),
            &geom,
            0.5,
            100,
            true,
            &sensors,
            &ctx,
        );
        assert!(screen_frame.iter().any(|c| c[0] > 0 || c[2] > 0));

        let spec_frame = render_zone_with_inputs(
            EffectId::Spectrum,
            &SceneParams::default(),
            &LayerStack::default(),
            &geom,
            0.5,
            100,
            true,
            &sensors,
            &ctx,
        );
        assert!(spec_frame.iter().any(|c| c[0] > 0 || c[1] > 0 || c[2] > 0));

        let pulse_frame = render_zone_with_inputs(
            EffectId::Pulse,
            &SceneParams::default(),
            &LayerStack::default(),
            &geom,
            0.5,
            100,
            true,
            &sensors,
            &ctx,
        );
        assert!(pulse_frame.iter().any(|c| c[2] > 50));

        let surprise = surprise_me_stack(42);
        assert!(surprise.layers.len() >= 2);
        let st_frame = render_zone_with_inputs(
            EffectId::Studio,
            &SceneParams::default(),
            &surprise,
            &geom,
            1.0,
            100,
            true,
            &sensors,
            &ctx,
        );
        assert_eq!(st_frame.len(), geom.len());
    }

    #[test]
    fn four_new_effects_and_ten_harmonies_render_vividly() {
        assert_eq!(EffectId::ALL.len(), 16);
        assert_eq!(HARMONIES.len(), 10);
        for h in HARMONIES {
            assert!(!h.name.is_empty());
        }

        let geom = ZoneGeometry::from_layout(&sample_case_zones()[0]);
        let params = SceneParams {
            colors: HARMONIES[0].colors.to_vec(),
            ..SceneParams::default()
        };
        let cool_sensors = SensorSnapshot {
            cpu_temp: Some(38.0),
            gpu_temp: Some(36.0),
            ..SensorSnapshot::default()
        };
        let hot_sensors = SensorSnapshot {
            cpu_temp: Some(84.0),
            gpu_temp: Some(82.0),
            ..SensorSnapshot::default()
        };

        for eff in [
            EffectId::Aurora,
            EffectId::FireflySwarm,
            EffectId::MeteorShower,
            EffectId::Heatmap,
        ] {
            assert!(eff.is_animated());
            assert!(!eff.needs_screen());
            assert!(!eff.needs_audio());
            let f1 = render_zone(eff, &params, &LayerStack::default(), &geom, 0.5, 100, true, &cool_sensors);
            let f2 = render_zone(eff, &params, &LayerStack::default(), &geom, 2.7, 100, true, &cool_sensors);
            assert_eq!(f1.len(), geom.len());
            assert!(f1.iter().any(|c| c[0] > 10 || c[1] > 10 || c[2] > 10));
            if eff != EffectId::Heatmap {
                assert_ne!(f1, f2, "effect {:?} should animate over time", eff);
            }
        }

        // Heatmap shifts significantly redder when CPU/GPU temps rise
        let cool_hm = render_zone(
            EffectId::Heatmap,
            &params,
            &LayerStack::default(),
            &geom,
            1.0,
            100,
            true,
            &cool_sensors,
        );
        let hot_hm = render_zone(
            EffectId::Heatmap,
            &params,
            &LayerStack::default(),
            &geom,
            1.0,
            100,
            true,
            &hot_sensors,
        );
        let cool_r: u32 = cool_hm.iter().map(|c| c[0] as u32).sum();
        let hot_r: u32 = hot_hm.iter().map(|c| c[0] as u32).sum();
        assert!(hot_r > cool_r + 1000, "hot heatmap should be much warmer than cool");
    }
}
