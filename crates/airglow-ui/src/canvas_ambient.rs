//! Full-window living atmospheric backdrop (`WindowBackdropCanvas`), crisp anti-aliased
//! geometric vector UI icons (`UiIcon` + `icon_canvas`), ambient bioluminescent Firefly
//! particles, and 24-band luminous audio spectrum visualizer.

#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::collections::hash_map::DefaultHasher;
use std::f32::consts::PI;
use std::hash::{Hash, Hasher};

use airglow_core::config::{DesignId, DesignPalette};
use iced::mouse;
use iced::widget::canvas::{
    self, Fill, Frame, Geometry, Gradient, LineCap, LineJoin, Path, Stroke, gradient,
};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};

use crate::theme::{rgb, rgba};

fn is_firefly_palette(p: DesignPalette) -> bool {
    p.ground == 0x221d30 || p.radius_panel >= 15.0
}

fn mix_color(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

fn resolve_accent(p: DesignPalette, accent_override: Option<[u8; 3]>) -> (Color, Color) {
    if let Some([r, g, b]) = accent_override {
        let base = Color::from_rgb8(r, g, b);
        let hi = mix_color(base, Color::WHITE, 0.28);
        (base, hi)
    } else {
        (rgb(p.brass), rgb(p.brass_hi))
    }
}

fn fill_bokeh_halo(
    frame: &mut Frame,
    center: Point,
    max_r: f32,
    color: Color,
    peak_alpha: f32,
    steps: usize,
) {
    if max_r <= 0.4 || peak_alpha <= 0.002 {
        return;
    }
    let n = steps.max(3);
    for i in (0..n).rev() {
        let f = (i + 1) as f32 / n as f32;
        let r = max_r * f;
        let falloff = (1.26 - f * 0.88).max(0.12);
        let a = ((peak_alpha / n as f32) * falloff * 1.42).clamp(0.0, 0.68);
        frame.fill(&Path::circle(center, r), Color { a, ..color });
    }
}

/// Smooth two-tone multi-ring radial nebula wash rendered once into a cached static layer.
fn fill_atmospheric_nebula(
    frame: &mut Frame,
    center: Point,
    max_r: f32,
    outer_col: Color,
    inner_col: Color,
    peak_alpha: f32,
    steps: usize,
) {
    if max_r <= 2.0 || peak_alpha <= 0.001 {
        return;
    }
    let n = steps.max(8);
    for i in (0..n).rev() {
        let f = (i + 1) as f32 / n as f32;
        let r = max_r * f;
        // Smooth cosine bell falloff -> zero visible ring edges
        let bell = 0.5 * (1.0 + (f * PI).cos());
        let a = ((peak_alpha / n as f32) * (0.55 + 1.85 * bell)).clamp(0.0, 0.28);
        let col = mix_color(inner_col, outer_col, f.sqrt());
        frame.fill(&Path::circle(center, r), Color { a, ..col });
    }
}

// ---------------------------------------------------------------------------
// 1. Full-Window Living Atmospheric Backdrop (`WindowBackdropCanvas`)
// ---------------------------------------------------------------------------

/// Full-viewport atmospheric backdrop with cached static base layer (deep multi-stop ground +
/// dual radial light nebulae + `24px` architectural constellation dot grid) and a lightweight
/// dynamic layer of 14 drifting bioluminescent ambient motes.
pub struct WindowBackdropCanvas {
    pub design: DesignId,
    pub accent_override: Option<[u8; 3]>,
    pub phase: f32,
    pub animate_motes: bool,
}

#[derive(Default)]
pub struct WindowBackdropState {
    cache: canvas::Cache,
    key: Cell<u64>,
}

struct BackdropMote {
    base_x: f32,
    base_y: f32,
    amp_x: f32,
    amp_y: f32,
    speed_x: f32,
    speed_y: f32,
    phase_offset: f32,
    pulse_hz: f32,
    radius: f32,
    hue_slot: u8,
}

const BACKDROP_MOTES: [BackdropMote; 14] = [
    BackdropMote { base_x: 0.12, base_y: 0.22, amp_x: 0.032, amp_y: 0.065, speed_x: 0.17, speed_y: 0.23, phase_offset: 0.3, pulse_hz: 0.42, radius: 2.1, hue_slot: 0 },
    BackdropMote { base_x: 0.28, base_y: 0.78, amp_x: 0.038, amp_y: 0.072, speed_x: 0.14, speed_y: 0.19, phase_offset: 1.7, pulse_hz: 0.36, radius: 1.8, hue_slot: 2 },
    BackdropMote { base_x: 0.44, base_y: 0.18, amp_x: 0.028, amp_y: 0.058, speed_x: 0.21, speed_y: 0.26, phase_offset: 2.9, pulse_hz: 0.48, radius: 1.6, hue_slot: 1 },
    BackdropMote { base_x: 0.63, base_y: 0.84, amp_x: 0.035, amp_y: 0.064, speed_x: 0.16, speed_y: 0.22, phase_offset: 4.1, pulse_hz: 0.39, radius: 2.2, hue_slot: 0 },
    BackdropMote { base_x: 0.84, base_y: 0.24, amp_x: 0.030, amp_y: 0.075, speed_x: 0.19, speed_y: 0.25, phase_offset: 0.9, pulse_hz: 0.51, radius: 2.4, hue_slot: 1 },
    BackdropMote { base_x: 0.91, base_y: 0.66, amp_x: 0.025, amp_y: 0.068, speed_x: 0.15, speed_y: 0.21, phase_offset: 3.4, pulse_hz: 0.44, radius: 1.9, hue_slot: 0 },
    BackdropMote { base_x: 0.08, base_y: 0.64, amp_x: 0.026, amp_y: 0.056, speed_x: 0.18, speed_y: 0.24, phase_offset: 5.2, pulse_hz: 0.37, radius: 1.7, hue_slot: 2 },
    BackdropMote { base_x: 0.74, base_y: 0.44, amp_x: 0.034, amp_y: 0.062, speed_x: 0.13, speed_y: 0.20, phase_offset: 2.2, pulse_hz: 0.46, radius: 2.0, hue_slot: 1 },
    BackdropMote { base_x: 0.36, base_y: 0.48, amp_x: 0.031, amp_y: 0.060, speed_x: 0.16, speed_y: 0.18, phase_offset: 4.8, pulse_hz: 0.34, radius: 1.5, hue_slot: 2 },
    BackdropMote { base_x: 0.54, base_y: 0.62, amp_x: 0.029, amp_y: 0.066, speed_x: 0.20, speed_y: 0.27, phase_offset: 1.2, pulse_hz: 0.49, radius: 1.8, hue_slot: 0 },
    BackdropMote { base_x: 0.19, base_y: 0.40, amp_x: 0.033, amp_y: 0.054, speed_x: 0.15, speed_y: 0.22, phase_offset: 3.9, pulse_hz: 0.41, radius: 1.6, hue_slot: 1 },
    BackdropMote { base_x: 0.68, base_y: 0.14, amp_x: 0.027, amp_y: 0.052, speed_x: 0.22, speed_y: 0.28, phase_offset: 5.7, pulse_hz: 0.53, radius: 2.1, hue_slot: 0 },
    BackdropMote { base_x: 0.88, base_y: 0.88, amp_x: 0.024, amp_y: 0.050, speed_x: 0.17, speed_y: 0.19, phase_offset: 0.6, pulse_hz: 0.38, radius: 1.7, hue_slot: 2 },
    BackdropMote { base_x: 0.49, base_y: 0.90, amp_x: 0.036, amp_y: 0.048, speed_x: 0.14, speed_y: 0.21, phase_offset: 2.6, pulse_hz: 0.43, radius: 1.9, hue_slot: 1 },
];

pub(crate) fn backdrop_cache_key(
    width: f32,
    height: f32,
    design: DesignId,
    accent_override: Option<[u8; 3]>,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    (width.round() as u32).hash(&mut hasher);
    (height.round() as u32).hash(&mut hasher);
    design.hash(&mut hasher);
    accent_override.hash(&mut hasher);
    hasher.finish()
}

impl<Message> canvas::Program<Message> for WindowBackdropCanvas {
    type State = WindowBackdropState;

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        if bounds.width < 8.0 || bounds.height < 8.0 {
            return Vec::new();
        }

        let w = bounds.width;
        let h = bounds.height;
        let p = self.design.palette();
        let (accent, accent_hi) = resolve_accent(p, self.accent_override);

        let key = backdrop_cache_key(w, h, self.design, self.accent_override);
        if state.key.get() != key {
            state.cache.clear();
            state.key.set(key);
        }

        let static_geo = state.cache.draw(renderer, bounds.size(), |frame| {
            // 1. Rich deep ground gradient
            let (g_top, g_mid, g_bot) = match self.design {
                DesignId::Firefly => (rgb(0x1b152a), rgb(0x151021), rgb(0x110d1b)),
                DesignId::Midnight => (rgb(0x040c24), rgb(0x02071a), rgb(0x010411)),
                DesignId::Kiln => (rgb(0x130907), rgb(0x0b0605), rgb(0x070303)),
                DesignId::Neon => (rgb(0x0c0a1a), rgb(0x07070f), rgb(0x04040a)),
                DesignId::Lumen => (rgb(0x0b091b), rgb(0x060510), rgb(0x04030a)),
                DesignId::Spore => (rgb(0x150e09), rgb(0x0f0a06), rgb(0x0a0604)),
                DesignId::Paper => (rgb(0xf1ede4), rgb(0xe8e4db), rgb(0xdfd9cd)),
                DesignId::Contrast => (Color::BLACK, Color::BLACK, Color::BLACK),
                _ => (
                    mix_color(rgb(p.ground), Color::WHITE, 0.02),
                    rgb(p.ground),
                    mix_color(rgb(p.ground), Color::BLACK, 0.28),
                ),
            };

            let ground_grad = gradient::Linear::new(Point::ORIGIN, Point::new(w * 0.30, h))
                .add_stop(0.0, g_top)
                .add_stop(0.52, g_mid)
                .add_stop(1.0, g_bot);
            frame.fill(
                &Path::rectangle(Point::ORIGIN, Size::new(w, h)),
                Fill::from(Gradient::Linear(ground_grad)),
            );

            if self.design == DesignId::Contrast {
                return;
            }

            // 2. Two multi-ring radial atmospheric light washes (+ subtle zenith bloom)
            let span = w.max(h);
            let ov_blend = if self.accent_override.is_some() { 0.42 } else { 0.0 };

            let (tr_outer, tr_inner, tr_alpha, bl_outer, bl_inner, bl_alpha) = match self.design {
                DesignId::Firefly => (
                    mix_color(rgb(0xd874a4), accent, ov_blend),
                    mix_color(rgb(0xffc78e), accent_hi, ov_blend),
                    0.135,
                    rgb(0x6b56bd),
                    rgb(0x8edac7),
                    0.105,
                ),
                DesignId::Midnight => (
                    mix_color(rgb(0x154ec2), accent, ov_blend),
                    mix_color(rgb(0x5cc8ff), accent_hi, ov_blend),
                    0.145,
                    rgb(0x0e3a7a),
                    rgb(0x38bdf8),
                    0.11,
                ),
                DesignId::Kiln => (
                    mix_color(rgb(0xd9480f), accent, ov_blend),
                    mix_color(rgb(0xffb347), accent_hi, ov_blend),
                    0.135,
                    rgb(0x8a2609),
                    rgb(0xff7a29),
                    0.10,
                ),
                DesignId::Neon => (
                    mix_color(rgb(0xd91b8f), accent, ov_blend),
                    mix_color(rgb(0xff74c5), accent_hi, ov_blend),
                    0.135,
                    rgb(0x3b28cc),
                    rgb(0x2fe6ff),
                    0.115,
                ),
                DesignId::Lumen => (
                    mix_color(rgb(0x5b46b8), accent, ov_blend),
                    mix_color(rgb(0xffb86b), accent_hi, ov_blend),
                    0.12,
                    rgb(0x29225e),
                    rgb(0x72d699),
                    0.09,
                ),
                DesignId::Spore => (
                    mix_color(rgb(0x9c5a49), accent, ov_blend),
                    mix_color(rgb(0xf6c3ba), accent_hi, ov_blend),
                    0.115,
                    rgb(0x5c3d28),
                    rgb(0xe8b98f),
                    0.085,
                ),
                DesignId::Paper => (
                    rgb(0xe3c896),
                    Color::from_rgb8(255, 252, 244),
                    0.22,
                    rgb(0xc7bca6),
                    rgb(0xd8ceb9),
                    0.14,
                ),
                _ => (
                    accent,
                    accent_hi,
                    0.095,
                    rgb(p.ok),
                    accent,
                    0.070,
                ),
            };

            fill_atmospheric_nebula(
                frame,
                Point::new(w * 0.83, h * 0.14),
                span * 0.56,
                tr_outer,
                tr_inner,
                tr_alpha,
                24,
            );
            fill_atmospheric_nebula(
                frame,
                Point::new(w * 0.15, h * 0.87),
                span * 0.54,
                bl_outer,
                bl_inner,
                bl_alpha,
                24,
            );

            // Subtle upper-center zenith horizon glow
            if !p.is_light {
                fill_atmospheric_nebula(
                    frame,
                    Point::new(w * 0.48, -h * 0.06),
                    w * 0.42,
                    tr_outer,
                    bl_inner,
                    0.055,
                    14,
                );
            }

            // 3. Ultra-subtle 24px architectural constellation dot grid (`alpha ~0.035`)
            let step = 24.0_f32;
            let dot_col = if p.is_light {
                Color::from_rgba(0.14, 0.12, 0.09, 0.045)
            } else {
                Color::from_rgba(1.0, 1.0, 1.0, 0.035)
            };
            let major_col = if p.is_light {
                Color::from_rgba(0.14, 0.12, 0.09, 0.085)
            } else {
                Color::from_rgba(1.0, 1.0, 1.0, 0.068)
            };

            let cols = (w / step).ceil() as usize;
            let rows = (h / step).ceil() as usize;
            for iy in 0..rows {
                let y = 12.0 + iy as f32 * step;
                if y >= h {
                    break;
                }
                for ix in 0..cols {
                    let x = 12.0 + ix as f32 * step;
                    if x >= w {
                        break;
                    }
                    if ix % 6 == 0 && iy % 6 == 0 {
                        // Subtle constellation node crosshair
                        frame.fill_rectangle(
                            Point::new(x - 2.5, y - 0.5),
                            Size::new(5.0, 1.0),
                            major_col,
                        );
                        frame.fill_rectangle(
                            Point::new(x - 0.5, y - 2.5),
                            Size::new(1.0, 5.0),
                            major_col,
                        );
                    } else {
                        frame.fill_rectangle(
                            Point::new(x - 0.65, y - 0.65),
                            Size::new(1.3, 1.3),
                            dot_col,
                        );
                    }
                }
            }
        });

        // 4. Lightweight dynamic pass: 14 drifting luminous bokeh motes on dark themes
        if !self.animate_motes || p.is_light || self.design == DesignId::Contrast {
            return vec![static_geo];
        }

        let mut dyn_frame = Frame::new(renderer, bounds.size());
        let t = self.phase;

        let hue_0 = if self.design == DesignId::Firefly && self.accent_override.is_none() {
            Color::from_rgb8(255, 210, 142) // warm lantern gold
        } else {
            accent_hi
        };
        let hue_1 = if self.design == DesignId::Firefly && self.accent_override.is_none() {
            Color::from_rgb8(244, 158, 192) // dusk rose-pink
        } else {
            accent
        };
        let hue_2 = if self.design == DesignId::Firefly {
            Color::from_rgb8(165, 222, 206) // bioluminescent mint
        } else {
            rgb(p.ok)
        };

        for m in &BACKDROP_MOTES {
            let nx = (m.base_x + (t * m.speed_x + m.phase_offset).sin() * m.amp_x).clamp(0.02, 0.98);
            let ny = (m.base_y
                + (t * m.speed_y + m.phase_offset * 1.35).cos() * m.amp_y
                + (t * m.speed_x * 0.5 + m.phase_offset).sin() * 0.02)
                .clamp(0.04, 0.96);
            let pt = Point::new(nx * w, ny * h);

            let pulse = 0.5 + 0.5 * (t * m.pulse_hz * PI + m.phase_offset * 1.9).sin();
            let glow_k = 0.25 + 0.75 * pulse.powf(1.5);

            let col = match m.hue_slot {
                0 => hue_0,
                1 => hue_1,
                _ => hue_2,
            };

            fill_bokeh_halo(
                &mut dyn_frame,
                pt,
                m.radius * (5.6 + 2.6 * pulse),
                col,
                0.15 * glow_k,
                3,
            );

            let core = mix_color(col, Color::WHITE, 0.45 * pulse + 0.20);
            dyn_frame.fill(
                &Path::circle(pt, m.radius * (0.68 + 0.28 * pulse)),
                Color {
                    a: (0.44 * glow_k).clamp(0.0, 0.58),
                    ..core
                },
            );
        }

        vec![static_geo, dyn_frame.into_geometry()]
    }
}

// ---------------------------------------------------------------------------
// 2. Crisp Vector UI Icon System (`UiIcon` + `icon_canvas`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UiIcon {
    Overview,
    Lighting,
    Studio,
    Fans,
    Automations,
    Devices,
    Settings,
    Customize,
    Plus,
    Check,
    Refresh,
    Sparkle,
    Expand,
    Minimize,
    Search,
    Power,
    Sliders,
    Swap,
    Reset,
}

impl UiIcon {
    pub const ALL: [UiIcon; 19] = [
        UiIcon::Overview,
        UiIcon::Lighting,
        UiIcon::Studio,
        UiIcon::Fans,
        UiIcon::Automations,
        UiIcon::Devices,
        UiIcon::Settings,
        UiIcon::Customize,
        UiIcon::Plus,
        UiIcon::Check,
        UiIcon::Refresh,
        UiIcon::Sparkle,
        UiIcon::Expand,
        UiIcon::Minimize,
        UiIcon::Search,
        UiIcon::Power,
        UiIcon::Sliders,
        UiIcon::Swap,
        UiIcon::Reset,
    ];
}

struct UiIconCanvas {
    icon: UiIcon,
    color: Color,
}

/// Renders a crisp, anti-aliased `LineCap::Round` geometric vector icon at `size × size`.
pub fn icon_canvas<'a, Message: 'a>(
    icon: UiIcon,
    color: Color,
    size: f32,
) -> Element<'a, Message> {
    let s = size.max(10.0);
    iced::widget::canvas(UiIconCanvas { icon, color })
        .width(Length::Fixed(s))
        .height(Length::Fixed(s))
        .into()
}

fn stroke_round(color: Color, width: f32) -> Stroke<'static> {
    Stroke {
        style: canvas::Style::Solid(color),
        width,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    }
}

fn arc_polyline(cx: f32, cy: f32, r: f32, start_rad: f32, end_rad: f32, segs: usize) -> Path {
    Path::new(|b| {
        let n = segs.max(6);
        for i in 0..=n {
            let t = i as f32 / n as f32;
            let a = start_rad + (end_rad - start_rad) * t;
            let pt = Point::new(cx + r * a.cos(), cy + r * a.sin());
            if i == 0 {
                b.move_to(pt);
            } else {
                b.line_to(pt);
            }
        }
    })
}

fn star4_path(cx: f32, cy: f32, outer_r: f32, inner_r: f32) -> Path {
    Path::new(|b| {
        let top = Point::new(cx, cy - outer_r);
        let right = Point::new(cx + outer_r, cy);
        let bot = Point::new(cx, cy + outer_r);
        let left = Point::new(cx - outer_r, cy);
        b.move_to(top);
        b.quadratic_curve_to(Point::new(cx + inner_r, cy - inner_r), right);
        b.quadratic_curve_to(Point::new(cx + inner_r, cy + inner_r), bot);
        b.quadratic_curve_to(Point::new(cx - inner_r, cy + inner_r), left);
        b.quadratic_curve_to(Point::new(cx - inner_r, cy - inner_r), top);
        b.close();
    })
}

impl<Message> canvas::Program<Message> for UiIconCanvas {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let dim = bounds.width.min(bounds.height).max(8.0);
        let mut frame = Frame::new(renderer, bounds.size());
        // Normalized 20x20 icon grid centered in bounds
        let ox = (bounds.width - dim) * 0.5;
        let oy = (bounds.height - dim) * 0.5;
        let k = dim / 20.0;
        let p = |x: f32, y: f32| Point::new(ox + x * k, oy + y * k);

        let sw = (1.6 * (dim / 16.0).clamp(0.84, 1.24)).max(1.2);
        let st = stroke_round(self.color, sw);
        let soft_fill = Color {
            a: (self.color.a * 0.20).clamp(0.0, 0.28),
            ..self.color
        };

        match self.icon {
            UiIcon::Overview => {
                let rad = iced::border::Radius::from(1.7 * k);
                for ( idx, &(gx, gy) ) in [
                    (2.6_f32, 2.6_f32),
                    (11.2, 2.6),
                    (2.6, 11.2),
                    (11.2, 11.2),
                ]
                .iter()
                .enumerate()
                {
                    let tile = Path::rounded_rectangle(p(gx, gy), Size::new(6.2 * k, 6.2 * k), rad);
                    if idx == 0 {
                        frame.fill(&tile, soft_fill);
                    }
                    frame.stroke(&tile, st);
                }
            }
            UiIcon::Lighting => {
                let core = Path::circle(p(10.0, 10.0), 3.7 * k);
                frame.fill(&core, soft_fill);
                frame.stroke(&core, st);
                for i in 0..8 {
                    let a = (i as f32) * PI * 0.25;
                    let r0 = 5.9;
                    let r1 = 8.1;
                    let ray = Path::line(
                        p(10.0 + r0 * a.cos(), 10.0 + r0 * a.sin()),
                        p(10.0 + r1 * a.cos(), 10.0 + r1 * a.sin()),
                    );
                    frame.stroke(&ray, st);
                }
            }
            UiIcon::Studio => {
                let top_layer = Path::new(|b| {
                    b.move_to(p(10.0, 2.5));
                    b.line_to(p(17.4, 6.7));
                    b.line_to(p(10.0, 10.9));
                    b.line_to(p(2.6, 6.7));
                    b.close();
                });
                frame.fill(&top_layer, soft_fill);
                frame.stroke(&top_layer, st);

                let mid_layer = Path::new(|b| {
                    b.move_to(p(2.6, 10.2));
                    b.line_to(p(10.0, 14.3));
                    b.line_to(p(17.4, 10.2));
                });
                frame.stroke(&mid_layer, st);

                let bot_layer = Path::new(|b| {
                    b.move_to(p(2.6, 13.5));
                    b.line_to(p(10.0, 17.6));
                    b.line_to(p(17.4, 13.5));
                });
                frame.stroke(&bot_layer, st);
            }
            UiIcon::Fans => {
                let hub = Path::circle(p(10.0, 10.0), 2.0 * k);
                frame.fill(&hub, soft_fill);
                frame.stroke(&hub, st);
                for i in 0..4 {
                    let a = (i as f32) * PI * 0.5 - 0.25;
                    let rot = |lx: f32, ly: f32| {
                        let (s, c) = a.sin_cos();
                        p(10.0 + lx * c - ly * s, 10.0 + lx * s + ly * c)
                    };
                    let blade = Path::new(|b| {
                        b.move_to(rot(1.9, -0.4));
                        b.quadratic_curve_to(rot(5.6, -4.2), rot(7.5, -0.8));
                        b.quadratic_curve_to(rot(6.8, 2.6), rot(1.8, 0.8));
                    });
                    frame.fill(&blade, soft_fill);
                    frame.stroke(&blade, st);
                }
            }
            UiIcon::Automations => {
                let bolt = Path::new(|b| {
                    b.move_to(p(11.3, 2.2));
                    b.line_to(p(4.6, 10.8));
                    b.line_to(p(9.8, 10.8));
                    b.line_to(p(8.7, 17.8));
                    b.line_to(p(15.4, 9.2));
                    b.line_to(p(10.2, 9.2));
                    b.close();
                });
                frame.fill(&bolt, soft_fill);
                frame.stroke(&bolt, st);
            }
            UiIcon::Devices => {
                let body = Path::rounded_rectangle(
                    p(5.3, 5.3),
                    Size::new(9.4 * k, 9.4 * k),
                    iced::border::Radius::from(2.0 * k),
                );
                frame.stroke(&body, st);
                let die = Path::rounded_rectangle(
                    p(8.0, 8.0),
                    Size::new(4.0 * k, 4.0 * k),
                    iced::border::Radius::from(0.8 * k),
                );
                frame.fill(&die, soft_fill);
                frame.stroke(&die, stroke_round(self.color, (sw * 0.85).max(1.0)));

                for &coord in &[7.8_f32, 12.2] {
                    frame.stroke(&Path::line(p(coord, 2.4), p(coord, 5.3)), st);
                    frame.stroke(&Path::line(p(coord, 14.7), p(coord, 17.6)), st);
                    frame.stroke(&Path::line(p(2.4, coord), p(5.3, coord)), st);
                    frame.stroke(&Path::line(p(14.7, coord), p(17.6, coord)), st);
                }
            }
            UiIcon::Settings => {
                let cx = ox + 10.0 * k;
                let cy = oy + 10.0 * k;
                let ring = Path::circle(Point::new(cx, cy), 5.3 * k);
                frame.stroke(&ring, st);
                let inner = Path::circle(Point::new(cx, cy), 2.3 * k);
                frame.fill(&inner, soft_fill);
                frame.stroke(&inner, stroke_round(self.color, (sw * 0.9).max(1.1)));
                for i in 0..6 {
                    let a = (i as f32) * PI / 3.0;
                    frame.stroke(
                        &Path::line(
                            p(10.0 + 5.3 * a.cos(), 10.0 + 5.3 * a.sin()),
                            p(10.0 + 7.7 * a.cos(), 10.0 + 7.7 * a.sin()),
                        ),
                        stroke_round(self.color, sw * 1.12),
                    );
                }
            }
            UiIcon::Customize => {
                let main_star = star4_path(ox + 9.0 * k, oy + 10.8 * k, 6.5 * k, 1.45 * k);
                frame.fill(&main_star, soft_fill);
                frame.stroke(&main_star, st);
                let mini_star = star4_path(ox + 15.4 * k, oy + 4.8 * k, 2.6 * k, 0.65 * k);
                frame.fill(&mini_star, self.color);
            }
            UiIcon::Sparkle => {
                let star = star4_path(ox + 10.0 * k, oy + 10.0 * k, 7.4 * k, 1.65 * k);
                frame.fill(&star, soft_fill);
                frame.stroke(&star, st);
                frame.fill(&Path::circle(p(15.6, 4.6), 1.05 * k), self.color);
                frame.fill(&Path::circle(p(4.6, 15.4), 0.85 * k), self.color);
            }
            UiIcon::Plus => {
                frame.stroke(&Path::line(p(10.0, 3.8), p(10.0, 16.2)), st);
                frame.stroke(&Path::line(p(3.8, 10.0), p(16.2, 10.0)), st);
            }
            UiIcon::Check => {
                let chk = Path::new(|b| {
                    b.move_to(p(3.8, 10.4));
                    b.line_to(p(8.1, 14.8));
                    b.line_to(p(16.4, 5.4));
                });
                frame.stroke(&chk, st);
            }
            UiIcon::Refresh => {
                let cx = ox + 10.0 * k;
                let cy = oy + 10.0 * k;
                let r = 6.3 * k;
                let arc1 = arc_polyline(cx, cy, r, -2.55, -0.35, 16);
                let arc2 = arc_polyline(cx, cy, r, 0.59, 2.79, 16);
                frame.stroke(&arc1, st);
                frame.stroke(&arc2, st);

                let head1 = Path::new(|b| {
                    b.move_to(p(15.9, 4.2));
                    b.line_to(p(15.9, 7.9));
                    b.line_to(p(12.2, 7.9));
                });
                let head2 = Path::new(|b| {
                    b.move_to(p(4.1, 15.8));
                    b.line_to(p(4.1, 12.1));
                    b.line_to(p(7.8, 12.1));
                });
                frame.stroke(&head1, st);
                frame.stroke(&head2, st);
            }
            UiIcon::Expand => {
                let tr = Path::new(|b| {
                    b.move_to(p(12.0, 3.2));
                    b.line_to(p(16.8, 3.2));
                    b.line_to(p(16.8, 8.0));
                });
                let bl = Path::new(|b| {
                    b.move_to(p(8.0, 16.8));
                    b.line_to(p(3.2, 16.8));
                    b.line_to(p(3.2, 12.0));
                });
                frame.stroke(&tr, st);
                frame.stroke(&bl, st);
                frame.stroke(&Path::line(p(16.5, 3.5), p(11.8, 8.2)), st);
                frame.stroke(&Path::line(p(3.5, 16.5), p(8.2, 11.8)), st);
            }
            UiIcon::Minimize => {
                let tr = Path::new(|b| {
                    b.move_to(p(12.0, 4.0));
                    b.line_to(p(12.0, 8.0));
                    b.line_to(p(16.0, 8.0));
                });
                let bl = Path::new(|b| {
                    b.move_to(p(8.0, 16.0));
                    b.line_to(p(8.0, 12.0));
                    b.line_to(p(4.0, 12.0));
                });
                frame.stroke(&tr, st);
                frame.stroke(&bl, st);
                frame.stroke(&Path::line(p(16.8, 3.2), p(12.0, 8.0)), st);
                frame.stroke(&Path::line(p(3.2, 16.8), p(8.0, 12.0)), st);
            }
            UiIcon::Search => {
                let lens = Path::circle(p(8.9, 8.9), 5.3 * k);
                frame.fill(&lens, soft_fill);
                frame.stroke(&lens, st);
                frame.stroke(&Path::line(p(12.8, 12.8), p(17.0, 17.0)), st);
            }
            UiIcon::Power => {
                let cx = ox + 10.0 * k;
                let cy = oy + 10.7 * k;
                let arc = arc_polyline(cx, cy, 6.2 * k, -0.88, PI + 0.88, 22);
                frame.stroke(&arc, st);
                frame.stroke(&Path::line(p(10.0, 2.8), p(10.0, 9.8)), st);
            }
            UiIcon::Sliders => {
                for &(y, kx) in &[(5.2_f32, 7.2_f32), (10.0, 13.0), (14.8, 8.8)] {
                    if kx - 2.3 > 2.8 {
                        frame.stroke(&Path::line(p(2.8, y), p(kx - 2.2, y)), st);
                    }
                    if kx + 2.3 < 17.2 {
                        frame.stroke(&Path::line(p(kx + 2.2, y), p(17.2, y)), st);
                    }
                    let knob = Path::circle(p(kx, y), 2.0 * k);
                    frame.fill(&knob, soft_fill);
                    frame.stroke(&knob, st);
                }
            }
            UiIcon::Swap => {
                frame.stroke(&Path::line(p(3.6, 6.8), p(16.4, 6.8)), st);
                let r_head = Path::new(|b| {
                    b.move_to(p(13.0, 3.8));
                    b.line_to(p(16.4, 6.8));
                    b.line_to(p(13.0, 9.8));
                });
                frame.stroke(&r_head, st);

                frame.stroke(&Path::line(p(16.4, 13.2), p(3.6, 13.2)), st);
                let l_head = Path::new(|b| {
                    b.move_to(p(7.0, 10.2));
                    b.line_to(p(3.6, 13.2));
                    b.line_to(p(7.0, 16.2));
                });
                frame.stroke(&l_head, st);
            }
            UiIcon::Reset => {
                let cx = ox + 10.2 * k;
                let cy = oy + 10.4 * k;
                let arc = arc_polyline(cx, cy, 6.1 * k, -2.35, PI * 1.18, 22);
                frame.stroke(&arc, st);
                let head = Path::new(|b| {
                    b.move_to(p(5.2, 3.0));
                    b.line_to(p(5.2, 6.9));
                    b.line_to(p(9.1, 6.9));
                });
                frame.stroke(&head, st);
            }
        }

        vec![frame.into_geometry()]
    }
}

// ---------------------------------------------------------------------------
// 3. Bioluminescent Firefly Particles (`FireflyParticlesCanvas`)
// ---------------------------------------------------------------------------

/// Bioluminescent drifting golden/rose-pink/mint bokeh fireflies, delicate floating pollen
/// motes, and a warm breathing lantern aura positioned behind the Overview moth's lantern.
pub struct FireflyParticlesCanvas {
    pub palette: DesignPalette,
    pub accent_override: Option<[u8; 3]>,
    pub anim_t: f32,
    pub enabled: bool,
    pub show_lantern_aura: bool,
}

/// Precomputed deterministic particle parameters (zero allocation per frame).
struct FireflySeed {
    base_x: f32,
    base_y: f32,
    amp_x: f32,
    amp_y: f32,
    speed_x: f32,
    speed_y: f32,
    phase: f32,
    pulse_hz: f32,
    radius: f32,
    hue_slot: u8, // 0 = warm lantern gold, 1 = rose-pink / accent, 2 = bioluminescent mint
}

const FIREFLIES: &[FireflySeed] = &[
    FireflySeed { base_x: 0.78, base_y: 0.34, amp_x: 0.036, amp_y: 0.16, speed_x: 0.31, speed_y: 0.43, phase: 0.4, pulse_hz: 0.72, radius: 2.3, hue_slot: 0 },
    FireflySeed { base_x: 0.66, base_y: 0.62, amp_x: 0.042, amp_y: 0.18, speed_x: 0.27, speed_y: 0.37, phase: 1.9, pulse_hz: 0.58, radius: 2.0, hue_slot: 1 },
    FireflySeed { base_x: 0.88, base_y: 0.58, amp_x: 0.028, amp_y: 0.20, speed_x: 0.35, speed_y: 0.49, phase: 3.1, pulse_hz: 0.84, radius: 2.5, hue_slot: 0 },
    FireflySeed { base_x: 0.56, base_y: 0.28, amp_x: 0.038, amp_y: 0.14, speed_x: 0.24, speed_y: 0.34, phase: 4.5, pulse_hz: 0.64, radius: 1.8, hue_slot: 2 },
    FireflySeed { base_x: 0.93, base_y: 0.26, amp_x: 0.024, amp_y: 0.15, speed_x: 0.29, speed_y: 0.41, phase: 2.3, pulse_hz: 0.77, radius: 2.1, hue_slot: 1 },
    FireflySeed { base_x: 0.44, base_y: 0.74, amp_x: 0.045, amp_y: 0.12, speed_x: 0.22, speed_y: 0.31, phase: 5.2, pulse_hz: 0.52, radius: 1.7, hue_slot: 0 },
    FireflySeed { base_x: 0.73, base_y: 0.78, amp_x: 0.034, amp_y: 0.13, speed_x: 0.33, speed_y: 0.39, phase: 0.9, pulse_hz: 0.69, radius: 2.2, hue_slot: 2 },
    FireflySeed { base_x: 0.84, base_y: 0.18, amp_x: 0.030, amp_y: 0.11, speed_x: 0.26, speed_y: 0.45, phase: 3.8, pulse_hz: 0.91, radius: 1.6, hue_slot: 0 },
    FireflySeed { base_x: 0.61, base_y: 0.46, amp_x: 0.032, amp_y: 0.17, speed_x: 0.28, speed_y: 0.36, phase: 1.3, pulse_hz: 0.61, radius: 1.9, hue_slot: 1 },
    FireflySeed { base_x: 0.33, base_y: 0.24, amp_x: 0.040, amp_y: 0.13, speed_x: 0.21, speed_y: 0.29, phase: 2.7, pulse_hz: 0.48, radius: 1.5, hue_slot: 2 },
    FireflySeed { base_x: 0.50, base_y: 0.55, amp_x: 0.035, amp_y: 0.15, speed_x: 0.25, speed_y: 0.42, phase: 5.8, pulse_hz: 0.74, radius: 1.8, hue_slot: 0 },
    FireflySeed { base_x: 0.96, base_y: 0.72, amp_x: 0.018, amp_y: 0.16, speed_x: 0.32, speed_y: 0.38, phase: 4.1, pulse_hz: 0.67, radius: 1.9, hue_slot: 1 },
    FireflySeed { base_x: 0.22, base_y: 0.78, amp_x: 0.030, amp_y: 0.11, speed_x: 0.19, speed_y: 0.27, phase: 0.2, pulse_hz: 0.54, radius: 1.4, hue_slot: 1 },
];

impl<Message> canvas::Program<Message> for FireflyParticlesCanvas {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        if !self.enabled || bounds.width < 12.0 || bounds.height < 12.0 {
            return Vec::new();
        }

        let mut frame = Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let h = bounds.height;
        let t = self.anim_t;
        let p = self.palette;
        let firefly = is_firefly_palette(p);
        let (accent, accent_hi) = resolve_accent(p, self.accent_override);

        let gold_col = if self.accent_override.is_some() {
            mix_color(accent_hi, Color::from_rgb8(255, 224, 156), 0.55)
        } else if firefly {
            Color::from_rgb8(255, 212, 138)
        } else {
            accent_hi
        };
        let rose_col = if self.accent_override.is_some() {
            accent
        } else if firefly {
            Color::from_rgb8(244, 166, 198)
        } else {
            accent
        };
        let mint_col = if firefly {
            Color::from_rgb8(165, 226, 208)
        } else {
            rgb(p.ok)
        };

        if self.show_lantern_aura && w > 260.0 {
            let lantern_x = (w - 238.0).clamp(w * 0.50, w - 52.0);
            let lantern_y = h * 0.58;
            let breath = 0.80 + 0.14 * (t * 1.25).sin() + 0.06 * (t * 2.6 + 0.7).cos();
            let aura_r = (h * 0.64).clamp(40.0, 68.0) * (0.94 + 0.06 * breath);
            let center = Point::new(lantern_x, lantern_y);

            fill_bokeh_halo(&mut frame, center, aura_r * 1.06, rose_col, 0.065 * breath, 4);
            fill_bokeh_halo(&mut frame, center, aura_r * 0.74, gold_col, 0.19 * breath, 5);
            fill_bokeh_halo(
                &mut frame,
                Point::new(lantern_x + 3.0, lantern_y + 3.0),
                aura_r * 0.30,
                Color::from_rgb8(255, 240, 200),
                0.24 * breath,
                4,
            );
        }

        let mote_count = 16;
        for i in 0..mote_count {
            let fi = i as f32;
            let seed_x = ((i * 37 + 13) % 97) as f32 / 97.0;
            let seed_y = ((i * 53 + 29) % 89) as f32 / 89.0;
            let drift_x = (seed_x + t * 0.008 * (0.6 + (i % 3) as f32 * 0.35)).fract();
            let sway_y = (seed_y - t * 0.014 * (0.5 + (i % 4) as f32 * 0.25)).rem_euclid(1.0);
            let x = drift_x * w + (t * 0.7 + fi * 1.3).sin() * 6.0;
            let y = sway_y * h;
            let twinkle = 0.25 + 0.75 * (0.5 + 0.5 * (t * (0.9 + fi * 0.11) + fi * 2.1).sin());
            let col = match i % 3 {
                0 => gold_col,
                1 => rose_col,
                _ => mint_col,
            };
            let alpha = (0.08 + 0.16 * twinkle) * if p.is_light { 0.75 } else { 1.0 };
            let r = 0.85 + (i % 3) as f32 * 0.3;
            frame.fill(&Path::circle(Point::new(x, y), r), Color { a: alpha, ..col });
        }

        let light_scale = if p.is_light { 0.70 } else { 1.0 };
        for ff in FIREFLIES {
            let nx = (ff.base_x + (t * ff.speed_x + ff.phase).sin() * ff.amp_x).clamp(0.03, 0.97);
            let ny = (ff.base_y
                + (t * ff.speed_y + ff.phase * 1.3).cos() * ff.amp_y
                + (t * ff.speed_x * 0.55 + ff.phase).sin() * 0.05)
                .clamp(0.08, 0.92);
            let pt = Point::new(nx * w, ny * h);

            let pulse_wave = 0.5 + 0.5 * (t * ff.pulse_hz * PI + ff.phase * 2.0).sin();
            let glow_k = (0.28 + 0.72 * pulse_wave.powf(1.6)) * light_scale;

            let col = match ff.hue_slot {
                0 => gold_col,
                1 => rose_col,
                _ => mint_col,
            };

            fill_bokeh_halo(
                &mut frame,
                pt,
                ff.radius * (5.2 + 2.4 * pulse_wave),
                col,
                0.34 * glow_k,
                4,
            );

            let core_col = mix_color(col, Color::WHITE, 0.55 * pulse_wave + 0.25);
            frame.fill(
                &Path::circle(pt, ff.radius * (0.72 + 0.32 * pulse_wave)),
                Color {
                    a: (0.88 * glow_k).clamp(0.0, 0.96),
                    ..core_col
                },
            );

            if pulse_wave > 0.78 {
                let spark = ((pulse_wave - 0.78) / 0.22) * light_scale;
                let arm = ff.radius * (1.8 + 1.4 * spark);
                let glint_col = Color::from_rgba(1.0, 0.98, 0.94, 0.42 * spark);
                frame.stroke(
                    &Path::line(Point::new(pt.x - arm, pt.y), Point::new(pt.x + arm, pt.y)),
                    Stroke::default()
                        .with_color(glint_col)
                        .with_width(0.75)
                        .with_line_cap(LineCap::Round),
                );
                frame.stroke(
                    &Path::line(Point::new(pt.x, pt.y - arm), Point::new(pt.x, pt.y + arm)),
                    Stroke::default()
                        .with_color(glint_col)
                        .with_width(0.75)
                        .with_line_cap(LineCap::Round),
                );
            }
        }

        vec![frame.into_geometry()]
    }
}

// ---------------------------------------------------------------------------
// 4. 24-Band Luminous Audio Spectrum Visualizer (`AudioVisualizerCanvas`)
// ---------------------------------------------------------------------------

/// Sleek 24-band luminous rounded-capsule audio equalizer strip with soft bloom,
/// beat-reactive bass wash, and gravity-decaying falling peak-hold dots.
pub struct AudioVisualizerCanvas {
    pub bands: [f32; 24],
    pub bass: f32,
    pub beat: bool,
    pub palette: DesignPalette,
    pub accent_override: Option<[u8; 3]>,
    pub active: bool,
}

pub struct AudioVisualizerState {
    smooth: RefCell<[f32; 24]>,
    peaks: RefCell<[f32; 24]>,
    idle_phase: Cell<f32>,
}

impl Default for AudioVisualizerState {
    fn default() -> Self {
        Self {
            smooth: RefCell::new([0.0; 24]),
            peaks: RefCell::new([0.0; 24]),
            idle_phase: Cell::new(0.0),
        }
    }
}

impl<Message> canvas::Program<Message> for AudioVisualizerCanvas {
    type State = AudioVisualizerState;

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let w = bounds.width.max(24.0);
        let h = bounds.height.max(14.0);
        let p = self.palette;
        let firefly = is_firefly_palette(p);
        let (accent, accent_hi) = resolve_accent(p, self.accent_override);

        let r_outer = if p.square_corners { 0.0 } else { 9.0 };
        let trough = Path::rounded_rectangle(
            Point::ORIGIN,
            Size::new(w, h),
            iced::border::Radius::from(r_outer),
        );
        let trough_grad = gradient::Linear::new(Point::ORIGIN, Point::new(0.0, h))
            .add_stop(0.0, mix_color(rgb(p.well), Color::BLACK, if p.is_light { 0.04 } else { 0.22 }))
            .add_stop(1.0, rgb(p.well));
        frame.fill(&trough, Fill::from(Gradient::Linear(trough_grad)));
        frame.stroke(
            &trough,
            Stroke::default()
                .with_color(if p.is_light {
                    Color::from_rgba(0.11, 0.11, 0.09, 0.14)
                } else {
                    Color::from_rgba(1.0, 1.0, 1.0, if self.active { 0.14 } else { 0.09 })
                })
                .with_width(1.0),
        );

        let mut smooth = state.smooth.borrow_mut();
        let mut peaks = state.peaks.borrow_mut();
        for i in 0..24 {
            let target = if self.active {
                self.bands[i].clamp(0.0, 1.0)
            } else {
                0.0
            };
            let prev = smooth[i];
            let alpha = if target > prev { 0.58 } else { 0.24 };
            let next = prev + (target - prev) * alpha;
            smooth[i] = if next < 0.004 { 0.0 } else { next };

            if smooth[i] >= peaks[i] {
                peaks[i] = smooth[i];
            } else {
                peaks[i] = (peaks[i] - 0.016).max(0.0);
            }
        }

        let bass_k = if self.active {
            self.bass.clamp(0.0, 1.0)
        } else {
            0.0
        };
        if bass_k > 0.08 {
            let beat_boost = if self.beat { 1.35 } else { 1.0 };
            let wash_col = if self.accent_override.is_some() {
                accent
            } else if firefly {
                Color::from_rgb8(242, 169, 196)
            } else {
                accent
            };
            let wash = gradient::Linear::new(Point::ORIGIN, Point::new(w * 0.55, 0.0))
                .add_stop(
                    0.0,
                    Color {
                        a: (0.16 * bass_k * beat_boost).clamp(0.0, 0.28),
                        ..wash_col
                    },
                )
                .add_stop(1.0, Color { a: 0.0, ..wash_col });
            frame.fill(&trough, Fill::from(Gradient::Linear(wash)));
        }

        let c_bass = if self.accent_override.is_some() {
            accent
        } else if firefly {
            Color::from_rgb8(244, 143, 177)
        } else {
            accent
        };
        let c_mid = if self.accent_override.is_some() {
            mix_color(accent_hi, rgb(p.warm), 0.45)
        } else if firefly {
            Color::from_rgb8(255, 198, 138)
        } else {
            accent_hi
        };
        let c_treble = if firefly {
            Color::from_rgb8(165, 226, 208)
        } else {
            rgb(p.ok)
        };

        let pad_x = 7.0;
        let pad_top = 5.0;
        let pad_bot = 4.0;
        let usable_w = (w - pad_x * 2.0).max(24.0);
        let usable_h = (h - pad_top - pad_bot).max(6.0);
        let base_y = h - pad_bot;

        frame.stroke(
            &Path::line(Point::new(pad_x, base_y), Point::new(w - pad_x, base_y)),
            Stroke::default()
                .with_color(rgba(p.line, 0.25))
                .with_width(0.8),
        );

        let gap = (usable_w * 0.012).clamp(1.6, 3.0);
        let bar_w = ((usable_w - gap * 23.0) / 24.0).max(2.0);
        let cap_r = if p.square_corners {
            0.0
        } else {
            (bar_w * 0.48).min(3.5)
        };

        let is_live =
            self.active || smooth.iter().any(|&v| v > 0.015) || peaks.iter().any(|&v| v > 0.025);

        if is_live {
            for i in 0..24 {
                let f = i as f32 / 23.0;
                let bar_col = if f < 0.5 {
                    mix_color(c_bass, c_mid, f * 2.0)
                } else {
                    mix_color(c_mid, c_treble, (f - 0.5) * 2.0)
                };

                let x = pad_x + i as f32 * (bar_w + gap);
                let lvl = smooth[i];
                let peak = peaks[i];

                if lvl > 0.01 {
                    let bh = (usable_h * lvl).max(3.0);
                    let y = base_y - bh;

                    if !p.square_corners {
                        let bloom_rect = Path::rounded_rectangle(
                            Point::new(x - 1.2, (y - 1.2).max(pad_top - 1.0)),
                            Size::new(bar_w + 2.4, (bh + 2.0).min(usable_h + 1.5)),
                            iced::border::Radius::from(cap_r + 1.0),
                        );
                        frame.fill(
                            &bloom_rect,
                            Color {
                                a: (0.14 + 0.16 * lvl).clamp(0.0, 0.30),
                                ..bar_col
                            },
                        );
                    }

                    let bar_rect = Path::rounded_rectangle(
                        Point::new(x, y),
                        Size::new(bar_w, bh),
                        iced::border::Radius::from(cap_r),
                    );
                    let bar_grad = gradient::Linear::new(Point::new(x, base_y), Point::new(x, y))
                        .add_stop(0.0, Color { a: 0.62, ..bar_col })
                        .add_stop(0.75, bar_col)
                        .add_stop(1.0, mix_color(bar_col, Color::WHITE, 0.38));
                    frame.fill(&bar_rect, Fill::from(Gradient::Linear(bar_grad)));
                } else {
                    let tick_h = 2.6;
                    let tick = Path::rounded_rectangle(
                        Point::new(x, base_y - tick_h),
                        Size::new(bar_w, tick_h),
                        iced::border::Radius::from(cap_r.min(1.2)),
                    );
                    frame.fill(&tick, rgba(p.line, 0.38));
                }

                if peak > 0.04 {
                    let py = (base_y - usable_h * peak - 2.2).clamp(pad_top * 0.5, base_y - 3.0);
                    let dot_r = (bar_w * 0.38).clamp(1.2, 2.3);
                    let dot_center = Point::new(x + bar_w * 0.5, py);
                    let dot_alpha = (peak * 1.4).clamp(0.25, 0.96);
                    let peak_col = mix_color(bar_col, Color::WHITE, 0.65);
                    if !p.square_corners {
                        frame.fill(
                            &Path::circle(dot_center, dot_r * 2.1),
                            Color {
                                a: 0.22 * dot_alpha,
                                ..bar_col
                            },
                        );
                    }
                    frame.fill(
                        &Path::circle(dot_center, dot_r),
                        Color {
                            a: dot_alpha,
                            ..peak_col
                        },
                    );
                }
            }
        } else {
            let phase = (state.idle_phase.get() + 0.045) % (2.0 * PI * 100.0);
            state.idle_phase.set(phase);

            for i in 0..24 {
                let f = i as f32 / 23.0;
                let bar_col = if f < 0.5 {
                    mix_color(c_bass, c_mid, f * 2.0)
                } else {
                    mix_color(c_mid, c_treble, (f - 0.5) * 2.0)
                };
                let x = pad_x + i as f32 * (bar_w + gap);
                let arch = 0.65 + 0.35 * (f * PI).sin();
                let wave =
                    0.5 + 0.34 * (f * 4.2 - phase).sin() + 0.16 * (f * 8.5 + phase * 0.65).cos();
                let idle_lvl = (0.14 + 0.22 * wave * arch).clamp(0.12, 0.38);
                let bh = (usable_h * idle_lvl).max(3.5);
                let y = base_y - bh;

                let bar_rect = Path::rounded_rectangle(
                    Point::new(x, y),
                    Size::new(bar_w, bh),
                    iced::border::Radius::from(cap_r),
                );
                let bar_grad = gradient::Linear::new(Point::new(x, base_y), Point::new(x, y))
                    .add_stop(0.0, Color { a: 0.14, ..bar_col })
                    .add_stop(1.0, Color { a: 0.36, ..bar_col });
                frame.fill(&bar_rect, Fill::from(Gradient::Linear(bar_grad)));

                let dot_r = (bar_w * 0.30).clamp(1.0, 1.8);
                frame.fill(
                    &Path::circle(Point::new(x + bar_w * 0.5, (y - 2.0).max(pad_top)), dot_r),
                    Color {
                        a: 0.34,
                        ..mix_color(bar_col, Color::WHITE, 0.45)
                    },
                );
            }

            if w >= 220.0 && h >= 28.0 {
                let hint = if w >= 330.0 {
                    "Enable Test beat or add a music layer for live EQ"
                } else {
                    "Enable Test beat for live EQ"
                };
                let pill_w = (hint.len() as f32 * 5.6 + 18.0).min(w - 20.0);
                let pill_h = 18.0;
                let px = (w - pill_w) * 0.5;
                let py = (h - pill_h) * 0.42;
                let pill = Path::rounded_rectangle(
                    Point::new(px, py),
                    Size::new(pill_w, pill_h),
                    iced::border::Radius::from(999.0),
                );
                frame.fill(&pill, rgba(p.ground, 0.80));
                frame.stroke(
                    &pill,
                    Stroke::default()
                        .with_color(rgba(p.line, 0.45))
                        .with_width(0.8),
                );
                frame.fill_text(canvas::Text {
                    content: hint.to_string(),
                    position: Point::new(w * 0.5, py + pill_h * 0.5),
                    color: rgba(p.ink_2, 0.86),
                    size: iced::Pixels(10.0),
                    align_x: iced::widget::text::Alignment::Center,
                    align_y: iced::alignment::Vertical::Center,
                    ..canvas::Text::default()
                });
            }
        }

        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backdrop_cache_key_stable_across_phase_and_sensitive_to_design_or_size() {
        let k1 = backdrop_cache_key(2048.0, 1152.0, DesignId::Firefly, None);
        let k2 = backdrop_cache_key(2048.2, 1151.9, DesignId::Firefly, None);
        assert_eq!(k1, k2);

        let k_resize = backdrop_cache_key(1440.0, 900.0, DesignId::Firefly, None);
        let k_design = backdrop_cache_key(2048.0, 1152.0, DesignId::Midnight, None);
        let k_accent = backdrop_cache_key(2048.0, 1152.0, DesignId::Firefly, Some([244, 143, 177]));
        assert_ne!(k1, k_resize);
        assert_ne!(k1, k_design);
        assert_ne!(k1, k_accent);
    }

    #[test]
    fn ui_icon_canvas_constructs_all_19_variants() {
        assert_eq!(UiIcon::ALL.len(), 19);
        for &icon in &UiIcon::ALL {
            let _el: Element<'static, ()> = icon_canvas(icon, Color::WHITE, 15.0);
        }
    }
}
