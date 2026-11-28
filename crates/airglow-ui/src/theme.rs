//! Maps all 13 Airglow interface designs (with Firefly as the flagship dusk-plum + moth + pink/apricot/mint look)
//! into Iced themes, typography helpers, and tactile milled-hardware surface & control styles.

#![allow(dead_code)]

use std::f32::consts::{FRAC_PI_2, PI};

use airglow_core::config::{DesignId, DesignPalette};
use iced::border::Radius;
use iced::gradient::Linear;
use iced::theme::Palette;
use iced::widget::{button, checkbox, container, slider, text_input};
use iced::{Background, Border, Color, Font, Gradient, Radians, Shadow, Theme, Vector, font};

/// 150-degree diagonal gradient angle (top-left -> bottom-right) for dimensional card surfaces.
const DIAG_150_RAD: f32 = 5.0 * PI / 6.0;

/// Extended design tokens beyond `DesignPalette` for Firefly & per-design craft.
#[derive(Debug, Clone, Copy)]
pub struct DesignTokens {
    pub design: DesignId,
    pub p: DesignPalette,
    pub accent_override: Option<[u8; 3]>,
    pub compact_density: bool,
    pub line_soft: u32,
    pub data_primary: u32,
    pub data_secondary: u32,
    pub pink_rose: u32,
    pub appbar_left: u32,
    pub appbar_mid: u32,
    pub appbar_right: u32,
    pub appbar_border: u32,
    pub primary_ink: u32,
    pub flat_surface: bool,
    pub pill_tabs: bool,
    pub pill_controls: bool,
}

impl DesignTokens {
    pub fn for_appearance(
        design: DesignId,
        accent_override: Option<[u8; 3]>,
        compact_density: bool,
    ) -> Self {
        let p = design.palette();
        let mut tokens = match design {
            DesignId::Firefly => Self {
                design,
                p,
                accent_override: None,
                compact_density,
                line_soft: 0x524260,
                data_primary: 0xa5dece,   // luminous mint
                data_secondary: 0xd1b9f0, // soft lilac
                pink_rose: 0xf49ec0,      // dusk rose-pink
                appbar_left: 0x221b2f,
                appbar_mid: 0x1f192c,
                appbar_right: 0x1c1728,
                appbar_border: 0x352b4a,
                primary_ink: 0x311e1b,
                flat_surface: false,
                pill_tabs: true,
                pill_controls: false,
            },
            DesignId::Graphite => Self {
                design,
                p,
                accent_override: None,
                compact_density,
                line_soft: 0x2f3137,
                data_primary: p.ink_2,
                data_secondary: p.ink_3,
                pink_rose: p.brass_hi,
                appbar_left: p.panel,
                appbar_mid: p.panel,
                appbar_right: p.panel,
                appbar_border: 0x2f3137,
                primary_ink: 0x1c1508,
                flat_surface: false,
                pill_tabs: false,
                pill_controls: false,
            },
            DesignId::Paper => Self {
                design,
                p,
                accent_override: None,
                compact_density,
                line_soft: 0xd8d1c4,
                data_primary: p.ink_2,
                data_secondary: p.ink_3,
                pink_rose: p.brass,
                appbar_left: 0xf7f4ed,
                appbar_mid: 0xf5f2eb,
                appbar_right: 0xf2eee5,
                appbar_border: 0xc9c2b4,
                primary_ink: 0xfffaf0,
                flat_surface: false,
                pill_tabs: false,
                pill_controls: false,
            },
            DesignId::Spore => Self {
                design,
                p,
                accent_override: None,
                compact_density,
                line_soft: 0x2f251e,
                data_primary: p.brass,
                data_secondary: p.ink_2,
                pink_rose: 0xf6c3ba,
                appbar_left: 0x19120e,
                appbar_mid: 0x17100c,
                appbar_right: 0x140d09,
                appbar_border: 0x372f28,
                primary_ink: 0x0f0a06,
                flat_surface: false,
                pill_tabs: true,
                pill_controls: true,
            },
            DesignId::Neon => Self {
                design,
                p,
                accent_override: None,
                compact_density,
                line_soft: 0x241a3f,
                data_primary: 0x2fe6ff,
                data_secondary: 0xa78bfa,
                pink_rose: 0xff74c5,
                appbar_left: 0x100e1e,
                appbar_mid: 0x0d0d17,
                appbar_right: 0x0a0a14,
                appbar_border: 0x2e2147,
                primary_ink: 0x12000a,
                flat_surface: false,
                pill_tabs: false,
                pill_controls: false,
            },
            DesignId::Kiln => Self {
                design,
                p,
                accent_override: None,
                compact_density,
                line_soft: 0x2a201c,
                data_primary: 0xff8a00,
                data_secondary: p.ink_2,
                pink_rose: 0xffcf66,
                appbar_left: 0x170e0b,
                appbar_mid: 0x140c09,
                appbar_right: 0x120a08,
                appbar_border: 0x2d2421,
                primary_ink: 0x100704,
                flat_surface: false,
                pill_tabs: true,
                pill_controls: true,
            },
            DesignId::Lumen => Self {
                design,
                p,
                accent_override: None,
                compact_density,
                line_soft: 0x232036,
                data_primary: p.ink_2,
                data_secondary: p.brass,
                pink_rose: 0xf3798b,
                appbar_left: 0x0f0c1d,
                appbar_mid: 0x0b0916,
                appbar_right: 0x090714,
                appbar_border: 0x262436,
                primary_ink: 0x090714,
                flat_surface: false,
                pill_tabs: true,
                pill_controls: true,
            },
            DesignId::Ascii
            | DesignId::Amber
            | DesignId::Contrast
            | DesignId::Midnight
            | DesignId::Rim
            | DesignId::Darkroom => {
                let data_primary = match design {
                    DesignId::Rim => 0x6db0f4,
                    DesignId::Ascii | DesignId::Amber | DesignId::Midnight => p.brass,
                    _ => p.ink,
                };
                Self {
                    design,
                    p,
                    accent_override: None,
                    compact_density,
                    line_soft: p.line,
                    data_primary,
                    data_secondary: p.ink_3,
                    pink_rose: p.brass_hi,
                    appbar_left: p.panel,
                    appbar_mid: p.panel,
                    appbar_right: p.panel,
                    appbar_border: p.line,
                    primary_ink: if p.is_light { 0xffffff } else { 0x040604 },
                    flat_surface: design == DesignId::Contrast,
                    pill_tabs: false,
                    pill_controls: false,
                }
            }
        };

        if let Some(ov) = accent_override {
            let hex = rgb_arr_to_hex(ov);
            let hi = if tokens.p.is_light {
                mix_hex(hex, 0x1d1614, 0.16)
            } else {
                mix_hex(hex, 0xfff6fa, 0.34)
            };
            tokens.accent_override = Some(ov);
            tokens.p.brass = hex;
            tokens.p.brass_hi = hi;
            tokens.pink_rose = if tokens.is_firefly() {
                mix_hex(hex, 0xf49ec0, 0.30)
            } else {
                hi
            };
            tokens.primary_ink = readable_ink_for_rgb(ov, tokens.is_firefly());
            if tokens.is_firefly() {
                tokens.appbar_border = mix_hex(tokens.appbar_border, hex, 0.30);
                tokens.line_soft = mix_hex(tokens.line_soft, hex, 0.16);
            }
        }

        tokens
    }

    pub fn is_firefly(self) -> bool {
        self.design == DesignId::Firefly
    }

    pub fn ctl_radius(self) -> f32 {
        if self.p.square_corners {
            0.0
        } else if self.pill_controls {
            999.0
        } else {
            self.p.radius_ctl.max(8.0)
        }
    }

    /// Density-aware layout spacing helper (`compact_density` scales smoothly by ~75%).
    pub fn sp(self, base: u32) -> u32 {
        if self.compact_density {
            ((base as f32) * 0.72).round().max(2.0) as u32
        } else {
            base
        }
    }

    /// Density-aware container padding helper.
    pub fn pad(self, base: u16) -> u16 {
        if self.compact_density {
            ((base as f32) * 0.75).round().max(6.0) as u16
        } else {
            base
        }
    }
}

pub fn rgb_arr_to_hex(c: [u8; 3]) -> u32 {
    ((c[0] as u32) << 16) | ((c[1] as u32) << 8) | (c[2] as u32)
}

pub fn hex_to_rgb_arr(hex: u32) -> [u8; 3] {
    [
        ((hex >> 16) & 0xFF) as u8,
        ((hex >> 8) & 0xFF) as u8,
        (hex & 0xFF) as u8,
    ]
}

pub fn mix_hex(a: u32, b: u32, t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    let ca = hex_to_rgb_arr(a);
    let cb = hex_to_rgb_arr(b);
    let r = (ca[0] as f32 + (cb[0] as f32 - ca[0] as f32) * t)
        .round()
        .clamp(0.0, 255.0) as u8;
    let g = (ca[1] as f32 + (cb[1] as f32 - ca[1] as f32) * t)
        .round()
        .clamp(0.0, 255.0) as u8;
    let bl = (ca[2] as f32 + (cb[2] as f32 - ca[2] as f32) * t)
        .round()
        .clamp(0.0, 255.0) as u8;
    rgb_arr_to_hex([r, g, bl])
}

/// Picks crisp high-contrast ink over any custom accent color.
pub fn readable_ink_for_rgb(c: [u8; 3], is_firefly: bool) -> u32 {
    let luma = (0.2126 * c[0] as f32 + 0.7152 * c[1] as f32 + 0.0722 * c[2] as f32) / 255.0;
    if luma > 0.48 {
        if is_firefly { 0x261722 } else { 0x101116 }
    } else {
        0xfff8fa
    }
}

pub fn rgb(hex: u32) -> Color {
    Color::from_rgb8(
        ((hex >> 16) & 0xFF) as u8,
        ((hex >> 8) & 0xFF) as u8,
        (hex & 0xFF) as u8,
    )
}

pub fn rgba(hex: u32, a: f32) -> Color {
    let c = rgb(hex);
    Color { a, ..c }
}

pub fn mix_color(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

pub fn lighten(hex: u32, amt: f32) -> Color {
    mix_color(rgb(hex), Color::WHITE, amt)
}

pub fn darken(hex: u32, amt: f32) -> Color {
    mix_color(rgb(hex), Color::BLACK, amt)
}

pub fn ui_font(design: DesignId) -> Font {
    match design {
        DesignId::Firefly | DesignId::Kiln | DesignId::Lumen | DesignId::Spore => {
            Font::with_name("Segoe UI")
        }
        DesignId::Ascii | DesignId::Amber | DesignId::Rim => Font::with_name("Consolas"),
        DesignId::Graphite
        | DesignId::Paper
        | DesignId::Contrast
        | DesignId::Neon
        | DesignId::Midnight
        | DesignId::Darkroom => Font::with_name("Bahnschrift"),
    }
}

pub fn display_font(design: DesignId) -> Font {
    match design {
        DesignId::Firefly | DesignId::Kiln | DesignId::Lumen => Font::with_name("Georgia"),
        DesignId::Spore => Font::with_name("Cambria"),
        DesignId::Ascii | DesignId::Amber | DesignId::Rim => Font::with_name("Consolas"),
        _ => Font::with_name("Bahnschrift"),
    }
}

pub fn ui_semibold(design: DesignId) -> Font {
    Font {
        weight: font::Weight::Semibold,
        ..ui_font(design)
    }
}

pub fn ui_bold(design: DesignId) -> Font {
    Font {
        weight: font::Weight::Bold,
        ..ui_font(design)
    }
}

pub fn display_semibold(design: DesignId) -> Font {
    Font {
        weight: font::Weight::Semibold,
        ..display_font(design)
    }
}

pub fn mono_font() -> Font {
    Font::with_name("Consolas")
}

pub fn iced_theme_with_accent(design: DesignId, accent_override: Option<[u8; 3]>) -> Theme {
    let t = DesignTokens::for_appearance(design, accent_override, false);
    let p = t.p;
    let ground_hex = if design == DesignId::Firefly {
        0x161222
    } else {
        p.ground
    };
    Theme::custom(
        design.label().to_string(),
        Palette {
            background: rgb(ground_hex),
            text: rgb(p.ink),
            primary: rgb(p.brass),
            success: rgb(p.ok),
            warning: rgb(p.warm),
            danger: rgb(p.hot),
        },
    )
}

// ---------------------------------------------------------------------------
// Internal physical material builders (150deg diagonal plates & milled buttons)
// ---------------------------------------------------------------------------

fn panel_radius(t: DesignTokens) -> f32 {
    if t.p.square_corners {
        0.0
    } else if t.is_firefly() {
        16.0
    } else {
        t.p.radius_panel.max(12.0)
    }
}

/// Core 150deg diagonal hardware surface (`panel_hi` top-left -> `panel` mid -> `mix(panel, ground, 0.42)`
/// bottom-right, optionally kissed with `7%` of `tint_hex`), crisp top-lit hairline, and grounded elevation.
fn build_diagonal_card_style(
    t: DesignTokens,
    tint_hex: Option<u32>,
    hovered: bool,
    radius: f32,
    hero_boost: bool,
) -> container::Style {
    let p = t.p;

    if t.design == DesignId::Contrast {
        let edge = match tint_hex {
            Some(c) if hovered => rgb(c),
            _ if hero_boost => rgb(p.brass),
            _ => rgb(p.line),
        };
        return container::Style {
            background: Some(Background::Color(rgb(p.panel))),
            border: Border {
                color: edge,
                width: 2.0,
                radius: Radius::from(radius),
            },
            text_color: Some(rgb(p.ink)),
            shadow: Shadow::default(),
            snap: true,
        };
    }

    let tint_amt_br = if hovered { 0.11 } else { 0.075 };
    let tint_amt_tl = if hero_boost {
        0.065
    } else if hovered {
        0.035
    } else {
        0.018
    };

    let background = if p.is_light {
        let mut tl = mix_color(rgb(p.raised), Color::WHITE, if hovered { 0.78 } else { 0.58 });
        let mut mid = if hovered {
            lighten(p.panel, 0.24)
        } else {
            rgb(p.panel)
        };
        let mut br = mix_color(rgb(p.panel), rgb(p.well), 0.38);
        if let Some(hex) = tint_hex {
            let tc = rgb(hex);
            tl = mix_color(tl, tc, tint_amt_tl * 0.65);
            mid = mix_color(mid, tc, 0.02);
            br = mix_color(br, tc, tint_amt_br * 0.80);
        }
        Background::Gradient(Gradient::Linear(
            Linear::new(Radians(DIAG_150_RAD))
                .add_stop(0.0, tl)
                .add_stop(0.46, mid)
                .add_stop(1.0, br),
        ))
    } else {
        let (base_hi, base_mid, base_ground) = if t.is_firefly() {
            (
                rgb(if hero_boost { 0x413657 } else { 0x382f4c }),
                rgb(if hero_boost { 0x302742 } else { 0x2a233a }),
                rgb(0x161122),
            )
        } else {
            (
                mix_color(rgb(p.raised), Color::WHITE, if hero_boost { 0.06 } else { 0.025 }),
                rgb(p.panel),
                rgb(p.ground),
            )
        };

        let mut tl = if hovered {
            mix_color(base_hi, Color::WHITE, 0.055)
        } else {
            base_hi
        };
        let mut mid = if hovered {
            mix_color(base_mid, Color::WHITE, 0.028)
        } else {
            base_mid
        };
        let mut br = mix_color(base_mid, base_ground, 0.42);

        if let Some(ov) = t.accent_override
            && tint_hex.is_none()
        {
            let ov_col = Color::from_rgb8(ov[0], ov[1], ov[2]);
            tl = mix_color(tl, ov_col, 0.03);
            br = mix_color(br, ov_col, 0.035);
        }

        if let Some(hex) = tint_hex {
            let tc = rgb(hex);
            tl = mix_color(tl, tc, tint_amt_tl);
            mid = mix_color(mid, tc, if hovered { 0.032 } else { 0.018 });
            br = mix_color(br, tc, tint_amt_br);
        } else if hero_boost {
            let ac = rgb(p.brass);
            tl = mix_color(tl, ac, 0.06);
            br = mix_color(br, rgb(t.pink_rose), 0.05);
        }

        Background::Gradient(Gradient::Linear(
            Linear::new(Radians(DIAG_150_RAD))
                .add_stop(0.0, tl)
                .add_stop(0.48, mid)
                .add_stop(1.0, br),
        ))
    };

    let border_col = if p.is_light {
        let base = Color::from_rgba(0.11, 0.11, 0.09, if hovered { 0.21 } else { 0.115 });
        if let Some(hex) = tint_hex {
            mix_color(base, rgba(hex, if hovered { 0.48 } else { 0.24 }), 0.38)
        } else if hero_boost {
            mix_color(base, rgba(p.brass, 0.45), 0.42)
        } else {
            base
        }
    } else {
        let base_alpha = if hovered {
            0.185
        } else if hero_boost {
            0.135
        } else {
            0.088
        };
        let base = Color::from_rgba(1.0, 1.0, 1.0, base_alpha);
        if let Some(hex) = tint_hex {
            mix_color(
                base,
                rgba(hex, if hovered { 0.56 } else { 0.30 }),
                if hovered { 0.42 } else { 0.22 },
            )
        } else if hero_boost {
            mix_color(base, rgba(p.brass_hi, 0.46), 0.36)
        } else {
            base
        }
    };

    let shadow = if p.is_light {
        Shadow {
            color: Color::from_rgba(0.20, 0.16, 0.10, if hovered { 0.15 } else { 0.105 }),
            offset: Vector::new(0.0, if hovered { 7.0 } else { 5.0 }),
            blur_radius: if hovered { 20.0 } else { 15.0 },
        }
    } else {
        Shadow {
            color: Color::from_rgba(
                0.02,
                0.01,
                0.05,
                if hero_boost {
                    0.54
                } else if hovered {
                    0.50
                } else {
                    0.42
                },
            ),
            offset: Vector::new(
                0.0,
                if hero_boost {
                    8.0
                } else if hovered {
                    7.0
                } else {
                    6.0
                },
            ),
            blur_radius: if hero_boost {
                26.0
            } else if hovered {
                22.0
            } else {
                18.0
            },
        }
    };

    container::Style {
        background: Some(background),
        border: Border {
            color: border_col,
            width: if hero_boost { 1.15 } else { 1.0 },
            radius: Radius::from(radius),
        },
        text_color: Some(rgb(p.ink)),
        shadow,
        snap: true,
    }
}

/// Shared tactile vertical glass-metal secondary / pill / ghost button builder with physical press.
fn build_tactile_secondary_btn(
    t: DesignTokens,
    active: bool,
    status: button::Status,
    radius: f32,
    subtle_ghost_rest: bool,
) -> button::Style {
    let p = t.p;
    let is_hovered = matches!(status, button::Status::Hovered);
    let is_pressed = matches!(status, button::Status::Pressed);
    let is_disabled = matches!(status, button::Status::Disabled);

    if t.design == DesignId::Contrast {
        return button::Style {
            background: Some(Background::Color(if active {
                rgb(p.raised)
            } else if is_hovered || is_pressed {
                rgb(0x222222)
            } else {
                rgb(p.panel)
            })),
            text_color: rgb(if active { p.brass_hi } else { p.ink }),
            border: Border {
                color: rgb(if active { p.brass } else { p.line }),
                width: 2.0,
                radius: Radius::from(radius),
            },
            shadow: Shadow::default(),
            snap: true,
        };
    }

    if active {
        let (top, bot, border_c, shadow, txt) = if p.is_light {
            let top = if is_pressed {
                mix_color(rgb(p.well), rgb(p.brass), 0.16)
            } else if is_hovered {
                mix_color(Color::WHITE, rgb(p.brass), 0.12)
            } else {
                mix_color(rgb(p.raised), rgb(p.brass), 0.10)
            };
            let bot = mix_color(rgb(p.panel), rgb(p.brass), if is_pressed { 0.22 } else { 0.18 });
            let sh = if is_pressed {
                Shadow::default()
            } else {
                Shadow {
                    color: rgba(p.brass, if is_hovered { 0.22 } else { 0.16 }),
                    offset: Vector::new(0.0, 2.0),
                    blur_radius: 8.0,
                }
            };
            (top, bot, rgba(p.brass, 0.68), sh, rgb(p.brass))
        } else {
            let hi_base = if t.is_firefly() {
                rgb(0x4b3b5d)
            } else {
                rgb(p.raised)
            };
            let lo_base = if t.is_firefly() {
                rgb(0x322742)
            } else {
                mix_color(rgb(p.panel), rgb(p.ground), 0.25)
            };
            if is_pressed {
                let top = mix_color(lo_base, rgb(p.brass), 0.16);
                let bot = mix_color(hi_base, rgb(p.brass_hi), 0.20);
                (
                    top,
                    bot,
                    rgba(p.brass_hi, 0.58),
                    Shadow {
                        color: rgba(p.brass, 0.14),
                        offset: Vector::new(0.0, 1.0),
                        blur_radius: 4.0,
                    },
                    rgb(p.brass_hi),
                )
            } else {
                let top = mix_color(
                    mix_color(hi_base, rgb(p.brass_hi), if is_hovered { 0.30 } else { 0.22 }),
                    Color::WHITE,
                    if is_hovered { 0.14 } else { 0.09 },
                );
                let bot = mix_color(lo_base, rgb(p.brass), if is_hovered { 0.20 } else { 0.14 });
                let edge = mix_color(rgb(p.brass_hi), Color::WHITE, 0.24);
                (
                    top,
                    bot,
                    Color {
                        a: if is_hovered { 0.80 } else { 0.64 },
                        ..edge
                    },
                    Shadow {
                        color: rgba(p.brass, if is_hovered { 0.30 } else { 0.22 }),
                        offset: Vector::new(0.0, if is_hovered { 3.0 } else { 2.0 }),
                        blur_radius: if is_hovered { 12.0 } else { 9.0 },
                    },
                    rgb(p.brass_hi),
                )
            }
        };

        return button::Style {
            background: Some(Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, top)
                    .add_stop(1.0, bot),
            ))),
            text_color: txt,
            border: Border {
                color: border_c,
                width: 1.15,
                radius: Radius::from(radius),
            },
            shadow,
            snap: true,
        };
    }

    // Inactive states: Pressed -> Hovered -> Resting (glass-metal bevel or subtle ghost pill)
    if is_pressed {
        let (top, bot, border_c) = if p.is_light {
            (
                darken(p.well, 0.06),
                rgb(p.panel),
                Color::from_rgba(0.11, 0.11, 0.09, 0.20),
            )
        } else {
            let well_c = if t.is_firefly() { 0x1a1526 } else { p.well };
            (
                darken(well_c, 0.18),
                mix_color(rgb(p.panel), rgb(p.ground), 0.22),
                Color::from_rgba(1.0, 1.0, 1.0, 0.10),
            )
        };
        return button::Style {
            background: Some(Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, top)
                    .add_stop(1.0, bot),
            ))),
            text_color: rgb(p.ink),
            border: Border {
                color: border_c,
                width: 1.0,
                radius: Radius::from(radius),
            },
            shadow: Shadow::default(),
            snap: true,
        };
    }

    if is_hovered {
        let (top, bot, border_c, shadow) = if p.is_light {
            (
                Color::WHITE,
                lighten(p.panel, 0.18),
                mix_color(
                    Color::from_rgba(0.11, 0.11, 0.09, 0.24),
                    rgba(p.brass, 0.42),
                    0.28,
                ),
                Shadow {
                    color: Color::from_rgba(0.20, 0.16, 0.10, 0.13),
                    offset: Vector::new(0.0, 3.0),
                    blur_radius: 8.0,
                },
            )
        } else {
            let panel_hi = if t.is_firefly() {
                rgb(0x463a59)
            } else {
                rgb(p.raised)
            };
            let top = mix_color(panel_hi, Color::WHITE, 0.14);
            let bot = mix_color(rgb(p.panel), rgb(p.ground), 0.20);
            let border_c = mix_color(
                Color::from_rgba(1.0, 1.0, 1.0, 0.26),
                rgba(p.brass_hi, 0.52),
                0.28,
            );
            (
                top,
                bot,
                border_c,
                Shadow {
                    color: Color::from_rgba(0.02, 0.01, 0.05, 0.42),
                    offset: Vector::new(0.0, 3.0),
                    blur_radius: 9.0,
                },
            )
        };
        return button::Style {
            background: Some(Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, top)
                    .add_stop(1.0, bot),
            ))),
            text_color: rgb(p.ink),
            border: Border {
                color: border_c,
                width: 1.0,
                radius: Radius::from(radius),
            },
            shadow,
            snap: true,
        };
    }

    if subtle_ghost_rest {
        let (top, bot, border_c) = if p.is_light {
            (
                rgba(p.raised, 0.62),
                rgba(p.well, 0.40),
                Color::from_rgba(0.11, 0.11, 0.09, 0.095),
            )
        } else {
            let hi = if t.is_firefly() { 0x3c324e } else { p.raised };
            (
                rgba(hi, 0.42),
                rgba(p.well, 0.30),
                Color::from_rgba(1.0, 1.0, 1.0, 0.085),
            )
        };
        return button::Style {
            background: Some(Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, top)
                    .add_stop(1.0, bot),
            ))),
            text_color: if is_disabled {
                rgba(p.ink_3, 0.48)
            } else {
                rgb(p.ink_2)
            },
            border: Border {
                color: border_c,
                width: 1.0,
                radius: Radius::from(radius),
            },
            shadow: Shadow::default(),
            snap: true,
        };
    }

    // Standard tactile secondary / pill resting bevel
    let (top, bot, border_c, shadow) = if p.is_light {
        (
            mix_color(rgb(p.raised), Color::WHITE, 0.65),
            mix_color(rgb(p.panel), rgb(p.well), 0.32),
            Color::from_rgba(0.11, 0.11, 0.09, 0.15),
            Shadow {
                color: Color::from_rgba(0.20, 0.16, 0.10, 0.10),
                offset: Vector::new(0.0, 2.0),
                blur_radius: 6.0,
            },
        )
    } else {
        let panel_hi = if t.is_firefly() {
            rgb(0x3e3350)
        } else {
            rgb(p.raised)
        };
        let top = mix_color(panel_hi, Color::WHITE, 0.08);
        let bot = mix_color(rgb(p.panel), rgb(p.ground), 0.35);
        (
            top,
            bot,
            Color::from_rgba(1.0, 1.0, 1.0, 0.135),
            Shadow {
                color: Color::from_rgba(0.02, 0.01, 0.05, 0.36),
                offset: Vector::new(0.0, 2.0),
                blur_radius: 6.0,
            },
        )
    };

    button::Style {
        background: Some(Background::Gradient(Gradient::Linear(
            Linear::new(Radians(PI))
                .add_stop(0.0, top)
                .add_stop(1.0, bot),
        ))),
        text_color: if is_disabled {
            rgba(p.ink_3, 0.45)
        } else {
            rgb(if t.is_firefly() { 0xf0e5ee } else { p.ink_2 })
        },
        border: Border {
            color: border_c,
            width: 1.0,
            radius: Radius::from(radius),
        },
        shadow: if is_disabled { Shadow::default() } else { shadow },
        snap: true,
    }
}

// ---------------------------------------------------------------------------
// Window, Appbar & Navigation Dock Chrome
// ---------------------------------------------------------------------------

/// Top-level window container style. Uses `background: None` so `WindowBackdropCanvas`
/// underneath shines through unobstructed while `Theme::palette().background` provides
/// the deep window clear color when no backdrop canvas is stacked behind.
pub fn window_ground_style(t: DesignTokens) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: None,
        text_color: Some(rgb(t.p.ink)),
        ..Default::default()
    }
}

/// Full-bleed frosted glass-plum top appbar with subtle sheen and crisp hairline separation.
pub fn appbar_style(t: DesignTokens) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let p = t.p;
        let bg = if t.is_firefly() {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(FRAC_PI_2))
                    .add_stop(0.0, rgba(0x231b31, 0.84))
                    .add_stop(0.5, rgba(0x1e172b, 0.80))
                    .add_stop(1.0, rgba(0x1b1427, 0.84)),
            ))
        } else if t.design == DesignId::Contrast {
            Background::Color(rgb(p.panel))
        } else if p.is_light {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, rgba(0xf9f7f2, 0.90))
                    .add_stop(1.0, rgba(0xf1ede3, 0.86)),
            ))
        } else {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, Color { a: 0.86, ..lighten(p.panel, 0.045) })
                    .add_stop(1.0, rgba(p.panel, 0.82)),
            ))
        };
        let border_col = if t.design == DesignId::Contrast {
            rgb(p.line)
        } else if p.is_light {
            Color::from_rgba(0.11, 0.11, 0.09, 0.12)
        } else {
            Color::from_rgba(1.0, 1.0, 1.0, 0.08)
        };
        container::Style {
            background: Some(bg),
            text_color: Some(rgb(p.ink)),
            border: Border {
                color: border_col,
                width: 1.0,
                radius: Radius::from(0.0),
            },
            shadow: if t.design == DesignId::Contrast {
                Shadow::default()
            } else {
                Shadow {
                    color: Color::from_rgba(
                        0.02,
                        0.01,
                        0.05,
                        if p.is_light { 0.07 } else { 0.28 },
                    ),
                    offset: Vector::new(0.0, 4.0),
                    blur_radius: 14.0,
                }
            },
            snap: true,
        }
    }
}

/// Recessed frosted capsule dock in the center of the top appbar (`Overview..Settings`).
pub fn nav_dock_style(tokens: DesignTokens) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let p = tokens.p;
        let radius = if p.square_corners { 0.0 } else { 999.0 };

        let bg = if tokens.design == DesignId::Contrast {
            Background::Color(rgb(p.well))
        } else if p.is_light {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, rgba(0xd7d1c5, 0.90))
                    .add_stop(1.0, rgba(0xe5e0d5, 0.92)),
            ))
        } else {
            let top_hex = if tokens.is_firefly() { 0x14101f } else { p.well };
            let bot_hex = if tokens.is_firefly() { 0x1d172c } else { p.well };
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, Color { a: 0.88, ..darken(top_hex, 0.16) })
                    .add_stop(1.0, rgba(bot_hex, 0.84)),
            ))
        };

        let border_col = if tokens.design == DesignId::Contrast {
            rgb(p.line)
        } else if p.is_light {
            Color::from_rgba(0.11, 0.11, 0.09, 0.14)
        } else {
            Color::from_rgba(1.0, 1.0, 1.0, 0.105)
        };

        container::Style {
            background: Some(bg),
            text_color: Some(rgb(p.ink)),
            border: Border {
                color: border_col,
                width: if tokens.design == DesignId::Contrast { 2.0 } else { 1.0 },
                radius: Radius::from(radius),
            },
            shadow: if tokens.design == DesignId::Contrast {
                Shadow::default()
            } else {
                Shadow {
                    color: Color::from_rgba(
                        0.01,
                        0.0,
                        0.04,
                        if p.is_light { 0.08 } else { 0.34 },
                    ),
                    offset: Vector::new(0.0, 2.0),
                    blur_radius: 10.0,
                }
            },
            snap: true,
        }
    }
}

/// Tactile navigation tab button inside `nav_dock_style`: elevated gradient capsule + subtle
/// accent glow when active; soft frosted glass highlight when hovered.
pub fn nav_tab_style(
    tokens: DesignTokens,
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let p = tokens.p;
        let is_hovered = matches!(status, button::Status::Hovered);
        let is_pressed = matches!(status, button::Status::Pressed);
        let radius = if p.square_corners { 0.0 } else { 999.0 };

        if tokens.design == DesignId::Contrast {
            return button::Style {
                background: Some(Background::Color(if active {
                    rgb(p.raised)
                } else if is_hovered || is_pressed {
                    rgb(0x1f1f1f)
                } else {
                    Color::TRANSPARENT
                })),
                text_color: rgb(if active { p.brass_hi } else { p.ink }),
                border: Border {
                    color: if active { rgb(p.brass) } else { Color::TRANSPARENT },
                    width: if active { 2.0 } else { 0.0 },
                    radius: Radius::from(radius),
                },
                shadow: Shadow::default(),
                snap: true,
            };
        }

        if active {
            let (bg, border_c, shadow, txt) = if p.is_light {
                let top = Color::WHITE;
                let bot = mix_color(rgb(p.raised), rgb(p.brass), 0.07);
                (
                    Background::Gradient(Gradient::Linear(
                        Linear::new(Radians(PI))
                            .add_stop(0.0, top)
                            .add_stop(1.0, bot),
                    )),
                    rgba(p.brass, 0.52),
                    Shadow {
                        color: Color::from_rgba(0.20, 0.15, 0.08, 0.14),
                        offset: Vector::new(0.0, 2.0),
                        blur_radius: 8.0,
                    },
                    rgb(p.ink),
                )
            } else {
                let hi_base = if tokens.is_firefly() {
                    rgb(0x4d3c60)
                } else {
                    rgb(p.raised)
                };
                let lo_base = if tokens.is_firefly() {
                    rgb(0x322743)
                } else {
                    rgb(p.panel)
                };
                let top = if is_pressed {
                    mix_color(lo_base, rgb(p.brass), 0.16)
                } else {
                    mix_color(
                        mix_color(hi_base, rgb(p.brass_hi), if is_hovered { 0.28 } else { 0.22 }),
                        Color::WHITE,
                        0.12,
                    )
                };
                let bot = if is_pressed {
                    mix_color(hi_base, rgb(p.brass), 0.18)
                } else {
                    mix_color(lo_base, rgb(p.brass), 0.11)
                };
                let edge = mix_color(rgb(p.brass_hi), Color::WHITE, 0.26);
                (
                    Background::Gradient(Gradient::Linear(
                        Linear::new(Radians(PI))
                            .add_stop(0.0, top)
                            .add_stop(1.0, bot),
                    )),
                    Color { a: 0.62, ..edge },
                    Shadow {
                        color: rgba(p.brass, if is_pressed { 0.14 } else if is_hovered { 0.30 } else { 0.23 }),
                        offset: Vector::new(0.0, if is_pressed { 1.0 } else { 2.0 }),
                        blur_radius: if is_pressed { 4.0 } else { 10.0 },
                    },
                    rgb(p.ink),
                )
            };

            return button::Style {
                background: Some(bg),
                text_color: txt,
                border: Border {
                    color: border_c,
                    width: 1.05,
                    radius: Radius::from(radius),
                },
                shadow,
                snap: true,
            };
        }

        if is_pressed {
            return button::Style {
                background: Some(Background::Color(if p.is_light {
                    rgba(p.well, 0.85)
                } else {
                    rgba(p.well, 0.72)
                })),
                text_color: rgb(p.ink),
                border: Border {
                    color: if p.is_light {
                        Color::from_rgba(0.11, 0.11, 0.09, 0.12)
                    } else {
                        Color::from_rgba(1.0, 1.0, 1.0, 0.09)
                    },
                    width: 1.0,
                    radius: Radius::from(radius),
                },
                shadow: Shadow::default(),
                snap: true,
            };
        }

        if is_hovered {
            let bg = if p.is_light {
                Background::Color(rgba(0xffffff, 0.72))
            } else {
                let hi = if tokens.is_firefly() { 0x3f3353 } else { p.raised };
                Background::Gradient(Gradient::Linear(
                    Linear::new(Radians(PI))
                        .add_stop(0.0, rgba(hi, 0.72))
                        .add_stop(1.0, rgba(p.panel, 0.48)),
                ))
            };
            return button::Style {
                background: Some(bg),
                text_color: rgb(p.ink),
                border: Border {
                    color: if p.is_light {
                        Color::from_rgba(0.11, 0.11, 0.09, 0.12)
                    } else {
                        Color::from_rgba(1.0, 1.0, 1.0, 0.13)
                    },
                    width: 1.0,
                    radius: Radius::from(radius),
                },
                shadow: Shadow::default(),
                snap: true,
            };
        }

        button::Style {
            background: Some(Background::Color(Color::TRANSPARENT)),
            text_color: rgb(p.ink_2),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: Radius::from(radius),
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Glowing brand badge wrapping `BrandMarkCanvas` in the header.
pub fn brand_badge_style(t: DesignTokens) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let p = t.p;
        let bg = if t.is_firefly() {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, mix_color(rgb(0x473756), rgb(p.brass_hi), 0.16))
                    .add_stop(1.0, rgb(0x2c2238)),
            ))
        } else if p.is_light {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, Color::WHITE)
                    .add_stop(1.0, rgb(p.panel)),
            ))
        } else {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, mix_color(rgb(p.raised), rgb(p.brass_hi), 0.14))
                    .add_stop(1.0, mix_color(rgb(p.panel), rgb(p.ground), 0.30)),
            ))
        };
        container::Style {
            background: Some(bg),
            border: Border {
                color: rgba(if p.is_light { p.brass } else { p.brass_hi }, 0.46),
                width: 1.0,
                radius: Radius::from(if p.square_corners { 0.0 } else { 11.0 }),
            },
            shadow: if t.design == DesignId::Contrast {
                Shadow::default()
            } else {
                Shadow {
                    color: rgba(p.brass, if p.is_light { 0.16 } else { 0.28 }),
                    offset: Vector::new(0.0, 2.0),
                    blur_radius: 12.0,
                }
            },
            text_color: Some(rgb(p.ink)),
            snap: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Dimensional Diagonal Cards, Hero Plates & Recessed Wells
// ---------------------------------------------------------------------------

/// Diagonal 150deg gradient instrument card subtly kissed at the bottom-right stop with `7%`
/// of `tint_hex`, crisp top-lit hairline (`0.085` -> `0.18` on hover), and grounded elevation.
pub fn rich_instrument_card_style(
    tokens: DesignTokens,
    tint_hex: Option<u32>,
    hovered: bool,
) -> impl Fn(&Theme) -> container::Style {
    move |_| build_diagonal_card_style(tokens, tint_hex, hovered, panel_radius(tokens), false)
}

/// Elevated panel / instrument card using the 150deg diagonal hardware plate material.
pub fn panel_style(t: DesignTokens) -> impl Fn(&Theme) -> container::Style {
    move |_| build_diagonal_card_style(t, None, false, panel_radius(t), false)
}

/// Featured cinematic hero panel (Living Rig showcase, Overview masthead, Theater stage,
/// Updates hero card) with luminous specular accent rim and deep layered elevation.
pub fn hero_panel_style(t: DesignTokens) -> impl Fn(&Theme) -> container::Style {
    move |_| build_diagonal_card_style(t, Some(t.p.brass), false, panel_radius(t), true)
}

/// Crisp layered bento/telemetry card style with 150deg diagonal sheen and specular rim.
pub fn card_style(t: DesignTokens) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let r = if t.p.square_corners {
            0.0
        } else if t.is_firefly() {
            14.0
        } else {
            t.p.radius_panel.max(12.0)
        };
        build_diagonal_card_style(t, None, false, r, false)
    }
}

/// Overview Bento card surface (`16px` radius, rich 150deg diagonal surface, crisp hairline,
/// grounded `(0, 6, blur 18)` shadow; glowing accent frame when `editing`).
pub fn bento_tile_style(
    t: DesignTokens,
    editing: bool,
) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        if editing {
            return editing_tile_style(t)(theme);
        }
        let radius = if t.p.square_corners { 0.0 } else { 16.0 };
        build_diagonal_card_style(t, None, false, radius, false)
    }
}

/// Glowing accent-framed tile surface shown while Overview Dashboard Edit Mode (`✦ Customize`) is active.
pub fn editing_tile_style(t: DesignTokens) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let p = t.p;
        let radius = if p.square_corners { 0.0 } else { 16.0 };
        let mut style = build_diagonal_card_style(t, Some(p.brass), true, radius, false);
        style.border = Border {
            color: rgba(if p.is_light { p.brass } else { p.brass_hi }, 0.82),
            width: if t.design == DesignId::Contrast { 2.2 } else { 1.6 },
            radius: Radius::from(radius),
        };
        if t.design != DesignId::Contrast {
            style.shadow = Shadow {
                color: rgba(p.brass, if p.is_light { 0.22 } else { 0.30 }),
                offset: Vector::new(0.0, 5.0),
                blur_radius: 20.0,
            };
        }
        style
    }
}

/// Modern left-sidebar navigation pill for the redesigned 2-pane categorized Settings view.
pub fn settings_nav_item_style(
    t: DesignTokens,
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let p = t.p;
        let is_hovered = matches!(status, button::Status::Hovered);
        let is_pressed = matches!(status, button::Status::Pressed);
        let radius = if p.square_corners {
            0.0
        } else if t.is_firefly() || t.pill_controls {
            11.0
        } else {
            p.radius_ctl.max(9.0)
        };

        let (bg, txt, border_col, border_w, shadow) = if active {
            if t.design == DesignId::Contrast {
                (
                    Background::Color(mix_color(rgb(p.raised), rgb(p.brass), 0.16)),
                    rgb(p.brass_hi),
                    rgb(p.brass),
                    2.0,
                    Shadow::default(),
                )
            } else if p.is_light {
                (
                    Background::Gradient(Gradient::Linear(
                        Linear::new(Radians(PI))
                            .add_stop(0.0, mix_color(Color::WHITE, rgb(p.brass), 0.10))
                            .add_stop(1.0, rgba(p.brass, 0.14)),
                    )),
                    rgb(p.brass),
                    rgba(p.brass, 0.58),
                    1.15,
                    Shadow {
                        color: rgba(p.brass, 0.14),
                        offset: Vector::new(0.0, 2.0),
                        blur_radius: 8.0,
                    },
                )
            } else {
                let hi = if t.is_firefly() { 0x4c3a5c } else { p.raised };
                let lo = if t.is_firefly() { 0x352845 } else { p.panel };
                (
                    Background::Gradient(Gradient::Linear(
                        Linear::new(Radians(PI))
                            .add_stop(0.0, mix_color(rgb(hi), rgb(p.brass_hi), 0.22))
                            .add_stop(1.0, mix_color(rgb(lo), rgb(t.pink_rose), 0.12)),
                    )),
                    rgb(p.brass_hi),
                    rgba(p.brass_hi, 0.60),
                    1.15,
                    Shadow {
                        color: rgba(p.brass, 0.22),
                        offset: Vector::new(0.0, 2.0),
                        blur_radius: 12.0,
                    },
                )
            }
        } else if is_hovered || is_pressed {
            let bg = if p.is_light {
                Background::Gradient(Gradient::Linear(
                    Linear::new(Radians(PI))
                        .add_stop(0.0, Color::WHITE)
                        .add_stop(1.0, rgb(p.panel)),
                ))
            } else {
                let hi = if t.is_firefly() { 0x3e3250 } else { p.raised };
                Background::Gradient(Gradient::Linear(
                    Linear::new(Radians(PI))
                        .add_stop(0.0, rgba(hi, if is_pressed { 0.55 } else { 0.78 }))
                        .add_stop(1.0, rgba(p.panel, 0.52)),
                ))
            };
            (
                bg,
                rgb(p.ink),
                if p.is_light {
                    Color::from_rgba(0.11, 0.11, 0.09, 0.16)
                } else {
                    Color::from_rgba(1.0, 1.0, 1.0, 0.15)
                },
                1.0,
                Shadow::default(),
            )
        } else {
            (
                Background::Color(Color::TRANSPARENT),
                rgb(p.ink_2),
                Color::TRANSPARENT,
                0.0,
                Shadow::default(),
            )
        };

        button::Style {
            background: Some(bg),
            text_color: txt,
            border: Border {
                color: border_col,
                width: border_w,
                radius: Radius::from(radius),
            },
            shadow,
            snap: true,
        }
    }
}

/// Recessed instrument well (dark machined glass / inset tray).
pub fn well_style(t: DesignTokens) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let p = t.p;
        let bg = if t.design == DesignId::Contrast {
            Background::Color(rgb(p.well))
        } else if p.is_light {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, darken(p.well, 0.05))
                    .add_stop(1.0, lighten(p.well, 0.12)),
            ))
        } else {
            let base = if t.is_firefly() { 0x1b1627 } else { p.well };
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, darken(base, 0.20))
                    .add_stop(1.0, rgb(base)),
            ))
        };
        let r = if p.square_corners {
            0.0
        } else if t.is_firefly() {
            12.0
        } else {
            p.radius_panel.max(10.0)
        };
        container::Style {
            background: Some(bg),
            border: Border {
                color: if t.design == DesignId::Contrast {
                    rgb(p.line)
                } else if p.is_light {
                    Color::from_rgba(0.11, 0.11, 0.09, 0.13)
                } else {
                    Color::from_rgba(1.0, 1.0, 1.0, 0.08)
                },
                width: if t.design == DesignId::Contrast { 2.0 } else { 1.0 },
                radius: Radius::from(r),
            },
            text_color: Some(rgb(p.ink)),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Recessed dark stage well inside the `Lighting canvas` (`2×2`) hero tile (`12px` radius).
pub fn bento_stage_well_style(t: DesignTokens) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let p = t.p;
        let radius = if p.square_corners { 0.0 } else { 12.0 };
        let bg = if t.is_firefly() {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(DIAG_150_RAD))
                    .add_stop(0.0, rgb(0x161221))
                    .add_stop(1.0, rgb(0x100d19)),
            ))
        } else if p.is_light {
            Background::Color(darken(p.well, 0.04))
        } else {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(DIAG_150_RAD))
                    .add_stop(0.0, darken(p.well, 0.18))
                    .add_stop(1.0, darken(p.well, 0.34)),
            ))
        };
        let border_col = if t.design == DesignId::Contrast {
            rgb(p.line)
        } else if p.is_light {
            Color::from_rgba(0.11, 0.11, 0.09, 0.11)
        } else {
            Color::from_rgba(1.0, 1.0, 1.0, 0.075)
        };
        container::Style {
            background: Some(bg),
            border: Border {
                color: border_col,
                width: if t.design == DesignId::Contrast { 1.5 } else { 1.0 },
                radius: Radius::from(radius),
            },
            text_color: Some(rgb(p.ink)),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Ambient telemetry capsule for top-bar CPU/GPU readout.
pub fn ambient_pill_style(t: DesignTokens) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let p = t.p;
        let radius = if p.square_corners { 0.0 } else { 999.0 };
        let bg = if t.design == DesignId::Contrast {
            Background::Color(rgb(p.well))
        } else if p.is_light {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, Color::WHITE)
                    .add_stop(1.0, rgb(p.well)),
            ))
        } else {
            let top = if t.is_firefly() { 0x2a223b } else { p.raised };
            let bot = if t.is_firefly() { 0x1b1628 } else { p.well };
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, rgba(top, 0.88))
                    .add_stop(1.0, rgba(bot, 0.90)),
            ))
        };
        let border_col = if t.design == DesignId::Contrast {
            rgb(p.line)
        } else if p.is_light {
            Color::from_rgba(0.11, 0.11, 0.09, 0.14)
        } else {
            Color::from_rgba(1.0, 1.0, 1.0, 0.12)
        };
        container::Style {
            background: Some(bg),
            border: Border {
                color: border_col,
                width: if t.design == DesignId::Contrast { 2.0 } else { 1.0 },
                radius: Radius::from(radius),
            },
            text_color: Some(rgb(p.ink)),
            shadow: if t.design == DesignId::Contrast {
                Shadow::default()
            } else {
                Shadow {
                    color: Color::from_rgba(
                        0.02,
                        0.01,
                        0.05,
                        if p.is_light { 0.08 } else { 0.30 },
                    ),
                    offset: Vector::new(0.0, 2.0),
                    blur_radius: 6.0,
                }
            },
            snap: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Segmented Controls (`segmented_track_style` + `segmented_pill_style` + aliases)
// ---------------------------------------------------------------------------

/// Recessed well capsule for segmented controls (`Silent | Balanced | Performance`, `Curve | Steady | Auto`, `UI Scale`).
pub fn segmented_track_style(tokens: DesignTokens) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let p = tokens.p;
        let radius = if p.square_corners { 0.0 } else { 999.0 };
        let bg = if tokens.design == DesignId::Contrast {
            Background::Color(rgb(p.well))
        } else if p.is_light {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, darken(p.well, 0.06))
                    .add_stop(1.0, rgb(p.well)),
            ))
        } else {
            let base = if tokens.is_firefly() { 0x191426 } else { p.well };
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, darken(base, 0.18))
                    .add_stop(1.0, lighten(base, 0.02)),
            ))
        };
        let border_col = if tokens.design == DesignId::Contrast {
            rgb(p.line)
        } else if p.is_light {
            Color::from_rgba(0.11, 0.11, 0.09, 0.15)
        } else {
            Color::from_rgba(1.0, 1.0, 1.0, 0.10)
        };
        container::Style {
            background: Some(bg),
            border: Border {
                color: border_col,
                width: if tokens.design == DesignId::Contrast { 2.0 } else { 1.0 },
                radius: Radius::from(radius),
            },
            text_color: Some(rgb(p.ink)),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Tactile raised segment button inside `segmented_track_style`.
pub fn segmented_pill_style(
    tokens: DesignTokens,
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let p = tokens.p;
        let is_hovered = matches!(status, button::Status::Hovered);
        let is_pressed = matches!(status, button::Status::Pressed);
        let radius = if p.square_corners { 0.0 } else { 999.0 };

        if tokens.design == DesignId::Contrast {
            return button::Style {
                background: Some(Background::Color(if active {
                    rgb(p.raised)
                } else if is_hovered || is_pressed {
                    rgb(0x202020)
                } else {
                    Color::TRANSPARENT
                })),
                text_color: rgb(if active { p.brass_hi } else { p.ink_2 }),
                border: Border {
                    color: if active { rgb(p.brass) } else { Color::TRANSPARENT },
                    width: if active { 1.5 } else { 0.0 },
                    radius: Radius::from(radius),
                },
                shadow: Shadow::default(),
                snap: true,
            };
        }

        if active {
            let (bg, border_c, shadow, txt) = if p.is_light {
                let top = Color::WHITE;
                let bot = mix_color(rgb(p.raised), rgb(p.brass), 0.06);
                (
                    Background::Gradient(Gradient::Linear(
                        Linear::new(Radians(PI))
                            .add_stop(0.0, top)
                            .add_stop(1.0, bot),
                    )),
                    rgba(p.brass, 0.54),
                    Shadow {
                        color: Color::from_rgba(0.18, 0.14, 0.08, 0.15),
                        offset: Vector::new(0.0, 2.0),
                        blur_radius: 7.0,
                    },
                    rgb(p.brass),
                )
            } else {
                let hi = if tokens.is_firefly() {
                    rgb(0x4d3c5e)
                } else {
                    rgb(p.raised)
                };
                let lo = if tokens.is_firefly() {
                    rgb(0x342845)
                } else {
                    rgb(p.panel)
                };
                let top = if is_pressed {
                    mix_color(lo, rgb(p.brass), 0.15)
                } else {
                    mix_color(
                        mix_color(hi, rgb(p.brass_hi), if is_hovered { 0.28 } else { 0.20 }),
                        Color::WHITE,
                        0.12,
                    )
                };
                let bot = mix_color(lo, rgb(p.brass), if is_pressed { 0.20 } else { 0.12 });
                let edge = mix_color(rgb(p.brass_hi), Color::WHITE, 0.24);
                (
                    Background::Gradient(Gradient::Linear(
                        Linear::new(Radians(PI))
                            .add_stop(0.0, top)
                            .add_stop(1.0, bot),
                    )),
                    Color { a: 0.62, ..edge },
                    Shadow {
                        color: rgba(p.brass, if is_pressed { 0.12 } else { 0.24 }),
                        offset: Vector::new(0.0, if is_pressed { 1.0 } else { 2.0 }),
                        blur_radius: if is_pressed { 4.0 } else { 8.0 },
                    },
                    rgb(p.brass_hi),
                )
            };

            return button::Style {
                background: Some(bg),
                text_color: txt,
                border: Border {
                    color: border_c,
                    width: 1.05,
                    radius: Radius::from(radius),
                },
                shadow,
                snap: true,
            };
        }

        if is_hovered || is_pressed {
            let bg = if p.is_light {
                Background::Color(rgba(0xffffff, if is_pressed { 0.45 } else { 0.72 }))
            } else {
                let hi = if tokens.is_firefly() { 0x3b304e } else { p.raised };
                Background::Color(rgba(hi, if is_pressed { 0.45 } else { 0.65 }))
            };
            return button::Style {
                background: Some(bg),
                text_color: rgb(p.ink),
                border: Border {
                    color: if p.is_light {
                        Color::from_rgba(0.11, 0.11, 0.09, 0.10)
                    } else {
                        Color::from_rgba(1.0, 1.0, 1.0, 0.10)
                    },
                    width: 1.0,
                    radius: Radius::from(radius),
                },
                shadow: Shadow::default(),
                snap: true,
            };
        }

        button::Style {
            background: Some(Background::Color(Color::TRANSPARENT)),
            text_color: rgb(p.ink_2),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: Radius::from(radius),
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Legacy alias for `segmented_track_style`.
pub fn segmented_rail_style(t: DesignTokens) -> impl Fn(&Theme) -> container::Style {
    segmented_track_style(t)
}

/// Legacy alias for `segmented_pill_style`.
pub fn segmented_item_btn(
    t: DesignTokens,
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    segmented_pill_style(t, active)
}

pub fn banner_style(t: DesignTokens, accent: u32) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let p = t.p;
        let bg = Background::Gradient(Gradient::Linear(
            Linear::new(Radians(FRAC_PI_2))
                .add_stop(0.0, rgba(accent, if p.is_light { 0.16 } else { 0.20 }))
                .add_stop(1.0, rgba(accent, if p.is_light { 0.07 } else { 0.09 })),
        ));
        container::Style {
            background: Some(bg),
            border: Border {
                color: rgba(accent, 0.68),
                width: 1.2,
                radius: Radius::from(if p.square_corners { 0.0 } else { 12.0 }),
            },
            text_color: Some(rgb(p.ink)),
            shadow: if t.design == DesignId::Contrast {
                Shadow::default()
            } else {
                Shadow {
                    color: rgba(accent, 0.14),
                    offset: Vector::new(0.0, 3.0),
                    blur_radius: 12.0,
                }
            },
            snap: true,
        }
    }
}

pub fn active_card_box(t: DesignTokens, active: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        if !active {
            return panel_style(t)(theme);
        }
        let p = t.p;
        let mut style = build_diagonal_card_style(t, Some(p.brass), true, panel_radius(t), false);
        style.border = Border {
            color: rgba(if p.is_light { p.brass } else { p.brass_hi }, 0.72),
            width: 1.4,
            radius: Radius::from(panel_radius(t)),
        };
        if t.design != DesignId::Contrast {
            style.shadow = Shadow {
                color: rgba(if t.is_firefly() { t.pink_rose } else { p.brass }, 0.22),
                offset: Vector::new(0.0, 4.0),
                blur_radius: 16.0,
            };
        }
        style
    }
}

/// Soft dusk scrim overlay for the Firefly hero banner (`#221d30` left -> transparent right).
pub fn hero_scrim_style(t: DesignTokens) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let left_hex = 0x221d30;
        container::Style {
            background: Some(Background::Gradient(Gradient::Linear(
                Linear::new(Radians(FRAC_PI_2))
                    .add_stop(0.0, rgba(left_hex, 0.96))
                    .add_stop(0.46, rgba(left_hex, 0.72))
                    .add_stop(0.76, rgba(left_hex, 0.14))
                    .add_stop(1.0, rgba(left_hex, 0.0)),
            ))),
            border: Border {
                radius: Radius::from(panel_radius(t)),
                ..Default::default()
            },
            text_color: Some(rgb(0xfff4e4)),
            ..Default::default()
        }
    }
}

/// Tactile badge pill (used for "IN USE", "Whole rig", automation sentence tokens, index chips).
pub fn chip_style(t: DesignTokens, accent: Option<u32>) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let p = t.p;
        let radius = if p.square_corners { 0.0 } else { 999.0 };
        let (bg, border_c, txt_c) = match accent {
            Some(c) => (
                Background::Gradient(Gradient::Linear(
                    Linear::new(Radians(PI))
                        .add_stop(0.0, rgba(c, if p.is_light { 0.18 } else { 0.24 }))
                        .add_stop(1.0, rgba(c, if p.is_light { 0.10 } else { 0.13 })),
                )),
                rgba(c, 0.56),
                rgb(if p.is_light { mix_hex(c, 0x1a1612, 0.25) } else { mix_hex(c, 0xffffff, 0.22) }),
            ),
            None => {
                let top = if p.is_light {
                    Color::WHITE
                } else if t.is_firefly() {
                    rgb(0x322944)
                } else {
                    lighten(p.well, 0.08)
                };
                let bot = if p.is_light {
                    rgb(p.well)
                } else if t.is_firefly() {
                    rgb(0x231c31)
                } else {
                    rgb(p.well)
                };
                (
                    Background::Gradient(Gradient::Linear(
                        Linear::new(Radians(PI))
                            .add_stop(0.0, top)
                            .add_stop(1.0, bot),
                    )),
                    if p.is_light {
                        Color::from_rgba(0.11, 0.11, 0.09, 0.14)
                    } else {
                        Color::from_rgba(1.0, 1.0, 1.0, 0.12)
                    },
                    rgb(p.ink_2),
                )
            }
        };
        container::Style {
            background: Some(bg),
            border: Border {
                color: border_c,
                width: 1.0,
                radius: Radius::from(radius),
            },
            text_color: Some(txt_c),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Tactile Milled-Hardware Buttons
// ---------------------------------------------------------------------------

/// Flagship primary action button: vertical anodized gradient (`mix(accent_hi, WHITE, 0.16)` -> `accent`),
/// crisp top-lit border (`mix(accent_hi, WHITE, 0.38)`), warm grounded shadow, hover lift, and physical press.
pub fn btn_primary(t: DesignTokens) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let p = t.p;
        let is_hovered = matches!(status, button::Status::Hovered);
        let is_pressed = matches!(status, button::Status::Pressed);
        let is_disabled = matches!(status, button::Status::Disabled);
        let radius = t.ctl_radius();

        if t.design == DesignId::Contrast {
            return button::Style {
                background: Some(Background::Color(rgb(if is_hovered {
                    p.brass_hi
                } else {
                    p.brass
                }))),
                text_color: rgb(t.primary_ink),
                border: Border {
                    color: rgb(p.ink),
                    width: 2.0,
                    radius: Radius::from(radius),
                },
                shadow: Shadow::default(),
                snap: true,
            };
        }

        let accent = rgb(p.brass);
        let accent_hi = rgb(p.brass_hi);

        let (top, mid, bot, border_col, shadow) = if is_pressed {
            (
                mix_color(accent, Color::BLACK, 0.18),
                mix_color(accent, Color::BLACK, 0.06),
                mix_color(accent_hi, accent, 0.55),
                Color {
                    a: 0.75,
                    ..mix_color(accent_hi, Color::WHITE, 0.20)
                },
                Shadow {
                    color: rgba(p.brass, 0.18),
                    offset: Vector::new(0.0, 1.0),
                    blur_radius: 4.0,
                },
            )
        } else if is_hovered {
            (
                mix_color(accent_hi, Color::WHITE, 0.28),
                mix_color(accent_hi, Color::WHITE, 0.08),
                mix_color(accent, accent_hi, 0.32),
                mix_color(accent_hi, Color::WHITE, 0.52),
                Shadow {
                    color: rgba(p.brass, if p.is_light { 0.32 } else { 0.42 }),
                    offset: Vector::new(0.0, 4.0),
                    blur_radius: 16.0,
                },
            )
        } else {
            (
                mix_color(accent_hi, Color::WHITE, 0.16),
                mix_color(accent_hi, accent, 0.38),
                accent,
                mix_color(accent_hi, Color::WHITE, 0.38),
                Shadow {
                    color: rgba(p.brass, if p.is_light { 0.24 } else { 0.30 }),
                    offset: Vector::new(0.0, 3.0),
                    blur_radius: 12.0,
                },
            )
        };

        let alpha = if is_disabled { 0.48 } else { 1.0 };

        button::Style {
            background: Some(Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, Color { a: alpha, ..top })
                    .add_stop(0.48, Color { a: alpha, ..mid })
                    .add_stop(1.0, Color { a: alpha, ..bot }),
            ))),
            text_color: Color {
                a: if is_disabled { 0.55 } else { 1.0 },
                ..rgb(t.primary_ink)
            },
            border: Border {
                color: Color { a: alpha, ..border_col },
                width: 1.0,
                radius: Radius::from(radius),
            },
            shadow: if is_disabled { Shadow::default() } else { shadow },
            snap: true,
        }
    }
}

/// Alias for `btn_primary` matching Linear/Raycast theme conventions.
pub fn primary_button_style(tokens: DesignTokens) -> impl Fn(&Theme, button::Status) -> button::Style {
    btn_primary(tokens)
}

/// Tactile secondary button with vertical glass-metal bevel, top-lit hairline, grounded shadow, and physical press.
pub fn btn_secondary(
    t: DesignTokens,
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| build_tactile_secondary_btn(t, active, status, t.ctl_radius(), false)
}

/// Alias for `btn_secondary`.
pub fn secondary_button_style(
    tokens: DesignTokens,
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    btn_secondary(tokens, active)
}

/// Tactile ruby-anodized destructive action button.
pub fn btn_danger(t: DesignTokens) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let p = t.p;
        let is_hovered = matches!(status, button::Status::Hovered);
        let is_pressed = matches!(status, button::Status::Pressed);
        let hot = rgb(p.hot);
        let (top, bot) = if is_pressed {
            (
                rgba(p.hot, 0.20),
                rgba(p.hot, 0.32),
            )
        } else if is_hovered {
            (
                mix_color(rgba(p.hot, 0.36), Color::WHITE, 0.12),
                rgba(p.hot, 0.22),
            )
        } else {
            (
                mix_color(rgba(p.hot, 0.26), Color::WHITE, 0.08),
                rgba(p.hot, 0.14),
            )
        };
        button::Style {
            background: Some(Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, top)
                    .add_stop(1.0, bot),
            ))),
            text_color: if is_hovered {
                mix_color(hot, Color::WHITE, if p.is_light { 0.0 } else { 0.22 })
            } else {
                hot
            },
            border: Border {
                color: rgba(p.hot, if is_hovered { 0.85 } else { 0.62 }),
                width: 1.0,
                radius: Radius::from(t.ctl_radius()),
            },
            shadow: if is_pressed || t.design == DesignId::Contrast {
                Shadow::default()
            } else {
                Shadow {
                    color: rgba(p.hot, if is_hovered { 0.24 } else { 0.14 }),
                    offset: Vector::new(0.0, 2.0),
                    blur_radius: 8.0,
                }
            },
            snap: true,
        }
    }
}

/// Low-profile tactile micro-capsule button (soft smoked glass at rest, full glass-metal bevel on hover/press).
pub fn btn_ghost(
    t: DesignTokens,
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let radius = if t.p.square_corners { 0.0 } else { 999.0 };
        build_tactile_secondary_btn(t, active, status, radius, true)
    }
}

/// Interactive card button (Profiles, Effect cards, Starter ideas, Settings Design cards).
pub fn card_button(
    t: DesignTokens,
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let p = t.p;
        let is_hovered = matches!(status, button::Status::Hovered);
        let is_pressed = matches!(status, button::Status::Pressed);
        let radius = if p.square_corners {
            0.0
        } else if t.is_firefly() {
            13.0
        } else {
            p.radius_panel.max(11.0)
        };

        let card = build_diagonal_card_style(
            t,
            if active { Some(p.brass) } else { None },
            is_hovered || active,
            radius,
            active,
        );

        let border_col = if active {
            rgba(if p.is_light { p.brass } else { p.brass_hi }, 0.86)
        } else if is_hovered {
            if p.is_light {
                mix_color(
                    Color::from_rgba(0.11, 0.11, 0.09, 0.24),
                    rgba(p.brass, 0.45),
                    0.35,
                )
            } else {
                mix_color(
                    Color::from_rgba(1.0, 1.0, 1.0, 0.24),
                    rgba(t.pink_rose, 0.55),
                    0.35,
                )
            }
        } else {
            card.border.color
        };

        let shadow = if is_pressed || t.design == DesignId::Contrast {
            Shadow::default()
        } else if active {
            Shadow {
                color: rgba(p.brass, if p.is_light { 0.20 } else { 0.28 }),
                offset: Vector::new(0.0, if is_hovered { 4.0 } else { 3.0 }),
                blur_radius: if is_hovered { 16.0 } else { 12.0 },
            }
        } else {
            Shadow {
                color: Color::from_rgba(
                    0.02,
                    0.01,
                    0.05,
                    if p.is_light { 0.09 } else { 0.34 },
                ),
                offset: Vector::new(0.0, if is_hovered { 4.0 } else { 2.5 }),
                blur_radius: if is_hovered { 12.0 } else { 8.0 },
            }
        };

        button::Style {
            background: card.background,
            text_color: rgb(p.ink),
            border: Border {
                color: border_col,
                width: if t.design == DesignId::Contrast {
                    2.0
                } else if active {
                    1.45
                } else {
                    1.0
                },
                radius: Radius::from(radius),
            },
            shadow,
            snap: true,
        }
    }
}

/// Legacy top navigation tab button delegating to `nav_tab_style`.
pub fn nav_tab_btn(
    t: DesignTokens,
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    nav_tab_style(t, active)
}

/// Circular Power orb button (`36x36` luminous anodized apricot-gold when ON, milled dark glass when OFF).
pub fn power_circle_btn(
    t: DesignTokens,
    on: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let p = t.p;
        let radius = if p.square_corners { 0.0 } else { 999.0 };
        if on {
            let mut s = btn_primary(t)(theme, status);
            s.border.radius = Radius::from(radius);
            if !matches!(status, button::Status::Pressed) && t.design != DesignId::Contrast {
                let hovered = matches!(status, button::Status::Hovered);
                s.shadow = Shadow {
                    color: rgba(p.brass, if hovered { 0.52 } else { 0.36 }),
                    offset: Vector::new(0.0, 2.0),
                    blur_radius: if hovered { 18.0 } else { 14.0 },
                };
            }
            s
        } else {
            build_tactile_secondary_btn(t, false, status, radius, false)
        }
    }
}

// ---------------------------------------------------------------------------
// Illuminated Sliders, Checkboxes, Pill Toggles & Recessed Inputs
// ---------------------------------------------------------------------------

/// Custom tactile Pill Toggle Switch (`42x22`) track button style.
pub fn toggle_track_btn(
    t: DesignTokens,
    on: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let p = t.p;
        let is_hovered = matches!(status, button::Status::Hovered);
        let is_pressed = matches!(status, button::Status::Pressed);
        let radius = if p.square_corners { 0.0 } else { 999.0 };

        let bg = if on {
            let left = if t.is_firefly() {
                mix_color(rgb(t.pink_rose), rgb(p.brass), 0.35)
            } else {
                rgb(p.brass)
            };
            let right = if is_hovered {
                mix_color(rgb(p.brass_hi), Color::WHITE, 0.20)
            } else {
                rgb(p.brass_hi)
            };
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(FRAC_PI_2))
                    .add_stop(0.0, Color { a: if p.is_light { 0.85 } else { 0.72 }, ..left })
                    .add_stop(1.0, Color { a: if p.is_light { 0.92 } else { 0.86 }, ..right }),
            ))
        } else if p.is_light {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, darken(p.well, 0.08))
                    .add_stop(1.0, rgb(if is_hovered { p.raised } else { p.well })),
            ))
        } else {
            let base = if t.is_firefly() { 0x1b1528 } else { p.well };
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, darken(base, 0.22))
                    .add_stop(1.0, if is_hovered { lighten(base, 0.08) } else { rgb(base) }),
            ))
        };

        button::Style {
            background: Some(bg),
            text_color: Color::TRANSPARENT,
            border: Border {
                color: if on {
                    mix_color(rgb(p.brass_hi), Color::WHITE, 0.28)
                } else if p.is_light {
                    Color::from_rgba(0.11, 0.11, 0.09, 0.18)
                } else {
                    Color::from_rgba(1.0, 1.0, 1.0, if is_hovered { 0.22 } else { 0.13 })
                },
                width: 1.15,
                radius: Radius::from(radius),
            },
            shadow: if on && !is_pressed && t.design != DesignId::Contrast {
                Shadow {
                    color: rgba(p.brass, if is_hovered { 0.38 } else { 0.28 }),
                    offset: Vector::new(0.0, 2.0),
                    blur_radius: 10.0,
                }
            } else {
                Shadow::default()
            },
            snap: true,
        }
    }
}

/// Elevated `16px` pearl thumb inside `pill_toggle`.
pub fn toggle_knob_style(t: DesignTokens, on: bool) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let p = t.p;
        let radius = if p.square_corners { 0.0 } else { 999.0 };
        let bg = if on {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, Color::WHITE)
                    .add_stop(1.0, mix_color(rgb(p.brass_hi), Color::WHITE, 0.62)),
            ))
        } else {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, lighten(p.ink_2, 0.22))
                    .add_stop(1.0, rgb(p.ink_3)),
            ))
        };
        container::Style {
            background: Some(bg),
            border: Border {
                color: if on {
                    mix_color(rgb(p.brass_hi), Color::WHITE, 0.45)
                } else {
                    rgba(p.ground, 0.45)
                },
                width: 1.0,
                radius: Radius::from(radius),
            },
            shadow: if t.design == DesignId::Contrast {
                Shadow::default()
            } else {
                Shadow {
                    color: if on {
                        rgba(p.brass, 0.48)
                    } else {
                        Color::from_rgba(0.0, 0.0, 0.0, 0.35)
                    },
                    offset: Vector::new(0.0, 1.5),
                    blur_radius: 5.0,
                }
            },
            ..Default::default()
        }
    }
}

/// Tactile jewel-lit circular color swatch button.
pub fn swatch_btn(
    col: [u8; 3],
    selected: bool,
    t: DesignTokens,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered);
        let pressed = matches!(status, button::Status::Pressed);
        let c = Color::from_rgb8(col[0], col[1], col[2]);
        let top = mix_color(c, Color::WHITE, if hovered { 0.26 } else { 0.16 });
        let bot = mix_color(c, Color::BLACK, if pressed { 0.18 } else { 0.08 });
        button::Style {
            background: Some(Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, top)
                    .add_stop(0.55, c)
                    .add_stop(1.0, bot),
            ))),
            text_color: Color::TRANSPARENT,
            border: Border {
                color: if selected {
                    Color::WHITE
                } else if hovered {
                    mix_color(c, Color::WHITE, 0.55)
                } else if t.p.is_light {
                    Color::from_rgba(0.11, 0.11, 0.09, 0.22)
                } else {
                    Color::from_rgba(1.0, 1.0, 1.0, 0.22)
                },
                width: if selected { 2.2 } else { 1.1 },
                radius: Radius::from(if t.p.square_corners { 0.0 } else { 999.0 }),
            },
            shadow: if selected || hovered {
                Shadow {
                    color: Color {
                        a: if selected { 0.52 } else { 0.32 },
                        ..c
                    },
                    offset: Vector::new(0.0, 2.0),
                    blur_radius: if selected { 10.0 } else { 7.0 },
                }
            } else {
                Shadow {
                    color: Color::from_rgba(0.0, 0.0, 0.0, 0.26),
                    offset: Vector::new(0.0, 1.5),
                    blur_radius: 4.0,
                }
            },
            snap: true,
        }
    }
}

/// Lush illuminated hardware slider (`5.5px` active rail + elevated `16px` pearl-bevel thumb).
pub fn slider_style(t: DesignTokens) -> impl Fn(&Theme, slider::Status) -> slider::Style {
    move |_, status| {
        let p = t.p;
        let active = matches!(status, slider::Status::Hovered | slider::Status::Dragged);

        let active_rail = if t.is_firefly() {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(FRAC_PI_2))
                    .add_stop(0.0, rgb(t.pink_rose))
                    .add_stop(0.55, rgb(p.brass))
                    .add_stop(1.0, mix_color(rgb(p.brass_hi), Color::WHITE, 0.20)),
            ))
        } else if t.design == DesignId::Contrast {
            Background::Color(rgb(p.brass))
        } else {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(FRAC_PI_2))
                    .add_stop(0.0, mix_color(rgb(p.brass), Color::BLACK, 0.08))
                    .add_stop(1.0, mix_color(rgb(p.brass_hi), Color::WHITE, 0.16)),
            ))
        };

        let inactive_rail = if p.is_light {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, darken(p.well, 0.08))
                    .add_stop(1.0, rgb(p.well)),
            ))
        } else {
            let well_hex = if t.is_firefly() { 0x191426 } else { p.well };
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, darken(well_hex, 0.24))
                    .add_stop(1.0, lighten(well_hex, 0.03)),
            ))
        };

        let thumb_bg = if t.design == DesignId::Contrast {
            Background::Color(rgb(if active { p.brass_hi } else { p.ink }))
        } else {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, Color::WHITE)
                    .add_stop(
                        0.55,
                        mix_color(rgb(p.brass_hi), Color::WHITE, if active { 0.68 } else { 0.52 }),
                    )
                    .add_stop(
                        1.0,
                        mix_color(rgb(p.brass_hi), Color::WHITE, if active { 0.36 } else { 0.18 }),
                    ),
            ))
        };

        slider::Style {
            rail: slider::Rail {
                backgrounds: (active_rail, inactive_rail),
                width: 5.5,
                border: Border {
                    color: if t.design == DesignId::Contrast {
                        rgb(p.line)
                    } else if p.is_light {
                        Color::from_rgba(0.11, 0.11, 0.09, 0.15)
                    } else {
                        Color::from_rgba(1.0, 1.0, 1.0, 0.11)
                    },
                    width: 1.0,
                    radius: Radius::from(if p.square_corners { 0.0 } else { 999.0 }),
                },
            },
            handle: slider::Handle {
                shape: if p.square_corners {
                    slider::HandleShape::Rectangle {
                        width: 12,
                        border_radius: Radius::from(0.0),
                    }
                } else {
                    slider::HandleShape::Circle {
                        radius: if active { 8.5 } else { 8.0 },
                    }
                },
                background: thumb_bg,
                border_width: if active { 2.2 } else { 1.8 },
                border_color: if p.is_light {
                    rgb(p.brass)
                } else {
                    mix_color(rgb(p.brass), Color::WHITE, if active { 0.42 } else { 0.18 })
                },
            },
        }
    }
}

/// Recessed precision hardware text input well with crisp focus glow border.
pub fn text_input_style(t: DesignTokens) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |_, status| {
        let p = t.p;
        let focused = matches!(status, text_input::Status::Focused { .. });
        let hovered = matches!(status, text_input::Status::Hovered);

        let bg = if t.design == DesignId::Contrast {
            Background::Color(rgb(p.well))
        } else if p.is_light {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, if focused { Color::WHITE } else { darken(p.well, 0.03) })
                    .add_stop(1.0, if focused { Color::WHITE } else { lighten(p.panel, 0.30) }),
            ))
        } else {
            let well_hex = if t.is_firefly() { 0x1a1527 } else { p.well };
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, darken(well_hex, if focused { 0.10 } else { 0.20 }))
                    .add_stop(1.0, if focused { lighten(well_hex, 0.05) } else { rgb(well_hex) }),
            ))
        };

        let border_col = if focused {
            rgb(if p.is_light { p.brass } else { p.brass_hi })
        } else if hovered {
            if p.is_light {
                Color::from_rgba(0.11, 0.11, 0.09, 0.26)
            } else {
                Color::from_rgba(1.0, 1.0, 1.0, 0.24)
            }
        } else if t.design == DesignId::Contrast {
            rgb(p.line)
        } else if p.is_light {
            Color::from_rgba(0.11, 0.11, 0.09, 0.15)
        } else {
            Color::from_rgba(1.0, 1.0, 1.0, 0.12)
        };

        text_input::Style {
            background: bg,
            border: Border {
                color: border_col,
                width: if focused { 1.5 } else { 1.0 },
                radius: Radius::from(if p.square_corners {
                    0.0
                } else {
                    t.ctl_radius().min(10.0)
                }),
            },
            icon: rgb(p.ink_3),
            placeholder: rgba(p.ink_3, 0.80),
            value: rgb(p.ink),
            selection: rgba(p.brass, 0.32),
        }
    }
}

/// Tactile beveled checkbox with crisp top-lit border.
pub fn checkbox_style(t: DesignTokens) -> impl Fn(&Theme, checkbox::Status) -> checkbox::Style {
    move |_, status| {
        let p = t.p;
        let (is_checked, hovered) = match status {
            checkbox::Status::Active { is_checked } => (is_checked, false),
            checkbox::Status::Hovered { is_checked } => (is_checked, true),
            checkbox::Status::Disabled { is_checked } => (is_checked, false),
        };

        let bg = if is_checked {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(
                        0.0,
                        mix_color(rgb(p.brass_hi), Color::WHITE, if hovered { 0.26 } else { 0.16 }),
                    )
                    .add_stop(1.0, rgb(p.brass)),
            ))
        } else if p.is_light {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, if hovered { Color::WHITE } else { darken(p.well, 0.04) })
                    .add_stop(1.0, rgb(if hovered { p.raised } else { p.well })),
            ))
        } else {
            let base = if t.is_firefly() { 0x1d172b } else { p.well };
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, if hovered { lighten(p.raised, 0.06) } else { darken(base, 0.16) })
                    .add_stop(1.0, rgb(if hovered { p.panel } else { base })),
            ))
        };

        checkbox::Style {
            background: bg,
            icon_color: rgb(t.primary_ink),
            border: Border {
                color: if is_checked {
                    mix_color(rgb(p.brass_hi), Color::WHITE, 0.35)
                } else if hovered {
                    rgba(if p.is_light { p.brass } else { p.brass_hi }, 0.72)
                } else if p.is_light {
                    Color::from_rgba(0.11, 0.11, 0.09, 0.22)
                } else {
                    Color::from_rgba(1.0, 1.0, 1.0, 0.18)
                },
                width: 1.2,
                radius: Radius::from(if p.square_corners { 0.0 } else { 5.5 }),
            },
            text_color: Some(rgb(p.ink)),
        }
    }
}

// ---------------------------------------------------------------------------
// Overlays, Command Palette, Masthead Pills & Bento Sub-Cards
// ---------------------------------------------------------------------------

/// Dimmed translucent dusk backdrop behind modal overlays (Command Palette & Shortcuts).
pub fn modal_backdrop_style(t: DesignTokens) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let base = if t.p.is_light { 0x1a1712 } else { 0x0b0813 };
        container::Style {
            background: Some(Background::Color(rgba(base, 0.74))),
            ..Default::default()
        }
    }
}

/// Centered frosted glass modal frame for the Command Palette (`Ctrl+K`).
pub fn palette_modal_style(t: DesignTokens) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let p = t.p;
        let radius = if p.square_corners { 0.0 } else { 18.0 };
        let mut style = build_diagonal_card_style(t, Some(p.brass), false, radius, true);
        style.border = Border {
            color: if t.is_firefly() {
                rgba(t.pink_rose, 0.56)
            } else {
                rgba(p.brass_hi, 0.58)
            },
            width: 1.35,
            radius: Radius::from(radius),
        };
        style.shadow = Shadow {
            color: Color::from_rgba(0.03, 0.01, 0.07, if p.is_light { 0.24 } else { 0.72 }),
            offset: Vector::new(0.0, 16.0),
            blur_radius: 38.0,
        };
        style
    }
}

/// Interactive Command Palette list item row.
pub fn palette_row_btn(
    t: DesignTokens,
    selected: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let p = t.p;
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let radius = if p.square_corners { 0.0 } else { 10.0 };
        let (bg, border_c, txt) = if selected {
            (
                Background::Gradient(Gradient::Linear(
                    Linear::new(Radians(FRAC_PI_2))
                        .add_stop(0.0, rgba(p.brass, 0.26))
                        .add_stop(
                            1.0,
                            rgba(if t.is_firefly() { t.pink_rose } else { p.brass_hi }, 0.14),
                        ),
                )),
                rgba(p.brass_hi, 0.62),
                rgb(p.ink),
            )
        } else if hovered {
            (
                Background::Gradient(Gradient::Linear(
                    Linear::new(Radians(PI))
                        .add_stop(0.0, rgba(p.raised, 0.78))
                        .add_stop(1.0, rgba(p.panel, 0.56)),
                )),
                if p.is_light {
                    Color::from_rgba(0.11, 0.11, 0.09, 0.15)
                } else {
                    Color::from_rgba(1.0, 1.0, 1.0, 0.14)
                },
                rgb(p.ink),
            )
        } else {
            (
                Background::Color(Color::TRANSPARENT),
                Color::TRANSPARENT,
                rgb(p.ink_2),
            )
        };
        button::Style {
            background: Some(bg),
            text_color: txt,
            border: Border {
                color: border_c,
                width: if selected || hovered { 1.0 } else { 0.0 },
                radius: Radius::from(radius),
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Sleek tactile keycap / icon capsule button in the top appbar (`⌘K`, `UI Scale`, `Fullscreen`, `Mini`).
pub fn shortcut_badge_btn(
    t: DesignTokens,
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let radius = if t.p.square_corners { 0.0 } else { 999.0 };
        build_tactile_secondary_btn(t, active, status, radius, false)
    }
}

/// Frosted tactile action pill (`Customize`, `Sleep`, `Theater`).
pub fn pill_action_btn(
    t: DesignTokens,
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let radius = if t.p.square_corners { 0.0 } else { 999.0 };
        build_tactile_secondary_btn(t, active, status, radius, false)
    }
}

/// Elevated translucent Customize drawer panel on Overview.
pub fn drawer_panel_style(t: DesignTokens) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let p = t.p;
        let radius = panel_radius(t);
        let mut style = build_diagonal_card_style(t, Some(p.brass), false, radius, true);
        style.border.color = if t.is_firefly() {
            rgba(t.pink_rose, 0.50)
        } else {
            rgba(p.brass, 0.54)
        };
        style
    }
}

/// Floating bottom-right Toast notification pill.
pub fn toast_pill_style(t: DesignTokens, accent: u32) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let p = t.p;
        let bg = if p.is_light {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, Color::WHITE)
                    .add_stop(1.0, rgb(p.panel)),
            ))
        } else {
            let hi = if t.is_firefly() { 0x3f3050 } else { p.raised };
            let lo = if t.is_firefly() { 0x281f36 } else { p.panel };
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, mix_color(rgb(hi), rgb(accent), 0.12))
                    .add_stop(1.0, rgb(lo)),
            ))
        };
        container::Style {
            background: Some(bg),
            border: Border {
                color: rgba(accent, 0.78),
                width: 1.3,
                radius: Radius::from(if p.square_corners { 0.0 } else { 999.0 }),
            },
            text_color: Some(rgb(p.ink)),
            shadow: Shadow {
                color: Color::from_rgba(0.03, 0.01, 0.07, 0.58),
                offset: Vector::new(0.0, 8.0),
                blur_radius: 22.0,
            },
            snap: true,
        }
    }
}

/// Subtle frameless click target used inside composite cards.
pub fn btn_frameless(t: DesignTokens) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(Background::Color(Color::TRANSPARENT)),
            text_color: rgb(if hovered { t.p.brass_hi } else { t.p.ink }),
            border: Border::default(),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Interactive Profile card container housing both the Apply click area and the in-card `Try` pill.
pub fn profile_card_frame(t: DesignTokens, active: bool) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let p = t.p;
        let radius = if p.square_corners {
            0.0
        } else if t.is_firefly() {
            13.0
        } else {
            p.radius_panel.max(11.0)
        };
        let mut style = build_diagonal_card_style(
            t,
            if active { Some(p.brass) } else { None },
            active,
            radius,
            active,
        );
        if active {
            style.border = Border {
                color: rgba(if p.is_light { p.brass } else { p.brass_hi }, 0.85),
                width: if t.design == DesignId::Contrast { 2.0 } else { 1.5 },
                radius: Radius::from(radius),
            };
        }
        style
    }
}

/// Tactile in-card pill button (used for `Try` badges inside Profile cards).
pub fn in_card_try_pill(
    t: DesignTokens,
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let radius = if t.p.square_corners { 0.0 } else { 999.0 };
        build_tactile_secondary_btn(t, active, status, radius, false)
    }
}

/// Tactile capsule pill button for the floating Overview masthead (`Control center`, `✦ Customize`, `Workspaces`).
pub fn masthead_pill_btn(
    t: DesignTokens,
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let radius = if t.p.square_corners { 0.0 } else { 999.0 };
        build_tactile_secondary_btn(t, active, status, radius, false)
    }
}

/// Tactile preset card button inside the `2×1` `Profiles` Bento tile & Lookbook swatches.
pub fn bento_profile_card_btn(
    t: DesignTokens,
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let p = t.p;
        let is_hovered = matches!(status, button::Status::Hovered);
        let is_pressed = matches!(status, button::Status::Pressed);
        let radius = if p.square_corners { 0.0 } else { 12.0 };

        let card = build_diagonal_card_style(
            t,
            if active { Some(p.brass) } else { None },
            is_hovered || active,
            radius,
            active,
        );

        let border_col = if active {
            rgba(if p.is_light { p.brass } else { p.brass_hi }, 0.86)
        } else if is_hovered {
            if p.is_light {
                Color::from_rgba(0.11, 0.11, 0.09, 0.24)
            } else {
                Color::from_rgba(1.0, 1.0, 1.0, 0.22)
            }
        } else if p.is_light {
            Color::from_rgba(0.11, 0.11, 0.09, 0.12)
        } else {
            Color::from_rgba(1.0, 1.0, 1.0, 0.10)
        };

        button::Style {
            background: card.background,
            text_color: rgb(p.ink),
            border: Border {
                color: border_col,
                width: if t.design == DesignId::Contrast {
                    2.0
                } else if active {
                    1.45
                } else {
                    1.0
                },
                radius: Radius::from(radius),
            },
            shadow: if is_pressed || t.design == DesignId::Contrast {
                Shadow::default()
            } else if active {
                Shadow {
                    color: rgba(p.brass, 0.26),
                    offset: Vector::new(0.0, 3.0),
                    blur_radius: 12.0,
                }
            } else {
                Shadow {
                    color: Color::from_rgba(
                        0.02,
                        0.01,
                        0.05,
                        if p.is_light { 0.08 } else { 0.32 },
                    ),
                    offset: Vector::new(0.0, 2.0),
                    blur_radius: 7.0,
                }
            },
            snap: true,
        }
    }
}

/// Subtle recessed glass strip for the live Activity Feed at the bottom of Overview.
pub fn activity_strip_style(t: DesignTokens) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let p = t.p;
        let bg = if p.is_light {
            Background::Color(rgba(p.well, 0.86))
        } else {
            let top = if t.is_firefly() { 0x261f36 } else { p.panel };
            let bot = if t.is_firefly() { 0x1d172b } else { p.well };
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, rgba(top, 0.86))
                    .add_stop(1.0, rgba(bot, 0.90)),
            ))
        };
        container::Style {
            background: Some(bg),
            border: Border {
                color: if p.is_light {
                    Color::from_rgba(0.11, 0.11, 0.09, 0.12)
                } else {
                    Color::from_rgba(1.0, 1.0, 1.0, 0.085)
                },
                width: 1.0,
                radius: Radius::from(if p.square_corners { 0.0 } else { 12.0 }),
            },
            text_color: Some(rgb(p.ink_2)),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accent_override_updates_tokens_and_readable_ink() {
        let rose = [244, 143, 177];
        let t = DesignTokens::for_appearance(DesignId::Firefly, Some(rose), false);
        assert_eq!(t.p.brass, 0xf48fb1);
        assert_eq!(t.accent_override, Some(rose));
        assert_eq!(t.primary_ink, 0x261722);

        let dark = [35, 50, 140];
        let td = DesignTokens::for_appearance(DesignId::Graphite, Some(dark), false);
        assert_eq!(td.primary_ink, 0xfff8fa);
    }

    #[test]
    fn compact_density_scales_spacing_and_padding() {
        let normal = DesignTokens::for_appearance(DesignId::Firefly, None, false);
        let compact = DesignTokens::for_appearance(DesignId::Firefly, None, true);
        assert!(compact.sp(12) < normal.sp(12));
        assert!(compact.pad(16) < normal.pad(16));
        assert!(compact.sp(2) >= 2);
        assert!(compact.pad(6) >= 6);
    }

    #[test]
    fn tactile_surfaces_docks_buttons_and_sliders_cover_all_13_designs() {
        let rose = [244, 143, 177];
        for design in DesignId::ALL {
            for ov in [None, Some(rose)] {
                let t = DesignTokens::for_appearance(design, ov, false);
                let theme = iced_theme_with_accent(design, ov);
                let p = panel_style(t)(&theme);
                let h = hero_panel_style(t)(&theme);
                let c = card_style(t)(&theme);
                let e = editing_tile_style(t)(&theme);
                let b = bento_tile_style(t, false)(&theme);
                let be = bento_tile_style(t, true)(&theme);
                let sw = bento_stage_well_style(t)(&theme);
                let rc = rich_instrument_card_style(t, Some(0xa5dece), false)(&theme);
                let rc_h = rich_instrument_card_style(t, Some(0xa5dece), true)(&theme);
                let dock = nav_dock_style(t)(&theme);
                let seg_tr = segmented_track_style(t)(&theme);

                assert!(p.background.is_some());
                assert!(h.background.is_some());
                assert!(c.background.is_some());
                assert!(e.background.is_some());
                assert!(b.background.is_some());
                assert!(be.background.is_some());
                assert!(sw.background.is_some());
                assert!(rc.background.is_some());
                assert!(rc_h.background.is_some());
                assert!(dock.background.is_some());
                assert!(seg_tr.background.is_some());
                assert!(e.border.width >= 1.5);

                // Primary & secondary button physical press states
                let pri_rest = btn_primary(t)(&theme, button::Status::Active);
                let pri_hover = btn_primary(t)(&theme, button::Status::Hovered);
                let pri_press = btn_primary(t)(&theme, button::Status::Pressed);
                assert!(pri_rest.background.is_some());
                assert!(pri_hover.shadow.blur_radius >= pri_rest.shadow.blur_radius);
                assert!(pri_press.shadow.blur_radius <= pri_rest.shadow.blur_radius);

                let sec_rest = btn_secondary(t, false)(&theme, button::Status::Active);
                let sec_press = btn_secondary(t, false)(&theme, button::Status::Pressed);
                assert!(sec_rest.background.is_some());
                assert_eq!(sec_press.shadow.blur_radius, 0.0);

                // Dock tabs & segmented pills
                let tab_on = nav_tab_style(t, true)(&theme, button::Status::Active);
                let tab_off = nav_tab_style(t, false)(&theme, button::Status::Active);
                let seg_on = segmented_pill_style(t, true)(&theme, button::Status::Active);
                let seg_off = segmented_pill_style(t, false)(&theme, button::Status::Active);
                assert!(tab_on.border.width >= 1.0);
                assert_eq!(tab_off.border.width, 0.0);
                assert!(seg_on.border.width >= 1.0);
                assert_eq!(seg_off.border.width, 0.0);

                // Settings nav items
                let nav_active = settings_nav_item_style(t, true)(&theme, button::Status::Active);
                let nav_idle = settings_nav_item_style(t, false)(&theme, button::Status::Active);
                let nav_hover = settings_nav_item_style(t, false)(&theme, button::Status::Hovered);
                assert!(nav_active.border.width >= 1.0);
                assert_eq!(nav_idle.border.width, 0.0);
                assert!(nav_hover.border.width >= 1.0);

                // Slider dimensions
                let sl = slider_style(t)(&theme, slider::Status::Active);
                assert!((sl.rail.width - 5.5).abs() < 0.01);
            }
        }
    }
}
