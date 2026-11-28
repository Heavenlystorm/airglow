//! Zone shapes -> LED positions on the shared 1.6 x 1.0 PC case canvas.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::f32::consts::PI;

use crate::config::{ColorOrder, ZoneConfig};

pub const CANVAS_W: f32 = 1.6;
pub const CANVAS_H: f32 = 1.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "lowercase")]
pub enum ZoneLayout {
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
        #[serde(default)]
        map: Vec<i32>,
    },
}

impl ZoneLayout {
    pub fn total_leds(&self) -> usize {
        match self {
            Self::Fans {
                fans,
                leds_per_fan,
                ..
            } => fans.saturating_mul(*leds_per_fan),
            Self::Strip { leds, .. } | Self::Matrix { leds, .. } => *leds,
        }
    }

    pub fn shape_label(&self) -> &'static str {
        match self {
            Self::Fans { .. } => "Fans",
            Self::Strip { .. } => "Strip",
            Self::Matrix { .. } => "Matrix",
        }
    }

    pub fn center(&self) -> (f32, f32) {
        match self {
            Self::Fans { x, y, .. } | Self::Strip { x, y, .. } | Self::Matrix { x, y, .. } => {
                (*x, *y)
            }
        }
    }

    pub fn set_center(&mut self, cx: f32, cy: f32) {
        match self {
            Self::Fans { x, y, .. } | Self::Strip { x, y, .. } | Self::Matrix { x, y, .. } => {
                *x = cx;
                *y = cy;
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ZoneGeometry {
    pub zone_id: String,
    pub zone_name: String,
    /// Canvas coordinates `(0..CANVAS_W, 0..CANVAS_H)` for drawing on the case stage.
    pub canvas_pos: Vec<[f32; 2]>,
    /// Normalized coordinates `(0..1, 0..1)` passed into lighting effects.
    pub norm_pos: Vec<[f32; 2]>,
    /// Normalized position `0..1` around each individual fan ring or along a strip/matrix row.
    pub local: Vec<f32>,
    /// Fan index within the zone.
    pub fan_index: Vec<usize>,
    /// Physical fan center coordinates in canvas units for drawing fan housing rings.
    pub fan_rings: Vec<([f32; 2], f32)>,
}

impl ZoneGeometry {
    pub fn from_layout(zone: &ZoneConfig) -> Self {
        let mut canvas_pos = Vec::new();
        let mut local = Vec::new();
        let mut fan_index = Vec::new();
        let mut fan_rings = Vec::new();

        match &zone.layout {
            ZoneLayout::Fans {
                fans,
                leds_per_fan,
                x,
                y,
                angle,
                size,
                reverse,
            } => {
                let rad = angle.to_radians();
                let (dx, dy) = (rad.cos(), rad.sin());
                let r = *size * 0.5;
                let gap = *size * 1.14;
                for f in 0..*fans {
                    let off = (f as f32 - (*fans as f32 - 1.0) * 0.5) * gap;
                    let cx = *x + dx * off;
                    let cy = *y + dy * off;
                    fan_rings.push(([cx, cy], r));
                    for i in 0..*leds_per_fan {
                        let t = i as f32 / (*leds_per_fan).max(1) as f32;
                        let th = -PI * 0.5 + t * 2.0 * PI;
                        canvas_pos.push([cx + r * th.cos(), cy + r * th.sin()]);
                        local.push(t);
                        fan_index.push(f);
                    }
                }
                if *reverse {
                    canvas_pos.reverse();
                    local.reverse();
                    fan_index.reverse();
                }
            }
            ZoneLayout::Strip {
                leds,
                x,
                y,
                angle,
                length,
                reverse,
            } => {
                let rad = angle.to_radians();
                let (dx, dy) = (rad.cos(), rad.sin());
                for i in 0..*leds {
                    let t = if *leds > 1 {
                        i as f32 / (*leds - 1) as f32
                    } else {
                        0.5
                    };
                    let off = (t - 0.5) * *length;
                    canvas_pos.push([*x + dx * off, *y + dy * off]);
                    local.push(t);
                    fan_index.push(0);
                }
                if *reverse {
                    canvas_pos.reverse();
                    local.reverse();
                }
            }
            ZoneLayout::Matrix {
                leds,
                rows,
                cols,
                x,
                y,
                angle,
                length,
                reverse,
                map,
            } => {
                let n = *leds;
                let r_cnt = (*rows).max(1);
                let c_cnt = (*cols).max(1);
                let rad = angle.to_radians();
                let (dx, dy) = (rad.cos(), rad.sin());
                let height = *length * r_cnt as f32 / c_cnt as f32;

                let mut cell: HashMap<usize, (usize, usize)> = HashMap::new();
                if map.is_empty() {
                    for led in 0..n {
                        let r = (led / c_cnt).min(r_cnt - 1);
                        let c = led % c_cnt;
                        cell.insert(led, (r, c));
                    }
                } else {
                    for (idx, &led_val) in map.iter().enumerate() {
                        if led_val >= 0 {
                            let led = led_val as usize;
                            if led < n {
                                let r = (idx / c_cnt).min(r_cnt - 1);
                                let c = idx % c_cnt;
                                cell.entry(led).or_insert((r, c));
                            }
                        }
                    }
                }

                for led in 0..n {
                    let (r, c) = cell.get(&led).copied().unwrap_or((r_cnt - 1, c_cnt - 1));
                    let ox = ((c as f32 + 0.5) / c_cnt as f32 - 0.5) * *length;
                    let oy = ((r as f32 + 0.5) / r_cnt as f32 - 0.5) * height;
                    canvas_pos.push([*x + dx * ox - dy * oy, *y + dy * ox + dx * oy]);
                    local.push(c as f32 / c_cnt.saturating_sub(1).max(1) as f32);
                    fan_index.push(0);
                }
                if *reverse {
                    canvas_pos.reverse();
                    local.reverse();
                    fan_index.reverse();
                }
            }
        }

        let norm_pos = canvas_pos
            .iter()
            .map(|[cx, cy]| [(cx / CANVAS_W).clamp(0.0, 1.0), (cy / CANVAS_H).clamp(0.0, 1.0)])
            .collect();

        Self {
            zone_id: zone.id.clone(),
            zone_name: zone.name.clone(),
            canvas_pos,
            norm_pos,
            local,
            fan_index,
            fan_rings,
        }
    }

    pub fn len(&self) -> usize {
        self.norm_pos.len()
    }

    pub fn is_empty(&self) -> bool {
        self.norm_pos.is_empty()
    }
}

/// Default sample PC rig matching Airglow's side-view case stage.
pub fn sample_case_zones() -> Vec<ZoneConfig> {
    vec![
        ZoneConfig {
            id: "front-intake".to_string(),
            name: "Front intake (3×120mm)".to_string(),
            enabled: true,
            color_order: ColorOrder::Grb,
            layout: ZoneLayout::Fans {
                fans: 3,
                leds_per_fan: 16,
                x: 1.36,
                y: 0.52,
                angle: 90.0,
                size: 0.21,
                reverse: false,
            },
        },
        ZoneConfig {
            id: "top-radiator".to_string(),
            name: "Top radiator (2×140mm)".to_string(),
            enabled: true,
            color_order: ColorOrder::Grb,
            layout: ZoneLayout::Fans {
                fans: 2,
                leds_per_fan: 16,
                x: 0.72,
                y: 0.17,
                angle: 0.0,
                size: 0.22,
                reverse: false,
            },
        },
        ZoneConfig {
            id: "rear-exhaust".to_string(),
            name: "Rear exhaust (1×120mm)".to_string(),
            enabled: true,
            color_order: ColorOrder::Grb,
            layout: ZoneLayout::Fans {
                fans: 1,
                leds_per_fan: 16,
                x: 0.22,
                y: 0.36,
                angle: 0.0,
                size: 0.21,
                reverse: false,
            },
        },
        ZoneConfig {
            id: "psu-strip".to_string(),
            name: "PSU shroud strip".to_string(),
            enabled: true,
            color_order: ColorOrder::Grb,
            layout: ZoneLayout::Strip {
                leds: 24,
                x: 0.66,
                y: 0.83,
                angle: 0.0,
                length: 0.92,
                reverse: false,
            },
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_case_builds_normalized_leds() {
        let zones = sample_case_zones();
        let geoms: Vec<_> = zones.iter().map(ZoneGeometry::from_layout).collect();
        assert_eq!(geoms[0].len(), 48);
        assert_eq!(geoms[3].len(), 24);
        for g in &geoms {
            for [x, y] in &g.norm_pos {
                assert!((0.0..=1.0).contains(x));
                assert!((0.0..=1.0).contains(y));
            }
        }
    }

    #[test]
    fn matrix_layout_maps_cells_and_handles_holes() {
        let zone = ZoneConfig {
            id: "kb-matrix".to_string(),
            name: "Keyboard Matrix".to_string(),
            enabled: true,
            color_order: ColorOrder::Rgb,
            layout: ZoneLayout::Matrix {
                leds: 4,
                rows: 2,
                cols: 3,
                x: 0.8,
                y: 0.5,
                angle: 0.0,
                length: 0.6,
                reverse: false,
                map: vec![0, -1, 1, 2, 3, -1],
            },
        };
        let geom = ZoneGeometry::from_layout(&zone);
        assert_eq!(geom.len(), 4);
        // LED 0 is col 0, row 0; LED 1 is col 2, row 0 -> x1 > x0, y1 == y0
        assert!(geom.canvas_pos[1][0] > geom.canvas_pos[0][0]);
        assert!((geom.canvas_pos[1][1] - geom.canvas_pos[0][1]).abs() < 1e-5);
        // LED 2 is col 0, row 1 -> y2 > y0
        assert!(geom.canvas_pos[2][1] > geom.canvas_pos[0][1]);
        assert_eq!(zone.layout.total_leds(), 4);
    }
}
