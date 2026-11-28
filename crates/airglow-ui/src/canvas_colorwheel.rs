//! Interactive GPU-rendered HSV Color Wheel (`ColorWheelCanvas`) with cached smooth
//! 72-wedge Hue-Saturation disc, recessed plum-glass bezel, glowing selector puck,
//! and responsive click-and-drag RGB picking.

use std::cell::Cell;
use std::collections::hash_map::DefaultHasher;
use std::f32::consts::PI;
use std::hash::{Hash, Hasher};

use airglow_core::config::DesignPalette;
use airglow_core::effects::hsv_to_rgb;
use iced::mouse;
use iced::widget::canvas::{
    self, Action, Event, Fill, Frame, Geometry, Gradient, LineCap, Path, Stroke, gradient, path::Arc,
};
use iced::{Color, Point, Rectangle, Renderer, Theme};

use crate::theme::{rgb, rgba};

#[derive(Debug, Clone)]
pub enum ColorWheelMessage {
    PickRgb([u8; 3]),
}

pub struct ColorWheelCanvas {
    pub rgb: [u8; 3],
    pub palette: DesignPalette,
    pub accent_override: Option<[u8; 3]>,
}

#[derive(Default)]
pub struct ColorWheelState {
    dragging: bool,
    wheel_cache: canvas::Cache,
    wheel_key: Cell<u64>,
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

fn resolve_accent(p: DesignPalette, accent_override: Option<[u8; 3]>) -> Color {
    if let Some([r, g, b]) = accent_override {
        Color::from_rgb8(r, g, b)
    } else if is_firefly_palette(p) {
        Color::from_rgb8(242, 169, 196)
    } else {
        rgb(p.brass)
    }
}

fn wheel_geometry(bounds: Rectangle) -> (Point, f32) {
    let center = Point::new(bounds.width * 0.5, bounds.height * 0.5);
    // Reserve 11.5px rim margin for the recessed bezel + 4-ring outer jewel ambient halo
    let radius = (bounds.width.min(bounds.height) * 0.5 - 11.5).max(16.0);
    (center, radius)
}

fn rgb_to_hsv(c: [u8; 3]) -> (f32, f32, f32) {
    let r = c[0] as f32 / 255.0;
    let g = c[1] as f32 / 255.0;
    let b = c[2] as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    let h = if delta < 1e-5 {
        0.0
    } else if (max - r).abs() < 1e-5 {
        ((g - b) / delta).rem_euclid(6.0) / 6.0
    } else if (max - g).abs() < 1e-5 {
        (((b - r) / delta) + 2.0) / 6.0
    } else {
        (((r - g) / delta) + 4.0) / 6.0
    };
    let s = if max < 1e-5 { 0.0 } else { delta / max };
    (h.rem_euclid(1.0), s.clamp(0.0, 1.0), max.clamp(0.0, 1.0))
}

fn pick_rgb_at(bounds: Rectangle, pos: Point) -> [u8; 3] {
    let (center, radius) = wheel_geometry(bounds);
    let dx = pos.x - center.x;
    let dy = pos.y - center.y;
    let dist = (dx * dx + dy * dy).sqrt();
    let sat = (dist / radius).clamp(0.0, 1.0);
    let hue = (dy.atan2(dx) / (2.0 * PI)).rem_euclid(1.0);
    let [r, g, b] = hsv_to_rgb(hue, sat, 1.0);
    [
        (r.clamp(0.0, 1.0) * 255.0).round() as u8,
        (g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (b.clamp(0.0, 1.0) * 255.0).round() as u8,
    ]
}

fn cache_key(bounds: Rectangle, p: DesignPalette, accent_override: Option<[u8; 3]>) -> u64 {
    let mut h = DefaultHasher::new();
    ((bounds.width * 2.0).round() as i32).hash(&mut h);
    ((bounds.height * 2.0).round() as i32).hash(&mut h);
    p.ground.hash(&mut h);
    p.well.hash(&mut h);
    p.line.hash(&mut h);
    p.brass.hash(&mut h);
    accent_override.hash(&mut h);
    h.finish()
}

impl canvas::Program<ColorWheelMessage> for ColorWheelCanvas {
    type State = ColorWheelState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<ColorWheelMessage>> {
        let (center, radius) = wheel_geometry(bounds);

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let pos = cursor.position_in(bounds)?;
                let dist = ((pos.x - center.x).powi(2) + (pos.y - center.y).powi(2)).sqrt();
                if dist <= radius + 6.0 {
                    state.dragging = true;
                    return Some(
                        Action::publish(ColorWheelMessage::PickRgb(pick_rgb_at(bounds, pos)))
                            .and_capture(),
                    );
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if state.dragging
                    && let Some(pos) = cursor.position_from(bounds.position())
                {
                    return Some(
                        Action::publish(ColorWheelMessage::PickRgb(pick_rgb_at(bounds, pos)))
                            .and_capture(),
                    );
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.dragging {
                    state.dragging = false;
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
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let (center, radius) = wheel_geometry(bounds);
        let p = self.palette;
        let firefly = is_firefly_palette(p);
        let accent = resolve_accent(p, self.accent_override);

        // Layer 1 (CACHED): Recessed plum bezel + silky 72-wedge Hue-Saturation radial disc
        let key = cache_key(bounds, p, self.accent_override);
        if state.wheel_key.get() != key {
            state.wheel_cache.clear();
            state.wheel_key.set(key);
        }

        let static_wheel = state.wheel_cache.draw(renderer, bounds.size(), |frame| {
            // Soft outer recessed bezel ring
            frame.fill(
                &Path::circle(Point::new(center.x, center.y + 1.5), radius + 5.2),
                Color::from_rgba(0.02, 0.01, 0.05, 0.42),
            );
            let bezel_col = if firefly {
                Color::from_rgb8(24, 19, 36)
            } else {
                rgb(p.well)
            };
            frame.fill(&Path::circle(center, radius + 4.5), bezel_col);
            frame.stroke(
                &Path::circle(center, radius + 4.5),
                Stroke::default()
                    .with_color(Color { a: 0.40, ..accent })
                    .with_width(1.1),
            );

            // 72 overlapping linear-gradient wedges for silky continuous Hue + Saturation
            const WEDGES: usize = 72;
            let step = (2.0 * PI) / WEDGES as f32;
            let overlap = 0.014_f32;
            for i in 0..WEDGES {
                let a0 = i as f32 * step;
                let a1 = a0 + step + overlap;
                let a_mid = a0 + step * 0.5;
                let hue = (a_mid / (2.0 * PI)).rem_euclid(1.0);
                let [r, g, b] = hsv_to_rgb(hue, 1.0, 1.0);
                let rim_col = Color::from_rgb(r, g, b);

                let p0 = Point::new(center.x + a0.cos() * radius, center.y + a0.sin() * radius);
                let p1 = Point::new(center.x + a1.cos() * radius, center.y + a1.sin() * radius);
                let p_mid = Point::new(
                    center.x + a_mid.cos() * radius,
                    center.y + a_mid.sin() * radius,
                );

                let wedge = Path::new(|pb| {
                    pb.move_to(center);
                    pb.line_to(p0);
                    pb.line_to(p1);
                    pb.close();
                });
                let grad = gradient::Linear::new(center, p_mid)
                    .add_stop(0.0, Color::WHITE)
                    .add_stop(0.24, mix_color(Color::WHITE, rim_col, 0.28))
                    .add_stop(1.0, rim_col);
                frame.fill(&wedge, Fill::from(Gradient::Linear(grad)));
            }

            // Soft white center blend cap to hide wedge apexes cleanly
            frame.fill(
                &Path::circle(center, (radius * 0.08).max(2.5)),
                Color::from_rgba(1.0, 1.0, 1.0, 0.92),
            );

            // Crisp dark outer disc lip + 1px inner specular bevel ring + top-left glass catchlight
            frame.stroke(
                &Path::circle(center, radius),
                Stroke::default()
                    .with_color(Color::from_rgba(0.05, 0.03, 0.10, 0.58))
                    .with_width(1.6),
            );
            frame.stroke(
                &Path::circle(center, radius - 0.9),
                Stroke::default()
                    .with_color(Color::from_rgba(1.0, 1.0, 1.0, 0.24))
                    .with_width(1.0),
            );
            let catchlight = Path::new(|b| {
                b.arc(Arc {
                    center,
                    radius: radius - 1.1,
                    start_angle: iced::Radians(205.0_f32.to_radians()),
                    end_angle: iced::Radians(310.0_f32.to_radians()),
                });
            });
            frame.stroke(
                &catchlight,
                Stroke::default()
                    .with_color(Color::from_rgba(1.0, 1.0, 1.0, 0.44))
                    .with_width(1.1)
                    .with_line_cap(LineCap::Round),
            );
        });

        // Layer 2 (DYNAMIC): Soft 4-ring luminous jewel halo + crisp white-rimmed selector puck
        let mut frame = Frame::new(renderer, bounds.size());
        let current_col = Color::from_rgb8(self.rgb[0], self.rgb[1], self.rgb[2]);
        let (h, s, _v) = rgb_to_hsv(self.rgb);
        let angle = h * 2.0 * PI;
        let dist = s * (radius - 2.0);
        let puck = Point::new(
            center.x + angle.cos() * dist,
            center.y + angle.sin() * dist,
        );

        // Soft outer ambient jewel halo fading outward over 4 concentric rings (subtler on light themes)
        let halo_scale = if p.is_light { 0.56 } else { 1.0 };
        for &(dr, w_ring, base_a) in &[
            (5.1_f32, 2.3_f32, 0.30_f32),
            (6.8, 2.1, 0.20),
            (8.4, 1.9, 0.11),
            (10.0, 1.6, 0.05),
        ] {
            frame.stroke(
                &Path::circle(center, radius + dr),
                Stroke::default()
                    .with_color(Color {
                        a: (base_a * halo_scale).clamp(0.0, 0.35),
                        ..current_col
                    })
                    .with_width(w_ring),
            );
        }

        let puck_r = if state.dragging { 8.2 } else { 6.8 };
        if state.dragging {
            frame.fill(
                &Path::circle(puck, puck_r + 5.5),
                Color {
                    a: 0.26,
                    ..current_col
                },
            );
        }
        // Soft drop shadow beneath selector puck
        frame.fill(
            &Path::circle(Point::new(puck.x, puck.y + 1.4), puck_r + 1.8),
            Color::from_rgba(0.0, 0.0, 0.0, 0.48),
        );
        // Selected RGB fill + crisp dark outer hairline + white ring
        frame.fill(&Path::circle(puck, puck_r), current_col);
        frame.stroke(
            &Path::circle(puck, puck_r + 1.0),
            Stroke::default()
                .with_color(rgba(p.ground, 0.78))
                .with_width(1.3),
        );
        frame.stroke(
            &Path::circle(puck, puck_r),
            Stroke::default().with_color(Color::WHITE).with_width(2.1),
        );

        vec![static_wheel, frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.dragging {
            return mouse::Interaction::Grabbing;
        }
        if let Some(pos) = cursor.position_in(bounds) {
            let (center, radius) = wheel_geometry(bounds);
            let dist = ((pos.x - center.x).powi(2) + (pos.y - center.y).powi(2)).sqrt();
            if dist <= radius + 5.0 {
                return mouse::Interaction::Crosshair;
            }
        }
        mouse::Interaction::default()
    }
}
