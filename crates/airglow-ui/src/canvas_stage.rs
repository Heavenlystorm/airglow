//! Interactive GPU-accelerated Living Rig stage canvas with smart multi-layer `canvas::Cache`
//! separation (static chassis/PCB/keycaps cached; dynamic LEDs, spinning blades, AIO LCD temp
//! screen, GPU micro-badge, airflow ribbons, hover Zone X-Ray tooltip, and Arrange snap
//! crosshairs rendered smoothly per frame).

use std::cell::Cell;
use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::f32::consts::PI;
use std::hash::{Hash, Hasher};

use airglow_core::config::{DesignPalette, RigStyle, TempUnit, ZoneOverride};
use airglow_core::effects::EffectId;
use airglow_core::engine::ZoneFrame;
use airglow_core::geometry::{CANVAS_H, CANVAS_W, ZoneLayout};
use iced::mouse;
use iced::widget::canvas::{
    self, Action, Event, Fill, Frame, Geometry, Gradient, LineCap, LineDash, LineJoin, Path,
    Stroke, gradient, path::Arc,
};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};

use crate::theme::{rgb, rgba};

#[derive(Debug, Clone)]
pub enum StageMessage {
    SelectZone(Option<String>),
    MoveZone { zone_id: String, x: f32, y: f32 },
}

pub struct CaseStageCanvas<'a> {
    pub frames: &'a [ZoneFrame],
    pub zone_overrides: &'a BTreeMap<String, ZoneOverride>,
    pub palette: DesignPalette,
    pub accent_override: Option<[u8; 3]>,
    pub rig_style: RigStyle,
    pub airflow_lines: bool,
    pub spinning_fans: bool,
    pub ambient_bloom: bool,
    pub lights_on: bool,
    pub show_labels: bool,
    pub arrange_mode: bool,
    pub selected_zone: Option<&'a str>,
    pub active_effect: EffectId,
    pub temp_unit: TempUnit,
    pub anim_t: f32,
    pub fan_duty: u8,
    pub cpu_temp: Option<f32>,
    pub gpu_temp: Option<f32>,
    pub ambient_rgb: [u8; 3],
    pub eyebrow: Option<&'a str>,
    pub animated: bool,
}

#[derive(Default)]
pub struct StageDragState {
    dragging: Option<(String, [f32; 2])>,
    static_cache: canvas::Cache,
    static_key: Cell<u64>,
    dynamic_cache: canvas::Cache,
    dynamic_key: Cell<u64>,
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
    } else if is_firefly_palette(p) {
        (Color::from_rgb8(242, 169, 196), rgb(p.brass_hi))
    } else {
        (rgb(p.brass), rgb(p.brass_hi))
    }
}

fn rig_style_id(style: RigStyle) -> u8 {
    match style {
        RigStyle::Living => 0,
        RigStyle::Architectural => 1,
        RigStyle::Minimal => 2,
    }
}

fn stage_world_bounds(frames: &[ZoneFrame], arrange_mode: bool) -> (f32, f32, f32, f32) {
    let mut x0 = 0.0_f32;
    let mut y0 = 0.0_f32;
    let mut x1 = CANVAS_W;
    let mut y1 = CANVAS_H;

    for z in frames {
        if !z.enabled && !arrange_mode {
            continue;
        }
        for &p in &z.canvas_pos {
            x0 = x0.min(p[0] - 0.05);
            y0 = y0.min(p[1] - 0.05);
            x1 = x1.max(p[0] + 0.05);
            y1 = y1.max(p[1] + 0.05);
        }
        for &([cx, cy], r) in &z.fan_rings {
            x0 = x0.min(cx - r - 0.04);
            y0 = y0.min(cy - r - 0.04);
            x1 = x1.max(cx + r + 0.04);
            y1 = y1.max(cy + r + 0.04);
        }
        if let ZoneLayout::Matrix {
            rows,
            cols,
            x,
            y,
            angle,
            length,
            ..
        } = &z.layout
        {
            let r_cnt = (*rows).max(1) as f32;
            let c_cnt = (*cols).max(1) as f32;
            let hw = *length * 0.5 + 0.055;
            let hh = (*length * r_cnt / c_cnt) * 0.5 + 0.055;
            let rad = angle.to_radians();
            let (ca, sa) = (rad.cos(), rad.sin());
            for &(lx, ly) in &[(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)] {
                let gx = *x + ca * lx - sa * ly;
                let gy = *y + sa * lx + ca * ly;
                x0 = x0.min(gx);
                y0 = y0.min(gy);
                x1 = x1.max(gx);
                y1 = y1.max(gy);
            }
        }
    }
    (x0, y0, x1, y1)
}

const TARGET_RIG_ASPECT: f32 = 1.62;

fn viewport(
    bounds: Rectangle,
    has_eyebrow: bool,
    show_labels: bool,
    frames: &[ZoneFrame],
    arrange_mode: bool,
) -> (f32, f32, f32, (f32, f32, f32, f32)) {
    let wb = stage_world_bounds(frames, arrange_mode);
    let world_w = (wb.2 - wb.0).max(0.5);
    let world_h = (wb.3 - wb.1).max(0.5);
    let pad_top = if has_eyebrow {
        (bounds.height * 0.075).clamp(20.0, 30.0)
    } else if show_labels || arrange_mode {
        (bounds.height * 0.055).clamp(16.0, 24.0)
    } else {
        (bounds.height * 0.04).clamp(6.0, 14.0)
    };
    let pad_bot = (bounds.height * 0.04).clamp(6.0, 16.0);
    let avail_h = (bounds.height - pad_top - pad_bot)
        .min(bounds.height * 0.92)
        .max(36.0);
    let avail_w = (bounds.width * 0.90)
        .min(bounds.width - 16.0)
        .max(40.0);
    let scale = (avail_w / world_w).min(avail_h / world_h);
    let ox = (bounds.width - world_w * scale) * 0.5 - wb.0 * scale;
    let oy = pad_top + (avail_h - world_h * scale) * 0.5 - wb.1 * scale;
    (scale, ox, oy, wb)
}

fn rig_scale_factor(scale: f32) -> f32 {
    (scale / 260.0).clamp(0.55, 2.40)
}

fn zone_center(zone: &ZoneFrame) -> Option<[f32; 2]> {
    if zone.canvas_pos.is_empty() {
        let (x, y) = zone.layout.center();
        return Some([x, y]);
    }
    let (mut sx, mut sy) = (0.0f32, 0.0f32);
    for p in &zone.canvas_pos {
        sx += p[0];
        sy += p[1];
    }
    let n = zone.canvas_pos.len() as f32;
    Some([sx / n, sy / n])
}

fn zone_bounds(zone: &ZoneFrame) -> Option<(f32, f32, f32, f32)> {
    let mut x0 = f32::INFINITY;
    let mut y0 = f32::INFINITY;
    let mut x1 = f32::NEG_INFINITY;
    let mut y1 = f32::NEG_INFINITY;
    for p in &zone.canvas_pos {
        x0 = x0.min(p[0]);
        y0 = y0.min(p[1]);
        x1 = x1.max(p[0]);
        y1 = y1.max(p[1]);
    }
    for &([cx, cy], r) in &zone.fan_rings {
        x0 = x0.min(cx - r);
        y0 = y0.min(cy - r);
        x1 = x1.max(cx + r);
        y1 = y1.max(cy + r);
    }
    if x0.is_finite() {
        Some((x0, y0, x1, y1))
    } else {
        None
    }
}

fn shorten_canvas_zone_label(name: &str, group_subzones: bool) -> String {
    let mut s = name.trim();
    for suffix in [" (case version)", " (Case Version)"] {
        if let Some(stripped) = s.strip_suffix(suffix) {
            s = stripped.trim();
        }
    }
    if group_subzones
        && let Some((prefix, _sub)) = s.split_once(": ")
        && prefix.trim().len() >= 3
    {
        s = prefix.trim();
    } else {
        for suffix in [
            ": Right fan",
            ": Center fan",
            ": Left fan",
            ": Top fan",
            ": Bottom fan",
            ": Front fan",
            ": Rear fan",
            ": Side",
            ": Logo",
            ": Backplate",
        ] {
            if let Some(stripped) = s.strip_suffix(suffix) {
                s = stripped.trim();
            }
        }
    }
    s.to_string()
}

#[derive(Debug, Clone)]
struct ResolvedZoneLabel {
    tag: String,
    anchor: Point,
    rect: Rectangle,
    swatch_col: Color,
    is_selected: bool,
    is_hovered: bool,
    has_override: bool,
    any_enabled: bool,
}

/// Groups duplicate device sub-zones (e.g. 4 `RTX 5080 Gaming OC: ...` strips -> 1 clean pill)
/// and runs a vertical + horizontal collision-avoidance solver enforcing >= 22px vertical pitch
/// so NO TWO LABELS EVER OVERLAP on any viewport!
#[allow(clippy::too_many_arguments)]
fn resolve_stage_labels(
    frames: &[ZoneFrame],
    zone_overrides: &BTreeMap<String, ZoneOverride>,
    selected_zone: Option<&str>,
    hovered_zone_id: Option<&str>,
    show_labels: bool,
    arrange_mode: bool,
    lights_on: bool,
    ox: f32,
    oy: f32,
    scale: f32,
    bounds: Rectangle,
    has_eyebrow: bool,
) -> Vec<ResolvedZoneLabel> {
    struct RawGroup {
        base_name: String,
        sum_x: f32,
        sum_y: f32,
        weight: f32,
        bx0: f32,
        by0: f32,
        bx1: f32,
        by1: f32,
        r_sum: f32,
        g_sum: f32,
        b_sum: f32,
        col_count: f32,
        is_selected: bool,
        is_hovered: bool,
        has_override: bool,
        any_enabled: bool,
    }

    let mut groups: Vec<RawGroup> = Vec::new();

    for zone in frames {
        if !zone.enabled && !arrange_mode {
            continue;
        }
        let is_sel = selected_zone == Some(zone.zone_id.as_str());
        if !(show_labels || arrange_mode || is_sel) {
            continue;
        }
        let Some(center) = zone_center(zone) else {
            continue;
        };
        let is_hov = hovered_zone_id == Some(zone.zone_id.as_str());
        let has_ov = zone_overrides.contains_key(&zone.zone_id);
        let base_name = shorten_canvas_zone_label(&zone.zone_name, !arrange_mode);
        let (zb_x0, zb_y0, zb_x1, zb_y1) = zone_bounds(zone).unwrap_or((
            center[0] - 0.035,
            center[1] - 0.035,
            center[0] + 0.035,
            center[1] + 0.035,
        ));
        let avg_c = zone_average_color(zone);
        let w = (zone.canvas_pos.len().max(1) as f32).sqrt();

        let existing = if !arrange_mode {
            groups.iter_mut().find(|g| g.base_name == base_name)
        } else {
            None
        };

        if let Some(g) = existing {
            g.sum_x += center[0] * w;
            g.sum_y += center[1] * w;
            g.weight += w;
            g.bx0 = g.bx0.min(zb_x0);
            g.by0 = g.by0.min(zb_y0);
            g.bx1 = g.bx1.max(zb_x1);
            g.by1 = g.by1.max(zb_y1);
            if zone.enabled {
                g.r_sum += avg_c.r;
                g.g_sum += avg_c.g;
                g.b_sum += avg_c.b;
                g.col_count += 1.0;
            }
            g.is_selected |= is_sel;
            g.is_hovered |= is_hov;
            g.has_override |= has_ov;
            g.any_enabled |= zone.enabled;
        } else {
            groups.push(RawGroup {
                base_name,
                sum_x: center[0] * w,
                sum_y: center[1] * w,
                weight: w,
                bx0: zb_x0,
                by0: zb_y0,
                bx1: zb_x1,
                by1: zb_y1,
                r_sum: if zone.enabled { avg_c.r } else { 0.0 },
                g_sum: if zone.enabled { avg_c.g } else { 0.0 },
                b_sum: if zone.enabled { avg_c.b } else { 0.0 },
                col_count: if zone.enabled { 1.0 } else { 0.0 },
                is_selected: is_sel,
                is_hovered: is_hov,
                has_override: has_ov,
                any_enabled: zone.enabled,
            });
        }
    }

    if groups.is_empty() {
        return Vec::new();
    }

    let top_min_y = if has_eyebrow { 24.0 } else { 6.0 };
    let pill_h = 18.0_f32;
    let bot_max_y = (bounds.height - pill_h - 5.0).max(top_min_y);
    let mid_world_x = CANVAS_W * 0.56;

    struct WorkPill {
        tag: String,
        anchor: Point,
        pref_x: f32,
        pref_y: f32,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        swatch_col: Color,
        is_selected: bool,
        is_hovered: bool,
        has_override: bool,
        any_enabled: bool,
        spine_rank: usize,
    }

    // Count how many zones cluster along the central motherboard/GPU spine so we can stagger X gently
    let mut spine_counter = 0usize;
    let mut pills: Vec<WorkPill> = groups
        .into_iter()
        .map(|g| {
            let cx_world = g.sum_x / g.weight.max(0.001);
            let cy_world = g.sum_y / g.weight.max(0.001);
            let anchor = Point::new(ox + cx_world * scale, oy + cy_world * scale);
            let z_top_px = oy + g.by0 * scale;
            let z_bot_px = oy + g.by1 * scale;

            let tag = if g.has_override {
                format!("{} *", g.base_name)
            } else if !g.any_enabled {
                format!("{} (off)", g.base_name)
            } else {
                g.base_name
            };

            let pw = (tag.chars().count() as f32 * 5.95 + 22.0)
                .clamp(50.0, (bounds.width - 16.0).clamp(50.0, 192.0));
            let max_x = (bounds.width - pw - 6.0).max(6.0);

            let on_center_spine = (cx_world - mid_world_x).abs() < 0.26
                && cy_world > 0.18
                && cy_world < 0.78;
            let rank = if on_center_spine {
                let r = spine_counter;
                spine_counter += 1;
                r
            } else {
                0
            };

            // Fan central spine callouts gently outward when horizontal canvas room permits
            let horiz_bias = if on_center_spine && bounds.width >= 540.0 {
                let spread = (bounds.width * 0.085).clamp(32.0, 96.0);
                if rank % 2 == 0 { -spread } else { spread }
            } else if cx_world < 0.32 {
                -10.0
            } else if cx_world > 1.25 {
                10.0
            } else {
                0.0
            };

            let pref_x = (anchor.x - pw * 0.5 + horiz_bias).clamp(6.0, max_x);
            let pref_y = if cy_world <= 0.19 {
                (z_top_px - pill_h - 4.0).clamp(top_min_y, bot_max_y)
            } else if cy_world >= 0.80 {
                (z_bot_px + 5.0).clamp(top_min_y, bot_max_y)
            } else {
                (anchor.y - pill_h * 0.5).clamp(top_min_y, bot_max_y)
            };

            let swatch_col = if lights_on && g.col_count > 0.0 {
                Color::from_rgb(
                    (g.r_sum / g.col_count).clamp(0.0, 1.0),
                    (g.g_sum / g.col_count).clamp(0.0, 1.0),
                    (g.b_sum / g.col_count).clamp(0.0, 1.0),
                )
            } else {
                Color::from_rgb8(146, 138, 164)
            };

            WorkPill {
                tag,
                anchor,
                pref_x,
                pref_y,
                x: pref_x,
                y: pref_y,
                w: pw,
                h: pill_h,
                swatch_col,
                is_selected: g.is_selected,
                is_hovered: g.is_hovered,
                has_override: g.has_override,
                any_enabled: g.any_enabled,
                spine_rank: rank,
            }
        })
        .collect();

    // Sort top-to-bottom by anchor Y for deterministic vertical ordering
    pills.sort_by(|a, b| {
        a.anchor
            .y
            .partial_cmp(&b.anchor.y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                a.anchor
                    .x
                    .partial_cmp(&b.anchor.x)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    const MIN_PITCH_Y: f32 = 22.0; // >= 22px center-to-center vertical separation!
    const MIN_GAP_X: f32 = 8.0;
    let n = pills.len();

    for iter in 0..42 {
        // Gentle anchor spring only in early passes; final 14 passes are pure hard separation
        if iter < 28 {
            let k = 0.06 * (1.0 - iter as f32 / 28.0);
            for p in &mut pills {
                let max_x = (bounds.width - p.w - 6.0).max(6.0);
                p.x = (p.x + (p.pref_x - p.x) * k).clamp(6.0, max_x);
                p.y = (p.y + (p.pref_y - p.y) * k).clamp(top_min_y, bot_max_y);
            }
        }

        let mut any_overlap = false;
        for i in 0..n {
            for j in (i + 1)..n {
                let cx_i = pills[i].x + pills[i].w * 0.5;
                let cy_i = pills[i].y + pills[i].h * 0.5;
                let cx_j = pills[j].x + pills[j].w * 0.5;
                let cy_j = pills[j].y + pills[j].h * 0.5;

                let req_dx = (pills[i].w + pills[j].w) * 0.5 + MIN_GAP_X;
                let dx = cx_j - cx_i;
                let dy = cy_j - cy_i;

                if dx.abs() < req_dx && dy.abs() < MIN_PITCH_Y {
                    any_overlap = true;
                    let overlap_y = MIN_PITCH_Y - dy.abs();
                    let dir_y = if dy.abs() > 0.25 {
                        dy.signum()
                    } else if pills[i].anchor.y <= pills[j].anchor.y {
                        1.0
                    } else {
                        -1.0
                    };

                    let (wi, wj) = match (pills[i].is_selected, pills[j].is_selected) {
                        (true, false) => (0.20, 0.80),
                        (false, true) => (0.80, 0.20),
                        _ => (0.50, 0.50),
                    };

                    let push_y = overlap_y + 0.35;
                    pills[i].y = (pills[i].y - dir_y * push_y * wi).clamp(top_min_y, bot_max_y);
                    pills[j].y = (pills[j].y + dir_y * push_y * wj).clamp(top_min_y, bot_max_y);

                    // If vertical clamping hit ceiling/floor and pair STILL overlaps vertically, separate horizontally!
                    let new_dy = (pills[j].y - pills[i].y).abs();
                    if new_dy < MIN_PITCH_Y - 0.25 {
                        let overlap_x = req_dx - dx.abs();
                        let dir_x = if dx.abs() > 1.0 {
                            dx.signum()
                        } else if (i + pills[i].spine_rank) % 2 == 0 {
                            1.0
                        } else {
                            -1.0
                        };
                        let max_xi = (bounds.width - pills[i].w - 6.0).max(6.0);
                        let max_xj = (bounds.width - pills[j].w - 6.0).max(6.0);
                        let push_x = overlap_x + 1.5;
                        pills[i].x = (pills[i].x - dir_x * push_x * wi).clamp(6.0, max_xi);
                        pills[j].x = (pills[j].x + dir_x * push_x * wj).clamp(6.0, max_xj);
                    }
                }
            }
        }
        if !any_overlap && iter >= 28 {
            break;
        }
    }

    pills
        .into_iter()
        .map(|p| ResolvedZoneLabel {
            tag: p.tag,
            anchor: p.anchor,
            rect: Rectangle::new(Point::new(p.x, p.y), Size::new(p.w, p.h)),
            swatch_col: p.swatch_col,
            is_selected: p.is_selected,
            is_hovered: p.is_hovered,
            has_override: p.has_override,
            any_enabled: p.any_enabled,
        })
        .collect()
}

fn zone_average_color(zone: &ZoneFrame) -> Color {
    if zone.colors.is_empty() {
        return Color::from_rgb8(165, 222, 206);
    }
    let (mut r, mut g, mut b) = (0u64, 0u64, 0u64);
    for c in &zone.colors {
        r += c[0] as u64;
        g += c[1] as u64;
        b += c[2] as u64;
    }
    let n = zone.colors.len() as u64;
    Color::from_rgb8((r / n) as u8, (g / n) as u8, (b / n) as u8)
}

fn hit_test_zone<'a>(
    frames: &'a [ZoneFrame],
    arrange_mode: bool,
    ox: f32,
    oy: f32,
    scale: f32,
    pos: Point,
) -> Option<&'a ZoneFrame> {
    let s_fac = rig_scale_factor(scale);
    let led_hit_r = (18.0 * s_fac).clamp(14.0, 28.0);
    let mut best: Option<(&'a ZoneFrame, f32)> = None;
    for z in frames {
        if !z.enabled && !arrange_mode {
            continue;
        }
        // Check LED proximity first
        for &pt in &z.canvas_pos {
            let sx = ox + pt[0] * scale;
            let sy = oy + pt[1] * scale;
            let d = ((sx - pos.x).powi(2) + (sy - pos.y).powi(2)).sqrt();
            if d <= led_hit_r && best.is_none_or(|(_, bd)| d < bd) {
                best = Some((z, d));
            }
        }
        // Also check inside fan ring discs
        for &([cx, cy], r_norm) in &z.fan_rings {
            let sx = ox + cx * scale;
            let sy = oy + cy * scale;
            let r = (r_norm * scale).max(7.0);
            let d = ((sx - pos.x).powi(2) + (sy - pos.y).powi(2)).sqrt();
            if d <= r + 4.0 * s_fac {
                let score = (d * 0.5).min(led_hit_r * 0.9);
                if best.is_none_or(|(_, bd)| score < bd) {
                    best = Some((z, score));
                }
            }
        }
        // And Matrix keyboard bounding box
        if matches!(z.layout, ZoneLayout::Matrix { .. })
            && let Some((bx0, by0, bx1, by1)) = zone_bounds(z)
        {
            let pad = 8.0 * s_fac;
            let rx0 = ox + bx0 * scale - pad;
            let ry0 = oy + by0 * scale - pad;
            let rx1 = ox + bx1 * scale + pad;
            let ry1 = oy + by1 * scale + pad;
            if pos.x >= rx0 && pos.x <= rx1 && pos.y >= ry0 && pos.y <= ry1 {
                let score = 12.0;
                if best.is_none_or(|(_, bd)| score < bd) {
                    best = Some((z, score));
                }
            }
        }
    }
    best.map(|(z, _)| z)
}

fn fill_radial_bloom(
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
        let layer_a = (peak_alpha / n as f32) * (1.25 - f * 0.85).max(0.15) * 1.45;
        frame.fill(
            &Path::circle(center, r),
            Color {
                a: layer_a.clamp(0.0, 0.65),
                ..color
            },
        );
    }
}

fn cubic_point(p0: Point, p1: Point, p2: Point, p3: Point, t: f32) -> Point {
    let u = 1.0 - t;
    let tt = t * t;
    let uu = u * u;
    let uuu = uu * u;
    let ttt = tt * t;
    Point::new(
        uuu * p0.x + 3.0 * uu * t * p1.x + 3.0 * u * tt * p2.x + ttt * p3.x,
        uuu * p0.y + 3.0 * uu * t * p1.y + 3.0 * u * tt * p2.y + ttt * p3.y,
    )
}

fn draw_pcb_trace_static(frame: &mut Frame, pts: &[Point], trace_col: Color, s_fac: f32) {
    if pts.len() < 2 {
        return;
    }
    let trace_path = Path::new(|b| {
        b.move_to(pts[0]);
        for &pt in &pts[1..] {
            b.line_to(pt);
        }
    });
    frame.stroke(
        &trace_path,
        Stroke::default()
            .with_color(trace_col)
            .with_width((1.0 * s_fac).clamp(0.8, 1.9))
            .with_line_join(LineJoin::Bevel),
    );
    let pad_r = (1.6 * s_fac).clamp(1.2, 3.0);
    for &end_pt in &[pts[0], pts[pts.len() - 1]] {
        frame.fill(&Path::circle(end_pt, pad_r), trace_col);
    }
}

fn draw_pcb_trace_pulse(
    frame: &mut Frame,
    pts: &[Point],
    pulse_col: Color,
    phase: f32,
    s_fac: f32,
) {
    if pts.len() < 2 {
        return;
    }
    let mut seg_lens = Vec::with_capacity(pts.len() - 1);
    let mut total = 0.0f32;
    for w in pts.windows(2) {
        let d = ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2)).sqrt();
        seg_lens.push(d);
        total += d;
    }
    if total < 4.0 {
        return;
    }
    let u = phase.fract();
    let sample_at = |dist_frac: f32| -> Point {
        let mut rem = dist_frac.clamp(0.0, 1.0) * total;
        for (i, &len) in seg_lens.iter().enumerate() {
            if rem <= len || i + 1 == seg_lens.len() {
                let f = if len > 0.001 {
                    (rem / len).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                return Point::new(
                    pts[i].x + (pts[i + 1].x - pts[i].x) * f,
                    pts[i].y + (pts[i + 1].y - pts[i].y) * f,
                );
            }
            rem -= len;
        }
        pts[pts.len() - 1]
    };

    let fade = (u * PI).sin().powf(0.8);
    let head = sample_at(u);
    let tail = sample_at((u - 0.14).max(0.0));
    frame.stroke(
        &Path::line(tail, head),
        Stroke::default()
            .with_color(Color {
                a: 0.40 * fade,
                ..pulse_col
            })
            .with_width((1.7 * s_fac).clamp(1.2, 3.2))
            .with_line_cap(LineCap::Round),
    );
    frame.fill(
        &Path::circle(head, (2.3 * s_fac).clamp(1.6, 4.2)),
        Color {
            a: 0.62 * fade,
            ..pulse_col
        },
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_airflow_ribbon(
    frame: &mut Frame,
    p0: Point,
    p1: Point,
    p2: Point,
    p3: Point,
    tint: Color,
    anim_t: f32,
    speed: f32,
    seed: f32,
    architectural: bool,
    s_fac: f32,
) {
    let base_curve = Path::new(|b| {
        b.move_to(p0);
        b.bezier_curve_to(p1, p2, p3);
    });
    if architectural {
        frame.stroke(
            &base_curve,
            Stroke {
                style: canvas::Style::Solid(Color { a: 0.24, ..tint }),
                width: (1.0 * s_fac).clamp(0.8, 1.9),
                line_cap: LineCap::Round,
                line_dash: LineDash {
                    segments: &[4.0, 5.0],
                    offset: 0,
                },
                ..Stroke::default()
            },
        );
    } else {
        frame.stroke(
            &base_curve,
            Stroke::default()
                .with_color(Color { a: 0.11, ..tint })
                .with_width((4.4 * s_fac).clamp(2.8, 8.8))
                .with_line_cap(LineCap::Round),
        );
        frame.stroke(
            &base_curve,
            Stroke::default()
                .with_color(Color { a: 0.27, ..tint })
                .with_width((1.25 * s_fac).clamp(0.9, 2.5))
                .with_line_cap(LineCap::Round),
        );
    }

    // Traveling luminous energy pulses per airflow stream
    let pulse_count = if architectural { 2 } else { 3 };
    for k in 0..pulse_count {
        let u = (anim_t * speed + seed + k as f32 / pulse_count as f32).fract();
        let envelope = (u * PI).sin();
        if envelope < 0.05 {
            continue;
        }
        let span = if architectural { 0.14 } else { 0.21 };
        let segs = if architectural { 5 } else { 7 };
        for s in 0..segs {
            let f0 = s as f32 / segs as f32;
            let f1 = (s + 1) as f32 / segs as f32;
            let t0 = (u - span * (1.0 - f0)).clamp(0.0, 1.0);
            let t1 = (u - span * (1.0 - f1)).clamp(0.0, 1.0);
            if t1 <= t0 + 0.001 {
                continue;
            }
            let a_pt = cubic_point(p0, p1, p2, p3, t0);
            let b_pt = cubic_point(p0, p1, p2, p3, t1);
            let tail_fade = f1 * f1;
            let alpha = (0.48 * envelope * tail_fade).clamp(0.0, 0.54);
            let width = if architectural {
                (1.3 * s_fac).clamp(0.9, 2.4)
            } else {
                ((1.1 + 1.9 * f1) * s_fac).clamp(0.95, 5.4)
            };
            frame.stroke(
                &Path::line(a_pt, b_pt),
                Stroke::default()
                    .with_color(Color { a: alpha, ..tint })
                    .with_width(width)
                    .with_line_cap(LineCap::Round),
            );
        }
        let head = cubic_point(p0, p1, p2, p3, u);
        if !architectural {
            fill_radial_bloom(
                frame,
                head,
                (7.0 * s_fac).clamp(4.4, 14.0),
                tint,
                0.36 * envelope,
                3,
            );
        }
        frame.fill(
            &Path::circle(
                head,
                if architectural {
                    (1.9 * s_fac).clamp(1.3, 3.4)
                } else {
                    (2.3 * s_fac).clamp(1.5, 4.2)
                },
            ),
            Color {
                a: 0.78 * envelope,
                ..mix_color(tint, Color::WHITE, 0.55)
            },
        );
    }
}

fn draw_fan_blades(
    frame: &mut Frame,
    center: Point,
    r: f32,
    theta: f32,
    firefly: bool,
    architectural: bool,
    s_fac: f32,
) {
    let hub_r = r * 0.29;
    let tip_r = r * 0.87;
    let blade_fill = if architectural {
        Color::from_rgba8(140, 180, 215, 0.08)
    } else if firefly {
        Color::from_rgba8(198, 180, 216, 0.13)
    } else {
        Color::from_rgba8(175, 190, 205, 0.12)
    };
    let blade_edge = if architectural {
        Color::from_rgba8(165, 205, 240, 0.24)
    } else if firefly {
        Color::from_rgba8(226, 208, 240, 0.22)
    } else {
        Color::from_rgba8(205, 220, 235, 0.20)
    };
    let edge_w = (0.85 * s_fac).clamp(0.65, 1.75);
    for b_idx in 0..7 {
        let a0 = theta + (b_idx as f32) * (2.0 * PI / 7.0);
        let a_tip = a0 + 0.42;
        let a_back = a0 + 0.12;
        let root0 = Point::new(center.x + a0.cos() * hub_r, center.y + a0.sin() * hub_r);
        let tip = Point::new(
            center.x + a_tip.cos() * tip_r,
            center.y + a_tip.sin() * tip_r,
        );
        let ctrl_lead = Point::new(
            center.x + (a0 + 0.10).cos() * (r * 0.62),
            center.y + (a0 + 0.10).sin() * (r * 0.62),
        );
        let ctrl_trail = Point::new(
            center.x + (a_tip + 0.18).cos() * (r * 0.56),
            center.y + (a_tip + 0.18).sin() * (r * 0.56),
        );
        let root1 = Point::new(
            center.x + a_back.cos() * hub_r,
            center.y + a_back.sin() * hub_r,
        );
        let blade = Path::new(|pb| {
            pb.move_to(root0);
            pb.quadratic_curve_to(ctrl_lead, tip);
            pb.quadratic_curve_to(ctrl_trail, root1);
            pb.close();
        });
        frame.fill(&blade, blade_fill);
        let lead = Path::new(|pb| {
            pb.move_to(root0);
            pb.quadratic_curve_to(ctrl_lead, tip);
        });
        frame.stroke(
            &lead,
            Stroke::default().with_color(blade_edge).with_width(edge_w),
        );
    }
}

fn thermal_color_for_temp(temp_c: f32, firefly: bool) -> Color {
    let cool = if firefly {
        Color::from_rgb8(165, 226, 208)
    } else {
        Color::from_rgb8(124, 192, 232)
    };
    let warm = if firefly {
        Color::from_rgb8(255, 208, 128)
    } else {
        Color::from_rgb8(232, 154, 68)
    };
    let hot = if firefly {
        Color::from_rgb8(255, 146, 152)
    } else {
        Color::from_rgb8(242, 92, 76)
    };
    if temp_c <= 62.0 {
        cool
    } else if temp_c <= 80.0 {
        mix_color(cool, warm, (temp_c - 62.0) / 18.0)
    } else {
        mix_color(warm, hot, ((temp_c - 80.0) / 14.0).clamp(0.0, 1.0))
    }
}

fn stage_static_hash(stage: &CaseStageCanvas<'_>, bounds: Rectangle) -> u64 {
    let mut h = DefaultHasher::new();
    ((bounds.width * 2.0).round() as i32).hash(&mut h);
    ((bounds.height * 2.0).round() as i32).hash(&mut h);
    stage.palette.ground.hash(&mut h);
    stage.palette.panel.hash(&mut h);
    stage.palette.well.hash(&mut h);
    stage.palette.line.hash(&mut h);
    stage.palette.brass.hash(&mut h);
    stage.palette.square_corners.hash(&mut h);
    stage.palette.radius_panel.to_bits().hash(&mut h);
    stage.accent_override.hash(&mut h);
    rig_style_id(stage.rig_style).hash(&mut h);
    stage.arrange_mode.hash(&mut h);
    stage.show_labels.hash(&mut h);
    (stage.spinning_fans && stage.animated).hash(&mut h);
    stage.eyebrow.is_some().hash(&mut h);
    stage.frames.len().hash(&mut h);
    for z in stage.frames {
        z.zone_id.hash(&mut h);
        z.enabled.hash(&mut h);
        z.canvas_pos.len().hash(&mut h);
        z.fan_rings.len().hash(&mut h);
        for &([cx, cy], r) in &z.fan_rings {
            ((cx * 1000.0).round() as i32).hash(&mut h);
            ((cy * 1000.0).round() as i32).hash(&mut h);
            ((r * 1000.0).round() as i32).hash(&mut h);
        }
        if let Some(first) = z.canvas_pos.first() {
            ((first[0] * 1000.0).round() as i32).hash(&mut h);
            ((first[1] * 1000.0).round() as i32).hash(&mut h);
        }
        if let Some(last) = z.canvas_pos.last() {
            ((last[0] * 1000.0).round() as i32).hash(&mut h);
            ((last[1] * 1000.0).round() as i32).hash(&mut h);
        }
        let (lx, ly) = z.layout.center();
        ((lx * 1000.0).round() as i32).hash(&mut h);
        ((ly * 1000.0).round() as i32).hash(&mut h);
    }
    h.finish()
}

fn stage_dynamic_hash(
    stage: &CaseStageCanvas<'_>,
    bounds: Rectangle,
    hovered_zone_id: Option<&str>,
    hovered_pos: Option<Point>,
    dragging_id: Option<&str>,
) -> u64 {
    let mut h = DefaultHasher::new();
    ((bounds.width * 2.0).round() as i32).hash(&mut h);
    ((bounds.height * 2.0).round() as i32).hash(&mut h);
    stage.palette.ground.hash(&mut h);
    stage.palette.panel.hash(&mut h);
    stage.palette.raised.hash(&mut h);
    stage.palette.well.hash(&mut h);
    stage.palette.brass.hash(&mut h);
    stage.palette.brass_hi.hash(&mut h);
    stage.palette.ink.hash(&mut h);
    stage.palette.ink_3.hash(&mut h);
    stage.palette.square_corners.hash(&mut h);
    stage.accent_override.hash(&mut h);
    rig_style_id(stage.rig_style).hash(&mut h);
    stage.lights_on.hash(&mut h);
    stage.show_labels.hash(&mut h);
    stage.arrange_mode.hash(&mut h);
    stage.selected_zone.hash(&mut h);
    hovered_zone_id.hash(&mut h);
    if hovered_zone_id.is_some()
        && let Some(hp) = hovered_pos
    {
        ((hp.x / 8.0).round() as i32).hash(&mut h);
        ((hp.y / 8.0).round() as i32).hash(&mut h);
    }
    dragging_id.hash(&mut h);
    stage.active_effect.label().hash(&mut h);
    matches!(stage.temp_unit, TempUnit::F).hash(&mut h);
    stage.fan_duty.hash(&mut h);
    stage.cpu_temp.map(|t| t.round() as i32).hash(&mut h);
    stage.gpu_temp.map(|t| t.round() as i32).hash(&mut h);
    [
        stage.ambient_rgb[0] / 8,
        stage.ambient_rgb[1] / 8,
        stage.ambient_rgb[2] / 8,
    ]
    .hash(&mut h);
    stage.eyebrow.hash(&mut h);
    stage.zone_overrides.len().hash(&mut h);
    for (zid, ov) in stage.zone_overrides {
        zid.hash(&mut h);
        ov.effect.label().hash(&mut h);
    }
    stage.frames.len().hash(&mut h);
    for z in stage.frames {
        z.zone_id.hash(&mut h);
        z.enabled.hash(&mut h);
        z.colors.len().hash(&mut h);
        for c in &z.colors {
            [c[0] / 8, c[1] / 8, c[2] / 8].hash(&mut h);
        }
    }
    h.finish()
}

impl<'a> canvas::Program<StageMessage> for CaseStageCanvas<'a> {
    type State = StageDragState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<StageMessage>> {
        let (scale, ox, oy, _wb) = viewport(
            bounds,
            self.eyebrow.is_some(),
            self.show_labels,
            self.frames,
            self.arrange_mode,
        );
        let pos = cursor.position_in(bounds)?;

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(zone) =
                    hit_test_zone(self.frames, self.arrange_mode, ox, oy, scale, pos)
                {
                    let zid = zone.zone_id.clone();
                    let center = zone_center(zone).unwrap_or([0.5, 0.5]);
                    if self.arrange_mode {
                        let cx = (pos.x - ox) / scale;
                        let cy = (pos.y - oy) / scale;
                        state.dragging = Some((zid.clone(), [center[0] - cx, center[1] - cy]));
                    }
                    return Some(
                        Action::publish(StageMessage::SelectZone(Some(zid))).and_capture(),
                    );
                }
                if self.selected_zone.is_some() {
                    return Some(Action::publish(StageMessage::SelectZone(None)));
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if self.arrange_mode
                    && let Some((zid, offset)) = &state.dragging
                {
                    let cx = ((pos.x - ox) / scale + offset[0]).clamp(0.08, CANVAS_W - 0.08);
                    let cy = ((pos.y - oy) / scale + offset[1]).clamp(0.08, CANVAS_H - 0.08);
                    return Some(
                        Action::publish(StageMessage::MoveZone {
                            zone_id: zid.clone(),
                            x: cx,
                            y: cy,
                        })
                        .and_capture(),
                    );
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.dragging.take().is_some() {
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
        let (scale, ox, oy, wb) = viewport(
            bounds,
            self.eyebrow.is_some(),
            self.show_labels,
            self.frames,
            self.arrange_mode,
        );
        let s_fac = rig_scale_factor(scale);
        let stage_w = CANVAS_W * scale;
        let stage_h = CANVAS_H * scale;
        let to_pt = |nx: f32, ny: f32| Point::new(ox + nx * stage_w, oy + ny * stage_h);

        let pad_ch_x = (scale * 0.024).clamp(5.0, 14.0);
        let pad_ch_y = (scale * 0.014).clamp(3.0, 9.0);
        let mut ch_x = (ox + wb.0 * scale - pad_ch_x).max(8.0);
        let ch_y = (oy + wb.1 * scale - pad_ch_y).max(if self.eyebrow.is_some() {
            22.0
        } else if self.show_labels || self.arrange_mode {
            14.0
        } else {
            5.0
        });
        let mut ch_r = (ox + wb.2 * scale + pad_ch_x).min(bounds.width - 8.0);
        let ch_b = (oy + wb.3 * scale + pad_ch_y).min(bounds.height - 6.0);
        let ch_h = (ch_b - ch_y).max(36.0);
        let mut ch_w = (ch_r - ch_x).max(40.0);
        // Preserve sleek 1.62:1 rig proportions whenever horizontal canvas space allows
        let max_ch_w = (bounds.width * 0.90).min(bounds.width - 16.0).max(40.0);
        let target_ch_w = (ch_h * TARGET_RIG_ASPECT).min(max_ch_w);
        if ch_w < target_ch_w {
            let extra = (target_ch_w - ch_w) * 0.5;
            ch_x = (ch_x - extra).max(8.0);
            ch_r = (ch_r + extra).min(bounds.width - 8.0);
            ch_w = (ch_r - ch_x).max(40.0);
        }
        let ch_rad = if p.square_corners || self.rig_style == RigStyle::Architectural {
            2.0
        } else {
            (10.0 * s_fac.sqrt()).clamp(7.0, 16.0)
        };
        let wide_margin = (bounds.width - ch_w) * 0.5;
        let is_wide_stage = wide_margin > 28.0;

        let trace_a = [
            to_pt(0.32, 0.24),
            to_pt(0.38, 0.24),
            to_pt(0.42, 0.28),
            to_pt(0.42, 0.48),
            to_pt(0.47, 0.53),
            to_pt(0.68, 0.53),
        ];
        let trace_b = [
            to_pt(0.64, 0.24),
            to_pt(0.70, 0.24),
            to_pt(0.70, 0.42),
            to_pt(0.65, 0.47),
            to_pt(0.34, 0.47),
        ];
        let aio_c = to_pt(0.47, 0.39);
        let aio_r = (stage_h * 0.098).clamp(13.0, 56.0);
        let gpu_tl = to_pt(0.27, 0.57);
        let gpu_w = stage_w * 0.47;
        let gpu_h = stage_h * 0.105;

        // =========================================================================
        // LAYER 1 (CACHED): Static glass backdrop, widescreen atmospheric staging,
        // dot-grid, sheen, chassis, PCB, VRM heatsink, AIO housing/hoses, GPU shroud,
        // PSU bulkhead, fan housings, and Keychron K2 HE keycap matrix!
        // =========================================================================
        let key = stage_static_hash(self, bounds);
        if state.static_key.get() != key {
            state.static_cache.clear();
            state.static_key.set(key);
        }

        let eff_spinning_fans = self.spinning_fans && self.animated;
        let eff_airflow = self.airflow_lines && self.animated;
        let eff_bloom = self.ambient_bloom && self.animated;
        let eff_anim_t = if self.animated { self.anim_t } else { 0.0 };

        let static_geom = state.static_cache.draw(renderer, bounds.size(), |frame| {
            let architectural = self.rig_style == RigStyle::Architectural;
            let minimal = self.rig_style == RigStyle::Minimal;

            // 1. Deep velvet-instrument glass backdrop (rich across full widescreen bounds — zero dead black bars!)
            let bg_grad = if architectural {
                gradient::Linear::new(Point::ORIGIN, Point::new(bounds.width * 0.45, bounds.height))
                    .add_stop(0.0, Color::from_rgb8(13, 24, 42))
                    .add_stop(0.52, Color::from_rgb8(9, 18, 34))
                    .add_stop(1.0, Color::from_rgb8(7, 13, 25))
            } else if firefly {
                gradient::Linear::new(Point::ORIGIN, Point::new(bounds.width * 0.42, bounds.height))
                    .add_stop(0.0, Color::from_rgb8(34, 27, 48))
                    .add_stop(0.50, Color::from_rgb8(25, 20, 38))
                    .add_stop(1.0, Color::from_rgb8(19, 16, 30))
            } else {
                gradient::Linear::new(Point::ORIGIN, Point::new(bounds.width * 0.40, bounds.height))
                    .add_stop(0.0, Color::from_rgb8(16, 19, 25))
                    .add_stop(0.55, Color::from_rgb8(11, 14, 19))
                    .add_stop(1.0, Color::from_rgb8(8, 10, 15))
            };
            frame.fill(
                &Path::rectangle(Point::ORIGIN, bounds.size()),
                Fill::from(Gradient::Linear(bg_grad)),
            );

            // 1b. Widescreen architectural blueprint framing & lush studio wings (zero dead side margins!)
            let line_tint = if architectural {
                Color::from_rgb8(118, 192, 250)
            } else if firefly {
                mix_color(accent_hi, Color::from_rgb8(204, 182, 232), 0.48)
            } else {
                mix_color(accent, Color::from_rgb8(152, 172, 194), 0.55)
            };

            if is_wide_stage {
                let stage_cx = ch_x + ch_w * 0.5;
                let stage_cy = ch_y + ch_h * 0.52;
                let desk_y = (ch_y + ch_h + 2.0 * s_fac).min(bounds.height - 4.0);

                // Lush ambient studio washes across center + left & right wings
                let halo_col = if architectural {
                    Color::from_rgb8(64, 148, 226)
                } else if firefly {
                    mix_color(accent, Color::from_rgb8(162, 122, 202), 0.52)
                } else {
                    mix_color(accent, Color::from_rgb8(96, 128, 158), 0.58)
                };
                let wing_tint_l = if firefly && self.accent_override.is_none() {
                    Color::from_rgb8(165, 222, 206)
                } else {
                    halo_col
                };
                let wing_tint_r = if firefly && self.accent_override.is_none() {
                    Color::from_rgb8(242, 169, 196)
                } else {
                    accent
                };

                fill_radial_bloom(
                    frame,
                    Point::new(stage_cx, stage_cy),
                    (ch_w * 0.80).min(bounds.width * 0.52),
                    halo_col,
                    if minimal { 0.06 } else { 0.11 },
                    5,
                );
                fill_radial_bloom(
                    frame,
                    Point::new(wide_margin * 0.55, bounds.height * 0.46),
                    (wide_margin * 1.15).max(90.0),
                    wing_tint_l,
                    if minimal { 0.045 } else { 0.085 },
                    4,
                );
                fill_radial_bloom(
                    frame,
                    Point::new(bounds.width - wide_margin * 0.55, bounds.height * 0.54),
                    (wide_margin * 1.15).max(90.0),
                    wing_tint_r,
                    if minimal { 0.045 } else { 0.085 },
                    4,
                );

                // Full-width architectural datum gridlines extending cleanly across the wide viewport
                for (idx, &frac) in [0.18_f32, 0.38, 0.58, 0.78].iter().enumerate() {
                    let ly = ch_y + ch_h * frac;
                    let peak_a = if idx == 1 || idx == 2 { 0.15 } else { 0.095 };
                    let h_grad = gradient::Linear::new(
                        Point::new(0.0, ly),
                        Point::new(bounds.width, ly),
                    )
                    .add_stop(0.0, Color { a: peak_a * 0.45, ..line_tint })
                    .add_stop(0.18, Color { a: peak_a, ..line_tint })
                    .add_stop(0.50, Color { a: peak_a * 0.55, ..line_tint })
                    .add_stop(0.82, Color { a: peak_a, ..line_tint })
                    .add_stop(1.0, Color { a: peak_a * 0.45, ..line_tint });
                    frame.stroke(
                        &Path::line(Point::new(8.0, ly), Point::new(bounds.width - 8.0, ly)),
                        Stroke {
                            style: canvas::Style::Gradient(Gradient::Linear(h_grad)),
                            width: (0.85 * s_fac.sqrt()).clamp(0.75, 1.35),
                            ..Stroke::default()
                        },
                    );
                }

                // Grounded illuminated studio desk horizon shelf spanning edge-to-edge
                let horizon_grad = gradient::Linear::new(
                    Point::new(0.0, desk_y),
                    Point::new(bounds.width, desk_y),
                )
                .add_stop(0.0, Color { a: 0.06, ..line_tint })
                .add_stop(0.16, Color { a: 0.20, ..line_tint })
                .add_stop(0.50, Color { a: 0.36, ..line_tint })
                .add_stop(0.84, Color { a: 0.20, ..line_tint })
                .add_stop(1.0, Color { a: 0.06, ..line_tint });
                frame.stroke(
                    &Path::line(Point::new(6.0, desk_y), Point::new(bounds.width - 6.0, desk_y)),
                    Stroke {
                        style: canvas::Style::Gradient(Gradient::Linear(horizon_grad)),
                        width: (1.25 * s_fac.sqrt()).clamp(1.0, 2.0),
                        ..Stroke::default()
                    },
                );

                // Flanking architectural wing columns, graduation ticks & reticle crosses
                if !minimal && wide_margin > 42.0 {
                    let wing_cols = [
                        (wide_margin * 0.34).clamp(18.0, ch_x - 16.0),
                        (wide_margin * 0.72).clamp(24.0, ch_x - 10.0),
                        (ch_r + wide_margin * 0.28).clamp(ch_r + 10.0, bounds.width - 24.0),
                        (ch_r + wide_margin * 0.66).clamp(ch_r + 16.0, bounds.width - 18.0),
                    ];
                    for (ci, &wx) in wing_cols.iter().enumerate() {
                        let is_outer = ci == 0 || ci == 3;
                        let v_grad = gradient::Linear::new(
                            Point::new(wx, 10.0),
                            Point::new(wx, bounds.height - 10.0),
                        )
                        .add_stop(0.0, Color { a: 0.02, ..line_tint })
                        .add_stop(
                            0.5,
                            Color {
                                a: if is_outer { 0.16 } else { 0.10 },
                                ..line_tint
                            },
                        )
                        .add_stop(1.0, Color { a: 0.02, ..line_tint });
                        frame.stroke(
                            &Path::line(Point::new(wx, 10.0), Point::new(wx, bounds.height - 10.0)),
                            Stroke {
                                style: canvas::Style::Gradient(Gradient::Linear(v_grad)),
                                width: 0.85,
                                line_dash: if is_outer {
                                    LineDash::default()
                                } else {
                                    LineDash {
                                        segments: &[3.0, 5.0],
                                        offset: 0,
                                    }
                                },
                                ..Stroke::default()
                            },
                        );

                        // Crisp calibration micro-ticks along outer wing stanchions
                        if is_outer {
                            let dir = if ci == 0 { 1.0_f32 } else { -1.0_f32 };
                            let mut ty = ch_y + 12.0;
                            let mut step_i = 0usize;
                            while ty < desk_y - 8.0 {
                                let tl = if step_i % 3 == 0 { 6.5 } else { 3.5 };
                                frame.stroke(
                                    &Path::line(
                                        Point::new(wx, ty),
                                        Point::new(wx + dir * tl, ty),
                                    ),
                                    Stroke::default()
                                        .with_color(Color {
                                            a: if step_i % 3 == 0 { 0.26 } else { 0.14 },
                                            ..line_tint
                                        })
                                        .with_width(0.9),
                                );
                                ty += 16.0;
                                step_i += 1;
                            }
                        } else {
                            // Precision '+' reticles at datum intersections in inner wings
                            for &frac in &[0.18_f32, 0.38, 0.58, 0.78] {
                                let ry = ch_y + ch_h * frac;
                                let arm = 3.2_f32;
                                let ret_c = Color { a: 0.24, ..line_tint };
                                frame.stroke(
                                    &Path::line(
                                        Point::new(wx - arm, ry),
                                        Point::new(wx + arm, ry),
                                    ),
                                    Stroke::default().with_color(ret_c).with_width(0.85),
                                );
                                frame.stroke(
                                    &Path::line(
                                        Point::new(wx, ry - arm),
                                        Point::new(wx, ry + arm),
                                    ),
                                    Stroke::default().with_color(ret_c).with_width(0.85),
                                );
                            }
                        }
                    }
                }
            }

            // 2. Full-bounds architectural blueprint grid, top/bottom scale rulers & outer corner brackets
            if architectural {
                let pitch = 20.0;
                let minor = Color::from_rgba8(106, 178, 236, 0.08);
                let major = Color::from_rgba8(106, 178, 236, 0.16);
                let mut gx = pitch;
                let mut col_idx = 1;
                while gx < bounds.width {
                    frame.stroke(
                        &Path::line(Point::new(gx, 0.0), Point::new(gx, bounds.height)),
                        Stroke::default()
                            .with_color(if col_idx % 4 == 0 { major } else { minor })
                            .with_width(0.8),
                    );
                    gx += pitch;
                    col_idx += 1;
                }
                let mut gy = pitch;
                let mut row_idx = 1;
                while gy < bounds.height {
                    frame.stroke(
                        &Path::line(Point::new(0.0, gy), Point::new(bounds.width, gy)),
                        Stroke::default()
                            .with_color(if row_idx % 4 == 0 { major } else { minor })
                            .with_width(0.8),
                    );
                    gy += pitch;
                    row_idx += 1;
                }
            } else if !minimal || self.arrange_mode {
                // Subtle architectural blueprint major lines + constellation dot matrix across full bounds
                let major_pitch = 64.0_f32;
                let major_col = Color {
                    a: if self.arrange_mode { 0.09 } else { 0.048 },
                    ..line_tint
                };
                let mut mx = major_pitch;
                while mx < bounds.width {
                    frame.stroke(
                        &Path::line(Point::new(mx, 0.0), Point::new(mx, bounds.height)),
                        Stroke::default().with_color(major_col).with_width(0.75),
                    );
                    mx += major_pitch;
                }
                let mut my = major_pitch;
                while my < bounds.height {
                    frame.stroke(
                        &Path::line(Point::new(0.0, my), Point::new(bounds.width, my)),
                        Stroke::default().with_color(major_col).with_width(0.75),
                    );
                    my += major_pitch;
                }

                let pitch = if self.arrange_mode { 18.0 } else { 16.0 };
                let dot_col = if self.arrange_mode {
                    Color { a: 0.22, ..line_tint }
                } else {
                    Color { a: 0.11, ..line_tint }
                };
                let mut gx = pitch * 0.5;
                while gx < bounds.width {
                    let mut gy = pitch * 0.5;
                    while gy < bounds.height {
                        frame.fill(&Path::circle(Point::new(gx, gy), 0.8), dot_col);
                        gy += pitch;
                    }
                    gx += pitch;
                }
            }

            // Edge-to-edge top & bottom precision telemetry ruler ticks + outer stage corner brackets
            if !minimal && bounds.width >= 260.0 {
                let mut rx = 20.0_f32;
                let mut idx = 0usize;
                while rx < bounds.width - 16.0 {
                    let is_maj = idx % 4 == 0;
                    let th = if is_maj { 4.8 } else { 2.4 };
                    let tc = Color {
                        a: if is_maj { 0.22 } else { 0.10 },
                        ..line_tint
                    };
                    if self.eyebrow.is_none() {
                        frame.stroke(
                            &Path::line(Point::new(rx, 2.0), Point::new(rx, 2.0 + th)),
                            Stroke::default().with_color(tc).with_width(0.85),
                        );
                    }
                    frame.stroke(
                        &Path::line(
                            Point::new(rx, bounds.height - 2.0 - th),
                            Point::new(rx, bounds.height - 2.0),
                        ),
                        Stroke::default().with_color(tc).with_width(0.85),
                    );
                    rx += 16.0;
                    idx += 1;
                }

                let brk = 11.0_f32;
                let brk_c = Color { a: 0.30, ..line_tint };
                for (bx, by, dx, dy) in [
                    (6.0_f32, 6.0_f32, 1.0_f32, 1.0_f32),
                    (bounds.width - 6.0, 6.0, -1.0, 1.0),
                    (6.0, bounds.height - 6.0, 1.0, -1.0),
                    (bounds.width - 6.0, bounds.height - 6.0, -1.0, -1.0),
                ] {
                    let corner = Path::new(|b| {
                        b.move_to(Point::new(bx, by + dy * brk));
                        b.line_to(Point::new(bx, by));
                        b.line_to(Point::new(bx + dx * brk, by));
                    });
                    frame.stroke(&corner, Stroke::default().with_color(brk_c).with_width(1.0));
                }
            }

            // 3. Tempered glass top-left diagonal reflection sheen (Living mode)
            if self.rig_style == RigStyle::Living {
                let sheen = Path::new(|b| {
                    b.move_to(Point::ORIGIN);
                    b.line_to(Point::new(bounds.width * 0.52, 0.0));
                    b.line_to(Point::new(bounds.width * 0.22, bounds.height));
                    b.line_to(Point::new(0.0, bounds.height));
                    b.close();
                });
                let sheen_grad = gradient::Linear::new(
                    Point::ORIGIN,
                    Point::new(bounds.width * 0.42, bounds.height * 0.72),
                )
                .add_stop(0.0, Color::from_rgba(1.0, 0.96, 1.0, 0.042))
                .add_stop(0.55, Color::from_rgba(1.0, 0.96, 1.0, 0.015))
                .add_stop(1.0, Color::from_rgba(1.0, 0.96, 1.0, 0.0));
                frame.fill(&sheen, Fill::from(Gradient::Linear(sheen_grad)));

                // Secondary subtle crisp specular reflection streak
                let streak = Path::new(|b| {
                    b.move_to(Point::new(bounds.width * 0.58, 0.0));
                    b.line_to(Point::new(bounds.width * 0.63, 0.0));
                    b.line_to(Point::new(bounds.width * 0.35, bounds.height));
                    b.line_to(Point::new(bounds.width * 0.30, bounds.height));
                    b.close();
                });
                let streak_grad = gradient::Linear::new(
                    Point::new(bounds.width * 0.58, 0.0),
                    Point::new(bounds.width * 0.32, bounds.height),
                )
                .add_stop(0.0, Color::from_rgba(1.0, 0.97, 1.0, 0.022))
                .add_stop(1.0, Color::from_rgba(1.0, 0.97, 1.0, 0.0));
                frame.fill(&streak, Fill::from(Gradient::Linear(streak_grad)));
            }

            // 4. Outer PC Chassis Plate + 4 Corner L-Bracket Registration Ticks
            let chassis = Path::rounded_rectangle(
                Point::new(ch_x, ch_y),
                Size::new(ch_w, ch_h),
                iced::border::Radius::from(ch_rad),
            );
            if !minimal {
                let plate_grad = if architectural {
                    gradient::Linear::new(
                        Point::new(ch_x, ch_y),
                        Point::new(ch_x + ch_w, ch_y + ch_h),
                    )
                    .add_stop(0.0, Color::from_rgba8(18, 34, 58, 0.36))
                    .add_stop(1.0, Color::from_rgba8(10, 20, 38, 0.42))
                } else if firefly {
                    gradient::Linear::new(
                        Point::new(ch_x, ch_y),
                        Point::new(ch_x + ch_w, ch_y + ch_h),
                    )
                    .add_stop(0.0, Color::from_rgba8(44, 36, 58, 0.52))
                    .add_stop(0.45, Color::from_rgba8(20, 17, 30, 0.58))
                    .add_stop(1.0, Color::from_rgba8(32, 26, 44, 0.52))
                } else {
                    gradient::Linear::new(
                        Point::new(ch_x, ch_y),
                        Point::new(ch_x + ch_w, ch_y + ch_h),
                    )
                    .add_stop(0.0, Color::from_rgba8(32, 37, 43, 0.48))
                    .add_stop(0.45, Color::from_rgba8(12, 16, 20, 0.55))
                    .add_stop(1.0, Color::from_rgba8(21, 25, 30, 0.48))
                };
                frame.fill(&chassis, Fill::from(Gradient::Linear(plate_grad)));
            }
            let chassis_rim = if architectural {
                Color::from_rgba8(116, 190, 248, 0.58)
            } else if minimal {
                Color::from_rgba8(106, 94, 124, 0.28)
            } else if firefly {
                Color::from_rgba8(106, 90, 124, 0.56)
            } else {
                Color::from_rgba8(68, 76, 86, 0.56)
            };
            frame.stroke(
                &chassis,
                Stroke::default()
                    .with_color(chassis_rim)
                    .with_width((1.05 * s_fac.sqrt()).clamp(0.9, 1.8)),
            );

            if !minimal {
                let tick_len = (ch_w / 18.0).clamp(7.0, 22.0);
                let tick_col = if architectural {
                    Color::from_rgba8(144, 212, 255, 0.78)
                } else if firefly {
                    Color::from_rgba8(158, 140, 176, 0.66)
                } else {
                    Color::from_rgba8(124, 133, 142, 0.65)
                };
                let corners = [
                    (ch_x, ch_y, 1.0_f32, 1.0_f32),
                    (ch_x + ch_w, ch_y, -1.0, 1.0),
                    (ch_x, ch_y + ch_h, 1.0, -1.0),
                    (ch_x + ch_w, ch_y + ch_h, -1.0, -1.0),
                ];
                for (cx, cy, dx, dy) in corners {
                    let tick = Path::new(|b| {
                        b.move_to(Point::new(cx + dx * 5.0, cy + dy * (5.0 + tick_len)));
                        b.line_to(Point::new(cx + dx * 5.0, cy + dy * 5.0));
                        b.line_to(Point::new(cx + dx * (5.0 + tick_len), cy + dy * 5.0));
                    });
                    frame.stroke(
                        &tick,
                        Stroke::default()
                            .with_color(tick_col)
                            .with_width((1.1 * s_fac.sqrt()).clamp(0.9, 2.0)),
                    );
                }
            }

            // 5. Architectural PC Internals (Living & Architectural modes only)
            if !minimal {
                let hw_fill = if architectural {
                    Color::from_rgba8(20, 40, 68, 0.24)
                } else if firefly {
                    Color::from_rgba8(34, 28, 48, 0.44)
                } else {
                    Color::from_rgba8(22, 26, 32, 0.44)
                };
                let hw_stroke = if architectural {
                    Color::from_rgba8(116, 188, 246, 0.42)
                } else if firefly {
                    Color::from_rgba8(96, 82, 116, 0.38)
                } else {
                    Color::from_rgba8(64, 74, 86, 0.38)
                };
                let trace_col = if architectural {
                    Color::from_rgba8(126, 198, 255, 0.24)
                } else if firefly {
                    Color::from_rgba8(186, 166, 206, 0.15)
                } else {
                    Color::from_rgba8(130, 146, 162, 0.15)
                };

                // Motherboard PCB tray
                let mb_tl = to_pt(0.24, 0.13);
                let mb_size = Size::new(stage_w * 0.50, stage_h * 0.59);
                let mb = Path::rounded_rectangle(
                    mb_tl,
                    mb_size,
                    iced::border::Radius::from(if architectural { 1.5 } else { 4.0 }),
                );
                frame.fill(&mb, hw_fill);
                frame.stroke(&mb, Stroke::default().with_color(hw_stroke).with_width(0.9));

                // Refined VRM heatsink block (top-left of motherboard) with milled cooling fins
                let vrm_tl = to_pt(0.26, 0.16);
                let vrm_w = stage_w * 0.056;
                let vrm_h = stage_h * 0.23;
                let vrm = Path::rounded_rectangle(
                    vrm_tl,
                    Size::new(vrm_w, vrm_h),
                    iced::border::Radius::from(2.0),
                );
                frame.fill(&vrm, hw_fill);
                frame.stroke(&vrm, Stroke::default().with_color(hw_stroke).with_width(0.85));
                for fin in 1..7 {
                    let fy = vrm_tl.y + vrm_h * (fin as f32 / 7.0);
                    frame.stroke(
                        &Path::line(
                            Point::new(vrm_tl.x + 2.0, fy),
                            Point::new(vrm_tl.x + vrm_w - 2.0, fy),
                        ),
                        Stroke::default()
                            .with_color(Color { a: 0.28, ..hw_stroke })
                            .with_width(0.75),
                    );
                }

                // Top VRM heatsink bar above CPU socket
                let top_vrm = Path::rounded_rectangle(
                    to_pt(0.34, 0.155),
                    Size::new(stage_w * 0.22, stage_h * 0.042),
                    iced::border::Radius::from(2.0),
                );
                frame.fill(&top_vrm, hw_fill);
                frame.stroke(
                    &top_vrm,
                    Stroke::default().with_color(hw_stroke).with_width(0.8),
                );

                // 2 vertical RAM DIMM slots to the right of CPU block
                for i in 0..2 {
                    let rx = 0.595 + i as f32 * 0.022;
                    let dimm = Path::rounded_rectangle(
                        to_pt(rx, 0.19),
                        Size::new(stage_w * 0.013, stage_h * 0.25),
                        iced::border::Radius::from(1.5),
                    );
                    frame.fill(&dimm, hw_fill);
                    frame.stroke(
                        &dimm,
                        Stroke::default().with_color(hw_stroke).with_width(0.75),
                    );
                }

                // Static chamfered 45° motherboard PCB traces
                draw_pcb_trace_static(frame, &trace_a, trace_col, s_fac);
                draw_pcb_trace_static(frame, &trace_b, trace_col, s_fac);

                // CPU AIO waterblock housing + curved coolant hoses
                frame.fill(&Path::circle(aio_c, aio_r), hw_fill);
                frame.stroke(
                    &Path::circle(aio_c, aio_r),
                    Stroke::default()
                        .with_color(hw_stroke)
                        .with_width((1.2 * s_fac.sqrt()).clamp(0.95, 2.2)),
                );
                for dx in [-0.012_f32, 0.014_f32] {
                    let h0 = Point::new(
                        aio_c.x + aio_r * 0.55 + dx * stage_w,
                        aio_c.y - aio_r * 0.45,
                    );
                    let h1 = to_pt(0.58 + dx, 0.22);
                    let h2 = to_pt(0.66 + dx, 0.14);
                    let h3 = to_pt(0.61 + dx, 0.06);
                    let hose = Path::new(|b| {
                        b.move_to(h0);
                        b.bezier_curve_to(h1, h2, h3);
                    });
                    frame.stroke(
                        &hose,
                        Stroke::default()
                            .with_color(Color { a: 0.45, ..hw_stroke })
                            .with_width(if architectural {
                                (2.0 * s_fac).clamp(1.3, 3.6)
                            } else {
                                (3.4 * s_fac).clamp(2.0, 6.2)
                            })
                            .with_line_cap(LineCap::Round),
                    );
                }

                // GPU shroud silhouette + angled ventilation slats
                let gpu = Path::rounded_rectangle(
                    gpu_tl,
                    Size::new(gpu_w, gpu_h),
                    iced::border::Radius::from(if architectural { 1.5 } else { 4.0 }),
                );
                frame.fill(&gpu, hw_fill);
                frame.stroke(&gpu, Stroke::default().with_color(hw_stroke).with_width(1.0));
                for i in 0..4 {
                    let sx = gpu_tl.x + gpu_w * (0.10 + i as f32 * 0.15);
                    let slat = Path::line(
                        Point::new(sx, gpu_tl.y + gpu_h * 0.24),
                        Point::new(sx + gpu_w * 0.055, gpu_tl.y + gpu_h * 0.76),
                    );
                    frame.stroke(
                        &slat,
                        Stroke::default()
                            .with_color(Color { a: 0.24, ..hw_stroke })
                            .with_width(0.85),
                    );
                }

                // PSU bulkhead divider + slotted vents
                let psu_y = oy + stage_h * 0.765;
                frame.stroke(
                    &Path::line(
                        Point::new(ch_x + 8.0, psu_y),
                        Point::new(ch_x + ch_w * 0.78, psu_y),
                    ),
                    Stroke::default().with_color(hw_stroke).with_width(1.0),
                );
                for i in 0..7 {
                    let vx = ch_x + 18.0 + i as f32 * (ch_w * 0.055);
                    frame.stroke(
                        &Path::line(
                            Point::new(vx, psu_y + 7.0),
                            Point::new(vx + ch_w * 0.032, psu_y + 7.0),
                        ),
                        Stroke::default()
                            .with_color(Color { a: 0.26, ..hw_stroke })
                            .with_width(1.6)
                            .with_line_cap(LineCap::Round),
                    );
                }
            }

            // 6. Static Zone Housings: Fan Wells/Shrouds, Keychron K2 HE Keycaps, Strip Backing Tracks
            let base_ring_col = if architectural {
                Color::from_rgba8(120, 192, 248, 0.50)
            } else if minimal {
                Color::from_rgba8(116, 102, 134, 0.34)
            } else if firefly {
                Color::from_rgba8(112, 96, 130, 0.46)
            } else {
                Color::from_rgba8(74, 82, 92, 0.46)
            };

            for zone in self.frames {
                if !zone.enabled && !self.arrange_mode {
                    continue;
                }

                // Fan housings (static disc + shrouds; plus static blades if !spinning_fans)
                for &([cx, cy], r_norm) in &zone.fan_rings {
                    let center = Point::new(ox + cx * scale, oy + cy * scale);
                    let r = (r_norm * scale).max(5.5);
                    let hub_r = r * 0.29;
                    let disc_col = if architectural {
                        Color::from_rgba8(10, 22, 38, 0.80)
                    } else if firefly {
                        Color::from_rgba8(22, 18, 32, 0.88)
                    } else {
                        Color::from_rgba8(12, 16, 20, 0.88)
                    };
                    frame.fill(&Path::circle(center, r), disc_col);
                    if !minimal {
                        frame.fill(
                            &Path::circle(
                                Point::new(center.x - r * 0.14, center.y - r * 0.18),
                                r * 0.65,
                            ),
                            Color::from_rgba(1.0, 0.97, 1.0, 0.03),
                        );
                    }
                    if !eff_spinning_fans && !minimal {
                        draw_fan_blades(frame, center, r, 0.35, firefly, architectural, s_fac);
                    }
                    frame.stroke(
                        &Path::circle(center, r),
                        Stroke::default()
                            .with_color(base_ring_col)
                            .with_width((1.0 * s_fac.sqrt()).clamp(0.85, 1.9)),
                    );
                    if !minimal {
                        frame.stroke(
                            &Path::circle(center, r * 0.76),
                            Stroke::default()
                                .with_color(Color {
                                    a: 0.35,
                                    ..base_ring_col
                                })
                                .with_width((0.8 * s_fac.sqrt()).clamp(0.7, 1.4)),
                        );
                    }
                    if !eff_spinning_fans {
                        frame.fill(
                            &Path::circle(center, hub_r),
                            if firefly {
                                Color::from_rgb8(26, 22, 37)
                            } else {
                                Color::from_rgb8(20, 24, 30)
                            },
                        );
                        frame.stroke(
                            &Path::circle(center, hub_r),
                            Stroke::default()
                                .with_color(base_ring_col)
                                .with_width((0.9 * s_fac.sqrt()).clamp(0.75, 1.6)),
                        );
                    }
                }

                // Keychron K2 HE / Matrix keyboard frame + individual keycaps (cached!)
                if let ZoneLayout::Matrix {
                    rows,
                    cols,
                    x,
                    y,
                    angle,
                    length,
                    ..
                } = &zone.layout
                {
                    let r_cnt = (*rows).max(1) as f32;
                    let c_cnt = (*cols).max(1) as f32;
                    let w = *length;
                    let h = w * r_cnt / c_cnt;
                    let pad_c = 0.024;
                    let hw = w * 0.5 + pad_c;
                    let hh = h * 0.5 + pad_c;
                    let rad = angle.to_radians();
                    let (ca, sa) = (rad.cos(), rad.sin());
                    let mat_corners = [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)];
                    let mat_path = Path::new(|b| {
                        for (i, &(lx, ly)) in mat_corners.iter().enumerate() {
                            let gx = *x + ca * lx - sa * ly;
                            let gy = *y + sa * lx + ca * ly;
                            let sp = Point::new(ox + gx * scale, oy + gy * scale);
                            if i == 0 {
                                b.move_to(sp);
                            } else {
                                b.line_to(sp);
                            }
                        }
                        b.close();
                    });
                    frame.fill(
                        &mat_path,
                        if architectural {
                            Color::from_rgba8(10, 22, 38, 0.90)
                        } else if firefly {
                            Color::from_rgba8(20, 17, 30, 0.92)
                        } else {
                            Color::from_rgba8(14, 17, 22, 0.92)
                        },
                    );
                    frame.stroke(
                        &mat_path,
                        Stroke::default()
                            .with_color(base_ring_col)
                            .with_width((1.1 * s_fac.sqrt()).clamp(0.9, 2.0)),
                    );
                    let cell_w = ((w / c_cnt) * scale * 0.78).clamp(2.8, 20.0);
                    let cell_h = ((h / r_cnt) * scale * 0.78).clamp(2.8, 20.0);
                    let cap_stroke = Color {
                        a: 0.35,
                        ..base_ring_col
                    };
                    for pos in &zone.canvas_pos {
                        let pt = Point::new(ox + pos[0] * scale, oy + pos[1] * scale);
                        let cap = Path::rounded_rectangle(
                            Point::new(pt.x - cell_w * 0.5, pt.y - cell_h * 0.5),
                            Size::new(cell_w, cell_h),
                            iced::border::Radius::from((1.5 * s_fac.sqrt()).clamp(1.0, 3.0)),
                        );
                        frame.fill(&cap, Color::from_rgba8(32, 28, 44, 0.65));
                        frame.stroke(
                            &cap,
                            Stroke::default().with_color(cap_stroke).with_width(0.6),
                        );
                    }
                } else if let ZoneLayout::Strip { .. } = &zone.layout {
                    if zone.canvas_pos.len() >= 2 {
                        let track = Path::new(|b| {
                            for (i, pos) in zone.canvas_pos.iter().enumerate() {
                                let pt = Point::new(ox + pos[0] * scale, oy + pos[1] * scale);
                                if i == 0 {
                                    b.move_to(pt);
                                } else {
                                    b.line_to(pt);
                                }
                            }
                        });
                        frame.stroke(
                            &track,
                            Stroke::default()
                                .with_color(if firefly {
                                    Color::from_rgba8(48, 40, 62, 0.75)
                                } else {
                                    Color::from_rgba8(36, 42, 48, 0.75)
                                })
                                .with_width((5.2 * s_fac).clamp(3.2, 10.5))
                                .with_line_cap(LineCap::Round)
                                .with_line_join(LineJoin::Round),
                        );
                    } else if let Some(&pt0) = zone.canvas_pos.first() {
                        let pt = Point::new(ox + pt0[0] * scale, oy + pt0[1] * scale);
                        frame.fill(
                            &Path::circle(pt, (5.2 * s_fac).clamp(3.2, 10.5)),
                            if firefly {
                                Color::from_rgba8(44, 36, 58, 0.85)
                            } else {
                                Color::from_rgba8(36, 42, 48, 0.85)
                            },
                        );
                    }
                }
            }
        });

        // =========================================================================
        // LAYER 2 (DYNAMIC PER FRAME): Ambient wash & case bounce light, PCB pulses,
        // AIO LCD pump temp screen, GPU temp badge, airflow ribbons, spinning blades,
        // volumetric LEDs, Arrange snap crosshairs, and Hover Zone X-Ray tooltip!
        // =========================================================================
        let hovered_pos = cursor.position_in(bounds);
        let hovered_zone = hovered_pos.and_then(|hp| {
            if state.dragging.is_some() {
                None
            } else {
                hit_test_zone(self.frames, self.arrange_mode, ox, oy, scale, hp)
            }
        });

        let render_layer2 = |frame: &mut Frame| {
        let architectural = self.rig_style == RigStyle::Architectural;
        let minimal = self.rig_style == RigStyle::Minimal;

        // 1. Volumetric interior ambient wash + colored bounce light on case frame
        let amb = Color::from_rgb8(
            self.ambient_rgb[0],
            self.ambient_rgb[1],
            self.ambient_rgb[2],
        );
        let amb_lum = (self.ambient_rgb[0] as f32
            + self.ambient_rgb[1] as f32
            + self.ambient_rgb[2] as f32)
            / (3.0 * 255.0);
        if self.lights_on && eff_bloom && amb_lum > 0.03 {
            let wash_r = stage_w.max(stage_h) * 0.58;

            // Sample actual left-wing & right-wing active zone colors for authentic directional spill!
            let mut left_spill = amb;
            let mut right_spill = amb;
            let (mut l_cnt, mut r_cnt) = (0.0_f32, 0.0_f32);
            let (mut lr, mut lg, mut lb) = (0.0_f32, 0.0_f32, 0.0_f32);
            let (mut rr, mut rg, mut rb) = (0.0_f32, 0.0_f32, 0.0_f32);
            for z in self.frames {
                if !z.enabled || z.colors.is_empty() {
                    continue;
                }
                if let Some([zx, _zy]) = zone_center(z) {
                    let zc = zone_average_color(z);
                    if zx < CANVAS_W * 0.42 {
                        lr += zc.r;
                        lg += zc.g;
                        lb += zc.b;
                        l_cnt += 1.0;
                    } else if zx > CANVAS_W * 0.62 {
                        rr += zc.r;
                        rg += zc.g;
                        rb += zc.b;
                        r_cnt += 1.0;
                    }
                }
            }
            if l_cnt > 0.0 {
                left_spill = Color::from_rgb(lr / l_cnt, lg / l_cnt, lb / l_cnt);
            }
            if r_cnt > 0.0 {
                right_spill = Color::from_rgb(rr / r_cnt, rg / r_cnt, rb / r_cnt);
            }

            // Wide studio radial color spill from active zones onto surrounding flanking wings!
            if is_wide_stage {
                let desk_y = (ch_y + ch_h + 2.0 * s_fac).min(bounds.height - 2.0);
                let wing_spill_r = (wide_margin * 1.25 + ch_h * 0.42).max(110.0);

                fill_radial_bloom(
                    frame,
                    Point::new((ch_x - wide_margin * 0.26).max(20.0), ch_y + ch_h * 0.50),
                    wing_spill_r,
                    left_spill,
                    (0.18 * amb_lum.sqrt()).clamp(0.06, 0.21),
                    5,
                );
                fill_radial_bloom(
                    frame,
                    Point::new(
                        (ch_r + wide_margin * 0.26).min(bounds.width - 20.0),
                        ch_y + ch_h * 0.54,
                    ),
                    wing_spill_r,
                    right_spill,
                    (0.18 * amb_lum.sqrt()).clamp(0.06, 0.21),
                    5,
                );
                fill_radial_bloom(
                    frame,
                    Point::new(ch_x + ch_w * 0.5, desk_y),
                    (ch_w * 0.76).min(bounds.width * 0.48),
                    amb,
                    (0.15 * amb_lum.sqrt()).clamp(0.05, 0.18),
                    5,
                );

                let reflect_h = (bounds.height - desk_y).clamp(2.0, 24.0);
                let floor_grad = gradient::Linear::new(
                    Point::new(0.0, desk_y),
                    Point::new(bounds.width, desk_y),
                )
                .add_stop(0.0, Color { a: 0.02, ..left_spill })
                .add_stop(
                    0.22,
                    Color {
                        a: (0.16 * amb_lum.sqrt()).clamp(0.05, 0.18),
                        ..left_spill
                    },
                )
                .add_stop(
                    0.50,
                    Color {
                        a: (0.25 * amb_lum.sqrt()).clamp(0.08, 0.28),
                        ..amb
                    },
                )
                .add_stop(
                    0.78,
                    Color {
                        a: (0.16 * amb_lum.sqrt()).clamp(0.05, 0.18),
                        ..right_spill
                    },
                )
                .add_stop(1.0, Color { a: 0.02, ..right_spill });
                frame.fill(
                    &Path::rectangle(Point::new(0.0, desk_y), Size::new(bounds.width, reflect_h)),
                    Fill::from(Gradient::Linear(floor_grad)),
                );
            }

            fill_radial_bloom(
                frame,
                to_pt(0.48, 0.52),
                wash_r,
                amb,
                (0.19 * amb_lum.sqrt()).clamp(0.07, 0.21),
                5,
            );
            fill_radial_bloom(
                frame,
                to_pt(0.22, 0.48),
                wash_r * 0.54,
                amb,
                (0.10 * amb_lum.sqrt()).clamp(0.04, 0.12),
                4,
            );
            if firefly && !architectural {
                fill_radial_bloom(
                    frame,
                    to_pt(0.76, 0.28),
                    wash_r * 0.42,
                    accent,
                    0.075,
                    4,
                );
            }
            // Soft colored bounce light catching the inner metallic rim of the chassis frame
            let bounce_rect = Path::rounded_rectangle(
                Point::new(ch_x + 1.0, ch_y + 1.0),
                Size::new((ch_w - 2.0).max(20.0), (ch_h - 2.0).max(20.0)),
                iced::border::Radius::from(ch_rad),
            );
            frame.stroke(
                &bounce_rect,
                Stroke::default()
                    .with_color(Color {
                        a: (0.28 * amb_lum.sqrt()).clamp(0.06, 0.30),
                        ..amb
                    })
                    .with_width((2.2 * s_fac.sqrt()).clamp(1.6, 3.4)),
            );
        } else if firefly && !minimal && !architectural && eff_bloom {
            fill_radial_bloom(
                frame,
                to_pt(0.50, 0.55),
                stage_w * 0.44,
                Color::from_rgb8(98, 72, 114),
                0.075,
                4,
            );
        }

        // 2. Dynamic Living Internals: PCB trace pulses & AIO coolant glints
        // (AIO LCD pump screen & GPU temp badge are drawn AFTER airflow ribbons & LEDs for clean Z-order!)
        if self.rig_style == RigStyle::Living && self.animated {
            let pulse_a = if firefly {
                Color::from_rgb8(165, 222, 206) // mint
            } else {
                accent_hi
            };
            let pulse_b = accent;

            draw_pcb_trace_pulse(frame, &trace_a, pulse_a, eff_anim_t * 0.22 + 0.15, s_fac);
            draw_pcb_trace_pulse(frame, &trace_b, pulse_b, eff_anim_t * 0.18 + 0.62, s_fac);

            // Faint coolant flow glint along curved AIO hoses
            for (idx, dx) in [-0.012_f32, 0.014_f32].iter().enumerate() {
                let h0 = Point::new(
                    aio_c.x + aio_r * 0.55 + dx * stage_w,
                    aio_c.y - aio_r * 0.45,
                );
                let h1 = to_pt(0.58 + dx, 0.22);
                let h2 = to_pt(0.66 + dx, 0.14);
                let h3 = to_pt(0.61 + dx, 0.06);
                let cu = (eff_anim_t * 0.26 + idx as f32 * 0.45).fract();
                let glint = cubic_point(h0, h1, h2, h3, cu);
                frame.fill(
                    &Path::circle(glint, (1.5 * s_fac).clamp(1.1, 3.0)),
                    Color {
                        a: 0.38 * (cu * PI).sin(),
                        ..pulse_a
                    },
                );
            }
        }

        // 3. Animated Airflow & Thermal Ribbons
        if eff_airflow && !minimal {
            let peak_temp = match (self.cpu_temp, self.gpu_temp) {
                (Some(c), Some(g)) => c.max(g),
                (Some(c), None) => c,
                (None, Some(g)) => g,
                (None, None) => 46.0,
            };
            let flow_tint = thermal_color_for_temp(peak_temp, firefly);
            let flow_speed = 0.22 + (self.fan_duty as f32 / 100.0) * 0.78;
            let secondary_ribbon_tint = if firefly && !architectural {
                accent
            } else {
                mix_color(flow_tint, accent_hi, 0.35)
            };
            draw_airflow_ribbon(
                frame,
                to_pt(0.08, 0.31),
                to_pt(0.32, 0.28),
                to_pt(0.58, 0.22),
                to_pt(0.92, 0.24),
                flow_tint,
                eff_anim_t,
                flow_speed,
                0.05,
                architectural,
                s_fac,
            );
            draw_airflow_ribbon(
                frame,
                to_pt(0.08, 0.50),
                to_pt(0.34, 0.47),
                to_pt(0.54, 0.31),
                to_pt(0.64, 0.06),
                secondary_ribbon_tint,
                eff_anim_t,
                flow_speed * 0.94,
                0.34,
                architectural,
                s_fac,
            );
            draw_airflow_ribbon(
                frame,
                to_pt(0.10, 0.69),
                to_pt(0.42, 0.68),
                to_pt(0.72, 0.52),
                to_pt(0.92, 0.30),
                flow_tint,
                eff_anim_t,
                flow_speed * 0.86,
                0.67,
                architectural,
                s_fac,
            );
            draw_airflow_ribbon(
                frame,
                to_pt(0.42, 0.86),
                to_pt(0.46, 0.66),
                to_pt(0.66, 0.48),
                to_pt(0.88, 0.28),
                mix_color(flow_tint, secondary_ribbon_tint, 0.45),
                eff_anim_t,
                flow_speed * 0.78,
                0.22,
                architectural,
                s_fac,
            );
        }

        // 4. Spinning 7-blade fans (dynamic when eff_spinning_fans is true) + selected/override accents
        let blade_theta = eff_anim_t * (1.2 + (self.fan_duty as f32 / 100.0) * 9.5);
        for zone in self.frames {
            if !zone.enabled && !self.arrange_mode {
                continue;
            }
            let is_selected = self.selected_zone == Some(zone.zone_id.as_str());
            let is_hovered = hovered_zone.is_some_and(|hz| hz.zone_id == zone.zone_id);
            let has_override = self.zone_overrides.contains_key(&zone.zone_id);

            let ring_accent = if is_selected || is_hovered {
                Some(Color {
                    a: if is_selected { 0.92 } else { 0.78 },
                    ..accent_hi
                })
            } else if has_override {
                Some(Color { a: 0.62, ..accent })
            } else {
                None
            };

            for &([cx, cy], r_norm) in &zone.fan_rings {
                let center = Point::new(ox + cx * scale, oy + cy * scale);
                let r = (r_norm * scale).max(5.5);
                let hub_r = r * 0.29;
                if eff_spinning_fans && !minimal {
                    draw_fan_blades(
                        frame,
                        center,
                        r,
                        blade_theta,
                        firefly,
                        architectural,
                        s_fac,
                    );
                    // Re-draw clean motor hub on top of spinning blades
                    frame.fill(
                        &Path::circle(center, hub_r),
                        if firefly {
                            Color::from_rgb8(26, 22, 37)
                        } else {
                            Color::from_rgb8(20, 24, 30)
                        },
                    );
                    frame.stroke(
                        &Path::circle(center, hub_r),
                        Stroke::default()
                            .with_color(
                                ring_accent
                                    .unwrap_or_else(|| Color::from_rgba8(110, 94, 128, 0.45)),
                            )
                            .with_width((0.9 * s_fac.sqrt()).clamp(0.75, 1.6)),
                    );
                }
                if let Some(rc) = ring_accent {
                    frame.stroke(
                        &Path::circle(center, r),
                        Stroke::default()
                            .with_color(rc)
                            .with_width(
                                (if is_selected { 1.8 } else { 1.4 }) * s_fac.sqrt(),
                            ),
                    );
                }
            }

            if let Some(rc) = ring_accent
                && matches!(zone.layout, ZoneLayout::Strip { .. })
                && zone.canvas_pos.len() >= 2
            {
                let track = Path::new(|b| {
                    for (i, pos) in zone.canvas_pos.iter().enumerate() {
                        let pt = Point::new(ox + pos[0] * scale, oy + pos[1] * scale);
                        if i == 0 {
                            b.move_to(pt);
                        } else {
                            b.line_to(pt);
                        }
                    }
                });
                frame.stroke(
                    &track,
                    Stroke::default()
                        .with_color(Color { a: 0.45, ..rc })
                        .with_width((7.5 * s_fac).clamp(4.6, 13.5))
                        .with_line_cap(LineCap::Round)
                        .with_line_join(LineJoin::Round),
                );
            }
        }

        // 5. Volumetric 4-Layer LED Bloom & Crisp Hot Cores
        let base_led_r = (scale * 0.0155).clamp(2.4, 9.2);
        let mut active_zones = 0usize;
        let mut total_leds = 0usize;

        for zone in self.frames {
            if !zone.enabled && !self.arrange_mode {
                continue;
            }
            if zone.enabled {
                active_zones += 1;
                total_leds += zone.colors.len();
            }
            let is_selected = self.selected_zone == Some(zone.zone_id.as_str());
            let is_hovered = hovered_zone.is_some_and(|hz| hz.zone_id == zone.zone_id);
            let is_matrix = matches!(zone.layout, ZoneLayout::Matrix { .. });
            let led_r = if is_matrix && zone.colors.len() > 24 {
                (base_led_r * 0.58).clamp(1.5, 5.2)
            } else {
                base_led_r
            };

            for (pos, rgb_arr) in zone.canvas_pos.iter().zip(zone.colors.iter()) {
                let pt = Point::new(ox + pos[0] * scale, oy + pos[1] * scale);
                let lit = self.lights_on && zone.enabled;
                let c = if lit {
                    Color::from_rgb8(rgb_arr[0], rgb_arr[1], rgb_arr[2])
                } else if firefly {
                    Color::from_rgb8(42, 36, 54)
                } else {
                    Color::from_rgb8(27, 30, 35)
                };

                let lum =
                    (rgb_arr[0] as f32 + rgb_arr[1] as f32 + rgb_arr[2] as f32) / (3.0 * 255.0);
                if lit && lum > 0.02 {
                    let k = lum.sqrt();
                    if eff_bloom {
                        let spread = if is_matrix { 2.8 } else { 4.3 };
                        // 4-layer volumetric halo
                        frame.fill(
                            &Path::circle(pt, led_r * spread),
                            Color {
                                a: (0.068 * k).clamp(0.012, 0.088),
                                ..c
                            },
                        );
                        frame.fill(
                            &Path::circle(pt, led_r * (spread * 0.70)),
                            Color {
                                a: (0.14 * k).clamp(0.028, 0.18),
                                ..c
                            },
                        );
                        frame.fill(
                            &Path::circle(pt, led_r * (spread * 0.44)),
                            Color {
                                a: (0.26 * k).clamp(0.055, 0.34),
                                ..c
                            },
                        );
                    } else {
                        // Crisp lightweight single halo when ambient_bloom is off
                        frame.fill(
                            &Path::circle(pt, led_r * 1.65),
                            Color {
                                a: (0.20 * k).clamp(0.04, 0.24),
                                ..c
                            },
                        );
                    }
                }

                if is_matrix {
                    let s = led_r * 1.9;
                    let key_led = Path::rounded_rectangle(
                        Point::new(pt.x - s * 0.5, pt.y - s * 0.5),
                        Size::new(s, s),
                        iced::border::Radius::from(1.2),
                    );
                    frame.fill(&key_led, c);
                    if lit && lum > 0.12 {
                        let hot = mix_color(c, Color::WHITE, 0.36 * lum);
                        frame.fill(&Path::circle(pt, led_r * 0.55), hot);
                    }
                } else {
                    frame.fill(&Path::circle(pt, led_r), c);
                    if lit && lum > 0.10 {
                        let hot = mix_color(c, Color::WHITE, 0.42 * lum);
                        frame.fill(&Path::circle(pt, led_r * 0.62), hot);
                    }
                }
            }

            // Selected or Hovered zone outline
            if (is_selected || is_hovered)
                && let Some((bx0, by0, bx1, by1)) = zone_bounds(zone)
            {
                let pad = 8.0;
                let rx = ox + bx0 * scale - pad;
                let ry = oy + by0 * scale - pad;
                let rw = (bx1 - bx0) * scale + pad * 2.0;
                let rh = (by1 - by0) * scale + pad * 2.0;
                let sel_rect = Path::rounded_rectangle(
                    Point::new(rx, ry),
                    Size::new(rw.max(16.0), rh.max(16.0)),
                    iced::border::Radius::from(6.0),
                );
                if is_selected {
                    let pulse = 0.65 + 0.30 * (eff_anim_t * 3.2).sin();
                    frame.stroke(
                        &sel_rect,
                        Stroke {
                            style: canvas::Style::Solid(Color {
                                a: pulse,
                                ..accent_hi
                            }),
                            width: 1.5,
                            line_cap: LineCap::Round,
                            line_join: LineJoin::Round,
                            line_dash: LineDash {
                                segments: &[5.0, 4.0],
                                offset: 0,
                            },
                        },
                    );
                } else {
                    // Hover Zone X-Ray crisp accent highlight outline
                    frame.stroke(
                        &sel_rect,
                        Stroke::default()
                            .with_color(Color { a: 0.78, ..accent })
                            .with_width(1.3),
                    );
                }
            }

            if self.arrange_mode
                && let Some(center) = zone_center(zone)
            {
                let h_pt = Point::new(ox + center[0] * scale, oy + center[1] * scale);
                frame.fill(&Path::circle(h_pt, 5.5), rgba(p.well, 0.92));
                frame.stroke(
                    &Path::circle(h_pt, 5.5),
                    Stroke::default()
                        .with_color(if is_selected { accent_hi } else { rgb(p.ink_3) })
                        .with_width(1.5),
                );
            }
        }

        // 5a. De-duplicated, Zero-Overlap Zone Callout Pills + Crisp 1px Leader Lines
        let resolved_labels = resolve_stage_labels(
            self.frames,
            self.zone_overrides,
            self.selected_zone,
            hovered_zone.map(|hz| hz.zone_id.as_str()),
            self.show_labels,
            self.arrange_mode,
            self.lights_on,
            ox,
            oy,
            scale,
            bounds,
            self.eyebrow.is_some(),
        );

        // Draw all 1px leader lines & anchor pips first so callout pills sit cleanly above them
        for lbl in &resolved_labels {
            let r = lbl.rect;
            let attach_x = lbl.anchor.x.clamp(r.x + 5.0, r.x + r.width - 5.0);
            let attach_y = lbl.anchor.y.clamp(r.y, r.y + r.height);
            let dx = lbl.anchor.x - attach_x;
            let dy = lbl.anchor.y - attach_y;
            let dist = dx.hypot(dy);

            if dist > 4.5 {
                let leader_col = if lbl.is_selected || lbl.is_hovered || lbl.has_override {
                    Color {
                        a: 0.76,
                        ..accent_hi
                    }
                } else if firefly {
                    Color::from_rgba8(204, 192, 224, 0.44)
                } else {
                    rgba(p.ink_3, 0.46)
                };

                let attach_pt = Point::new(attach_x, attach_y);
                let path = if dx.abs() > 14.0 && dy.abs() > 8.0 {
                    let mid = Point::new(attach_x + dx * 0.45, attach_y);
                    Path::new(|b| {
                        b.move_to(attach_pt);
                        b.line_to(mid);
                        b.line_to(lbl.anchor);
                    })
                } else {
                    Path::line(attach_pt, lbl.anchor)
                };
                frame.stroke(
                    &path,
                    Stroke::default()
                        .with_color(leader_col)
                        .with_width(1.0)
                        .with_line_cap(LineCap::Round)
                        .with_line_join(LineJoin::Round),
                );
                frame.fill(
                    &Path::circle(lbl.anchor, 2.0),
                    Color {
                        a: (leader_col.a * 1.25).min(0.90),
                        ..leader_col
                    },
                );
            }
        }

        // Draw frosted hardware callout pills
        for lbl in &resolved_labels {
            let r = lbl.rect;
            let pill_rad = iced::border::Radius::from(if p.square_corners { 1.5 } else { 5.5 });
            let highlighted = lbl.is_selected || lbl.is_hovered || lbl.has_override;

            // Subtle grounded pill drop shadow
            frame.fill(
                &Path::rounded_rectangle(
                    Point::new(r.x, r.y + 1.5),
                    r.size(),
                    pill_rad,
                ),
                Color::from_rgba(0.01, 0.01, 0.03, 0.48),
            );

            let pill_path = Path::rounded_rectangle(r.position(), r.size(), pill_rad);
            let pill_bg = if lbl.is_selected {
                if firefly {
                    Color::from_rgba8(54, 42, 74, 0.95)
                } else {
                    rgba(p.raised, 0.95)
                }
            } else if firefly {
                Color::from_rgba8(24, 19, 38, 0.88)
            } else {
                Color::from_rgba8(14, 17, 23, 0.88)
            };
            frame.fill(&pill_path, pill_bg);
            frame.stroke(
                &pill_path,
                Stroke::default()
                    .with_color(if highlighted {
                        Color { a: 0.78, ..accent }
                    } else {
                        Color::from_rgba(1.0, 1.0, 1.0, 0.14)
                    })
                    .with_width(if lbl.is_selected { 1.15 } else { 0.9 }),
            );

            // Live zone RGB pip + crisp label text
            let dot_c = if lbl.any_enabled {
                lbl.swatch_col
            } else {
                rgb(p.ink_3)
            };
            frame.fill(
                &Path::circle(Point::new(r.x + 8.0, r.y + r.height * 0.5), 2.5),
                dot_c,
            );
            frame.fill_text(canvas::Text {
                content: lbl.tag.clone(),
                position: Point::new(r.x + 14.5, r.y + r.height * 0.5),
                color: if highlighted {
                    accent_hi
                } else if firefly {
                    Color::from_rgb8(232, 226, 240)
                } else {
                    Color::from_rgb8(216, 222, 230)
                },
                size: iced::Pixels(10.2),
                align_y: iced::alignment::Vertical::Center,
                ..canvas::Text::default()
            });
        }

        // 5b. Living Mode AIO LCD Pump Screen & GPU Temp Micro-Badge (drawn ABOVE ribbons & hoses;
        // suppressed when `show_labels == true` on Lighting tab so zone labels stay 100% uncluttered!)
        if self.rig_style == RigStyle::Living && !self.show_labels {
            let cpu_c = self.cpu_temp.unwrap_or(46.0);
            let cpu_col = thermal_color_for_temp(cpu_c, firefly);
            let lcd_r = (aio_r * 0.86).clamp(11.0, 48.0);
            let bezel_w = (1.15 * s_fac.sqrt()).clamp(0.95, 2.2);

            // Soft drop shadow + metallic outer bezel
            frame.fill(
                &Path::circle(Point::new(aio_c.x, aio_c.y + 1.5 * s_fac.sqrt()), aio_r + 1.0),
                Color::from_rgba(0.02, 0.01, 0.05, 0.65),
            );
            frame.fill(&Path::circle(aio_c, aio_r), Color::from_rgb8(26, 22, 38));
            frame.stroke(
                &Path::circle(aio_c, aio_r),
                Stroke::default()
                    .with_color(Color { a: 0.48, ..accent })
                    .with_width(bezel_w),
            );
            // Deep obsidian OLED display face + subtle symmetric inner rim
            frame.fill(&Path::circle(aio_c, lcd_r), Color::from_rgb8(11, 9, 20));
            frame.stroke(
                &Path::circle(aio_c, lcd_r),
                Stroke::default()
                    .with_color(Color::from_rgba(1.0, 1.0, 1.0, 0.12))
                    .with_width((0.8 * s_fac.sqrt()).clamp(0.7, 1.4)),
            );
            if eff_bloom {
                fill_radial_bloom(frame, aio_c, lcd_r * 1.18, cpu_col, 0.22, 3);
            }
            // Subtle top-left glass specular crescent
            let glass_arc = Path::new(|b| {
                b.arc(Arc {
                    center: aio_c,
                    radius: lcd_r * 0.72,
                    start_angle: iced::Radians(205.0_f32.to_radians()),
                    end_angle: iced::Radians(295.0_f32.to_radians()),
                });
            });
            frame.stroke(
                &glass_arc,
                Stroke::default()
                    .with_color(Color::from_rgba(1.0, 1.0, 1.0, 0.15))
                    .with_width((1.2 * s_fac.sqrt()).clamp(0.9, 2.2))
                    .with_line_cap(LineCap::Round),
            );
            // 270° thermal progress ring around the OLED bezel
            let temp_frac = ((cpu_c - 28.0) / 67.0).clamp(0.10, 1.0);
            let arc_start = 135.0_f32.to_radians();
            let arc_sweep = 270.0_f32.to_radians();
            let ring_w = (2.0 * s_fac.sqrt()).clamp(1.3, 3.8);
            let ring_r = (lcd_r - ring_w * 0.95).max(5.0);
            let track_arc = Path::new(|b| {
                b.arc(Arc {
                    center: aio_c,
                    radius: ring_r,
                    start_angle: iced::Radians(arc_start),
                    end_angle: iced::Radians(arc_start + arc_sweep),
                });
            });
            frame.stroke(
                &track_arc,
                Stroke::default()
                    .with_color(Color::from_rgba(1.0, 1.0, 1.0, 0.20))
                    .with_width(ring_w * 0.90)
                    .with_line_cap(LineCap::Round),
            );
            let val_arc = Path::new(|b| {
                b.arc(Arc {
                    center: aio_c,
                    radius: ring_r,
                    start_angle: iced::Radians(arc_start),
                    end_angle: iced::Radians(arc_start + arc_sweep * temp_frac),
                });
            });
            frame.stroke(
                &val_arc,
                Stroke::default()
                    .with_color(cpu_col)
                    .with_width(ring_w)
                    .with_line_cap(LineCap::Round),
            );
            // Dead-centered glowing tabular CPU temperature digits scaled smoothly with OLED radius
            let cpu_disp = format!("{:.0}°", self.temp_unit.convert(cpu_c));
            let show_sub = lcd_r >= 19.0;
            let font_sz = (lcd_r * if show_sub { 0.54 } else { 0.64 }).clamp(8.5, 25.0);
            frame.fill_text(canvas::Text {
                content: cpu_disp,
                position: Point::new(aio_c.x, aio_c.y - if show_sub { lcd_r * 0.13 } else { 0.0 }),
                color: mix_color(cpu_col, Color::WHITE, 0.62),
                size: iced::Pixels(font_sz),
                font: crate::theme::mono_font(),
                align_x: iced::alignment::Horizontal::Center.into(),
                align_y: iced::alignment::Vertical::Center,
                ..canvas::Text::default()
            });
            if show_sub {
                frame.fill_text(canvas::Text {
                    content: "CPU".to_string(),
                    position: Point::new(aio_c.x, aio_c.y + lcd_r * 0.42),
                    color: rgba(p.ink_3, 0.84),
                    size: iced::Pixels((lcd_r * 0.27).clamp(6.8, 12.0)),
                    font: crate::theme::mono_font(),
                    align_x: iced::alignment::Horizontal::Center.into(),
                    align_y: iced::alignment::Vertical::Center,
                    ..canvas::Text::default()
                });
            }

            // GPU shroud live GPU temp micro-badge (scales smoothly from Mini-Mode to Fullscreen!)
            if let Some(gpu_c) = self.gpu_temp
                && gpu_w > 54.0
                && gpu_h > 12.0
            {
                let gpu_col = thermal_color_for_temp(gpu_c, firefly);
                let g_txt = if gpu_w > 106.0 {
                    format!("GPU {:.0}°", self.temp_unit.convert(gpu_c))
                } else {
                    format!("{:.0}°", self.temp_unit.convert(gpu_c))
                };
                let bh = (gpu_h * 0.72).clamp(13.0, 30.0);
                let font_sz = (bh * 0.54).clamp(8.2, 15.0);
                let dot_r = (bh * 0.13).clamp(1.8, 3.6);
                let pad_l = (bh * 0.36).clamp(5.0, 11.0);
                let bw = (g_txt.len() as f32 * font_sz * 0.62 + pad_l + dot_r * 2.8 + 8.0)
                    .min(gpu_w * 0.58);
                let bx = gpu_tl.x + gpu_w - bw - (6.0 * s_fac.sqrt()).clamp(4.0, 12.0);
                let by = gpu_tl.y + (gpu_h - bh) * 0.5;
                let b_rad = (bh * 0.28).clamp(3.5, 8.0);
                let badge = Path::rounded_rectangle(
                    Point::new(bx, by),
                    Size::new(bw, bh),
                    iced::border::Radius::from(b_rad),
                );
                frame.fill(
                    &Path::rounded_rectangle(
                        Point::new(bx, by + 1.5),
                        Size::new(bw, bh),
                        iced::border::Radius::from(b_rad),
                    ),
                    Color::from_rgba(0.02, 0.01, 0.05, 0.55),
                );
                frame.fill(&badge, Color::from_rgba8(14, 11, 24, 0.95));
                frame.stroke(
                    &badge,
                    Stroke::default()
                        .with_color(Color { a: 0.65, ..gpu_col })
                        .with_width((0.95 * s_fac.sqrt()).clamp(0.85, 1.8)),
                );
                frame.fill(
                    &Path::circle(Point::new(bx + pad_l, by + bh * 0.5), dot_r),
                    gpu_col,
                );
                frame.fill_text(canvas::Text {
                    content: g_txt,
                    position: Point::new(bx + pad_l + dot_r * 2.2, by + bh * 0.5),
                    color: mix_color(gpu_col, Color::WHITE, 0.62),
                    size: iced::Pixels(font_sz),
                    font: crate::theme::mono_font(),
                    align_y: iced::alignment::Vertical::Center,
                    ..canvas::Text::default()
                });
            }
        }

        // 6. Arrange Mode Snap Crosshairs (within 0.03 canvas units of another zone's X or Y center)
        if self.arrange_mode
            && let Some((drag_id, _)) = &state.dragging
            && let Some(dragged) = self.frames.iter().find(|z| &z.zone_id == drag_id)
            && let Some(dc) = zone_center(dragged)
        {
            let d_pt = Point::new(ox + dc[0] * scale, oy + dc[1] * scale);
            let guide_col = Color { a: 0.62, ..accent_hi };
            for other in self.frames {
                if &other.zone_id == drag_id {
                    continue;
                }
                let Some(oc) = zone_center(other) else {
                    continue;
                };
                let o_pt = Point::new(ox + oc[0] * scale, oy + oc[1] * scale);

                if (dc[0] - oc[0]).abs() <= 0.03 {
                    let gx = o_pt.x;
                    frame.stroke(
                        &Path::line(Point::new(gx, ch_y + 4.0), Point::new(gx, ch_y + ch_h - 4.0)),
                        Stroke {
                            style: canvas::Style::Solid(guide_col),
                            width: 1.0,
                            line_dash: LineDash {
                                segments: &[4.0, 4.0],
                                offset: 0,
                            },
                            ..Stroke::default()
                        },
                    );
                    for pt in [Point::new(gx, d_pt.y), o_pt] {
                        frame.fill(&Path::circle(pt, 3.0), accent_hi);
                    }
                }

                if (dc[1] - oc[1]).abs() <= 0.03 {
                    let gy = o_pt.y;
                    frame.stroke(
                        &Path::line(Point::new(ch_x + 4.0, gy), Point::new(ch_x + ch_w - 4.0, gy)),
                        Stroke {
                            style: canvas::Style::Solid(guide_col),
                            width: 1.0,
                            line_dash: LineDash {
                                segments: &[4.0, 4.0],
                                offset: 0,
                            },
                            ..Stroke::default()
                        },
                    );
                    for pt in [Point::new(d_pt.x, gy), o_pt] {
                        frame.fill(&Path::circle(pt, 3.0), accent_hi);
                    }
                }
            }
        }

        // 7. Hover Zone X-Ray Frosted-Plum Tooltip Pill ("Front F420 RGB · 48 LEDs · Aurora")
        if let (Some(hz), Some(hpos)) = (hovered_zone, hovered_pos) {
            let eff_label = self
                .zone_overrides
                .get(&hz.zone_id)
                .map(|ov| ov.effect.label())
                .unwrap_or_else(|| self.active_effect.label());
            let tip = format!(
                "{} · {} LEDs · {}",
                hz.zone_name,
                hz.colors.len(),
                eff_label
            );
            let pill_h = 22.0;
            let pill_w = (tip.len() as f32 * 5.9 + 26.0).clamp(110.0, bounds.width - 16.0);
            let tx = if hpos.x + 14.0 + pill_w > bounds.width - 8.0 {
                (hpos.x - pill_w - 12.0).max(8.0)
            } else {
                hpos.x + 14.0
            };
            let ty = if hpos.y - pill_h - 10.0 < 8.0 {
                (hpos.y + 14.0).min(bounds.height - pill_h - 8.0)
            } else {
                hpos.y - pill_h - 10.0
            };
            let pill_rect = Path::rounded_rectangle(
                Point::new(tx, ty),
                Size::new(pill_w, pill_h),
                iced::border::Radius::from(7.0),
            );
            // Soft drop shadow
            frame.fill(
                &Path::rounded_rectangle(
                    Point::new(tx, ty + 2.0),
                    Size::new(pill_w, pill_h),
                    iced::border::Radius::from(7.0),
                ),
                Color::from_rgba(0.02, 0.01, 0.05, 0.45),
            );
            // Frosted-plum glass fill
            let pill_bg = if firefly {
                Color::from_rgba8(44, 34, 58, 0.93)
            } else {
                rgba(p.raised, 0.94)
            };
            frame.fill(&pill_rect, pill_bg);
            frame.stroke(
                &pill_rect,
                Stroke::default()
                    .with_color(Color { a: 0.72, ..accent })
                    .with_width(1.1),
            );
            // Live average zone color dot
            let swatch_col = if self.lights_on && hz.enabled {
                zone_average_color(hz)
            } else {
                rgb(p.ink_3)
            };
            frame.fill(
                &Path::circle(Point::new(tx + 10.0, ty + pill_h * 0.5), 3.6),
                swatch_col,
            );
            frame.fill_text(canvas::Text {
                content: tip,
                position: Point::new(tx + 18.0, ty + 4.5),
                color: if firefly {
                    Color::from_rgb8(255, 244, 228)
                } else {
                    rgb(p.ink)
                },
                size: iced::Pixels(10.5),
                ..canvas::Text::default()
            });
        }

        // 8. Optional Overview "LIVING RIG" top glass eyebrow caption
        if let Some(label) = self.eyebrow {
            let dot_pt = Point::new(16.0, 15.0);
            let dot_col = if self.lights_on && amb_lum > 0.14 {
                mix_color(amb, Color::WHITE, 0.25)
            } else if firefly {
                Color::from_rgb8(165, 222, 206)
            } else {
                accent_hi
            };
            if self.lights_on && eff_bloom {
                fill_radial_bloom(frame, dot_pt, 7.5, dot_col, 0.42, 4);
            }
            frame.fill(&Path::circle(dot_pt, 3.0), dot_col);
            frame.fill_text(canvas::Text {
                content: label.to_string(),
                position: Point::new(26.0, 9.0),
                color: if firefly {
                    Color::from_rgb8(236, 226, 238)
                } else {
                    Color::from_rgb8(225, 228, 232)
                },
                size: iced::Pixels(10.5),
                ..canvas::Text::default()
            });

            let meta = format!(
                "{} zone{} · {} LEDs",
                active_zones,
                if active_zones == 1 { "" } else { "s" },
                total_leds
            );
            let meta_w = meta.len() as f32 * 5.8;
            frame.fill_text(canvas::Text {
                content: meta,
                position: Point::new((bounds.width - meta_w - 14.0).max(120.0), 9.0),
                color: if firefly {
                    Color::from_rgb8(186, 174, 198)
                } else {
                    Color::from_rgb8(167, 175, 183)
                },
                size: iced::Pixels(10.5),
                ..canvas::Text::default()
            });
        }
        };

        let dynamic_geom = if self.animated {
            let mut frame = Frame::new(renderer, bounds.size());
            render_layer2(&mut frame);
            frame.into_geometry()
        } else {
            let dyn_key = stage_dynamic_hash(
                self,
                bounds,
                hovered_zone.map(|hz| hz.zone_id.as_str()),
                hovered_pos,
                state.dragging.as_ref().map(|(id, _)| id.as_str()),
            );
            if state.dynamic_key.get() != dyn_key {
                state.dynamic_cache.clear();
                state.dynamic_key.set(dyn_key);
            }
            state.dynamic_cache.draw(renderer, bounds.size(), |frame| {
                render_layer2(frame);
            })
        };

        vec![static_geom, dynamic_geom]
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.dragging.is_some() {
            return mouse::Interaction::Grabbing;
        }
        if let Some(pos) = cursor.position_in(bounds) {
            if self.arrange_mode {
                return mouse::Interaction::Grab;
            }
            let (scale, ox, oy, _wb) = viewport(
                bounds,
                self.eyebrow.is_some(),
                self.show_labels,
                self.frames,
                self.arrange_mode,
            );
            if hit_test_zone(self.frames, self.arrange_mode, ox, oy, scale, pos).is_some() {
                return mouse::Interaction::Pointer;
            }
        }
        mouse::Interaction::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_fills_mini_and_fullscreen_without_postage_stamp() {
        // Mini-Mode 430x404 (width-constrained)
        let mini = Rectangle::new(Point::ORIGIN, Size::new(430.0, 404.0));
        let (scale_m, _ox_m, _oy_m, wb_m) = viewport(mini, false, false, &[], false);
        let stage_w_m = (wb_m.2 - wb_m.0) * scale_m;
        let stage_h_m = (wb_m.3 - wb_m.1) * scale_m;
        assert!(stage_w_m >= mini.width * 0.88 && stage_w_m <= mini.width * 0.91);
        assert!(stage_h_m <= mini.height * 0.92 + 0.5);

        // Fullscreen hero 1600x520 (height-constrained, fills ~91% height)
        let full = Rectangle::new(Point::ORIGIN, Size::new(1600.0, 520.0));
        let (scale_f, _ox_f, _oy_f, wb_f) = viewport(full, true, false, &[], false);
        let stage_h_f = (wb_f.3 - wb_f.1) * scale_f;
        assert!(stage_h_f >= full.height * 0.88 && stage_h_f <= full.height * 0.92 + 0.5);
        assert!(rig_scale_factor(scale_f) > rig_scale_factor(scale_m));
    }

    #[test]
    fn stage_labels_deduplicate_subzones_and_never_overlap() {
        let mk_strip = |id: &str, name: &str, x: f32, y: f32| ZoneFrame {
            zone_id: id.to_string(),
            zone_name: name.to_string(),
            enabled: true,
            layout: ZoneLayout::Strip {
                leds: 8,
                x,
                y,
                angle: 0.0,
                length: 0.4,
                reverse: false,
            },
            canvas_pos: vec![[x - 0.1, y], [x + 0.1, y]],
            fan_rings: vec![],
            colors: vec![[255, 160, 90], [165, 222, 206]],
        };

        // Mirrors the user's real 12-zone rig including colliding ARGB headers + 4 RTX 5080 subzones
        let frames = vec![
            mk_strip("argb1", "CPU cooler ring", 0.92, 0.10),
            mk_strip("board", "Motherboard", 0.92, 0.27),
            mk_strip("argb2", "ARGB header 2", 0.98, 0.47),
            mk_strip("argb3", "ARGB header 3", 1.00, 0.54),
            mk_strip("gpu0", "RTX 5080 Gaming OC: Right fan", 0.718, 0.66),
            mk_strip("gpu1", "RTX 5080 Gaming OC: Left fan", 0.872, 0.66),
            mk_strip("gpu2", "RTX 5080 Gaming OC: Center fan", 1.027, 0.66),
            mk_strip("gpu3", "RTX 5080 Gaming OC: Side", 1.182, 0.66),
            mk_strip("bot", "Bottom F420 RGB", 0.80, 0.85),
            mk_strip("front", "Front F420 RGB", 0.18, 0.50),
            mk_strip("rear", "Rear F120 RGB Core (case version)", 1.42, 0.27),
            mk_strip("kbd", "K2 HE", 1.48, 0.84),
        ];
        let overrides = BTreeMap::new();

        for (w, h) in [(1200.0_f32, 300.0_f32), (640.0, 240.0)] {
            let bounds = Rectangle::new(Point::ORIGIN, Size::new(w, h));
            let (scale, ox, oy, _wb) = viewport(bounds, false, true, &frames, false);
            let resolved = resolve_stage_labels(
                &frames,
                &overrides,
                None,
                None,
                true,
                false,
                true,
                ox,
                oy,
                scale,
                bounds,
                false,
            );

            // 12 zones with 4 GPU subzones -> exactly 9 unique non-empty labels!
            assert_eq!(resolved.len(), 9);
            assert_eq!(
                resolved
                    .iter()
                    .filter(|l| l.tag == "RTX 5080 Gaming OC")
                    .count(),
                1
            );

            // Verify zero pairwise bounding-box overlaps (and >= 21.5px vertical pitch when horizontally overlapping)
            for i in 0..resolved.len() {
                for j in (i + 1)..resolved.len() {
                    let a = resolved[i].rect;
                    let b = resolved[j].rect;
                    let horiz_overlap = a.x < b.x + b.width && b.x < a.x + a.width;
                    let vert_overlap = a.y < b.y + b.height && b.y < a.y + a.height;
                    assert!(
                        !(horiz_overlap && vert_overlap),
                        "Labels '{}' ({:?}) and '{}' ({:?}) overlapped at {w}x{h}!",
                        resolved[i].tag,
                        a,
                        resolved[j].tag,
                        b
                    );
                    if horiz_overlap {
                        assert!(
                            (a.y - b.y).abs() >= 21.0,
                            "Vertically stacked labels '{}' and '{}' closer than 22px pitch: dy={}",
                            resolved[i].tag,
                            resolved[j].tag,
                            (a.y - b.y).abs()
                        );
                    }
                }
            }
        }
    }
}
