//! GPU-rendered instrument widgets, Living-Light animated graphs with interactive hover probes
//! and smart `canvas::Cache` separation, all 16 live-animated effect artworks, header glow strip,
//! and cached 13-design miniature UI previews honoring `accent_override`.

use std::cell::Cell;
use std::collections::hash_map::DefaultHasher;
use std::f32::consts::PI;
use std::hash::{Hash, Hasher};

use airglow_core::config::{DesignId, DesignPalette, TempUnit};
use airglow_core::effects::EffectId;
use iced::mouse;
use iced::widget::canvas::{
    self, Fill, Frame, Geometry, Gradient, LineCap, LineDash, LineJoin, Path, Stroke, gradient,
    path::Arc,
};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};

use crate::theme::{rgb, rgba};

#[derive(Default)]
pub struct CachedLayerState {
    cache: canvas::Cache,
    key: Cell<u64>,
}

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
        (base, mix_color(base, Color::WHITE, 0.28))
    } else {
        (rgb(p.brass), rgb(p.brass_hi))
    }
}

fn resolve_semantic_or_accent(
    color_hex: u32,
    p: DesignPalette,
    accent_override: Option<[u8; 3]>,
) -> Color {
    if let Some([r, g, b]) = accent_override
        && (color_hex == p.brass || color_hex == p.brass_hi)
    {
        let base = Color::from_rgb8(r, g, b);
        return if color_hex == p.brass_hi {
            mix_color(base, Color::WHITE, 0.26)
        } else {
            base
        };
    }
    rgb(color_hex)
}

fn fill_radial_glow(
    frame: &mut Frame,
    center: Point,
    max_r: f32,
    color: Color,
    peak_alpha: f32,
    steps: usize,
) {
    if max_r <= 0.5 || peak_alpha <= 0.002 {
        return;
    }
    let n = steps.max(3);
    for i in (0..n).rev() {
        let f = (i + 1) as f32 / n as f32;
        let r = max_r * f;
        let layer_a = (peak_alpha / n as f32) * (1.28 - f * 0.88).max(0.14) * 1.45;
        frame.fill(
            &Path::circle(center, r),
            Color {
                a: layer_a.clamp(0.0, 0.72),
                ..color
            },
        );
    }
}

/// Fritsch–Carlson monotonic cubic Hermite tangents (never overshoots readings).
fn monotonic_tangents(pts: &[Point]) -> Vec<f32> {
    let n = pts.len();
    let mut m = vec![0.0_f32; n];
    if n < 2 {
        return m;
    }
    let mut h = vec![0.0_f32; n - 1];
    let mut d = vec![0.0_f32; n - 1];
    for i in 0..n - 1 {
        h[i] = (pts[i + 1].x - pts[i].x).max(0.001);
        d[i] = (pts[i + 1].y - pts[i].y) / h[i];
    }
    m[0] = d[0];
    m[n - 1] = d[n - 2];
    for i in 1..n - 1 {
        if d[i - 1] * d[i] <= 0.0 {
            m[i] = 0.0;
        } else {
            let a = 2.0 * h[i] + h[i - 1];
            let b = h[i] + 2.0 * h[i - 1];
            m[i] = (a + b) / (a / d[i - 1] + b / d[i]);
        }
    }
    m
}

fn build_smooth_curve_path(pts: &[Point], close_bottom_y: Option<f32>) -> Path {
    let m = monotonic_tangents(pts);
    Path::new(|b| {
        if let Some(by) = close_bottom_y {
            b.move_to(Point::new(pts[0].x, by));
            b.line_to(pts[0]);
        } else {
            b.move_to(pts[0]);
        }
        for i in 0..pts.len() - 1 {
            let p0 = pts[i];
            let p1 = pts[i + 1];
            let dx = (p1.x - p0.x) / 3.0;
            let c0 = Point::new(p0.x + dx, p0.y + m[i] * dx);
            let c1 = Point::new(p1.x - dx, p1.y - m[i + 1] * dx);
            b.bezier_curve_to(c0, c1, p1);
        }
        if let Some(by) = close_bottom_y {
            b.line_to(Point::new(pts[pts.len() - 1].x, by));
            b.close();
        }
    })
}

fn sample_interpolated(samples: &[f32], frac: f32) -> Option<f32> {
    if samples.is_empty() {
        return None;
    }
    if samples.len() == 1 {
        return Some(samples[0]);
    }
    let idx_f = frac.clamp(0.0, 1.0) * (samples.len() - 1) as f32;
    let i0 = (idx_f.floor() as usize).min(samples.len() - 1);
    let i1 = (i0 + 1).min(samples.len() - 1);
    let t = idx_f - i0 as f32;
    Some(samples[i0] + (samples[i1] - samples[i0]) * t)
}

fn sample_point_at_frac(pts: &[Point], frac: f32) -> Option<Point> {
    if pts.is_empty() {
        return None;
    }
    if pts.len() == 1 {
        return Some(pts[0]);
    }
    let idx_f = frac.clamp(0.0, 1.0) * (pts.len() - 1) as f32;
    let i0 = (idx_f.floor() as usize).min(pts.len() - 1);
    let i1 = (i0 + 1).min(pts.len() - 1);
    let t = idx_f - i0 as f32;
    Some(Point::new(
        pts[i0].x + (pts[i1].x - pts[i0].x) * t,
        pts[i0].y + (pts[i1].y - pts[i0].y) * t,
    ))
}

/// Renders Airglow's signature Living-Light curve (smooth monotonic cubic Bezier, vertical wash,
/// 3-pass neon tube bloom, pale white inner core, traveling shimmer glint, glowing live head + ripple).
#[allow(clippy::too_many_arguments)]
fn draw_living_light_series(
    frame: &mut Frame,
    pts: &[Point],
    plot_top: f32,
    plot_bottom: f32,
    plot_left: f32,
    plot_w: f32,
    col: Color,
    fill_alpha: f32,
    stroke_w: f32,
    dashed: bool,
    show_ripple: bool,
    anim_t: f32,
    glow_strength: f32,
) {
    if pts.len() < 2 {
        return;
    }

    // 1. Vertical gradient area wash under curve
    if fill_alpha > 0.005 {
        let area = build_smooth_curve_path(pts, Some(plot_bottom));
        let wash = gradient::Linear::new(Point::new(0.0, plot_top), Point::new(0.0, plot_bottom))
            .add_stop(0.0, Color { a: fill_alpha, ..col })
            .add_stop(1.0, Color { a: 0.0, ..col });
        frame.fill(&area, Fill::from(Gradient::Linear(wash)));
    }

    let curve = build_smooth_curve_path(pts, None);

    // 2. 3-pass soft neon bloom
    if glow_strength > 0.05 {
        for &(w, a) in &[(11.0_f32, 0.05_f32), (6.5, 0.10), (3.6, 0.20)] {
            frame.stroke(
                &curve,
                Stroke {
                    style: canvas::Style::Solid(Color {
                        a: (a * glow_strength).clamp(0.0, 0.45),
                        ..col
                    }),
                    width: w,
                    line_cap: LineCap::Round,
                    line_join: LineJoin::Round,
                    ..Stroke::default()
                },
            );
        }
    }

    // 3. Crisp main colored stroke
    let mut main_stroke = Stroke {
        style: canvas::Style::Solid(col),
        width: stroke_w,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    };
    if dashed {
        main_stroke.line_dash = LineDash {
            segments: &[5.0, 4.0],
            offset: 0,
        };
    }
    frame.stroke(&curve, main_stroke);

    // 4. Pale white inner lit-tube core
    if glow_strength > 0.05 && !dashed {
        frame.stroke(
            &curve,
            Stroke {
                style: canvas::Style::Solid(Color::from_rgba(1.0, 1.0, 1.0, 0.34 * glow_strength)),
                width: 0.85,
                line_cap: LineCap::Round,
                line_join: LineJoin::Round,
                ..Stroke::default()
            },
        );
    }

    // 5. Periodic traveling shimmer glint sweeping left-to-right every ~6.2s
    if glow_strength > 0.05 && plot_w > 24.0 {
        let cycle = (anim_t * 0.16).fract();
        if cycle < 0.28 {
            let sh = cycle / 0.28;
            let centre = plot_left + plot_w * (-0.08 + 1.16 * sh);
            let band = (plot_w * 0.18).max(36.0);
            let sweep = gradient::Linear::new(
                Point::new(centre - band, 0.0),
                Point::new(centre + band, 0.0),
            )
            .add_stop(0.0, Color::from_rgba(1.0, 1.0, 1.0, 0.0))
            .add_stop(0.5, Color::from_rgba(1.0, 1.0, 1.0, 0.52 * glow_strength))
            .add_stop(1.0, Color::from_rgba(1.0, 1.0, 1.0, 0.0));
            frame.stroke(
                &curve,
                Stroke {
                    style: canvas::Style::Gradient(Gradient::Linear(sweep)),
                    width: stroke_w + 1.0,
                    line_cap: LineCap::Round,
                    line_join: LineJoin::Round,
                    ..Stroke::default()
                },
            );
        }
    }

    // 6. Glowing live head + expanding ripple ring
    let head = pts[pts.len() - 1];
    if glow_strength > 0.05 {
        fill_radial_glow(frame, head, 11.5, col, 0.55 * glow_strength, 5);
        if show_ripple {
            let rip = (anim_t * 0.52).fract();
            if rip < 0.55 {
                let k = rip / 0.55;
                let ease = 1.0 - (1.0 - k).powi(3);
                let rr = 3.0 + 10.5 * ease;
                let ra = (1.0 - k).powi(2) * 0.44 * glow_strength;
                frame.stroke(
                    &Path::circle(head, rr),
                    Stroke::default()
                        .with_color(Color { a: ra, ..col })
                        .with_width(1.0),
                );
            }
        }
    }
    let core_col = if glow_strength > 0.05 {
        mix_color(col, Color::WHITE, 0.70)
    } else {
        col
    };
    frame.fill(&Path::circle(head, 2.6), core_col);
}

/// Crisp horizon, planet limb, and layered luminous 557.7 nm airglow arc.
pub struct BrandMarkCanvas {
    pub palette: DesignPalette,
    pub accent_override: Option<[u8; 3]>,
}

impl<Message> canvas::Program<Message> for BrandMarkCanvas {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let s = bounds.width.min(bounds.height);
        let cx = bounds.width * 0.5;
        let cy = bounds.height * 0.72;
        let firefly = is_firefly_palette(self.palette);
        let (accent, _) = resolve_accent(self.palette, self.accent_override);
        let body = rgb(self.palette.ink);
        let glow = if self.accent_override.is_some() {
            mix_color(accent, Color::from_rgb8(165, 222, 206), 0.35)
        } else if firefly {
            Color::from_rgb8(165, 222, 206)
        } else {
            Color::from_rgb8(110, 240, 160)
        };

        fill_radial_glow(
            &mut frame,
            Point::new(cx, cy - s * 0.16),
            s * 0.48,
            if firefly || self.accent_override.is_some() {
                accent
            } else {
                glow
            },
            0.24,
            4,
        );

        let horizon = Path::line(Point::new(cx - s * 0.42, cy), Point::new(cx + s * 0.42, cy));
        frame.stroke(
            &horizon,
            Stroke::default()
                .with_color(body)
                .with_width(1.8)
                .with_line_cap(LineCap::Round),
        );

        let limb = Path::new(|b| {
            b.move_to(Point::new(cx - s * 0.21, cy));
            b.arc(Arc {
                center: Point::new(cx, cy),
                radius: s * 0.21,
                start_angle: iced::Radians(PI),
                end_angle: iced::Radians(2.0 * PI),
            });
            b.close();
        });
        frame.fill(&limb, body);

        let arc_path = Path::new(|b| {
            b.arc(Arc {
                center: Point::new(cx, cy),
                radius: s * 0.37,
                start_angle: iced::Radians(196.0_f32.to_radians()),
                end_angle: iced::Radians(344.0_f32.to_radians()),
            });
        });
        frame.stroke(
            &arc_path,
            Stroke::default()
                .with_color(Color { a: 0.20, ..glow })
                .with_width(5.6)
                .with_line_cap(LineCap::Round),
        );
        frame.stroke(
            &arc_path,
            Stroke::default()
                .with_color(glow)
                .with_width(2.0)
                .with_line_cap(LineCap::Round),
        );
        frame.stroke(
            &arc_path,
            Stroke::default()
                .with_color(Color::from_rgba(1.0, 1.0, 1.0, 0.42))
                .with_width(0.8)
                .with_line_cap(LineCap::Round),
        );

        vec![frame.into_geometry()]
    }
}

/// Full-width 2px luminous accent hairline directly beneath the Appbar (honors `accent_override`).
pub struct HeaderGlowCanvas {
    pub palette: DesignPalette,
    pub accent_override: Option<[u8; 3]>,
    pub ambient_rgb: [u8; 3],
    pub lights_on: bool,
    pub anim_t: f32,
}

impl<Message> canvas::Program<Message> for HeaderGlowCanvas {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let w = bounds.width.max(10.0);
        let h = bounds.height.max(1.0);
        let p = self.palette;
        let firefly = is_firefly_palette(p);
        let (accent, accent_hi) = resolve_accent(p, self.accent_override);

        let amb = if self.lights_on {
            Color::from_rgb8(
                self.ambient_rgb[0],
                self.ambient_rgb[1],
                self.ambient_rgb[2],
            )
        } else {
            accent
        };

        let left_col = if self.accent_override.is_some() {
            Color { a: 0.62, ..accent }
        } else if firefly {
            Color::from_rgba8(242, 169, 196, 0.58)
        } else {
            Color { a: 0.38, ..accent }
        };
        let mid_col = if self.lights_on {
            Color {
                a: 0.74,
                ..if self.accent_override.is_some() {
                    mix_color(amb, accent_hi, 0.45)
                } else {
                    amb
                }
            }
        } else if self.accent_override.is_some() {
            Color {
                a: 0.70,
                ..accent_hi
            }
        } else if firefly {
            Color::from_rgba8(255, 195, 139, 0.65)
        } else {
            Color {
                a: 0.55,
                ..accent_hi
            }
        };
        let right_col = if firefly {
            Color::from_rgba8(165, 222, 206, 0.52)
        } else if self.accent_override.is_some() {
            Color { a: 0.48, ..accent }
        } else {
            rgba(p.line, 0.35)
        };

        let base = gradient::Linear::new(Point::ORIGIN, Point::new(w, 0.0))
            .add_stop(0.0, Color { a: 0.08, ..left_col })
            .add_stop(0.24, left_col)
            .add_stop(0.52, mid_col)
            .add_stop(0.80, right_col)
            .add_stop(1.0, Color { a: 0.08, ..right_col });
        frame.fill(
            &Path::rectangle(Point::ORIGIN, Size::new(w, h)),
            Fill::from(Gradient::Linear(base)),
        );

        // Gentle traveling luminous shimmer
        let phase = (self.anim_t * 0.11).fract();
        let sx = w * (-0.15 + 1.3 * phase);
        let bw = (w * 0.16).max(90.0);
        let shimmer = gradient::Linear::new(Point::new(sx - bw, 0.0), Point::new(sx + bw, 0.0))
            .add_stop(0.0, Color::from_rgba(1.0, 0.96, 0.92, 0.0))
            .add_stop(0.5, Color::from_rgba(1.0, 0.96, 0.92, 0.55))
            .add_stop(1.0, Color::from_rgba(1.0, 0.96, 0.92, 0.0));
        frame.fill(
            &Path::rectangle(Point::ORIGIN, Size::new(w, h)),
            Fill::from(Gradient::Linear(shimmer)),
        );

        vec![frame.into_geometry()]
    }
}

/// Overview Masthead Living-Light Pulse Strip with cached horizon guide & interactive cursor
/// hover probe (`"CPU 48°C · GPU 38°C"`).
pub struct PulseGraphCanvas<'a> {
    pub series_a: &'a [f32],
    pub series_b: &'a [f32],
    pub color_a: u32,
    pub color_b: u32,
    pub temp_unit: TempUnit,
    pub palette: DesignPalette,
    pub accent_override: Option<[u8; 3]>,
    pub anim_t: f32,
}

impl<'a, Message> canvas::Program<Message> for PulseGraphCanvas<'a> {
    type State = CachedLayerState;

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let w = bounds.width.max(24.0);
        let h = bounds.height.max(18.0);
        let pad_x = 8.0;
        let pad_y = 5.0;
        let plot_w = (w - pad_x * 2.0).max(8.0);
        let plot_h = (h - pad_y * 2.0).max(6.0);
        let p = self.palette;
        let glow = if p.square_corners { 0.35 } else { 1.0 };

        // Cached static midpoint horizon guide
        let mut hasher = DefaultHasher::new();
        ((w * 2.0).round() as i32).hash(&mut hasher);
        ((h * 2.0).round() as i32).hash(&mut hasher);
        p.ink_3.hash(&mut hasher);
        let key = hasher.finish();
        if state.key.get() != key {
            state.cache.clear();
            state.key.set(key);
        }
        let static_geom = state.cache.draw(renderer, bounds.size(), |frame| {
            let mid_y = pad_y + plot_h * 0.52;
            frame.stroke(
                &Path::line(Point::new(pad_x, mid_y), Point::new(pad_x + plot_w, mid_y)),
                Stroke {
                    style: canvas::Style::Solid(rgba(p.ink_3, 0.14)),
                    width: 1.0,
                    line_dash: LineDash {
                        segments: &[3.0, 5.0],
                        offset: 0,
                    },
                    ..Stroke::default()
                },
            );
        });

        let mut frame = Frame::new(renderer, bounds.size());
        let col_a = resolve_semantic_or_accent(self.color_a, p, self.accent_override);
        let col_b = resolve_semantic_or_accent(self.color_b, p, self.accent_override);
        let (accent, accent_hi) = resolve_accent(p, self.accent_override);

        let to_pts = |samples: &[f32], phase_bias: f32| -> Vec<Point> {
            if samples.len() >= 2 {
                let mut lo = f32::INFINITY;
                let mut hi = f32::NEG_INFINITY;
                for &v in samples {
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
                let mid = (lo + hi) * 0.5;
                let span = (hi - lo).max(12.0);
                let lo_adj = mid - span * 0.55;
                let max_i = (samples.len() - 1) as f32;
                samples
                    .iter()
                    .enumerate()
                    .map(|(i, &v)| {
                        let x = pad_x + (i as f32 / max_i) * plot_w;
                        let n = ((v - lo_adj) / (span * 1.1)).clamp(0.10, 0.90);
                        let y = pad_y + (1.0 - n) * plot_h;
                        Point::new(x, y)
                    })
                    .collect()
            } else {
                let count = 16;
                (0..count)
                    .map(|i| {
                        let f = i as f32 / (count - 1) as f32;
                        let wave = 0.5
                            + 0.16 * (f * 4.5 - self.anim_t * 0.8 + phase_bias).sin()
                            + 0.06 * (f * 9.0 + self.anim_t * 0.4).cos();
                        Point::new(pad_x + f * plot_w, pad_y + (1.0 - wave) * plot_h)
                    })
                    .collect()
            }
        };

        let pts_b = to_pts(self.series_b, 1.4);
        draw_living_light_series(
            &mut frame,
            &pts_b,
            pad_y,
            h - 1.0,
            pad_x,
            plot_w,
            col_b,
            0.11,
            1.6,
            false,
            false,
            self.anim_t + 1.8,
            glow * 0.85,
        );

        let pts_a = to_pts(self.series_a, 0.0);
        draw_living_light_series(
            &mut frame,
            &pts_a,
            pad_y,
            h - 1.0,
            pad_x,
            plot_w,
            col_a,
            0.22,
            2.0,
            false,
            true,
            self.anim_t,
            glow,
        );

        // Interactive cursor hover probe (hairline + intersection dots + compact frosted pill)
        if let Some(hpos) = cursor.position_in(bounds)
            && hpos.x >= pad_x
            && hpos.x <= pad_x + plot_w
            && (self.series_a.len() >= 2 || self.series_b.len() >= 2)
        {
            let frac = ((hpos.x - pad_x) / plot_w).clamp(0.0, 1.0);
            let px = pad_x + frac * plot_w;

            // Vertical probe hairline
            let probe_g = gradient::Linear::new(Point::new(px, 1.0), Point::new(px, h - 1.0))
                .add_stop(0.0, rgba(p.ink, 0.12))
                .add_stop(0.5, rgba(p.ink, 0.55))
                .add_stop(1.0, rgba(p.ink, 0.16));
            frame.stroke(
                &Path::line(Point::new(px, 1.0), Point::new(px, h - 1.0)),
                Stroke {
                    style: canvas::Style::Gradient(Gradient::Linear(probe_g)),
                    width: 1.0,
                    ..Stroke::default()
                },
            );

            // Intersection glowing dots
            if let Some(pt_b) = sample_point_at_frac(&pts_b, frac) {
                fill_radial_glow(&mut frame, pt_b, 6.5, col_b, 0.50, 3);
                frame.fill(&Path::circle(pt_b, 2.5), mix_color(col_b, Color::WHITE, 0.6));
            }
            if let Some(pt_a) = sample_point_at_frac(&pts_a, frac) {
                fill_radial_glow(&mut frame, pt_a, 7.0, col_a, 0.55, 3);
                frame.fill(&Path::circle(pt_a, 2.7), mix_color(col_a, Color::WHITE, 0.65));
            }

            // Compact readout pill
            let va = sample_interpolated(self.series_a, frac);
            let vb = sample_interpolated(self.series_b, frac);
            let tip = match (va, vb) {
                (Some(a), Some(b)) => format!(
                    "CPU {} · GPU {}",
                    self.temp_unit.format_temp(a),
                    self.temp_unit.format_temp(b)
                ),
                (Some(a), None) => format!("CPU {}", self.temp_unit.format_temp(a)),
                (None, Some(b)) => format!("GPU {}", self.temp_unit.format_temp(b)),
                (None, None) => String::new(),
            };
            if !tip.is_empty() {
                let pill_h = 18.0;
                let pill_w = (tip.len() as f32 * 5.6 + 12.0).min(w - 10.0);
                let tx = if px + 10.0 + pill_w > w - 4.0 {
                    (px - pill_w - 8.0).max(4.0)
                } else {
                    px + 8.0
                };
                let ty = ((h - pill_h) * 0.5).clamp(1.0, (h - pill_h - 1.0).max(1.0));
                let pill = Path::rounded_rectangle(
                    Point::new(tx, ty),
                    Size::new(pill_w, pill_h),
                    iced::border::Radius::from(5.0),
                );
                frame.fill(&pill, rgba(p.ground, 0.94));
                frame.stroke(
                    &pill,
                    Stroke::default()
                        .with_color(Color { a: 0.65, ..accent })
                        .with_width(0.95),
                );
                frame.fill_text(canvas::Text {
                    content: tip,
                    position: Point::new(tx + 6.0, ty + 3.0),
                    color: accent_hi,
                    size: iced::Pixels(9.8),
                    ..canvas::Text::default()
                });
            }
        }

        vec![static_geom, frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.position_in(bounds).is_some()
            && (self.series_a.len() >= 2 || self.series_b.len() >= 2)
        {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::default()
        }
    }
}

/// Living-Light monotonic cubic sparkline used in Overview & Fans metric cards.
pub struct SparklineCanvas<'a> {
    pub samples: &'a [f32],
    pub min_val: f32,
    pub max_val: f32,
    pub color: u32,
    pub palette: DesignPalette,
    pub accent_override: Option<[u8; 3]>,
    pub anim_t: f32,
}

impl<'a, Message> canvas::Program<Message> for SparklineCanvas<'a> {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let w = bounds.width.max(8.0);
        let h = bounds.height.max(8.0);
        let pad_r = 8.0;
        let pad_y = 5.0;
        let plot_w = (w - pad_r).max(6.0);
        let plot_h = (h - pad_y * 2.0).max(4.0);
        let stroke_col = resolve_semantic_or_accent(self.color, self.palette, self.accent_override);

        frame.stroke(
            &Path::line(Point::new(0.0, h - 1.0), Point::new(plot_w, h - 1.0)),
            Stroke::default()
                .with_color(rgba(self.palette.line, 0.30))
                .with_width(1.0),
        );

        if self.samples.len() >= 2 {
            let auto_max = self
                .samples
                .iter()
                .copied()
                .fold(self.max_val, f32::max);
            let span = (auto_max - self.min_val).max(1.0);
            let max_i = (self.samples.len() - 1) as f32;
            let pts: Vec<Point> = self
                .samples
                .iter()
                .enumerate()
                .map(|(i, &v)| {
                    let x = (i as f32 / max_i) * plot_w;
                    let norm = ((v - self.min_val) / span).clamp(0.06, 0.94);
                    let y = pad_y + (1.0 - norm) * plot_h;
                    Point::new(x, y)
                })
                .collect();

            let glow = if self.palette.square_corners { 0.35 } else { 1.0 };
            draw_living_light_series(
                &mut frame,
                &pts,
                pad_y,
                h - 1.0,
                0.0,
                plot_w,
                stroke_col,
                0.24,
                1.9,
                false,
                true,
                self.anim_t,
                glow,
            );
        } else {
            let flat_y = h * 0.65;
            frame.stroke(
                &Path::line(Point::new(0.0, flat_y), Point::new(plot_w, flat_y)),
                Stroke::default()
                    .with_color(rgba(self.palette.line, 0.5))
                    .with_width(1.2),
            );
            frame.fill(&Path::circle(Point::new(plot_w, flat_y), 2.2), stroke_col);
        }

        vec![frame.into_geometry()]
    }
}

/// 270-degree luminous circular load gauge honoring `accent_override`.
pub struct RingGaugeCanvas {
    pub percent: f32,
    pub accent: u32,
    pub palette: DesignPalette,
    pub accent_override: Option<[u8; 3]>,
}

impl<Message> canvas::Program<Message> for RingGaugeCanvas {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let center = Point::new(bounds.width * 0.5, bounds.height * 0.5);
        let size = bounds.width.min(bounds.height);
        let stroke_w = (size * 0.095).clamp(4.5, 7.2);
        let radius = (size * 0.5 - stroke_w - 4.0).max(8.0);
        let start = 135.0_f32.to_radians();
        let sweep = 270.0_f32.to_radians();
        let firefly = is_firefly_palette(self.palette);
        let cap = if self.palette.square_corners {
            LineCap::Butt
        } else {
            LineCap::Round
        };

        let track_col = if firefly {
            Color::from_rgba8(73, 62, 85, 0.85)
        } else {
            rgba(self.palette.line, 0.65)
        };
        let track = Path::new(|b| {
            b.arc(Arc {
                center,
                radius,
                start_angle: iced::Radians(start),
                end_angle: iced::Radians(start + sweep),
            });
        });
        frame.stroke(
            &track,
            Stroke::default()
                .with_color(track_col)
                .with_width(stroke_w)
                .with_line_cap(cap),
        );

        let dot_r = (radius - stroke_w * 0.5 - 4.8).max(3.0);
        let dot_col = rgba(self.palette.ink_3, 0.45);
        for i in 0..=4 {
            let a = start + sweep * (i as f32 / 4.0);
            let pt = Point::new(center.x + a.cos() * dot_r, center.y + a.sin() * dot_r);
            frame.fill(&Path::circle(pt, 1.15), dot_col);
        }

        let frac = (self.percent / 100.0).clamp(0.02, 1.0);
        let end = start + sweep * frac;
        let col = resolve_semantic_or_accent(self.accent, self.palette, self.accent_override);
        let glow = if self.palette.square_corners { 0.25 } else { 1.0 };

        if glow > 0.05 {
            let glow_start = (end - 1.1).max(start);
            let lead_arc = Path::new(|b| {
                b.arc(Arc {
                    center,
                    radius,
                    start_angle: iced::Radians(glow_start),
                    end_angle: iced::Radians(end),
                });
            });
            for &(extra, a) in &[(10.0_f32, 0.065_f32), (5.0, 0.13)] {
                frame.stroke(
                    &lead_arc,
                    Stroke::default()
                        .with_color(Color { a: a * glow, ..col })
                        .with_width(stroke_w + extra)
                        .with_line_cap(cap),
                );
            }
        }

        let segs = 12;
        for i in 0..segs {
            let f0 = i as f32 / segs as f32;
            let f1 = (i + 1) as f32 / segs as f32;
            let a0 = start + (end - start) * f0;
            let a1 = start + (end - start) * f1;
            let alpha = 0.42 + 0.58 * f1;
            let seg_path = Path::new(|b| {
                b.arc(Arc {
                    center,
                    radius,
                    start_angle: iced::Radians(a0),
                    end_angle: iced::Radians(a1),
                });
            });
            frame.stroke(
                &seg_path,
                Stroke::default()
                    .with_color(Color { a: alpha, ..col })
                    .with_width(stroke_w)
                    .with_line_cap(if i == 0 || i + 1 == segs {
                        cap
                    } else {
                        LineCap::Butt
                    }),
            );
        }

        if glow > 0.05 {
            let tip = Point::new(center.x + end.cos() * radius, center.y + end.sin() * radius);
            fill_radial_glow(
                &mut frame,
                tip,
                stroke_w * 1.9,
                mix_color(col, Color::WHITE, 0.45),
                0.65 * glow,
                5,
            );
            frame.fill(
                &Path::circle(tip, stroke_w * 0.30),
                mix_color(col, Color::WHITE, 0.78),
            );
        }

        vec![frame.into_geometry()]
    }
}

/// Dual-series Living-Light Power Draw chart with cached grid/labels and interactive cursor
/// hover probe (`"CPU 74 W · GPU 112 W"`).
pub struct PowerGraphCanvas<'a> {
    pub cpu_w: &'a [f32],
    pub gpu_w: &'a [f32],
    pub palette: DesignPalette,
    pub accent_override: Option<[u8; 3]>,
    pub anim_t: f32,
}

impl<'a, Message> canvas::Program<Message> for PowerGraphCanvas<'a> {
    type State = CachedLayerState;

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let p = self.palette;
        let firefly = is_firefly_palette(p);
        let (accent, accent_hi) = resolve_accent(p, self.accent_override);
        let w = bounds.width.max(24.0);
        let h = bounds.height.max(24.0);
        let peak = self
            .cpu_w
            .iter()
            .chain(self.gpu_w.iter())
            .copied()
            .fold(250.0_f32, f32::max);
        let max_w: f32 = if peak > 250.0 { 420.0 } else { 250.0 };

        let plot_left = 6.0;
        let plot_top = 8.0;
        let plot_bottom = (h - 16.0).max(plot_top + 10.0);
        let plot_h = plot_bottom - plot_top;
        let plot_right = (w - 58.0).max(plot_left + 16.0);
        let span_w = (plot_right - plot_left - 12.0).max(8.0);

        // Layer 1 (CACHED): Well backdrop, reference lines & wattage labels
        let mut hasher = DefaultHasher::new();
        ((w * 2.0).round() as i32).hash(&mut hasher);
        ((h * 2.0).round() as i32).hash(&mut hasher);
        (max_w as u32).hash(&mut hasher);
        p.well.hash(&mut hasher);
        p.line.hash(&mut hasher);
        p.ink_3.hash(&mut hasher);
        p.square_corners.hash(&mut hasher);
        let key = hasher.finish();
        if state.key.get() != key {
            state.cache.clear();
            state.key.set(key);
        }

        let static_geom = state.cache.draw(renderer, bounds.size(), |frame| {
            let bg = Path::rounded_rectangle(
                Point::ORIGIN,
                bounds.size(),
                iced::border::Radius::from(if p.square_corners { 0.0 } else { 8.0 }),
            );
            frame.fill(&bg, rgb(p.well));

            let mid = (max_w * 0.5).round();
            for watts in [mid, max_w.round()] {
                let y = plot_top + (1.0 - watts / max_w) * plot_h;
                frame.stroke(
                    &Path::line(Point::new(plot_left, y), Point::new(plot_right, y)),
                    Stroke::default()
                        .with_color(rgba(p.line, 0.35))
                        .with_width(1.0),
                );
                frame.fill_text(canvas::Text {
                    content: format!("{watts:.0} W"),
                    position: Point::new((plot_right + 8.0).min(w - 46.0), (y - 6.0).max(2.0)),
                    color: rgb(p.ink_3),
                    size: iced::Pixels(10.0),
                    ..canvas::Text::default()
                });
            }

            frame.fill_text(canvas::Text {
                content: "10 min".to_string(),
                position: Point::new(plot_left, (h - 13.0).max(2.0)),
                color: rgba(p.ink_3, 0.8),
                size: iced::Pixels(9.5),
                ..canvas::Text::default()
            });
        });

        // Layer 2 (DYNAMIC): Curves + interactive cursor hover probe
        let mut frame = Frame::new(renderer, bounds.size());

        let to_pts = |samples: &[f32]| -> Vec<Point> {
            let max_i = (samples.len() - 1) as f32;
            samples
                .iter()
                .enumerate()
                .map(|(i, &val)| {
                    Point::new(
                        plot_left + (i as f32 / max_i) * span_w,
                        plot_top + (1.0 - (val / max_w).clamp(0.0, 1.0)) * plot_h,
                    )
                })
                .collect()
        };

        let cpu_col = if firefly {
            Color::from_rgb8(165, 222, 206)
        } else {
            accent
        };
        let gpu_col = if firefly {
            if let Some([r, g, b]) = self.accent_override {
                mix_color(Color::from_rgb8(r, g, b), Color::from_rgb8(215, 195, 245), 0.35)
            } else {
                Color::from_rgb8(209, 185, 240)
            }
        } else {
            rgb(p.ink_2)
        };
        let glow = if p.square_corners { 0.35 } else { 1.0 };

        let gpu_pts = if self.gpu_w.len() >= 2 {
            let pts = to_pts(self.gpu_w);
            draw_living_light_series(
                &mut frame,
                &pts,
                plot_top,
                plot_bottom,
                plot_left,
                span_w,
                gpu_col,
                0.12,
                1.7,
                true,
                false,
                self.anim_t + 2.1,
                glow * 0.8,
            );
            Some(pts)
        } else {
            None
        };

        let cpu_pts = if self.cpu_w.len() >= 2 {
            let pts = to_pts(self.cpu_w);
            draw_living_light_series(
                &mut frame,
                &pts,
                plot_top,
                plot_bottom,
                plot_left,
                span_w,
                cpu_col,
                0.24,
                2.0,
                false,
                true,
                self.anim_t,
                glow,
            );
            Some(pts)
        } else {
            None
        };

        // Interactive cursor hover probe (hairline + glowing intersection dots + wattage pill)
        if let Some(hpos) = cursor.position_in(bounds)
            && hpos.x >= plot_left
            && hpos.x <= plot_left + span_w
            && (cpu_pts.is_some() || gpu_pts.is_some())
        {
            let frac = ((hpos.x - plot_left) / span_w).clamp(0.0, 1.0);
            let px = plot_left + frac * span_w;

            let probe_g = gradient::Linear::new(
                Point::new(px, plot_top),
                Point::new(px, plot_bottom),
            )
            .add_stop(0.0, rgba(p.ink, 0.12))
            .add_stop(0.4, rgba(p.ink, 0.50))
            .add_stop(1.0, rgba(p.ink, 0.14));
            frame.stroke(
                &Path::line(Point::new(px, plot_top), Point::new(px, plot_bottom)),
                Stroke {
                    style: canvas::Style::Gradient(Gradient::Linear(probe_g)),
                    width: 1.0,
                    ..Stroke::default()
                },
            );

            let mut anchor_y = hpos.y;
            if let Some(pts) = &gpu_pts
                && let Some(pt) = sample_point_at_frac(pts, frac)
            {
                fill_radial_glow(&mut frame, pt, 7.0, gpu_col, 0.50, 3);
                frame.fill(&Path::circle(pt, 2.8), mix_color(gpu_col, Color::WHITE, 0.60));
                anchor_y = pt.y;
            }
            if let Some(pts) = &cpu_pts
                && let Some(pt) = sample_point_at_frac(pts, frac)
            {
                fill_radial_glow(&mut frame, pt, 7.5, cpu_col, 0.55, 3);
                frame.fill(&Path::circle(pt, 3.0), mix_color(cpu_col, Color::WHITE, 0.65));
                anchor_y = anchor_y.min(pt.y);
            }

            let cv = sample_interpolated(self.cpu_w, frac);
            let gv = sample_interpolated(self.gpu_w, frac);
            let tip = match (cv, gv) {
                (Some(c), Some(g)) => format!("CPU {c:.0} W · GPU {g:.0} W"),
                (Some(c), None) => format!("CPU {c:.0} W"),
                (None, Some(g)) => format!("GPU {g:.0} W"),
                (None, None) => String::new(),
            };
            if !tip.is_empty() {
                let pill_h = 19.0;
                let pill_w = (tip.len() as f32 * 5.7 + 14.0).min(w - 12.0);
                let tx = if px + 10.0 + pill_w > plot_right - 2.0 {
                    (px - pill_w - 8.0).max(4.0)
                } else {
                    px + 8.0
                };
                let ty = (anchor_y - pill_h - 6.0)
                    .clamp(plot_top, (plot_bottom - pill_h - 2.0).max(plot_top));
                let pill = Path::rounded_rectangle(
                    Point::new(tx, ty),
                    Size::new(pill_w, pill_h),
                    iced::border::Radius::from(5.5),
                );
                frame.fill(&pill, rgba(p.raised, 0.94));
                frame.stroke(
                    &pill,
                    Stroke::default()
                        .with_color(Color { a: 0.68, ..accent })
                        .with_width(1.0),
                );
                frame.fill_text(canvas::Text {
                    content: tip,
                    position: Point::new(tx + 7.0, ty + 3.0),
                    color: accent_hi,
                    size: iced::Pixels(10.0),
                    ..canvas::Text::default()
                });
            }
        }

        vec![static_geom, frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.position_in(bounds).is_some()
            && (self.cpu_w.len() >= 2 || self.gpu_w.len() >= 2)
        {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::default()
        }
    }
}

/// Luminous light-tube meter bar (Firefly / rounded) or crisp discrete tick bar (console themes).
pub struct SegmentedBarCanvas {
    pub fraction: f32,
    pub accent: u32,
    pub palette: DesignPalette,
    pub accent_override: Option<[u8; 3]>,
    pub segments: usize,
}

impl<Message> canvas::Program<Message> for SegmentedBarCanvas {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let p = self.palette;
        let frac = self.fraction.clamp(0.0, 1.0);
        let col = resolve_semantic_or_accent(self.accent, p, self.accent_override);
        let h = bounds.height.max(6.0);
        let w = bounds.width.max(12.0);

        if !p.square_corners {
            let rad = iced::border::Radius::from(h * 0.5);
            let trough = Path::rounded_rectangle(Point::ORIGIN, Size::new(w, h), rad);
            frame.fill(&trough, rgb(p.well));
            frame.stroke(
                &trough,
                Stroke::default()
                    .with_color(rgba(p.line, 0.45))
                    .with_width(0.8),
            );

            let count = self.segments.max(12);
            let inner_pad = 2.0;
            let inner_w = (w - inner_pad * 2.0).max(4.0);
            let inner_h = (h - inner_pad * 2.0).max(3.0);
            let step = inner_w / count as f32;
            let bar_w = (step * 0.68).max(2.0);
            let lit = (frac * count as f32).round() as usize;

            for i in 0..count {
                let x = inner_pad + i as f32 * step;
                let seg_rect = Path::rounded_rectangle(
                    Point::new(x, inner_pad),
                    Size::new(bar_w, inner_h),
                    iced::border::Radius::from((bar_w * 0.45).min(inner_h * 0.5)),
                );
                if i < lit {
                    let progress = if lit > 1 {
                        i as f32 / (lit - 1) as f32
                    } else {
                        1.0
                    };
                    let seg_col = mix_color(
                        Color { a: 0.58, ..col },
                        mix_color(col, Color::WHITE, 0.25),
                        progress,
                    );
                    frame.fill(&seg_rect, seg_col);
                    frame.fill_rectangle(
                        Point::new(x + 0.5, inner_pad + 0.5),
                        Size::new((bar_w - 1.0).max(1.0), (inner_h * 0.30).max(1.0)),
                        Color::from_rgba(1.0, 1.0, 1.0, 0.24 * progress),
                    );
                } else {
                    frame.fill(&seg_rect, rgba(p.line, 0.28));
                }
            }

            if lit > 0 {
                let lead_x =
                    (inner_pad + (lit as f32 - 0.5) * step).clamp(inner_pad, w - inner_pad);
                fill_radial_glow(
                    &mut frame,
                    Point::new(lead_x, h * 0.5),
                    h * 1.1,
                    mix_color(col, Color::WHITE, 0.4),
                    0.42,
                    4,
                );
            }
        } else {
            let count = self.segments.max(8);
            let step = w / count as f32;
            let bar_w = (step * 0.58).max(1.5);
            let lit = (frac * count as f32).round() as usize;
            for i in 0..count {
                let x = i as f32 * step;
                let c = if i < lit { col } else { rgba(p.line, 0.52) };
                frame.fill_rectangle(
                    Point::new(x, 1.0),
                    Size::new(bar_w, (h - 2.0).max(4.0)),
                    c,
                );
            }
        }

        vec![frame.into_geometry()]
    }
}

/// Live-animated miniature gallery artwork swatch for all 16 built-in effects + Studio effects.
pub struct EffectThumbCanvas {
    pub effect: EffectId,
    pub color: [u8; 3],
    pub secondary: [u8; 3],
    pub palette: DesignPalette,
    pub accent_override: Option<[u8; 3]>,
    pub anim_t: f32,
    pub selected: bool,
}

impl EffectThumbCanvas {
    /// Sets bespoke primary & secondary colours for a Studio preset thumbnail so each
    /// Saved Studio Look displays its own unique glowing colour signature.
    pub fn with_custom_colors(mut self, primary: [u8; 3], secondary: [u8; 3]) -> Self {
        self.color = primary;
        self.secondary = secondary;
        self
    }
}

impl<Message> canvas::Program<Message> for EffectThumbCanvas {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let w = bounds.width.max(10.0);
        let h = bounds.height.max(10.0);
        let t = self.anim_t;
        let (_, accent_hi) = resolve_accent(self.palette, self.accent_override);
        let rad = iced::border::Radius::from(if self.palette.square_corners { 0.0 } else { 6.0 });

        let bg = Path::rounded_rectangle(Point::ORIGIN, bounds.size(), rad);
        frame.fill(&bg, Color::from_rgb8(8, 8, 14));

        let c1 = Color::from_rgb8(self.color[0], self.color[1], self.color[2]);
        let c2 = Color::from_rgb8(self.secondary[0], self.secondary[1], self.secondary[2]);

        match self.effect {
            EffectId::Solid => {
                let g = gradient::Linear::new(Point::ORIGIN, Point::new(0.0, h))
                    .add_stop(0.0, mix_color(c1, Color::WHITE, 0.16))
                    .add_stop(0.55, c1)
                    .add_stop(1.0, mix_color(c1, Color::BLACK, 0.22));
                frame.fill(&bg, Fill::from(Gradient::Linear(g)));
            }
            EffectId::Breathing => {
                // Full-frame lush ambient breathing wash + expanding luminous halo rings
                let breath = 0.5 + 0.5 * (t * 1.9).sin();
                let c_deep = mix_color(c1, Color::from_rgb8(14, 10, 26), 0.72);
                let bg_g = gradient::Linear::new(Point::ORIGIN, Point::new(w, h))
                    .add_stop(0.0, c_deep)
                    .add_stop(0.5, mix_color(c1, c_deep, 0.45 - 0.25 * breath))
                    .add_stop(1.0, c_deep);
                frame.fill(&bg, Fill::from(Gradient::Linear(bg_g)));

                let center = Point::new(w * 0.5, h * 0.5);
                // Wide horizontal ambient wing glows filling the tile
                fill_radial_glow(
                    &mut frame,
                    Point::new(w * 0.28, h * 0.5),
                    h * (0.72 + 0.28 * breath),
                    c1,
                    0.28 + 0.28 * breath,
                    5,
                );
                fill_radial_glow(
                    &mut frame,
                    Point::new(w * 0.72, h * 0.5),
                    h * (0.72 + 0.28 * breath),
                    mix_color(c1, Color::from_rgb8(244, 166, 206), 0.25),
                    0.26 + 0.26 * breath,
                    5,
                );
                fill_radial_glow(
                    &mut frame,
                    center,
                    h * (0.58 + 0.45 * breath),
                    mix_color(c1, Color::WHITE, 0.22),
                    0.50 + 0.42 * breath,
                    6,
                );
                // Soft concentric breathing aura rings
                for &(mult, alpha) in &[(0.40_f32, 0.34_f32), (0.64, 0.18)] {
                    frame.stroke(
                        &Path::circle(center, h * (mult + 0.16 * breath)),
                        Stroke::default()
                            .with_color(Color {
                                a: alpha * (0.45 + 0.55 * breath),
                                ..mix_color(c1, Color::WHITE, 0.45)
                            })
                            .with_width(1.2),
                    );
                }
                frame.fill(
                    &Path::circle(center, h * (0.16 + 0.12 * breath)),
                    mix_color(c1, Color::WHITE, 0.48 + 0.25 * breath),
                );
            }
            EffectId::Cycle => {
                let h0 = (t * 0.14).fract();
                let h1 = (h0 + 0.18).fract();
                let [r0, g0, b0] = airglow_core::effects::hsv_to_rgb(h0, 0.92, 0.95);
                let [r1, g1, b1] = airglow_core::effects::hsv_to_rgb(h1, 0.92, 0.85);
                let g = gradient::Linear::new(Point::ORIGIN, Point::new(w, h))
                    .add_stop(0.0, Color::from_rgb(r0, g0, b0))
                    .add_stop(1.0, Color::from_rgb(r1, g1, b1));
                frame.fill(&bg, Fill::from(Gradient::Linear(g)));
            }
            EffectId::Rainbow => {
                let steps = 24;
                let sw = w / steps as f32;
                for i in 0..steps {
                    let hue = ((i as f32 / steps as f32) + t * 0.32).fract();
                    let [r, g, b] = airglow_core::effects::hsv_to_rgb(hue, 0.95, 0.96);
                    frame.fill_rectangle(
                        Point::new(i as f32 * sw, 0.0),
                        Size::new(sw + 1.0, h),
                        Color::from_rgb(r, g, b),
                    );
                }
            }
            EffectId::Gradient => {
                let shift = 0.5 + 0.35 * (t * 1.1).sin();
                let g = gradient::Linear::new(Point::new(0.0, 0.0), Point::new(w, h * 0.85))
                    .add_stop(0.0, c1)
                    .add_stop(shift.clamp(0.15, 0.85), mix_color(c1, c2, 0.5))
                    .add_stop(1.0, c2);
                frame.fill(&bg, Fill::from(Gradient::Linear(g)));
            }
            EffectId::Comet => {
                // Full-width sweeping orbital comet with lush glowing crescent tail + ember sparks
                let sky = gradient::Linear::new(Point::ORIGIN, Point::new(w, h))
                    .add_stop(0.0, Color::from_rgb8(10, 12, 24))
                    .add_stop(0.5, mix_color(Color::from_rgb8(14, 16, 30), c1, 0.12))
                    .add_stop(1.0, Color::from_rgb8(12, 10, 22));
                frame.fill(&bg, Fill::from(Gradient::Linear(sky)));

                let cx = w * 0.5;
                let cy = h * 0.52;
                let rx = w * 0.37;
                let ry = h * 0.33;
                let orbit_pt = |a: f32| Point::new(cx + a.cos() * rx, cy + a.sin() * ry);

                // Subtle illuminated orbital ring
                let track = Path::new(|b| {
                    let steps = 32;
                    for i in 0..=steps {
                        let a = (i as f32 / steps as f32) * 2.0 * PI;
                        let pt = orbit_pt(a);
                        if i == 0 {
                            b.move_to(pt);
                        } else {
                            b.line_to(pt);
                        }
                    }
                    b.close();
                });
                frame.stroke(
                    &track,
                    Stroke::default()
                        .with_color(Color { a: 0.16, ..c1 })
                        .with_width(1.6),
                );

                let head_a = t * 2.6;
                let segs = 22;
                let tail_arc = 3.6_f32;
                let accent_tail = mix_color(c1, Color::from_rgb8(255, 196, 132), 0.32);

                // Primary sweeping comet with soft outer glow + bright inner streak
                for s in 0..segs {
                    let f0 = s as f32 / segs as f32;
                    let f1 = (s + 1) as f32 / segs as f32;
                    let a0 = head_a - tail_arc * (1.0 - f0);
                    let a1 = head_a - tail_arc * (1.0 - f1);
                    let p0 = orbit_pt(a0);
                    let p1 = orbit_pt(a1);
                    let col = mix_color(c1, Color::WHITE, f1 * f1 * 0.65);
                    let power = f1.powf(1.5);
                    frame.stroke(
                        &Path::line(p0, p1),
                        Stroke::default()
                            .with_color(Color {
                                a: power * 0.28,
                                ..c1
                            })
                            .with_width(3.0 + 4.5 * f1)
                            .with_line_cap(LineCap::Round),
                    );
                    frame.stroke(
                        &Path::line(p0, p1),
                        Stroke::default()
                            .with_color(Color {
                                a: power * 0.96,
                                ..col
                            })
                            .with_width(1.2 + 2.4 * f1)
                            .with_line_cap(LineCap::Round),
                    );
                }

                // Subtle counter-orbiting warm echo comet for rich depth
                let echo_a = head_a + PI;
                for s in 0..10 {
                    let f0 = s as f32 / 10.0;
                    let f1 = (s + 1) as f32 / 10.0;
                    let p0 = orbit_pt(echo_a - 1.6 * (1.0 - f0));
                    let p1 = orbit_pt(echo_a - 1.6 * (1.0 - f1));
                    frame.stroke(
                        &Path::line(p0, p1),
                        Stroke::default()
                            .with_color(Color {
                                a: f1 * f1 * 0.52,
                                ..accent_tail
                            })
                            .with_width(1.0 + 1.4 * f1)
                            .with_line_cap(LineCap::Round),
                    );
                }

                // Bright flaring head + trailing ember sparks
                let head_pt = orbit_pt(head_a);
                fill_radial_glow(&mut frame, head_pt, 10.5, c1, 0.80, 5);
                frame.fill(&Path::circle(head_pt, 2.7), Color::WHITE);
                for k in 1..=3 {
                    let sa = head_a - 0.28 * k as f32;
                    let sp = orbit_pt(sa);
                    let jitter_y = if k % 2 == 0 { -2.2 } else { 2.0 };
                    frame.fill(
                        &Path::circle(Point::new(sp.x, sp.y + jitter_y), 1.15),
                        Color {
                            a: 0.72 - k as f32 * 0.16,
                            ..mix_color(accent_tail, Color::WHITE, 0.5)
                        },
                    );
                }
            }
            EffectId::Aurora => {
                // Polar twilight indigo-violet sky + 3 undulating emerald/rose/violet curtains + twinkling stars
                let sky = gradient::Linear::new(Point::ORIGIN, Point::new(0.0, h))
                    .add_stop(0.0, Color::from_rgb8(9, 12, 28))
                    .add_stop(1.0, Color::from_rgb8(14, 24, 38));
                frame.fill(&bg, Fill::from(Gradient::Linear(sky)));

                let ribbons = [
                    (0.46_f32, 0.16_f32, 1.15_f32, 0.0_f32, Color::from_rgb8(144, 92, 255), 0.42_f32),
                    (0.56_f32, 0.18_f32, 0.95_f32, 1.7_f32, Color::from_rgb8(244, 124, 194), 0.44_f32),
                    (0.64_f32, 0.15_f32, 1.30_f32, 3.1_f32, Color::from_rgb8(76, 242, 192), 0.58_f32),
                ];
                let samples = 18;
                for (y_base, amp, spd, phase, col, alpha) in ribbons {
                    let mut crest = Vec::with_capacity(samples + 1);
                    for i in 0..=samples {
                        let fx = i as f32 / samples as f32;
                        let wave = (fx * 4.8 + t * spd + phase).sin() * amp
                            + (fx * 9.2 - t * spd * 0.7 + phase).cos() * (amp * 0.35);
                        let py = ((y_base + wave) * h).clamp(4.0, h - 4.0);
                        crest.push(Point::new(fx * w, py));
                    }
                    let curtain = Path::new(|b| {
                        b.move_to(Point::new(0.0, 0.0));
                        for &pt in &crest {
                            b.line_to(pt);
                        }
                        b.line_to(Point::new(w, 0.0));
                        b.close();
                    });
                    let cg = gradient::Linear::new(Point::new(0.0, 2.0), Point::new(0.0, h * 0.92))
                        .add_stop(0.0, Color { a: 0.02, ..col })
                        .add_stop(0.55, Color { a: alpha * 0.55, ..col })
                        .add_stop(1.0, Color { a: alpha, ..col });
                    frame.fill(&curtain, Fill::from(Gradient::Linear(cg)));

                    let ridge = Path::new(|b| {
                        b.move_to(crest[0]);
                        for &pt in &crest[1..] {
                            b.line_to(pt);
                        }
                    });
                    frame.stroke(
                        &ridge,
                        Stroke::default()
                            .with_color(Color {
                                a: (alpha * 1.2).clamp(0.0, 0.9),
                                ..mix_color(col, Color::WHITE, 0.35)
                            })
                            .with_width(1.6)
                            .with_line_cap(LineCap::Round),
                    );
                }

                // Twinkling polar micro-stars
                for i in 0..8 {
                    let sx = ((i * 29 + 11) % 88 + 6) as f32 / 100.0 * w;
                    let sy = ((i * 19 + 7) % 44 + 8) as f32 / 100.0 * h;
                    let tw = 0.3 + 0.7 * (0.5 + 0.5 * (t * (1.8 + i as f32 * 0.3) + i as f32).sin());
                    frame.fill(
                        &Path::circle(Point::new(sx, sy), 0.95),
                        Color::from_rgba(1.0, 0.98, 1.0, 0.78 * tw),
                    );
                }
            }
            EffectId::FireflySwarm => {
                // Dusk plum meadow sky + drifting glowing golden/rose/mint fireflies
                let dusk = gradient::Linear::new(Point::ORIGIN, Point::new(0.0, h))
                    .add_stop(0.0, Color::from_rgb8(24, 18, 38))
                    .add_stop(1.0, Color::from_rgb8(14, 12, 24));
                frame.fill(&bg, Fill::from(Gradient::Linear(dusk)));

                let bugs = [
                    (0.22_f32, 0.52_f32, 0.14_f32, 0.22_f32, 1.1_f32, 0.2_f32, Color::from_rgb8(255, 212, 124)),
                    (0.48_f32, 0.36_f32, 0.16_f32, 0.20_f32, 0.9_f32, 1.8_f32, Color::from_rgb8(248, 166, 200)),
                    (0.74_f32, 0.58_f32, 0.13_f32, 0.24_f32, 1.25_f32, 3.4_f32, Color::from_rgb8(255, 200, 118)),
                    (0.36_f32, 0.72_f32, 0.15_f32, 0.16_f32, 0.85_f32, 4.9_f32, Color::from_rgb8(165, 232, 210)),
                    (0.62_f32, 0.32_f32, 0.14_f32, 0.22_f32, 1.05_f32, 2.5_f32, Color::from_rgb8(255, 224, 152)),
                    (0.84_f32, 0.34_f32, 0.09_f32, 0.20_f32, 1.15_f32, 0.9_f32, Color::from_rgb8(244, 156, 194)),
                ];
                for (bx, by, ax, ay, spd, ph, col) in bugs {
                    let x = (bx + (t * spd + ph).sin() * ax) * w;
                    let y = (by + (t * spd * 1.25 + ph).cos() * ay) * h;
                    let tx = (bx + ((t - 0.18) * spd + ph).sin() * ax) * w;
                    let ty = (by + ((t - 0.18) * spd * 1.25 + ph).cos() * ay) * h;
                    let pulse = 0.35 + 0.65 * (0.5 + 0.5 * (t * spd * 2.1 + ph).sin()).powf(1.4);
                    // Faint glowing drift trail
                    frame.stroke(
                        &Path::line(Point::new(tx, ty), Point::new(x, y)),
                        Stroke::default()
                            .with_color(Color {
                                a: 0.24 * pulse,
                                ..col
                            })
                            .with_width(1.2)
                            .with_line_cap(LineCap::Round),
                    );
                    fill_radial_glow(&mut frame, Point::new(x, y), h * 0.28, col, 0.55 * pulse, 4);
                    frame.fill(
                        &Path::circle(Point::new(x, y), 1.9),
                        Color {
                            a: 0.95 * pulse,
                            ..mix_color(col, Color::WHITE, 0.65)
                        },
                    );
                }
            }
            EffectId::MeteorShower => {
                // Deep starfield + 4 diagonally streaking comets with glowing ember trails
                let sky = gradient::Linear::new(Point::ORIGIN, Point::new(w, h))
                    .add_stop(0.0, Color::from_rgb8(8, 10, 22))
                    .add_stop(1.0, Color::from_rgb8(18, 12, 28));
                frame.fill(&bg, Fill::from(Gradient::Linear(sky)));

                for i in 0..9 {
                    let sx = ((i * 31 + 9) % 92 + 4) as f32 / 100.0 * w;
                    let sy = ((i * 43 + 17) % 86 + 7) as f32 / 100.0 * h;
                    frame.fill(
                        &Path::circle(Point::new(sx, sy), 0.75),
                        Color::from_rgba(0.95, 0.95, 1.0, 0.35),
                    );
                }

                let meteors = [
                    (0.18_f32, 0.0_f32, 0.65_f32, Color::from_rgb8(255, 198, 122)),
                    (0.55_f32, 0.32_f32, 0.78_f32, Color::from_rgb8(255, 126, 178)),
                    (0.84_f32, 0.61_f32, 0.58_f32, Color::from_rgb8(136, 226, 255)),
                    (0.38_f32, 0.80_f32, 0.72_f32, Color::from_rgb8(255, 168, 106)),
                ];
                for (start_x, offset, spd, col) in meteors {
                    let u = (t * spd + offset).fract();
                    let env = (u * PI).sin();
                    let head = Point::new(
                        (start_x - 0.10 + u * 0.48) * w,
                        (-0.10 + u * 1.18) * h,
                    );
                    let tail = Point::new(head.x - w * 0.22, head.y - h * 0.44);
                    let trail_g = gradient::Linear::new(tail, head)
                        .add_stop(0.0, Color { a: 0.0, ..col })
                        .add_stop(0.65, Color { a: 0.55 * env, ..col })
                        .add_stop(1.0, Color { a: 0.95 * env, ..mix_color(col, Color::WHITE, 0.7) });
                    frame.stroke(
                        &Path::line(tail, head),
                        Stroke {
                            style: canvas::Style::Gradient(Gradient::Linear(trail_g)),
                            width: 2.1,
                            line_cap: LineCap::Round,
                            ..Stroke::default()
                        },
                    );
                    fill_radial_glow(&mut frame, head, 6.0, col, 0.65 * env, 4);
                    frame.fill(
                        &Path::circle(head, 1.9),
                        Color::from_rgba(1.0, 0.98, 0.94, 0.95 * env),
                    );
                }
            }
            EffectId::Heatmap => {
                // Smooth spatial thermal contour field shifting from cool mint/indigo to apricot/coral
                let base_g = gradient::Linear::new(Point::ORIGIN, Point::new(w, h * 0.4))
                    .add_stop(0.0, Color::from_rgb8(18, 42, 76))
                    .add_stop(0.45, Color::from_rgb8(42, 138, 146))
                    .add_stop(0.78, Color::from_rgb8(242, 152, 74))
                    .add_stop(1.0, Color::from_rgb8(236, 72, 78));
                frame.fill(&bg, Fill::from(Gradient::Linear(base_g)));

                let hx = w * (0.68 + 0.08 * (t * 0.9).sin());
                let hy = h * (0.46 + 0.10 * (t * 0.7).cos());
                let hot_pt = Point::new(hx, hy);
                fill_radial_glow(
                    &mut frame,
                    hot_pt,
                    h * 0.72,
                    Color::from_rgb8(255, 86, 62),
                    0.72,
                    5,
                );
                fill_radial_glow(
                    &mut frame,
                    Point::new(w * 0.24, h * 0.54),
                    h * 0.65,
                    Color::from_rgb8(76, 224, 206),
                    0.48,
                    4,
                );
                // Crisp delicate thermal contour rings around hotspot
                for ring in [0.24_f32, 0.44, 0.64] {
                    frame.stroke(
                        &Path::circle(hot_pt, h * ring),
                        Stroke::default()
                            .with_color(Color::from_rgba(1.0, 0.95, 0.88, 0.28 - ring * 0.22))
                            .with_width(0.9),
                    );
                }
            }
            EffectId::Temperature => {
                let grad = gradient::Linear::new(Point::ORIGIN, Point::new(w, 0.0))
                    .add_stop(0.0, Color::from_rgb8(0, 180, 255))
                    .add_stop(0.5, Color::from_rgb8(138, 92, 255))
                    .add_stop(1.0, Color::from_rgb8(255, 38, 12));
                frame.fill(&bg, Fill::from(Gradient::Linear(grad)));
                let sweep = 0.5 - 0.5 * (t * 1.4).cos();
                let nx = w * (0.14 + 0.72 * sweep);
                frame.stroke(
                    &Path::line(Point::new(nx, 5.0), Point::new(nx, h - 5.0)),
                    Stroke::default()
                        .with_color(Color::from_rgba(1.0, 1.0, 1.0, 0.30))
                        .with_width(5.0)
                        .with_line_cap(LineCap::Round),
                );
                frame.stroke(
                    &Path::line(Point::new(nx, 5.0), Point::new(nx, h - 5.0)),
                    Stroke::default()
                        .with_color(Color::WHITE)
                        .with_width(2.0)
                        .with_line_cap(LineCap::Round),
                );
            }
            EffectId::Screen => {
                let p0 = 0.72 + 0.28 * (t * 1.3).sin();
                let p1 = 0.72 + 0.28 * (t * 1.3 + 1.6).sin();
                let p2 = 0.72 + 0.28 * (t * 1.3 + 3.1).sin();
                let p3 = 0.72 + 0.28 * (t * 1.3 + 4.7).sin();
                fill_radial_glow(
                    &mut frame,
                    Point::new(w * 0.20, h * 0.50),
                    h * 0.52,
                    Color::from_rgb8(255, 95, 36),
                    0.55 * p0,
                    4,
                );
                fill_radial_glow(
                    &mut frame,
                    Point::new(w * 0.80, h * 0.50),
                    h * 0.52,
                    Color::from_rgb8(47, 130, 255),
                    0.55 * p1,
                    4,
                );
                fill_radial_glow(
                    &mut frame,
                    Point::new(w * 0.50, h * 0.16),
                    h * 0.42,
                    Color::from_rgb8(34, 224, 132),
                    0.45 * p2,
                    4,
                );
                fill_radial_glow(
                    &mut frame,
                    Point::new(w * 0.50, h * 0.84),
                    h * 0.42,
                    Color::from_rgb8(168, 72, 255),
                    0.45 * p3,
                    4,
                );
                let mon = Path::rounded_rectangle(
                    Point::new(w * 0.23, h * 0.21),
                    Size::new(w * 0.54, h * 0.58),
                    iced::border::Radius::from(3.5),
                );
                frame.fill(&mon, Color::from_rgb8(24, 26, 34));
                frame.stroke(
                    &mon,
                    Stroke::default()
                        .with_color(Color::from_rgba8(230, 232, 240, 0.45))
                        .with_width(1.1),
                );
            }
            EffectId::Spectrum => {
                let bars = 8;
                let pad_x = 7.0;
                let gap = 2.5;
                let bw = ((w - pad_x * 2.0) - gap * (bars as f32 - 1.0)) / bars as f32;
                let max_h = (h - 10.0).max(6.0);
                for i in 0..bars {
                    let phase = t * 4.2 + i as f32 * 0.72;
                    let level = (0.24
                        + 0.44 * (0.5 + 0.5 * phase.sin())
                        + 0.26 * (0.5 + 0.5 * (phase * 1.7 + 0.9).cos()))
                    .clamp(0.16, 0.96);
                    let bh = max_h * level;
                    let x = pad_x + i as f32 * (bw + gap);
                    let y = h - 5.0 - bh;
                    let [r, g, b] = airglow_core::effects::hsv_to_rgb(
                        (0.62 - (i as f32 / bars as f32) * 0.56).clamp(0.0, 1.0),
                        0.95,
                        1.0,
                    );
                    let bar_col = Color::from_rgb(r, g, b);
                    frame.fill(
                        &Path::rounded_rectangle(
                            Point::new(x, y),
                            Size::new(bw.max(2.0), bh),
                            iced::border::Radius::from(1.2),
                        ),
                        bar_col,
                    );
                    frame.fill_rectangle(
                        Point::new(x, (y - 2.5).max(3.0)),
                        Size::new(bw.max(2.0), 1.5),
                        mix_color(bar_col, Color::WHITE, 0.65),
                    );
                }
            }
            EffectId::Pulse => {
                // Full-width beat soundwave rings + symmetrical acoustic wave + glowing beat core
                let beat_t = (t * 2.0).fract();
                let decay = (1.0 - beat_t).powf(2.0);
                let center = Point::new(w * 0.5, h * 0.5);
                let secondary_wave = mix_color(c1, Color::from_rgb8(244, 114, 182), 0.45);

                let bg_g = gradient::Linear::new(Point::ORIGIN, Point::new(w, h))
                    .add_stop(0.0, mix_color(Color::from_rgb8(12, 10, 24), c1, 0.14))
                    .add_stop(0.5, mix_color(Color::from_rgb8(18, 12, 34), secondary_wave, 0.22 + 0.18 * decay))
                    .add_stop(1.0, mix_color(Color::from_rgb8(12, 10, 24), c1, 0.14));
                frame.fill(&bg, Fill::from(Gradient::Linear(bg_g)));

                // Symmetrical horizontal acoustic waveform across full tile width
                let wave_steps = 28;
                let wave_path = Path::new(|b| {
                    for i in 0..=wave_steps {
                        let fx = i as f32 / wave_steps as f32;
                        let env = (fx * PI).sin().powf(1.4);
                        let amp = h * (0.14 + 0.24 * decay) * env;
                        let y = h * 0.5 + (fx * 16.0 - t * 6.0).sin() * amp;
                        let pt = Point::new(fx * w, y);
                        if i == 0 {
                            b.move_to(pt);
                        } else {
                            b.line_to(pt);
                        }
                    }
                });
                frame.stroke(
                    &wave_path,
                    Stroke::default()
                        .with_color(Color {
                            a: 0.24 + 0.28 * decay,
                            ..c1
                        })
                        .with_width(3.2)
                        .with_line_cap(LineCap::Round),
                );
                frame.stroke(
                    &wave_path,
                    Stroke::default()
                        .with_color(Color {
                            a: 0.65 + 0.30 * decay,
                            ..mix_color(c1, Color::WHITE, 0.45)
                        })
                        .with_width(1.2)
                        .with_line_cap(LineCap::Round),
                );

                // 3 staggered expanding concentric soundwave ripples
                for k in 0..3 {
                    let phase = (beat_t + k as f32 / 3.0).fract();
                    let ring_r = h * (0.16 + 0.72 * phase);
                    let ring_col = if k % 2 == 0 { c1 } else { secondary_wave };
                    frame.stroke(
                        &Path::circle(center, ring_r),
                        Stroke::default()
                            .with_color(Color {
                                a: (1.0 - phase).powf(1.6) * 0.72,
                                ..mix_color(ring_col, Color::WHITE, 0.30)
                            })
                            .with_width(1.4),
                    );
                }

                let base_r = h * (0.20 + 0.14 * decay);
                fill_radial_glow(&mut frame, center, h * 0.68, c1, 0.52 + 0.38 * decay, 5);
                frame.fill(
                    &Path::circle(center, base_r),
                    mix_color(c1, Color::WHITE, 0.46 + 0.28 * decay),
                );
            }
            EffectId::Studio => {
                // Full-width multi-layer composited artwork driven by bespoke primary/secondary colors
                let lum_of = |col: Color| 0.299 * col.r + 0.587 * col.g + 0.114 * col.b;
                let l1 = lum_of(c1);
                let l2 = lum_of(c2);

                // Ensure both ribbons stay luminous even if a preset has a very dark base or bg color
                let ribbon_a = if l1 < 0.14 && l2 >= 0.18 {
                    mix_color(c2, Color::from_rgb8(255, 110, 42), 0.45)
                } else if l1 < 0.14 {
                    Color::from_rgb8(0, 232, 206)
                } else {
                    c1
                };
                let ribbon_b = if l2 < 0.14 {
                    // Harmonic lively companion when secondary is near-black (e.g. default scene bg)
                    if ribbon_a.r > ribbon_a.b + 0.25 {
                        Color::from_rgb8(138, 86, 255)
                    } else {
                        Color::from_rgb8(255, 72, 186)
                    }
                } else {
                    c2
                };
                let ribbon_mid = mix_color(
                    mix_color(ribbon_a, ribbon_b, 0.5),
                    Color::WHITE,
                    0.22,
                );
                let deep_a = mix_color(Color::from_rgb8(12, 10, 24), ribbon_a, 0.34);
                let deep_b = mix_color(Color::from_rgb8(14, 10, 26), ribbon_b, 0.34);
                let deep_mid = mix_color(Color::from_rgb8(16, 12, 30), ribbon_mid, 0.28);

                let base_g = gradient::Linear::new(Point::ORIGIN, Point::new(w, h))
                    .add_stop(0.0, deep_a)
                    .add_stop(0.5, deep_mid)
                    .add_stop(1.0, deep_b);
                frame.fill(&bg, Fill::from(Gradient::Linear(base_g)));

                // Overlapping luminous plasma blobs across the full width
                let blobs = [
                    (0.18 + 0.10 * (t * 0.9).sin(), 0.48 + 0.14 * (t * 0.7).cos(), h * 0.66, ribbon_a),
                    (0.50 + 0.12 * (t * 0.8 + 1.5).cos(), 0.46 + 0.14 * (t * 1.0).sin(), h * 0.62, ribbon_mid),
                    (0.82 + 0.10 * (t * 0.65 + 2.4).sin(), 0.54 + 0.14 * (t * 0.9).cos(), h * 0.64, ribbon_b),
                ];
                for (nx, ny, r, col) in blobs {
                    fill_radial_glow(&mut frame, Point::new(w * nx, h * ny), r, col, 0.54, 5);
                }

                // Flowing dual-ribbon artwork + blended highlight mid-wave
                for (phase, amp, col, stroke_w, alpha) in [
                    (1.05_f32, 0.15_f32, ribbon_mid, 1.0_f32, 0.42_f32),
                    (0.0_f32, 0.22_f32, ribbon_a, 1.35_f32, 0.92_f32),
                    (2.15_f32, 0.20_f32, ribbon_b, 1.35_f32, 0.92_f32),
                ] {
                    let ribbon = Path::new(|b| {
                        let steps = 22;
                        for i in 0..=steps {
                            let fx = i as f32 / steps as f32;
                            let y = h * (0.50 + amp * (fx * 5.2 + t * 1.4 + phase).sin());
                            let pt = Point::new(fx * w, y);
                            if i == 0 {
                                b.move_to(pt);
                            } else {
                                b.line_to(pt);
                            }
                        }
                    });
                    frame.stroke(
                        &ribbon,
                        Stroke::default()
                            .with_color(Color {
                                a: 0.34 * alpha,
                                ..col
                            })
                            .with_width(stroke_w * 3.1)
                            .with_line_cap(LineCap::Round),
                    );
                    frame.stroke(
                        &ribbon,
                        Stroke::default()
                            .with_color(Color {
                                a: alpha,
                                ..mix_color(col, Color::WHITE, 0.48)
                            })
                            .with_width(stroke_w)
                            .with_line_cap(LineCap::Round),
                    );
                }

                // Crisp top sparkle layer tinted by the blended preset highlight
                for i in 0..7 {
                    let sx = ((i * 23 + 11) % 86 + 7) as f32 / 100.0 * w;
                    let sy = ((i * 37 + 15) % 74 + 13) as f32 / 100.0 * h;
                    let tw =
                        0.35 + 0.65 * (0.5 + 0.5 * (t * (2.2 + i as f32 * 0.35) + i as f32).sin());
                    let pt = Point::new(sx, sy);
                    let sp_col = if i % 2 == 0 { ribbon_a } else { ribbon_b };
                    frame.fill(
                        &Path::circle(pt, 1.35),
                        Color {
                            a: 0.90 * tw,
                            ..mix_color(sp_col, Color::WHITE, 0.68)
                        },
                    );
                }
            }
            EffectId::Off => {
                let c = Point::new(w * 0.5, h * 0.5);
                let r = h * 0.25;
                let off_col = rgba(self.palette.ink_3, 0.65);
                let arc = Path::new(|b| {
                    b.arc(Arc {
                        center: c,
                        radius: r,
                        start_angle: iced::Radians(-60.0_f32.to_radians()),
                        end_angle: iced::Radians(240.0_f32.to_radians()),
                    });
                });
                frame.stroke(
                    &arc,
                    Stroke::default()
                        .with_color(off_col)
                        .with_width(1.6)
                        .with_line_cap(LineCap::Round),
                );
                frame.stroke(
                    &Path::line(
                        Point::new(c.x, c.y - r * 1.15),
                        Point::new(c.x, c.y - r * 0.2),
                    ),
                    Stroke::default()
                        .with_color(off_col)
                        .with_width(1.6)
                        .with_line_cap(LineCap::Round),
                );
            }
        }

        // Crisp subtle inner edge + selected accent outline
        frame.stroke(
            &bg,
            Stroke::default()
                .with_color(if self.selected {
                    Color {
                        a: 0.90,
                        ..accent_hi
                    }
                } else {
                    Color::from_rgba(1.0, 1.0, 1.0, 0.08)
                })
                .with_width(if self.selected { 1.6 } else { 1.0 }),
        );

        vec![frame.into_geometry()]
    }
}

/// Authentic miniature Airglow UI preview for all 13 `DesignId`s on Settings, cached via
/// `canvas::Cache` so unselected cards perform zero per-frame tessellation and honoring
/// `accent_override`!
pub struct DesignPreviewCanvas {
    pub design: DesignId,
    pub accent_override: Option<[u8; 3]>,
    pub selected: bool,
    pub anim_t: f32,
}

impl<Message> canvas::Program<Message> for DesignPreviewCanvas {
    type State = CachedLayerState;

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let p = self.design.palette();
        let (accent, accent_hi) = resolve_accent(p, self.accent_override);
        let w = bounds.width.max(40.0);
        let h = bounds.height.max(40.0);
        let r_win = if p.square_corners {
            0.0
        } else {
            (p.radius_panel * 0.55).min(9.0)
        };

        let mut hasher = DefaultHasher::new();
        ((w * 2.0).round() as i32).hash(&mut hasher);
        ((h * 2.0).round() as i32).hash(&mut hasher);
        self.design.hash(&mut hasher);
        self.accent_override.hash(&mut hasher);
        self.selected.hash(&mut hasher);
        let key = hasher.finish();
        if state.key.get() != key {
            state.cache.clear();
            state.key.set(key);
        }

        let static_geom = state.cache.draw(renderer, bounds.size(), |frame| {
            let r_card = if p.square_corners {
                0.0
            } else {
                (p.radius_panel * 0.45).min(8.0)
            };
            let r_btn = if p.square_corners {
                0.0
            } else if matches!(
                self.design,
                DesignId::Spore | DesignId::Kiln | DesignId::Lumen | DesignId::Firefly
            ) {
                999.0
            } else {
                p.radius_ctl.min(4.0)
            };

            // 1. Mini window ground
            let outer = Path::rounded_rectangle(
                Point::ORIGIN,
                bounds.size(),
                iced::border::Radius::from(r_win),
            );
            frame.fill(&outer, rgb(p.ground));

            // 2. Mini appbar strip at top
            let bar_h = 18.0;
            let bar_rect = Path::rectangle(Point::ORIGIN, Size::new(w, bar_h));
            if self.design == DesignId::Firefly {
                let appbar_g = gradient::Linear::new(Point::ORIGIN, Point::new(w, 0.0))
                    .add_stop(0.0, Color::from_rgb8(48, 37, 50))
                    .add_stop(0.5, Color::from_rgb8(38, 31, 48))
                    .add_stop(1.0, Color::from_rgb8(33, 28, 41));
                frame.fill(&bar_rect, Fill::from(Gradient::Linear(appbar_g)));
            } else {
                frame.fill(&bar_rect, rgb(p.panel));
            }
            frame.stroke(
                &Path::line(Point::new(0.0, bar_h), Point::new(w, bar_h)),
                Stroke::default()
                    .with_color(if self.design == DesignId::Contrast {
                        rgb(p.line)
                    } else {
                        rgba(p.line, 0.65)
                    })
                    .with_width(if self.design == DesignId::Contrast {
                        1.5
                    } else {
                        1.0
                    }),
            );

            // Mini brand dot + active tab marker + right temp pill
            let dot_col = if self.accent_override.is_some() {
                accent
            } else {
                match self.design {
                    DesignId::Firefly => Color::from_rgb8(242, 169, 196),
                    DesignId::Ascii => accent,
                    _ => accent_hi,
                }
            };
            frame.fill(&Path::circle(Point::new(10.0, bar_h * 0.5), 3.2), dot_col);
            if self.design == DesignId::Firefly {
                let pill = Path::rounded_rectangle(
                    Point::new(19.0, 4.0),
                    Size::new(26.0, 10.0),
                    iced::border::Radius::from(5.0),
                );
                frame.fill(&pill, Color { a: 0.24, ..accent });
                frame.stroke(
                    &pill,
                    Stroke::default()
                        .with_color(Color { a: 0.50, ..accent })
                        .with_width(0.8),
                );
            } else {
                frame.fill_rectangle(
                    Point::new(20.0, 6.0),
                    Size::new(22.0, 4.5),
                    rgba(p.ink, 0.85),
                );
                frame.fill_rectangle(
                    Point::new(20.0, bar_h - 2.0),
                    Size::new(22.0, 2.0),
                    accent,
                );
            }
            frame.fill_rectangle(
                Point::new(48.0, 7.0),
                Size::new(16.0, 3.5),
                rgba(p.ink_3, 0.55),
            );
            let pill_bg = if p.is_light {
                Color::from_rgb8(22, 23, 26)
            } else {
                rgb(p.well)
            };
            frame.fill(
                &Path::rounded_rectangle(
                    Point::new(w - 30.0, 4.0),
                    Size::new(24.0, 10.0),
                    iced::border::Radius::from(if p.square_corners { 0.0 } else { 4.0 }),
                ),
                pill_bg,
            );
            frame.fill(
                &Path::circle(Point::new(w - 22.0, 9.0), 2.0),
                if self.design == DesignId::Firefly {
                    rgb(p.ok)
                } else {
                    Color::from_rgb8(233, 229, 218)
                },
            );

            // 3. Body split: Left mini telemetry plate + Right controls card
            let pad = 8.0;
            let body_y = bar_h + 7.0;
            let body_h = (h - body_y - 7.0).max(24.0);
            let left_w = ((w - pad * 3.0) * 0.54).max(30.0);
            let right_x = pad * 2.0 + left_w;
            let right_w = (w - right_x - pad).max(24.0);

            let left_card = Path::rounded_rectangle(
                Point::new(pad, body_y),
                Size::new(left_w, body_h),
                iced::border::Radius::from(r_card),
            );
            if self.design == DesignId::Graphite {
                let g = gradient::Linear::new(
                    Point::new(pad, body_y),
                    Point::new(pad, body_y + body_h),
                )
                .add_stop(0.0, Color::from_rgb8(40, 42, 46))
                .add_stop(0.35, rgb(p.panel))
                .add_stop(1.0, Color::from_rgb8(26, 27, 30));
                frame.fill(&left_card, Fill::from(Gradient::Linear(g)));
            } else if self.design == DesignId::Firefly {
                let g = gradient::Linear::new(
                    Point::new(pad, body_y),
                    Point::new(pad + left_w, body_y + body_h),
                )
                .add_stop(0.0, Color::from_rgb8(56, 46, 72))
                .add_stop(1.0, Color::from_rgb8(40, 33, 53));
                frame.fill(&left_card, Fill::from(Gradient::Linear(g)));
            } else {
                frame.fill(&left_card, rgb(p.panel));
            }

            // Design-specific structural motifs inside left panel
            match self.design {
                DesignId::Lumen => {
                    let grid_c = Color::from_rgba8(48, 44, 74, 0.7);
                    let mut gx = pad + 14.0;
                    while gx < pad + left_w {
                        frame.stroke(
                            &Path::line(
                                Point::new(gx, body_y),
                                Point::new(gx, body_y + body_h),
                            ),
                            Stroke::default().with_color(grid_c).with_width(0.8),
                        );
                        gx += 14.0;
                    }
                    let mut gy = body_y + 14.0;
                    while gy < body_y + body_h {
                        frame.stroke(
                            &Path::line(
                                Point::new(pad, gy),
                                Point::new(pad + left_w, gy),
                            ),
                            Stroke::default().with_color(grid_c).with_width(0.8),
                        );
                        gy += 14.0;
                    }
                }
                DesignId::Spore | DesignId::Darkroom => {
                    let grain_c = if self.design == DesignId::Spore {
                        rgba(0xe89f92, 0.14)
                    } else {
                        Color::from_rgba(1.0, 1.0, 1.0, 0.10)
                    };
                    for i in 0..24 {
                        let gx =
                            pad + 4.0 + ((i * 29 + 7) % 100) as f32 / 100.0 * (left_w - 8.0);
                        let gy =
                            body_y + 4.0 + ((i * 47 + 13) % 100) as f32 / 100.0 * (body_h - 8.0);
                        frame.fill(&Path::circle(Point::new(gx, gy), 0.75), grain_c);
                    }
                }
                DesignId::Amber => {
                    let mut sy = body_y + 2.0;
                    while sy < body_y + body_h {
                        frame.stroke(
                            &Path::line(
                                Point::new(pad, sy),
                                Point::new(pad + left_w, sy),
                            ),
                            Stroke::default()
                                .with_color(Color::from_rgba(0.0, 0.0, 0.0, 0.28))
                                .with_width(1.0),
                        );
                        sy += 3.0;
                    }
                }
                DesignId::Firefly => {
                    let glow_pt = Point::new(pad + left_w - 12.0, body_y + 12.0);
                    fill_radial_glow(frame, glow_pt, 14.0, accent, 0.42, 4);
                    frame.fill(&Path::circle(glow_pt, 2.2), accent_hi);
                    frame.fill(
                        &Path::circle(Point::new(pad + left_w - 20.0, body_y + 16.0), 1.6),
                        Color::from_rgb8(242, 169, 196),
                    );
                }
                _ => {}
            }

            // Border rules per design
            match self.design {
                DesignId::Midnight => {}
                DesignId::Rim => {
                    frame.stroke(
                        &Path::line(
                            Point::new(pad, body_y),
                            Point::new(pad + left_w, body_y),
                        ),
                        Stroke::default().with_color(rgb(p.line)).with_width(1.0),
                    );
                    let tl = Path::new(|b| {
                        b.move_to(Point::new(pad, body_y + 7.0));
                        b.line_to(Point::new(pad, body_y));
                        b.line_to(Point::new(pad + 7.0, body_y));
                    });
                    let br = Path::new(|b| {
                        b.move_to(Point::new(pad + left_w - 7.0, body_y + body_h));
                        b.line_to(Point::new(pad + left_w, body_y + body_h));
                        b.line_to(Point::new(pad + left_w, body_y + body_h - 7.0));
                    });
                    frame.stroke(
                        &tl,
                        Stroke::default().with_color(rgb(p.ink_3)).with_width(1.1),
                    );
                    frame.stroke(
                        &br,
                        Stroke::default().with_color(rgb(p.ink_3)).with_width(1.1),
                    );
                }
                DesignId::Darkroom => {
                    frame.stroke(
                        &Path::line(
                            Point::new(pad, body_y + 12.0),
                            Point::new(pad + left_w, body_y + 12.0),
                        ),
                        Stroke::default().with_color(rgb(p.line)).with_width(1.0),
                    );
                }
                _ => {
                    frame.stroke(
                        &left_card,
                        Stroke::default()
                            .with_color(rgb(p.line))
                            .with_width(if self.design == DesignId::Contrast {
                                1.8
                            } else {
                                1.0
                            }),
                    );
                }
            }

            if self.design == DesignId::Ascii {
                frame.fill_text(canvas::Text {
                    content: "+".to_string(),
                    position: Point::new(pad + 2.0, body_y - 1.0),
                    color: accent,
                    size: iced::Pixels(10.0),
                    ..canvas::Text::default()
                });
                frame.fill_text(canvas::Text {
                    content: "+".to_string(),
                    position: Point::new(pad + left_w - 8.0, body_y + body_h - 11.0),
                    color: accent,
                    size: iced::Pixels(10.0),
                    ..canvas::Text::default()
                });
            }

            frame.fill_text(canvas::Text {
                content: "46°".to_string(),
                position: Point::new(pad + 8.0, body_y + 6.0),
                color: rgb(p.ink),
                size: iced::Pixels(15.0),
                ..canvas::Text::default()
            });

            let spark_col = match self.design {
                DesignId::Firefly => Color::from_rgb8(165, 222, 206),
                DesignId::Neon => Color::from_rgb8(47, 230, 255),
                DesignId::Rim => Color::from_rgb8(109, 176, 244),
                DesignId::Ascii | DesignId::Midnight => accent,
                _ => rgb(p.ink_2),
            };
            let sp_y0 = body_y + body_h - 6.0;
            let sp_pts = [
                Point::new(pad + 8.0, sp_y0 - 4.0),
                Point::new(pad + left_w * 0.30, sp_y0 - 11.0),
                Point::new(pad + left_w * 0.52, sp_y0 - 6.0),
                Point::new(pad + left_w * 0.74, sp_y0 - 15.0),
                Point::new(pad + left_w - 10.0, sp_y0 - 10.0),
            ];
            let sp_path = build_smooth_curve_path(&sp_pts, None);
            if matches!(
                self.design,
                DesignId::Firefly | DesignId::Neon | DesignId::Amber
            ) {
                frame.stroke(
                    &sp_path,
                    Stroke::default()
                        .with_color(Color {
                            a: 0.25,
                            ..spark_col
                        })
                        .with_width(4.5)
                        .with_line_cap(LineCap::Round),
                );
            }
            frame.stroke(
                &sp_path,
                Stroke::default()
                    .with_color(spark_col)
                    .with_width(1.6)
                    .with_line_cap(LineCap::Round),
            );
            let head = sp_pts[sp_pts.len() - 1];
            frame.fill(
                &Path::circle(head, 2.0),
                mix_color(spark_col, Color::WHITE, 0.5),
            );

            // 4. Right side: Mini primary button + toggle + segmented/heat meter
            let btn_h = 16.0;
            let btn_rect = Path::rounded_rectangle(
                Point::new(right_x, body_y + 2.0),
                Size::new(right_w, btn_h),
                iced::border::Radius::from(r_btn),
            );
            if self.design == DesignId::Neon {
                fill_radial_glow(
                    frame,
                    Point::new(right_x + right_w * 0.5, body_y + 10.0),
                    right_w * 0.52,
                    accent,
                    0.30,
                    4,
                );
            }
            if self.design == DesignId::Ascii {
                frame.stroke(
                    &btn_rect,
                    Stroke::default().with_color(accent).with_width(1.1),
                );
                frame.fill_text(canvas::Text {
                    content: "[SET]".to_string(),
                    position: Point::new(right_x + 6.0, body_y + 4.0),
                    color: accent_hi,
                    size: iced::Pixels(9.5),
                    ..canvas::Text::default()
                });
            } else {
                frame.fill(&btn_rect, accent);
                let btn_ink = if p.is_light {
                    Color::from_rgb8(255, 250, 240)
                } else if self.design == DesignId::Firefly {
                    Color::from_rgb8(55, 36, 31)
                } else {
                    Color::from_rgb8(18, 16, 14)
                };
                frame.fill_rectangle(
                    Point::new(right_x + right_w * 0.24, body_y + 8.5),
                    Size::new(right_w * 0.52, 3.0),
                    btn_ink,
                );
            }

            // Mini toggle row
            let tog_y = body_y + 25.0;
            let tog_r = Path::rounded_rectangle(
                Point::new(right_x, tog_y),
                Size::new(22.0, 11.0),
                iced::border::Radius::from(if p.square_corners { 0.0 } else { 999.0 }),
            );
            frame.fill(&tog_r, Color { a: 0.25, ..accent });
            frame.stroke(
                &tog_r,
                Stroke::default().with_color(accent).with_width(1.0),
            );
            frame.fill(
                &Path::circle(Point::new(right_x + 16.0, tog_y + 5.5), 3.6),
                accent_hi,
            );
            frame.fill(
                &Path::circle(Point::new(right_x + 31.0, tog_y + 5.5), 2.5),
                rgb(p.ok),
            );
            frame.fill(
                &Path::circle(Point::new(right_x + 40.0, tog_y + 5.5), 2.5),
                rgb(p.hot),
            );

            // Mini meter bar at bottom-right
            let met_y = body_y + body_h - 11.0;
            let met_h = 7.0;
            let met_bg = Path::rounded_rectangle(
                Point::new(right_x, met_y),
                Size::new(right_w, met_h),
                iced::border::Radius::from(if p.square_corners { 0.0 } else { 4.0 }),
            );
            frame.fill(&met_bg, rgb(p.well));
            let fill_w = right_w * 0.72;
            if self.design == DesignId::Kiln && self.accent_override.is_none() {
                let heat = gradient::Linear::new(
                    Point::new(right_x, met_y),
                    Point::new(right_x + fill_w, met_y),
                )
                .add_stop(0.0, Color::from_rgb8(138, 5, 13))
                .add_stop(0.55, Color::from_rgb8(255, 138, 0))
                .add_stop(1.0, Color::from_rgb8(255, 249, 219));
                let fill_r = Path::rounded_rectangle(
                    Point::new(right_x, met_y),
                    Size::new(fill_w, met_h),
                    iced::border::Radius::from(4.0),
                );
                frame.fill(&fill_r, Fill::from(Gradient::Linear(heat)));
            } else if self.design == DesignId::Ascii {
                let mut bx = right_x + 1.0;
                while bx + 4.0 < right_x + fill_w {
                    frame.fill_rectangle(
                        Point::new(bx, met_y + 1.0),
                        Size::new(4.0, met_h - 2.0),
                        accent,
                    );
                    bx += 6.0;
                }
            } else {
                let mcol = if self.design == DesignId::Firefly && self.accent_override.is_none() {
                    Color::from_rgb8(165, 222, 206)
                } else {
                    accent
                };
                let fill_r = Path::rounded_rectangle(
                    Point::new(right_x, met_y),
                    Size::new(fill_w, met_h),
                    iced::border::Radius::from(if p.square_corners { 0.0 } else { 4.0 }),
                );
                frame.fill(&fill_r, mcol);
            }

            // Static outer border when NOT selected (zero dynamic work!)
            if !self.selected {
                frame.stroke(
                    &outer,
                    Stroke::default()
                        .with_color(rgba(p.line, 0.75))
                        .with_width(1.0),
                );
            }
        });

        if !self.selected {
            return vec![static_geom];
        }

        // Lightweight dynamic pulsing selection border only for the active card
        let mut sel_frame = Frame::new(renderer, bounds.size());
        let outer = Path::rounded_rectangle(
            Point::ORIGIN,
            bounds.size(),
            iced::border::Radius::from(r_win),
        );
        let pulse = 0.78 + 0.22 * (self.anim_t * 2.4).sin();
        sel_frame.stroke(
            &outer,
            Stroke::default()
                .with_color(Color {
                    a: pulse,
                    ..accent_hi
                })
                .with_width(1.8),
        );
        vec![static_geom, sel_frame.into_geometry()]
    }
}
