//! Interactive GPU-accelerated fan curve editor (drag, double-click to add, right-click to remove)
//! with smart multi-layer `canvas::Cache` separation (static grid/labels/safety hatch cached),
//! Firefly living-light curve bloom, acoustic hysteresis band, pulsing live "Now" ring,
//! accent_override support, hover probe tooltip, and full °C / °F axis formatting.

use std::cell::Cell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use airglow_core::config::{CurvePoint, DesignPalette, MIN_FAN_DUTY, SAFETY_TEMP, TempUnit};
use iced::mouse;
use iced::widget::canvas::{
    self, Action, Event, Fill, Frame, Geometry, Gradient, LineCap, LineDash, LineJoin, Path,
    Stroke, gradient,
};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};

use crate::theme::{rgb, rgba};

#[derive(Debug, Clone)]
pub enum CurveMessage {
    PointsChanged(Vec<CurvePoint>),
}

pub struct FanCurveCanvas<'a> {
    pub points: &'a [CurvePoint],
    pub current_temp: Option<f32>,
    pub current_duty: u8,
    pub temp_unit: TempUnit,
    pub palette: DesignPalette,
    pub accent_override: Option<[u8; 3]>,
    pub hysteresis_c: f32,
    pub anim_t: f32,
}

#[derive(Default)]
pub struct CurveDragState {
    dragging_idx: Option<usize>,
    last_click: Option<(std::time::Instant, Point)>,
    static_cache: canvas::Cache,
    static_key: Cell<u64>,
}

const TEMP_MIN: f32 = 20.0;
const TEMP_MAX: f32 = 100.0;

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
        let hi = mix_color(base, Color::WHITE, 0.26);
        (base, hi)
    } else {
        (rgb(p.brass), rgb(p.brass_hi))
    }
}

fn plot_rect(bounds: Rectangle) -> Rectangle {
    Rectangle {
        x: 48.0,
        y: 26.0,
        width: (bounds.width - 64.0).max(60.0),
        height: (bounds.height - 54.0).max(60.0),
    }
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

fn to_screen(plot: Rectangle, temp: f32, duty: f32) -> Point {
    let x = plot.x + ((temp - TEMP_MIN) / (TEMP_MAX - TEMP_MIN)).clamp(0.0, 1.0) * plot.width;
    let y = plot.y + (1.0 - (duty / 100.0).clamp(0.0, 1.0)) * plot.height;
    Point::new(x, y)
}

fn from_screen(plot: Rectangle, pt: Point) -> (u8, u8) {
    let tx = ((pt.x - plot.x) / plot.width).clamp(0.0, 1.0);
    let ty = (1.0 - (pt.y - plot.y) / plot.height).clamp(0.0, 1.0);
    let temp = (TEMP_MIN + tx * (TEMP_MAX - TEMP_MIN)).round() as u8;
    let duty = (ty * 100.0).round().clamp(MIN_FAN_DUTY as f32, 100.0) as u8;
    (temp.clamp(20, 100), duty)
}

fn eval_curve(points: &[CurvePoint], temp: f32) -> f32 {
    if points.is_empty() {
        return 40.0;
    }
    if temp >= SAFETY_TEMP {
        return 100.0;
    }
    if temp <= points[0].temp as f32 {
        return points[0].duty as f32;
    }
    for w in points.windows(2) {
        let t0 = w[0].temp as f32;
        let t1 = w[1].temp as f32;
        if temp <= t1 {
            let f = if t1 > t0 { (temp - t0) / (t1 - t0) } else { 0.0 };
            return w[0].duty as f32 + (w[1].duty as f32 - w[0].duty as f32) * f;
        }
    }
    points[points.len() - 1].duty as f32
}

fn static_cache_hash(
    bounds: Rectangle,
    p: DesignPalette,
    temp_unit: TempUnit,
    accent_override: Option<[u8; 3]>,
) -> u64 {
    let mut h = DefaultHasher::new();
    ((bounds.width * 2.0).round() as i32).hash(&mut h);
    ((bounds.height * 2.0).round() as i32).hash(&mut h);
    p.ground.hash(&mut h);
    p.well.hash(&mut h);
    p.line.hash(&mut h);
    p.ink_3.hash(&mut h);
    p.hot.hash(&mut h);
    p.radius_panel.to_bits().hash(&mut h);
    temp_unit.hash(&mut h);
    accent_override.hash(&mut h);
    h.finish()
}

impl<'a> canvas::Program<CurveMessage> for FanCurveCanvas<'a> {
    type State = CurveDragState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<CurveMessage>> {
        let plot = plot_rect(bounds);
        let pos = cursor.position_in(bounds)?;

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let mut best: Option<(usize, f32)> = None;
                for (i, pt) in self.points.iter().enumerate() {
                    let sp = to_screen(plot, pt.temp as f32, pt.duty as f32);
                    let dist = ((sp.x - pos.x).powi(2) + (sp.y - pos.y).powi(2)).sqrt();
                    if dist <= 14.0 && best.is_none_or(|(_, d)| dist < d) {
                        best = Some((i, dist));
                    }
                }
                if let Some((idx, _)) = best {
                    state.dragging_idx = Some(idx);
                    state.last_click = None;
                    return Some(Action::capture());
                }

                // Double-click empty plot area to add a control point (up to 8)
                let now = std::time::Instant::now();
                let is_double = state.last_click.is_some_and(|(t, prev)| {
                    now.duration_since(t).as_millis() < 400
                        && ((prev.x - pos.x).powi(2) + (prev.y - pos.y).powi(2)).sqrt() < 12.0
                });
                state.last_click = Some((now, pos));
                if is_double && self.points.len() < 8 {
                    let (new_t, new_d) = from_screen(plot, pos);
                    if !self.points.iter().any(|p| p.temp.abs_diff(new_t) < 2) {
                        let mut updated = self.points.to_vec();
                        updated.push(CurvePoint {
                            temp: new_t,
                            duty: new_d,
                        });
                        updated.sort_by_key(|p| p.temp);
                        state.last_click = None;
                        return Some(
                            Action::publish(CurveMessage::PointsChanged(updated)).and_capture(),
                        );
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                // Right-click a control point to remove it (min 2 points)
                if self.points.len() > 2 {
                    for (i, pt) in self.points.iter().enumerate() {
                        let sp = to_screen(plot, pt.temp as f32, pt.duty as f32);
                        if ((sp.x - pos.x).powi(2) + (sp.y - pos.y).powi(2)).sqrt() <= 14.0 {
                            let mut updated = self.points.to_vec();
                            updated.remove(i);
                            return Some(
                                Action::publish(CurveMessage::PointsChanged(updated)).and_capture(),
                            );
                        }
                    }
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let Some(idx) = state.dragging_idx
                    && idx < self.points.len()
                {
                    let (mut new_temp, new_duty) = from_screen(plot, pos);
                    let min_t = if idx > 0 {
                        self.points[idx - 1].temp.saturating_add(2)
                    } else {
                        20
                    };
                    let max_t = if idx + 1 < self.points.len() {
                        self.points[idx + 1].temp.saturating_sub(2)
                    } else {
                        100
                    };
                    new_temp = new_temp.clamp(min_t, max_t.max(min_t));
                    let mut updated = self.points.to_vec();
                    updated[idx] = CurvePoint {
                        temp: new_temp,
                        duty: new_duty,
                    };
                    return Some(
                        Action::publish(CurveMessage::PointsChanged(updated)).and_capture(),
                    );
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.dragging_idx.take().is_some() {
                    return Some(Action::capture());
                }
            }
            _ => {}
        }
        None
    }

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
        let plot = plot_rect(bounds);

        // Layer 1 (CACHED): Scope backdrop, red hatched safety band, 20% stall floor, grid & labels
        let key = static_cache_hash(bounds, p, self.temp_unit, self.accent_override);
        if state.static_key.get() != key {
            state.static_cache.clear();
            state.static_key.set(key);
        }

        let static_geom = state.static_cache.draw(renderer, bounds.size(), |frame| {
            // 1. Recessed instrument scope backdrop + Firefly top-right warm dusk wash
            let scope_bg = if firefly {
                let top_tint = if let Some([r, g, b]) = self.accent_override {
                    mix_color(
                        Color::from_rgb8(38, 30, 50),
                        Color::from_rgb8(r, g, b),
                        0.12,
                    )
                } else {
                    Color::from_rgb8(48, 37, 58)
                };
                gradient::Linear::new(
                    Point::new(bounds.width * 0.75, 0.0),
                    Point::new(bounds.width * 0.22, bounds.height),
                )
                .add_stop(0.0, top_tint)
                .add_stop(0.48, Color::from_rgb8(29, 24, 41))
                .add_stop(1.0, Color::from_rgb8(22, 19, 33))
            } else {
                gradient::Linear::new(Point::ORIGIN, Point::new(0.0, bounds.height))
                    .add_stop(0.0, rgb(p.well))
                    .add_stop(1.0, mix_color(rgb(p.well), rgb(p.ground), 0.25))
            };
            frame.fill(
                &Path::rectangle(Point::ORIGIN, bounds.size()),
                Fill::from(Gradient::Linear(scope_bg)),
            );

            // 2. Subtle thermal regime bands (Cool <45°, Nominal 45–70°, Warm 70–85°, Critical 90°+)
            let x_20 = plot.x;
            let x_45 = to_screen(plot, 45.0, 0.0).x;
            let x_70 = to_screen(plot, 70.0, 0.0).x;
            let x_85 = to_screen(plot, 85.0, 0.0).x;
            let safety_x = to_screen(plot, SAFETY_TEMP, 100.0).x;
            let plot_r = plot.x + plot.width;
            let plot_b = plot.y + plot.height;

            let fmt_deg = |c: f32| -> String {
                format!("{:.0}°", self.temp_unit.convert(c).round())
            };
            let cool_hdr = format!("COOL <{}", fmt_deg(45.0));
            let nom_hdr = format!(
                "NOMINAL {}–{}",
                self.temp_unit.convert(45.0).round() as i32,
                fmt_deg(70.0)
            );
            let warm_hdr = format!(
                "WARM {}–{}",
                self.temp_unit.convert(70.0).round() as i32,
                fmt_deg(85.0)
            );
            let crit_hdr = format!("CRITICAL {}+", fmt_deg(90.0));

            let regimes = [
                (x_20, x_45, rgb(p.ok), 0.065_f32, 0.015_f32, Some((cool_hdr, rgba(p.ok, 0.86)))),
                (
                    x_45,
                    x_70,
                    accent,
                    0.042,
                    0.010,
                    Some((nom_hdr, rgba(p.ink_2, 0.80))),
                ),
                (
                    x_70,
                    x_85,
                    rgb(p.warm),
                    0.072,
                    0.018,
                    Some((warm_hdr, rgba(p.warm, 0.90))),
                ),
                (x_85, safety_x, rgb(p.hot), 0.078, 0.024, None),
            ];
            for (rx0, rx1, rcol, a_top, a_bot, hdr_opt) in regimes {
                let rw = (rx1 - rx0).max(0.0);
                if rw <= 1.0 {
                    continue;
                }
                let band_g = gradient::Linear::new(
                    Point::new(rx0, plot.y),
                    Point::new(rx0, plot_b),
                )
                .add_stop(0.0, Color { a: a_top, ..rcol })
                .add_stop(1.0, Color { a: a_bot, ..rcol });
                frame.fill(
                    &Path::rectangle(Point::new(rx0, plot.y), Size::new(rw, plot.height)),
                    Fill::from(Gradient::Linear(band_g)),
                );

                if rx0 > x_20 + 1.0 {
                    frame.stroke(
                        &Path::line(Point::new(rx0, plot.y), Point::new(rx0, plot_b)),
                        Stroke {
                            style: canvas::Style::Solid(Color { a: 0.24, ..rcol }),
                            width: 1.0,
                            line_dash: LineDash {
                                segments: &[3.0, 4.0],
                                offset: 0,
                            },
                            ..Stroke::default()
                        },
                    );
                }

                if let Some((hdr_txt, hdr_col)) = hdr_opt
                    && rw >= 58.0
                {
                    frame.fill_text(canvas::Text {
                        content: hdr_txt,
                        position: Point::new(rx0 + rw * 0.5, (plot.y - 11.0).max(6.0)),
                        color: hdr_col,
                        size: iced::Pixels(9.5),
                        font: crate::theme::mono_font(),
                        align_x: iced::alignment::Horizontal::Center.into(),
                        align_y: iced::alignment::Vertical::Center,
                        ..canvas::Text::default()
                    });
                }
            }

            // 2b. 90°–100° Critical thermal safety band + glowing 90°+ hairline + 45° hazard hatch
            let safety_w = (plot_r - safety_x).max(0.0);
            frame.fill_rectangle(
                Point::new(safety_x, plot.y),
                Size::new(safety_w, plot.height),
                rgba(p.hot, 0.14),
            );
            if safety_w > 2.0 {
                let step = 9.0;
                let mut offset = -plot.height;
                while offset < safety_w + plot.height {
                    let mut x0 = safety_x + offset;
                    let mut y0 = plot.y;
                    let mut x1 = safety_x + offset + plot.height;
                    let mut y1 = plot_b;
                    if x0 < safety_x {
                        y0 += safety_x - x0;
                        x0 = safety_x;
                    }
                    if x1 > plot_r {
                        y1 -= x1 - plot_r;
                        x1 = plot_r;
                    }
                    if y0 < plot_b && y1 > plot.y && x1 > x0 {
                        frame.stroke(
                            &Path::line(Point::new(x0, y0), Point::new(x1, y1)),
                            Stroke::default()
                                .with_color(rgba(p.hot, 0.22))
                                .with_width(1.0),
                        );
                    }
                    offset += step;
                }
                // Crisp Critical 90°+ vertical hairline + soft red glow
                frame.stroke(
                    &Path::line(Point::new(safety_x, plot.y), Point::new(safety_x, plot_b)),
                    Stroke::default()
                        .with_color(rgba(p.hot, 0.22))
                        .with_width(4.0),
                );
                frame.stroke(
                    &Path::line(Point::new(safety_x, plot.y), Point::new(safety_x, plot_b)),
                    Stroke::default()
                        .with_color(rgba(p.hot, 0.78))
                        .with_width(1.3),
                );
            }
            if safety_w >= 44.0 {
                frame.fill_text(canvas::Text {
                    content: if safety_w >= 84.0 {
                        crit_hdr
                    } else {
                        "90°+".to_string()
                    },
                    position: Point::new(safety_x + safety_w * 0.5, (plot.y - 11.0).max(6.0)),
                    color: rgba(p.hot, 0.96),
                    size: iced::Pixels(9.5),
                    font: crate::theme::mono_font(),
                    align_x: iced::alignment::Horizontal::Center.into(),
                    align_y: iced::alignment::Vertical::Center,
                    ..canvas::Text::default()
                });

                let safety_lbl = if safety_w >= 96.0 {
                    "100% ceiling"
                } else {
                    "100%"
                };
                let sw = (safety_lbl.len() as f32 * 5.8 + 10.0).min(safety_w - 6.0);
                let sx = safety_x + ((safety_w - sw) * 0.5).max(3.0);
                let sy = plot.y + 8.0;
                let badge = Path::rounded_rectangle(
                    Point::new(sx, sy),
                    Size::new(sw, 17.0),
                    iced::border::Radius::from(4.0),
                );
                frame.fill(&badge, rgba(p.ground, 0.80));
                frame.stroke(
                    &badge,
                    Stroke::default()
                        .with_color(rgba(p.hot, 0.52))
                        .with_width(0.85),
                );
                frame.fill_text(canvas::Text {
                    content: safety_lbl.to_string(),
                    position: Point::new(sx + sw * 0.5, sy + 8.5),
                    color: rgba(p.hot, 0.95),
                    size: iced::Pixels(9.2),
                    font: crate::theme::mono_font(),
                    align_x: iced::alignment::Horizontal::Center.into(),
                    align_y: iced::alignment::Vertical::Center,
                    ..canvas::Text::default()
                });
            }

            // 3. 20% minimum fan stall band
            let floor_y = to_screen(plot, TEMP_MIN, MIN_FAN_DUTY as f32).y;
            frame.fill_rectangle(
                Point::new(plot.x, floor_y),
                Size::new(plot.width, (plot_b - floor_y).max(0.0)),
                rgba(p.line, 0.16),
            );
            frame.stroke(
                &Path::line(Point::new(plot.x, floor_y), Point::new(plot_r, floor_y)),
                Stroke {
                    style: canvas::Style::Solid(rgba(p.line, 0.48)),
                    width: 1.0,
                    line_dash: LineDash {
                        segments: &[3.0, 4.0],
                        offset: 0,
                    },
                    ..Stroke::default()
                },
            );
            frame.fill_text(canvas::Text {
                content: "20% stall protection floor".to_string(),
                position: Point::new(plot.x + 8.0, (plot_b - 15.0).max(plot.y)),
                color: rgba(p.ink_3, 0.78),
                size: iced::Pixels(9.8),
                ..canvas::Text::default()
            });

            // 4. Gridlines, minor 10% subdivisions & axis labels (°C / °F aware)
            for t in [20, 30, 40, 50, 60, 70, 80, 90, 100] {
                let x = to_screen(plot, t as f32, 0.0).x;
                frame.stroke(
                    &Path::line(Point::new(x, plot.y), Point::new(x, plot_b)),
                    Stroke::default()
                        .with_color(rgba(p.line, if t % 20 == 0 { 0.34 } else { 0.18 }))
                        .with_width(1.0),
                );
                let disp = self.temp_unit.convert(t as f32).round() as i32;
                frame.fill_text(canvas::Text {
                    content: format!("{disp}°"),
                    position: Point::new(x, plot_b + 12.0),
                    color: rgb(p.ink_3),
                    size: iced::Pixels(10.5),
                    font: crate::theme::mono_font(),
                    align_x: iced::alignment::Horizontal::Center.into(),
                    align_y: iced::alignment::Vertical::Center,
                    ..canvas::Text::default()
                });
            }
            if plot.height >= 190.0 {
                for d_minor in [30, 50, 70, 90] {
                    let ym = to_screen(plot, TEMP_MIN, d_minor as f32).y;
                    frame.stroke(
                        &Path::line(Point::new(plot.x, ym), Point::new(plot_r, ym)),
                        Stroke {
                            style: canvas::Style::Solid(rgba(p.line, 0.12)),
                            width: 0.85,
                            line_dash: LineDash {
                                segments: &[2.0, 5.0],
                                offset: 0,
                            },
                            ..Stroke::default()
                        },
                    );
                }
            }
            for d in [0, 20, 40, 60, 80, 100] {
                let y = to_screen(plot, TEMP_MIN, d as f32).y;
                frame.stroke(
                    &Path::line(Point::new(plot.x, y), Point::new(plot_r, y)),
                    Stroke::default()
                        .with_color(rgba(p.line, if d % 40 == 0 { 0.34 } else { 0.20 }))
                        .with_width(1.0),
                );
                frame.fill_text(canvas::Text {
                    content: format!("{d}%"),
                    position: Point::new(plot.x - 7.0, y),
                    color: rgb(p.ink_3),
                    size: iced::Pixels(10.5),
                    font: crate::theme::mono_font(),
                    align_x: iced::alignment::Horizontal::Right.into(),
                    align_y: iced::alignment::Vertical::Center,
                    ..canvas::Text::default()
                });
            }

            // Crisp outer scope plot frame
            frame.stroke(
                &Path::rectangle(Point::new(plot.x, plot.y), plot.size()),
                Stroke::default()
                    .with_color(rgba(p.line, 0.48))
                    .with_width(1.0),
            );
        });

        // Layer 2 (DYNAMIC): Curve, acoustic hysteresis band, live "Now" crosshair, hover probe & handles
        let mut frame = Frame::new(renderer, bounds.size());
        let plot_b = plot.y + plot.height;
        let plot_r = plot.x + plot.width;

        // 5. Richer glowing fan curve area wash + acoustic hysteresis band + multi-pass neon stroke
        if !self.points.is_empty() {
            let mut curve_pts: Vec<Point> = Vec::with_capacity(self.points.len() + 3);
            let first = self.points[0];
            curve_pts.push(to_screen(plot, TEMP_MIN, first.duty as f32));
            for pt in self.points {
                curve_pts.push(to_screen(plot, pt.temp as f32, pt.duty as f32));
            }
            let last = self.points[self.points.len() - 1];
            if (last.temp as f32) < SAFETY_TEMP {
                curve_pts.push(to_screen(plot, SAFETY_TEMP, 100.0));
            }
            curve_pts.push(to_screen(plot, TEMP_MAX, 100.0_f32.max(last.duty as f32)));

            // Acoustic cooling hysteresis deadband ribbon (shifted left by hysteresis_c)
            let hyst = self.hysteresis_c.clamp(0.0, 10.0);
            if hyst >= 0.4 {
                let mut hyst_pts: Vec<Point> = Vec::with_capacity(self.points.len() + 3);
                hyst_pts.push(to_screen(plot, TEMP_MIN, first.duty as f32));
                for pt in self.points {
                    let ht = (pt.temp as f32 - hyst).clamp(TEMP_MIN, TEMP_MAX);
                    hyst_pts.push(to_screen(plot, ht, pt.duty as f32));
                }
                if (last.temp as f32) < SAFETY_TEMP {
                    hyst_pts.push(to_screen(
                        plot,
                        (SAFETY_TEMP - hyst).clamp(TEMP_MIN, TEMP_MAX),
                        100.0,
                    ));
                }
                hyst_pts.push(to_screen(plot, TEMP_MAX, 100.0_f32.max(last.duty as f32)));

                let ribbon = Path::new(|b| {
                    b.move_to(curve_pts[0]);
                    for &pt in &curve_pts[1..] {
                        b.line_to(pt);
                    }
                    for &pt in hyst_pts.iter().rev() {
                        b.line_to(pt);
                    }
                    b.close();
                });
                let hyst_col = if firefly { rgb(p.ok) } else { accent };
                frame.fill(&ribbon, Color { a: 0.13, ..hyst_col });

                let hyst_edge = Path::new(|b| {
                    b.move_to(hyst_pts[0]);
                    for &pt in &hyst_pts[1..] {
                        b.line_to(pt);
                    }
                });
                frame.stroke(
                    &hyst_edge,
                    Stroke {
                        style: canvas::Style::Solid(Color { a: 0.38, ..hyst_col }),
                        width: 1.05,
                        line_dash: LineDash {
                            segments: &[3.0, 4.0],
                            offset: 0,
                        },
                        ..Stroke::default()
                    },
                );
            }

            let mut curve_min_y = plot_b;
            for pt in &curve_pts {
                curve_min_y = curve_min_y.min(pt.y);
            }

            let area = Path::new(|b| {
                b.move_to(Point::new(curve_pts[0].x, plot_b));
                for &pt in &curve_pts {
                    b.line_to(pt);
                }
                b.line_to(Point::new(curve_pts[curve_pts.len() - 1].x, plot_b));
                b.close();
            });
            let cool_tint = if firefly && self.accent_override.is_none() {
                rgb(p.ok)
            } else {
                accent
            };
            let area_grad = gradient::Linear::new(
                Point::new(plot.x, curve_min_y.min(plot_b - 6.0)),
                Point::new(plot.x, plot_b),
            )
            .add_stop(0.0, Color { a: 0.34, ..accent })
            .add_stop(0.42, Color { a: 0.18, ..cool_tint })
            .add_stop(0.80, Color { a: 0.06, ..cool_tint })
            .add_stop(1.0, Color { a: 0.0, ..cool_tint });
            frame.fill(&area, Fill::from(Gradient::Linear(area_grad)));

            // Subtle horizontal regime warmth kiss over the curve area
            let area_horiz = gradient::Linear::new(
                Point::new(plot.x, plot.y),
                Point::new(plot_r, plot.y),
            )
            .add_stop(0.0, Color { a: 0.05, ..rgb(p.ok) })
            .add_stop(0.55, Color { a: 0.04, ..accent })
            .add_stop(0.82, Color { a: 0.08, ..rgb(p.warm) })
            .add_stop(1.0, Color { a: 0.12, ..rgb(p.hot) });
            frame.fill(&area, Fill::from(Gradient::Linear(area_horiz)));

            let curve_path = Path::new(|b| {
                b.move_to(curve_pts[0]);
                for &pt in &curve_pts[1..] {
                    b.line_to(pt);
                }
            });
            let stroke_grad = gradient::Linear::new(
                Point::new(plot.x, plot.y),
                Point::new(plot_r, plot.y),
            )
            .add_stop(0.0, cool_tint)
            .add_stop(0.42, accent)
            .add_stop(0.72, accent_hi)
            .add_stop(0.88, rgb(p.warm))
            .add_stop(1.0, rgb(p.hot));

            // Multi-pass living-light bloom behind curve
            frame.stroke(
                &curve_path,
                Stroke {
                    style: canvas::Style::Solid(Color { a: 0.11, ..accent }),
                    width: 11.5,
                    line_cap: LineCap::Round,
                    line_join: LineJoin::Round,
                    ..Stroke::default()
                },
            );
            frame.stroke(
                &curve_path,
                Stroke {
                    style: canvas::Style::Solid(Color { a: 0.24, ..accent }),
                    width: 5.5,
                    line_cap: LineCap::Round,
                    line_join: LineJoin::Round,
                    ..Stroke::default()
                },
            );
            frame.stroke(
                &curve_path,
                Stroke {
                    style: canvas::Style::Gradient(Gradient::Linear(stroke_grad)),
                    width: 2.8,
                    line_cap: LineCap::Round,
                    line_join: LineJoin::Round,
                    ..Stroke::default()
                },
            );
            frame.stroke(
                &curve_path,
                Stroke {
                    style: canvas::Style::Solid(Color::from_rgba(1.0, 1.0, 1.0, 0.34)),
                    width: 0.95,
                    line_cap: LineCap::Round,
                    line_join: LineJoin::Round,
                    ..Stroke::default()
                },
            );

            // Subtle traveling shimmer along the curve
            let cycle = (self.anim_t * 0.15).fract();
            if cycle < 0.30 && plot.width > 80.0 {
                let sh = cycle / 0.30;
                let cx = plot.x + plot.width * (-0.08 + 1.16 * sh);
                let bw = (plot.width * 0.16).max(42.0);
                let shimmer = gradient::Linear::new(
                    Point::new(cx - bw, 0.0),
                    Point::new(cx + bw, 0.0),
                )
                .add_stop(0.0, Color::from_rgba(1.0, 1.0, 1.0, 0.0))
                .add_stop(0.5, Color::from_rgba(1.0, 1.0, 1.0, 0.45))
                .add_stop(1.0, Color::from_rgba(1.0, 1.0, 1.0, 0.0));
                frame.stroke(
                    &curve_path,
                    Stroke {
                        style: canvas::Style::Gradient(Gradient::Linear(shimmer)),
                        width: 3.2,
                        line_cap: LineCap::Round,
                        line_join: LineJoin::Round,
                        ..Stroke::default()
                    },
                );
            }
        }

        // 6. Live operating crosshair + pulsing multi-ring orb + floating "XX°C → YY%" readout pill
        if let Some(temp) = self.current_temp {
            let now_pt = to_screen(plot, temp, self.current_duty as f32);
            let now_col = if temp >= 85.0 {
                rgb(p.hot)
            } else if temp >= 70.0 {
                rgb(p.warm)
            } else if firefly {
                rgb(p.ok)
            } else {
                accent_hi
            };

            let cross_dash = LineDash {
                segments: &[3.5, 4.0],
                offset: 0,
            };
            // Vertical live crosshair line
            frame.stroke(
                &Path::line(Point::new(now_pt.x, plot.y), Point::new(now_pt.x, plot_b)),
                Stroke {
                    style: canvas::Style::Solid(Color { a: 0.58, ..now_col }),
                    width: 1.15,
                    line_dash: cross_dash,
                    ..Stroke::default()
                },
            );
            // Horizontal live crosshair line
            frame.stroke(
                &Path::line(Point::new(plot.x, now_pt.y), Point::new(plot_r, now_pt.y)),
                Stroke {
                    style: canvas::Style::Solid(Color { a: 0.42, ..now_col }),
                    width: 1.05,
                    line_dash: cross_dash,
                    ..Stroke::default()
                },
            );

            // Axis anchor ticks on bottom X & left Y axes
            frame.stroke(
                &Path::line(
                    Point::new(now_pt.x, plot_b - 3.0),
                    Point::new(now_pt.x, plot_b + 3.0),
                ),
                Stroke::default().with_color(now_col).with_width(2.0),
            );
            frame.stroke(
                &Path::line(
                    Point::new(plot.x - 3.0, now_pt.y),
                    Point::new(plot.x + 3.0, now_pt.y),
                ),
                Stroke::default().with_color(now_col).with_width(2.0),
            );

            let pulse = 0.5 + 0.5 * (self.anim_t * 2.8).sin();
            fill_radial_glow(&mut frame, now_pt, 17.0, now_col, 0.45, 5);
            let ring_r = 7.8 + pulse * 4.8;
            frame.stroke(
                &Path::circle(now_pt, ring_r),
                Stroke::default()
                    .with_color(Color {
                        a: 0.56 * (1.0 - pulse * 0.45),
                        ..now_col
                    })
                    .with_width(1.25),
            );
            frame.fill(&Path::circle(now_pt, 4.8), now_col);
            frame.fill(&Path::circle(now_pt, 2.2), Color::WHITE);

            // Floating "XX°C → YY%" readout pill right on the canvas
            let label = format!(
                "{} → {}%",
                self.temp_unit.format_temp(temp),
                self.current_duty
            );
            let ph = 22.0_f32;
            let pw = (label.chars().count() as f32 * 6.5 + 26.0).max(96.0);
            let lx = (now_pt.x - pw * 0.5).clamp(
                plot.x + 6.0,
                (plot_r - pw - 6.0).max(plot.x + 6.0),
            );
            let ly = if self.current_duty > 80 {
                (now_pt.y + 16.0).min(plot_b - ph - 6.0)
            } else {
                (now_pt.y - ph - 14.0).max(plot.y + 6.0)
            };
            let pill_rad = iced::border::Radius::from(if p.square_corners { 0.0 } else { 6.5 });
            frame.fill(
                &Path::rounded_rectangle(
                    Point::new(lx, ly + 2.0),
                    Size::new(pw, ph),
                    pill_rad,
                ),
                Color::from_rgba(0.0, 0.0, 0.0, 0.36),
            );
            let pill = Path::rounded_rectangle(Point::new(lx, ly), Size::new(pw, ph), pill_rad);
            frame.fill(&pill, rgba(p.ground, 0.94));
            frame.stroke(
                &pill,
                Stroke::default()
                    .with_color(Color { a: 0.72, ..now_col })
                    .with_width(1.1),
            );
            frame.fill(&Path::circle(Point::new(lx + 10.0, ly + ph * 0.5), 3.1), now_col);
            frame.fill_text(canvas::Text {
                content: label,
                position: Point::new(lx + 18.0, ly + ph * 0.5),
                color: rgb(p.ink),
                size: iced::Pixels(10.8),
                font: crate::theme::mono_font(),
                align_y: iced::alignment::Vertical::Center,
                ..canvas::Text::default()
            });
        }

        // 7. Interactive hover vertical probe + curve dot + floating readout pill
        let hovered_pos = cursor.position_in(bounds).filter(|pt| {
            pt.x >= plot.x && pt.x <= plot_r && pt.y >= plot.y && pt.y <= plot_b
        });
        if let Some(hpos) = hovered_pos
            && state.dragging_idx.is_none()
        {
            let h_temp = TEMP_MIN + ((hpos.x - plot.x) / plot.width) * (TEMP_MAX - TEMP_MIN);
            let h_duty = eval_curve(self.points, h_temp);
            let on_curve = to_screen(plot, h_temp, h_duty);

            let probe_grad = gradient::Linear::new(
                Point::new(on_curve.x, plot.y),
                Point::new(on_curve.x, plot_b),
            )
            .add_stop(0.0, rgba(p.ink, 0.06))
            .add_stop(0.3, rgba(p.ink, 0.44))
            .add_stop(1.0, rgba(p.ink, 0.14));
            frame.stroke(
                &Path::line(Point::new(on_curve.x, plot.y), Point::new(on_curve.x, plot_b)),
                Stroke {
                    style: canvas::Style::Gradient(Gradient::Linear(probe_grad)),
                    width: 1.0,
                    ..Stroke::default()
                },
            );
            fill_radial_glow(&mut frame, on_curve, 9.5, accent_hi, 0.35, 3);
            frame.fill(&Path::circle(on_curve, 5.0), rgb(p.well));
            frame.stroke(
                &Path::circle(on_curve, 4.3),
                Stroke::default().with_color(accent_hi).with_width(1.8),
            );

            let tip = format!("{} → {:.0}%", self.temp_unit.format_temp(h_temp), h_duty);
            let tw = (tip.chars().count() as f32 * 6.5 + 16.0).max(78.0);
            let tx = if on_curve.x + 12.0 + tw > plot_r {
                (on_curve.x - tw - 10.0).max(plot.x + 4.0)
            } else {
                on_curve.x + 10.0
            };
            let ty = (on_curve.y + 10.0).min(plot_b - 24.0);
            let tip_rect = Path::rounded_rectangle(
                Point::new(tx, ty),
                Size::new(tw, 21.0),
                iced::border::Radius::from(if p.square_corners { 0.0 } else { 6.0 }),
            );
            frame.fill(&tip_rect, rgba(p.raised, 0.95));
            frame.stroke(
                &tip_rect,
                Stroke::default()
                    .with_color(Color { a: 0.72, ..accent })
                    .with_width(1.05),
            );
            frame.fill_text(canvas::Text {
                content: tip,
                position: Point::new(tx + tw * 0.5, ty + 10.5),
                color: accent_hi,
                size: iced::Pixels(10.5),
                font: crate::theme::mono_font(),
                align_x: iced::alignment::Horizontal::Center.into(),
                align_y: iced::alignment::Vertical::Center,
                ..canvas::Text::default()
            });
        }

        // 8. Luminous tactile milled-hardware control handles + live drag readout pill
        for (i, pt) in self.points.iter().enumerate() {
            let sp = to_screen(plot, pt.temp as f32, pt.duty as f32);
            let active = state.dragging_idx == Some(i);
            let hovered = hovered_pos.is_some_and(|hp| {
                ((sp.x - hp.x).powi(2) + (sp.y - hp.y).powi(2)).sqrt() <= 14.0
            });
            let r = if active {
                9.2
            } else if hovered {
                8.0
            } else {
                6.8
            };

            fill_radial_glow(
                &mut frame,
                sp,
                if active || hovered { r + 9.0 } else { r + 4.5 },
                if active { accent_hi } else { accent },
                if active {
                    0.48
                } else if hovered {
                    0.34
                } else {
                    0.18
                },
                4,
            );

            // Grounded drop shadow
            frame.fill(
                &Path::circle(Point::new(sp.x, sp.y + 1.8), r + 0.6),
                Color::from_rgba(0.0, 0.0, 0.0, 0.42),
            );

            // Outer vertical bevel knob ring
            let knob_g = gradient::Linear::new(
                Point::new(sp.x, sp.y - r),
                Point::new(sp.x, sp.y + r),
            )
            .add_stop(0.0, mix_color(accent_hi, Color::WHITE, if active { 0.42 } else { 0.22 }))
            .add_stop(1.0, accent);
            frame.fill(&Path::circle(sp, r), Fill::from(Gradient::Linear(knob_g)));
            frame.stroke(
                &Path::circle(sp, r),
                Stroke::default()
                    .with_color(mix_color(accent_hi, Color::WHITE, 0.35))
                    .with_width(0.95),
            );

            // Recessed dark inner well + luminous jewel pip
            frame.fill(&Path::circle(sp, r * 0.56), rgb(p.ground));
            frame.fill(
                &Path::circle(sp, r * 0.28),
                if active || hovered {
                    Color::WHITE
                } else {
                    accent_hi
                },
            );

            if active {
                let drag_lbl = format!(
                    "{} → {}%",
                    self.temp_unit.format_temp(pt.temp as f32),
                    pt.duty
                );
                let dw = (drag_lbl.chars().count() as f32 * 6.5 + 16.0).max(80.0);
                let dx = (sp.x - dw * 0.5).clamp(plot.x + 4.0, (plot_r - dw - 4.0).max(plot.x + 4.0));
                let dy = if sp.y - 34.0 >= plot.y + 4.0 {
                    sp.y - 34.0
                } else {
                    (sp.y + 14.0).min(plot_b - 24.0)
                };
                let d_pill = Path::rounded_rectangle(
                    Point::new(dx, dy),
                    Size::new(dw, 21.0),
                    iced::border::Radius::from(if p.square_corners { 0.0 } else { 6.0 }),
                );
                frame.fill(&d_pill, rgba(p.raised, 0.96));
                frame.stroke(
                    &d_pill,
                    Stroke::default().with_color(accent_hi).with_width(1.2),
                );
                frame.fill_text(canvas::Text {
                    content: drag_lbl,
                    position: Point::new(dx + dw * 0.5, dy + 10.5),
                    color: rgb(p.ink),
                    size: iced::Pixels(10.5),
                    font: crate::theme::mono_font(),
                    align_x: iced::alignment::Horizontal::Center.into(),
                    align_y: iced::alignment::Vertical::Center,
                    ..canvas::Text::default()
                });
            }
        }

        vec![static_geom, frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.dragging_idx.is_some() {
            return mouse::Interaction::Grabbing;
        }
        if let Some(pos) = cursor.position_in(bounds) {
            let plot = plot_rect(bounds);
            for pt in self.points {
                let sp = to_screen(plot, pt.temp as f32, pt.duty as f32);
                if ((sp.x - pos.x).powi(2) + (sp.y - pos.y).powi(2)).sqrt() <= 14.0 {
                    return mouse::Interaction::Grab;
                }
            }
            if pos.x >= plot.x
                && pos.x <= plot.x + plot.width
                && pos.y >= plot.y
                && pos.y <= plot.y + plot.height
            {
                return mouse::Interaction::Crosshair;
            }
        }
        mouse::Interaction::default()
    }
}
