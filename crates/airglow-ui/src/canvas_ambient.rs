//! Ambient bioluminescent Firefly particles, warm lantern aura, and 24-band luminous
//! rounded-capsule audio spectrum visualizer with soft bloom and falling peak-hold dots.

use std::cell::{Cell, RefCell};
use std::f32::consts::PI;

use airglow_core::config::DesignPalette;
use iced::mouse;
use iced::widget::canvas::{
    self, Fill, Frame, Geometry, Gradient, LineCap, Path, Stroke, gradient,
};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};

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

        // Harmonized 3-hue bioluminescent palette adapting to active design + accent_override
        let gold_col = if self.accent_override.is_some() {
            mix_color(accent_hi, Color::from_rgb8(255, 224, 156), 0.55)
        } else if firefly {
            Color::from_rgb8(255, 212, 138) // warm lantern gold
        } else {
            accent_hi
        };
        let rose_col = if self.accent_override.is_some() {
            accent
        } else if firefly {
            Color::from_rgb8(244, 166, 198) // dusk rose-pink
        } else {
            accent
        };
        let mint_col = if firefly {
            Color::from_rgb8(165, 226, 208) // bioluminescent mint
        } else {
            rgb(p.ok)
        };

        // 1. Warm breathing golden lantern aura positioned softly behind the moth's lantern
        // (restrained radius & alpha so the dusk bellflowers stay rich and unwashed!)
        if self.show_lantern_aura && w > 260.0 {
            let lantern_x = (w - 238.0).clamp(w * 0.50, w - 52.0);
            let lantern_y = h * 0.58;
            let breath = 0.80
                + 0.14 * (t * 1.25).sin()
                + 0.06 * (t * 2.6 + 0.7).cos();
            let aura_r = (h * 0.64).clamp(40.0, 68.0) * (0.94 + 0.06 * breath);
            let center = Point::new(lantern_x, lantern_y);

            // Delicate rose outer corona
            fill_bokeh_halo(
                &mut frame,
                center,
                aura_r * 1.06,
                rose_col,
                0.065 * breath,
                4,
            );
            // Warm golden lantern wash
            fill_bokeh_halo(
                &mut frame,
                center,
                aura_r * 0.74,
                gold_col,
                0.19 * breath,
                5,
            );
            // Focused inner lantern focal glimmer
            fill_bokeh_halo(
                &mut frame,
                Point::new(lantern_x + 3.0, lantern_y + 3.0),
                aura_r * 0.30,
                Color::from_rgb8(255, 240, 200),
                0.24 * breath,
                4,
            );
        }

        // 2. Delicate floating dusk pollen motes (atmospheric depth)
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

        // 3. Drifting bioluminescent bokeh fireflies with sinusoidal sway & asynchronous glow
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

            // Soft outer bokeh halo
            fill_bokeh_halo(
                &mut frame,
                pt,
                ff.radius * (5.2 + 2.4 * pulse_wave),
                col,
                0.34 * glow_k,
                4,
            );

            // Bright bioluminescent core
            let core_col = mix_color(col, Color::WHITE, 0.55 * pulse_wave + 0.25);
            frame.fill(
                &Path::circle(pt, ff.radius * (0.72 + 0.32 * pulse_wave)),
                Color {
                    a: (0.88 * glow_k).clamp(0.0, 0.96),
                    ..core_col
                },
            );

            // Crisp micro-starlight glint when firefly reaches peak flash
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

        let r_outer = if p.square_corners { 0.0 } else { 7.0 };
        let trough = Path::rounded_rectangle(
            Point::ORIGIN,
            Size::new(w, h),
            iced::border::Radius::from(r_outer),
        );
        frame.fill(&trough, rgb(p.well));
        frame.stroke(
            &trough,
            Stroke::default()
                .with_color(rgba(p.line, if self.active { 0.62 } else { 0.36 }))
                .with_width(1.0),
        );

        // Smooth bar envelopes + falling peak-hold physics
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

        // Low-frequency beat glow wash behind bass bars
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

        // 3-stop spectrum palette: Bass -> Mid -> Treble
        let c_bass = if self.accent_override.is_some() {
            accent
        } else if firefly {
            Color::from_rgb8(244, 143, 177) // rose-pink
        } else {
            accent
        };
        let c_mid = if self.accent_override.is_some() {
            mix_color(accent_hi, rgb(p.warm), 0.45)
        } else if firefly {
            Color::from_rgb8(255, 198, 138) // warm apricot
        } else {
            accent_hi
        };
        let c_treble = if firefly {
            Color::from_rgb8(165, 226, 208) // luminous mint
        } else {
            rgb(p.ok)
        };

        let pad_x = 6.0;
        let pad_top = 5.0;
        let pad_bot = 4.0;
        let usable_w = (w - pad_x * 2.0).max(24.0);
        let usable_h = (h - pad_top - pad_bot).max(6.0);
        let base_y = h - pad_bot;

        // Subtle baseline floor rule
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

        let is_live = self.active
            || smooth.iter().any(|&v| v > 0.015)
            || peaks.iter().any(|&v| v > 0.025);

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

                    // Soft luminous bloom behind active capsule
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

                    // Luminous vertical gradient capsule
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

                // Falling peak-hold luminous dot hovering above the bar
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
            // Refined resting idle state: low-glow breathing 24-bar harmonic wave + micro-hint
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
                // Gentle symmetrical harmonic breathing wave (16%..34% height)
                let arch = 0.65 + 0.35 * (f * PI).sin();
                let wave = 0.5
                    + 0.34 * (f * 4.2 - phase).sin()
                    + 0.16 * (f * 8.5 + phase * 0.65).cos();
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

                // Soft resting cap dot
                let dot_r = (bar_w * 0.30).clamp(1.0, 1.8);
                frame.fill(
                    &Path::circle(Point::new(x + bar_w * 0.5, (y - 2.0).max(pad_top)), dot_r),
                    Color {
                        a: 0.34,
                        ..mix_color(bar_col, Color::WHITE, 0.45)
                    },
                );
            }

            // Centered subtle micro-hint caption over a soft translucent pill scrim
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
                frame.fill(&pill, rgba(p.ground, 0.78));
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
