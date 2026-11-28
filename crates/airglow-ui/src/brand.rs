//! Procedural Airglow brand mark & dynamic live-tinted tray icon RGBA generator.

pub fn app_icon_rgba(size: u32) -> Vec<u8> {
    render_mark(size, Some([18, 20, 24]), [236, 235, 230], [110, 240, 160])
}

pub fn tray_mark_rgba(size: u32, rgb: [u8; 3]) -> Vec<u8> {
    let tinted = tray_tint(rgb);
    render_mark(size, None, tinted, tinted)
}

pub fn tray_tint(rgb: [u8; 3]) -> [u8; 3] {
    let peak = rgb[0].max(rgb[1]).max(rgb[2]);
    if peak <= 8 {
        return [140, 146, 154];
    }
    if peak < 170 {
        let scale = 170.0 / peak as f32;
        [
            (rgb[0] as f32 * scale).round().min(255.0) as u8,
            (rgb[1] as f32 * scale).round().min(255.0) as u8,
            (rgb[2] as f32 * scale).round().min(255.0) as u8,
        ]
    } else {
        rgb
    }
}

fn render_mark(size: u32, bg: Option<[u8; 3]>, body: [u8; 3], glow: [u8; 3]) -> Vec<u8> {
    let mut out = vec![0u8; (size * size * 4) as usize];
    let s = size as f32;
    let cx = s * 0.5;
    let cy = s * 0.68;
    let r_glow = s * 0.34;
    let r_planet = s * 0.19;
    let stroke = (s * 0.075).max(1.5);
    let corner_r = s * 0.22;

    for y in 0..size {
        for x in 0..size {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;

            let mut col = [0.0f32; 4];

            if let Some(bg_rgb) = bg {
                let in_tile = inside_rounded_rect(px, py, s, s, corner_r);
                if in_tile {
                    col = [bg_rgb[0] as f32, bg_rgb[1] as f32, bg_rgb[2] as f32, 255.0];
                }
            }

            let dx = px - cx;
            let dy = py - cy;
            let dist = (dx * dx + dy * dy).sqrt();

            // Planet half-disc above horizon
            if py <= cy && dist <= r_planet {
                col = [body[0] as f32, body[1] as f32, body[2] as f32, 255.0];
            }

            // Horizon line
            if (py - cy).abs() <= stroke * 0.5 && px >= s * 0.12 && px <= s * 0.88 {
                col = [body[0] as f32, body[1] as f32, body[2] as f32, 255.0];
            }

            // Airglow arc (195° to 345° in screen space, i.e. above horizon)
            let ang = dy.atan2(dx).to_degrees().rem_euclid(360.0);
            if (195.0..=345.0).contains(&ang) && (dist - r_glow).abs() <= stroke * 0.5 {
                col = [glow[0] as f32, glow[1] as f32, glow[2] as f32, 255.0];
            }

            let idx = ((y * size + x) * 4) as usize;
            out[idx] = col[0].round().clamp(0.0, 255.0) as u8;
            out[idx + 1] = col[1].round().clamp(0.0, 255.0) as u8;
            out[idx + 2] = col[2].round().clamp(0.0, 255.0) as u8;
            out[idx + 3] = col[3].round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

fn inside_rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> bool {
    let rx = x.clamp(r, w - r);
    let ry = y.clamp(r, h - r);
    let dx = x - rx;
    let dy = y - ry;
    dx * dx + dy * dy <= r * r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_tint_lifts_dim_colors_and_preserves_bright_ones() {
        assert_eq!(tray_tint([0, 0, 0]), [140, 146, 154]);
        assert_eq!(tray_tint([200, 100, 50]), [200, 100, 50]);
        let lifted = tray_tint([0, 40, 80]);
        assert_eq!(lifted[2], 170);
    }

    #[test]
    fn app_icon_has_expected_rgba_length() {
        assert_eq!(app_icon_rgba(32).len(), 32 * 32 * 4);
    }
}
