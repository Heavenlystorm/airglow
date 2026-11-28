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
        x: 46.0,
        y: 16.0,
        width: (bounds.width - 62.0).max(60.0),
        height: (bounds.height - 44.0).max(60.0),
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
                    Color::from_rgb8(50, 39, 58)
                };
                gradient::Linear::new(
                    Point::new(bounds.width * 0.72, 0.0),
                    Point::new(bounds.width * 0.25, bounds.height),
                )
                .add_stop(0.0, top_tint)
                .add_stop(0.45, Color::from_rgb8(31, 27, 43))
                .add_stop(1.0, Color::from_rgb8(26, 23, 37))
            } else {
                gradient::Linear::new(Point::ORIGIN, Point::new(0.0, bounds.height))
                    .add_stop(0.0, rgb(p.well))
                    .add_stop(1.0, rgb(p.well))
            };
            frame.fill(
                &Path::rectangle(Point::ORIGIN, bounds.size()),
                Fill::from(Gradient::Linear(scope_bg)),
            );

            // 2. 90°–100° thermal safety band + crisp 45° diagonal hazard hatch lines
            let safety_x = to_screen(plot, SAFETY_TEMP, 100.0).x;
            let safety_w = (plot.x + plot.width - safety_x).max(0.0);
            frame.fill_rectangle(
                Point::new(safety_x, plot.y),
                Size::new(safety_w, plot.height),
                rgba(p.hot, 0.13),
            );
            if safety_w > 2.0 {
                let step = 9.0;
                let right = plot.x + plot.width;
                let bottom = plot.y + plot.height;
                let mut offset = -plot.height;
                while offset < safety_w + plot.height {
                    let mut x0 = safety_x + offset;
                    let mut y0 = plot.y;
                    let mut x1 = safety_x + offset + plot.height;
                    let mut y1 = bottom;
                    if x0 < safety_x {
                        y0 += safety_x - x0;
                        x0 = safety_x;
                    }
                    if x1 > right {
                        y1 -= x1 - right;
                        x1 = right;
                    }
                    if y0 < bottom && y1 > plot.y && x1 > x0 {
                        frame.stroke(
                            &Path::line(Point::new(x0, y0), Point::new(x1, y1)),
                            Stroke::default()
                                .with_color(rgba(p.hot, 0.22))
                                .with_width(1.0),
                        );
                    }
                    offset += step;
                }
                frame.stroke(
                    &Path::line(Point::new(safety_x, plot.y), Point::new(safety_x, bottom)),
                    Stroke {
                        style: canvas::Style::Solid(rgba(p.hot, 0.45)),
                        width: 1.0,
                        line_dash: LineDash {
                            segments: &[3.0, 4.0],
                            offset: 0,
                        },
                        ..Stroke::default()
                    },
                );
            }
            if safety_w >= 48.0 {
                let safety_lbl = if safety_w >= 96.0 {
                    "Always 100%"
                } else {
                    "Full speed"
                };
                let sw = (safety_lbl.len() as f32 * 5.8 + 10.0).min(safety_w - 8.0);
                let sx = safety_x + ((safety_w - sw) * 0.5).max(4.0);
                let sy = plot.y + 8.0;
                let badge = Path::rounded_rectangle(
                    Point::new(sx, sy),
                    Size::new(sw, 17.0),
                    iced::border::Radius::from(4.0),
                );
                frame.fill(&badge, rgba(p.ground, 0.72));
                frame.fill_text(canvas::Text {
                    content: safety_lbl.to_string(),
                    position: Point::new(sx + 5.0, sy + 2.5),
                    color: rgba(p.hot, 0.95),
                    size: iced::Pixels(9.5),
                    ..canvas::Text::default()
                });
            }

            // 3. 20% minimum fan stall band
            let floor_y = to_screen(plot, TEMP_MIN, MIN_FAN_DUTY as f32).y;
            frame.fill_rectangle(
                Point::new(plot.x, floor_y),
                Size::new(plot.width, (plot.y + plot.height - floor_y).max(0.0)),
                rgba(p.line, 0.16),
            );
            frame.stroke(
                &Path::line(
                    Point::new(plot.x, floor_y),
                    Point::new(plot.x + plot.width, floor_y),
                ),
                Stroke {
                    style: canvas::Style::Solid(rgba(p.line, 0.45)),
                    width: 1.0,
                    line_dash: LineDash {
                        segments: &[3.0, 4.0],
                        offset: 0,
                    },
                    ..Stroke::default()
                },
            );
            frame.fill_text(canvas::Text {
                content: "Below 20% some fans stall, so curves stay above it".to_string(),
                position: Point::new(plot.x + 8.0, (plot.y + plot.height - 16.0).max(plot.y)),
                color: rgba(p.ink_3, 0.78),
                size: iced::Pixels(10.0),
                ..canvas::Text::default()
            });

            // 4. Gridlines & axis labels (°C / °F aware)
            for t in [20, 30, 40, 50, 60, 70, 80, 90, 100] {
                let x = to_screen(plot, t as f32, 0.0).x;
                frame.stroke(
                    &Path::line(Point::new(x, plot.y), Point::new(x, plot.y + plot.height)),
                    Stroke::default()
                        .with_color(rgba(p.line, if t % 20 == 0 { 0.36 } else { 0.20 }))
                        .with_width(1.0),
                );
                let disp = self.temp_unit.convert(t as f32).round() as i32;
                frame.fill_text(canvas::Text {
                    content: format!("{disp}°"),
                    position: Point::new(x - 11.0, plot.y + plot.height + 6.0),
                    color: rgb(p.ink_3),
                    size: iced::Pixels(10.5),
                    ..canvas::Text::default()
                });
            }
            for d in [0, 20, 40, 60, 80, 100] {
                let y = to_screen(plot, TEMP_MIN, d as f32).y;
                frame.stroke(
                    &Path::line(Point::new(plot.x, y), Point::new(plot.x + plot.width, y)),
                    Stroke::default()
                        .with_color(rgba(p.line, if d % 40 == 0 { 0.36 } else { 0.20 }))
                        .with_width(1.0),
                );
                frame.fill_text(canvas::Text {
                    content: format!("{d}%"),
                    position: Point::new(6.0, y - 6.0),
                    color: rgb(p.ink_3),
                    size: iced::Pixels(10.5),
                    ..canvas::Text::default()
                });
            }
        });

        // Layer 2 (DYNAMIC): Curve, acoustic hysteresis band, live "Now" crosshair, hover probe & handles
        let mut frame = Frame::new(renderer, bounds.size());

        // 5. Fan curve gradient area wash + optional hysteresis band + multi-pass glowing stroke
        if !self.points.is_empty() {
            let mut curve_pts: Vec<Point> = Vec::with_capacity(self.points.len() + 2);
            let first = self.points[0];
            curve_pts.push(to_screen(plot, TEMP_MIN, first.duty as f32));
            for pt in self.points {
                curve_pts.push(to_screen(plot, pt.temp as f32, pt.duty as f32));
            }
            let last = self.points[self.points.len() - 1];
            curve_pts.push(to_screen(plot, TEMP_MAX, last.duty as f32));

            // Acoustic cooling hysteresis deadband ribbon (shifted left by hysteresis_c)
            let hyst = self.hysteresis_c.clamp(0.0, 10.0);
            if hyst >= 0.4 {
                let mut hyst_pts: Vec<Point> = Vec::with_capacity(self.points.len() + 2);
                hyst_pts.push(to_screen(plot, TEMP_MIN, first.duty as f32));
                for pt in self.points {
                    let ht = (pt.temp as f32 - hyst).clamp(TEMP_MIN, TEMP_MAX);
                    hyst_pts.push(to_screen(plot, ht, pt.duty as f32));
                }
                hyst_pts.push(to_screen(plot, TEMP_MAX, last.duty as f32));

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
                frame.fill(&ribbon, Color { a: 0.11, ..hyst_col });

                let hyst_edge = Path::new(|b| {
                    b.move_to(hyst_pts[0]);
                    for &pt in &hyst_pts[1..] {
                        b.line_to(pt);
                    }
                });
                frame.stroke(
                    &hyst_edge,
                    Stroke {
                        style: canvas::Style::Solid(Color { a: 0.34, ..hyst_col }),
                        width: 1.0,
                        line_dash: LineDash {
                            segments: &[3.0, 4.0],
                            offset: 0,
                        },
                        ..Stroke::default()
                    },
                );
            }

            let area = Path::new(|b| {
                b.move_to(Point::new(curve_pts[0].x, plot.y + plot.height));
                for &pt in &curve_pts {
                    b.line_to(pt);
                }
                b.line_to(Point::new(
                    curve_pts[curve_pts.len() - 1].x,
                    plot.y + plot.height,
                ));
                b.close();
            });
            let area_grad = gradient::Linear::new(
                Point::new(plot.x, plot.y),
                Point::new(plot.x, plot.y + plot.height),
            )
            .add_stop(0.0, Color { a: 0.24, ..accent })
            .add_stop(
                0.65,
                Color {
                    a: 0.06,
                    ..if firefly && self.accent_override.is_none() {
                        rgb(p.ok)
                    } else {
                        accent
                    }
                },
            )
            .add_stop(1.0, Color { a: 0.0, ..accent });
            frame.fill(&area, Fill::from(Gradient::Linear(area_grad)));

            let curve_path = Path::new(|b| {
                b.move_to(curve_pts[0]);
                for &pt in &curve_pts[1..] {
                    b.line_to(pt);
                }
            });
            let stroke_grad = gradient::Linear::new(
                Point::new(plot.x, plot.y),
                Point::new(plot.x + plot.width, plot.y),
            )
            .add_stop(0.0, accent)
            .add_stop(0.74, accent_hi)
            .add_stop(0.92, rgb(p.hot))
            .add_stop(1.0, rgb(p.hot));

            // Soft living-light bloom behind curve
            frame.stroke(
                &curve_path,
                Stroke {
                    style: canvas::Style::Solid(Color { a: 0.13, ..accent }),
                    width: 8.5,
                    line_cap: LineCap::Round,
                    line_join: LineJoin::Round,
                    ..Stroke::default()
                },
            );
            frame.stroke(
                &curve_path,
                Stroke {
                    style: canvas::Style::Solid(Color { a: 0.26, ..accent }),
                    width: 4.5,
                    line_cap: LineCap::Round,
                    line_join: LineJoin::Round,
                    ..Stroke::default()
                },
            );
            frame.stroke(
                &curve_path,
                Stroke {
                    style: canvas::Style::Gradient(Gradient::Linear(stroke_grad)),
                    width: 2.5,
                    line_cap: LineCap::Round,
                    line_join: LineJoin::Round,
                    ..Stroke::default()
                },
            );
            frame.stroke(
                &curve_path,
                Stroke {
                    style: canvas::Style::Solid(Color::from_rgba(1.0, 1.0, 1.0, 0.28)),
                    width: 0.85,
                    line_cap: LineCap::Round,
                    line_join: LineJoin::Round,
                    ..Stroke::default()
                },
            );
        }

        // 6. Live "Now" crosshair + pulsing ring + glowing dot
        if let Some(temp) = self.current_temp {
            let now_pt = to_screen(plot, temp, self.current_duty as f32);
            let now_col = if firefly { rgb(p.ok) } else { rgb(p.ink) };
            frame.stroke(
                &Path::line(
                    Point::new(now_pt.x, plot.y),
                    Point::new(now_pt.x, plot.y + plot.height),
                ),
                Stroke {
                    style: canvas::Style::Solid(Color { a: 0.62, ..now_col }),
                    width: 1.1,
                    line_dash: LineDash {
                        segments: &[3.0, 4.0],
                        offset: 0,
                    },
                    ..Stroke::default()
                },
            );
            let pulse = 0.5 + 0.5 * (self.anim_t * 2.8).sin();
            let ring_r = 7.5 + pulse * 4.5;
            frame.fill(
                &Path::circle(now_pt, ring_r),
                Color {
                    a: 0.14 * (1.0 - pulse * 0.35),
                    ..now_col
                },
            );
            frame.stroke(
                &Path::circle(now_pt, ring_r),
                Stroke::default()
                    .with_color(Color {
                        a: 0.58 * (1.0 - pulse * 0.45),
                        ..now_col
                    })
                    .with_width(1.2),
            );
            frame.fill(&Path::circle(now_pt, 4.6), now_col);
            frame.fill(&Path::circle(now_pt, 2.2), Color::WHITE);

            let label = format!(
                "Now {}, {}%",
                self.temp_unit.format_temp(temp),
                self.current_duty
            );
            let lw = (label.len() as f32 * 6.4 + 14.0).max(94.0);
            let lx = (now_pt.x - lw * 0.5).clamp(
                plot.x + 6.0,
                (plot.x + plot.width - lw - 6.0).max(plot.x + 6.0),
            );
            let ly = if self.current_duty > 82 {
                (now_pt.y + 18.0).min(plot.y + plot.height - 24.0)
            } else {
                (now_pt.y - 34.0).max(plot.y + 4.0)
            };
            let pill = Path::rounded_rectangle(
                Point::new(lx, ly),
                Size::new(lw, 20.0),
                iced::border::Radius::from(6.0),
            );
            frame.fill(&pill, rgba(p.ground, 0.92));
            frame.stroke(
                &pill,
                Stroke::default()
                    .with_color(Color { a: 0.55, ..now_col })
                    .with_width(1.0),
            );
            frame.fill_text(canvas::Text {
                content: label,
                position: Point::new(lx + 7.0, ly + 3.5),
                color: rgb(p.ink),
                size: iced::Pixels(10.5),
                ..canvas::Text::default()
            });
        }

        // 7. Interactive hover vertical probe + curve dot + floating readout pill
        let hovered_pos = cursor.position_in(bounds).filter(|pt| {
            pt.x >= plot.x
                && pt.x <= plot.x + plot.width
                && pt.y >= plot.y
                && pt.y <= plot.y + plot.height
        });
        if let Some(hpos) = hovered_pos
            && state.dragging_idx.is_none()
        {
            let h_temp = TEMP_MIN + ((hpos.x - plot.x) / plot.width) * (TEMP_MAX - TEMP_MIN);
            let h_duty = eval_curve(self.points, h_temp);
            let on_curve = to_screen(plot, h_temp, h_duty);

            let probe_grad = gradient::Linear::new(
                Point::new(on_curve.x, plot.y),
                Point::new(on_curve.x, plot.y + plot.height),
            )
            .add_stop(0.0, rgba(p.ink, 0.05))
            .add_stop(0.3, rgba(p.ink, 0.42))
            .add_stop(1.0, rgba(p.ink, 0.14));
            frame.stroke(
                &Path::line(
                    Point::new(on_curve.x, plot.y),
                    Point::new(on_curve.x, plot.y + plot.height),
                ),
                Stroke {
                    style: canvas::Style::Gradient(Gradient::Linear(probe_grad)),
                    width: 1.0,
                    ..Stroke::default()
                },
            );
            frame.fill(&Path::circle(on_curve, 5.0), rgb(p.well));
            frame.stroke(
                &Path::circle(on_curve, 4.2),
                Stroke::default().with_color(accent_hi).with_width(1.8),
            );

            let tip = format!("{} → {:.0}%", self.temp_unit.format_temp(h_temp), h_duty);
            let tw = (tip.len() as f32 * 6.4 + 14.0).max(74.0);
            let tx = if on_curve.x + 12.0 + tw > plot.x + plot.width {
                (on_curve.x - tw - 10.0).max(plot.x + 4.0)
            } else {
                on_curve.x + 10.0
            };
            let ty = (on_curve.y + 8.0).min(plot.y + plot.height - 24.0);
            let tip_rect = Path::rounded_rectangle(
                Point::new(tx, ty),
                Size::new(tw, 20.0),
                iced::border::Radius::from(6.0),
            );
            frame.fill(&tip_rect, rgba(p.raised, 0.94));
            frame.stroke(
                &tip_rect,
                Stroke::default()
                    .with_color(Color { a: 0.68, ..accent })
                    .with_width(1.0),
            );
            frame.fill_text(canvas::Text {
                content: tip,
                position: Point::new(tx + 7.0, ty + 3.5),
                color: accent_hi,
                size: iced::Pixels(10.5),
                ..canvas::Text::default()
            });
        }

        // 8. Hollow accent control handles with hover/drag glow
        for (i, pt) in self.points.iter().enumerate() {
            let sp = to_screen(plot, pt.temp as f32, pt.duty as f32);
            let active = state.dragging_idx == Some(i);
            let hovered = hovered_pos.is_some_and(|hp| {
                ((sp.x - hp.x).powi(2) + (sp.y - hp.y).powi(2)).sqrt() <= 13.0
            });
            let r = if active {
                8.0
            } else if hovered {
                7.0
            } else {
                6.0
            };
            if active || hovered {
                frame.fill(&Path::circle(sp, r + 5.5), Color { a: 0.24, ..accent });
            }
            frame.fill(&Path::circle(sp, r), rgb(p.well));
            frame.stroke(
                &Path::circle(sp, r),
                Stroke::default()
                    .with_color(if active || hovered { accent_hi } else { accent })
                    .with_width(if active { 2.4 } else { 2.0 }),
            );
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
