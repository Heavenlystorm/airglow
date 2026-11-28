//! Maps all 13 Airglow interface designs (with Firefly as the flagship dusk-plum + moth + pink/apricot/mint look)
//! into Iced themes, typography helpers, and hardware-instrument surface & control styles.

use std::f32::consts::{FRAC_PI_2, PI};

use airglow_core::config::{DesignId, DesignPalette};
use iced::border::Radius;
use iced::gradient::Linear;
use iced::theme::Palette;
use iced::widget::{button, checkbox, container, slider, text_input};
use iced::{Background, Border, Color, Font, Gradient, Radians, Shadow, Theme, Vector, font};

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
                appbar_left: 0x312434,
                appbar_mid: 0x271f32,
                appbar_right: 0x201a2a,
                appbar_border: 0x604859,
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
                line_soft: 0x27282c,
                data_primary: p.ink_2,
                data_secondary: p.ink_3,
                pink_rose: p.brass_hi,
                appbar_left: p.panel,
                appbar_mid: p.panel,
                appbar_right: p.panel,
                appbar_border: 0x2c2d31,
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
                line_soft: 0xddd7cb,
                data_primary: p.ink_2,
                data_secondary: p.ink_3,
                pink_rose: p.brass,
                appbar_left: 0xf5f2eb,
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
                line_soft: 0x241c16,
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
                line_soft: 0x1a1430,
                data_primary: 0x2fe6ff,
                data_secondary: 0xa78bfa,
                pink_rose: 0xff74c5,
                appbar_left: 0x0f0d1b,
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
                line_soft: 0x1d1715,
                data_primary: 0xff8a00,
                data_secondary: p.ink_2,
                pink_rose: 0xffcf66,
                appbar_left: 0x170e0b,
                appbar_mid: 0x140c09,
                appbar_right: 0x120a08,
                appbar_border: 0x272220,
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
                line_soft: 0x1a182a,
                data_primary: p.ink_2,
                data_secondary: p.brass,
                pink_rose: 0xf3798b,
                appbar_left: 0x0d0b19,
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
                    flat_surface: true,
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
        if self.pill_controls {
            999.0
        } else {
            self.p.radius_ctl
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
    Theme::custom(
        design.label().to_string(),
        Palette {
            background: rgb(p.ground),
            text: rgb(p.ink),
            primary: rgb(p.brass),
            success: rgb(p.ok),
            warning: rgb(p.warm),
            danger: rgb(p.hot),
        },
    )
}

/// Top-level window background (velvet dusk plum gradient on Firefly, clean ground elsewhere).
pub fn window_ground_style(t: DesignTokens) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let background = if t.is_firefly() {
            Some(Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, rgb(0x261f35))
                    .add_stop(0.42, rgb(t.p.ground))
                    .add_stop(1.0, rgb(0x1b1727)),
            )))
        } else {
            Some(Background::Color(rgb(t.p.ground)))
        };
        container::Style {
            background,
            text_color: Some(rgb(t.p.ink)),
            ..Default::default()
        }
    }
}

/// Full-bleed top appbar gradient (`#302532 -> #261f30 -> #211c29` with rose-plum bottom border on Firefly).
pub fn appbar_style(t: DesignTokens) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let bg = if t.is_firefly() {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(FRAC_PI_2))
                    .add_stop(0.0, rgb(t.appbar_left))
                    .add_stop(0.45, rgb(t.appbar_mid))
                    .add_stop(1.0, rgb(t.appbar_right)),
            ))
        } else if t.flat_surface {
            Background::Color(rgb(t.p.panel))
        } else {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, lighten(t.p.panel, 0.035))
                    .add_stop(1.0, rgb(t.p.panel)),
            ))
        };
        container::Style {
            background: Some(bg),
            text_color: Some(rgb(t.p.ink)),
            border: Border {
                color: rgb(t.appbar_border),
                width: 0.0,
                radius: Radius::from(0.0),
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Glowing brand badge wrapping `BrandMarkCanvas` in the header.
pub fn brand_badge_style(t: DesignTokens) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let bg = if t.is_firefly() {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, rgb(0x3a2d3d))
                    .add_stop(1.0, rgb(0x2c2231)),
            ))
        } else {
            Background::Color(rgb(t.p.well))
        };
        container::Style {
            background: Some(bg),
            border: Border {
                color: if t.is_firefly() {
                    rgba(t.p.brass, 0.34)
                } else {
                    rgba(t.p.line, 0.65)
                },
                width: 1.0,
                radius: Radius::from(if t.p.square_corners { 0.0 } else { 10.0 }),
            },
            shadow: if t.is_firefly() {
                Shadow {
                    color: rgba(t.p.brass, 0.22),
                    offset: Vector::new(0.0, 2.0),
                    blur_radius: 10.0,
                }
            } else {
                Shadow::default()
            },
            text_color: Some(rgb(t.p.ink)),
            snap: true,
        }
    }
}

/// Elevated panel / instrument card ("lit from within" plum plate on Firefly, top-lit plate on Graphite).
pub fn panel_style(t: DesignTokens) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let p = t.p;
        let background = if t.is_firefly() {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, rgb(0x372e47))
                    .add_stop(0.32, rgb(0x30283f))
                    .add_stop(1.0, rgb(0x282136)),
            ))
        } else if t.flat_surface {
            Background::Color(rgb(p.panel))
        } else if p.is_light {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, lighten(p.panel, 0.25))
                    .add_stop(0.35, rgb(p.panel))
                    .add_stop(1.0, darken(p.panel, 0.02)),
            ))
        } else {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, lighten(p.panel, 0.045))
                    .add_stop(0.28, rgb(p.panel))
                    .add_stop(1.0, darken(p.panel, 0.05)),
            ))
        };

        let border = if t.design == DesignId::Contrast {
            Border {
                color: rgb(p.line),
                width: 2.0,
                radius: Radius::from(p.radius_panel),
            }
        } else if t.design == DesignId::Midnight {
            Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: Radius::from(0.0),
            }
        } else {
            Border {
                color: if t.is_firefly() {
                    rgb(t.line_soft)
                } else {
                    rgba(p.line, 0.65)
                },
                width: 1.0,
                radius: Radius::from(p.radius_panel),
            }
        };

        let shadow = if t.is_firefly() {
            Shadow {
                color: rgba(0x0b0812, 0.44),
                offset: Vector::new(0.0, 5.0),
                blur_radius: 16.0,
            }
        } else if t.flat_surface {
            Shadow::default()
        } else if p.is_light {
            Shadow {
                color: Color::from_rgba(0.27, 0.23, 0.16, 0.12),
                offset: Vector::new(0.0, 2.0),
                blur_radius: 8.0,
            }
        } else {
            Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.34),
                offset: Vector::new(0.0, 2.0),
                blur_radius: 10.0,
            }
        };

        container::Style {
            background: Some(background),
            border,
            text_color: Some(rgb(p.ink)),
            shadow,
            snap: true,
        }
    }
}

/// Recessed instrument well (dark glass / inset tray).
pub fn well_style(t: DesignTokens) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let p = t.p;
        let bg = if t.flat_surface {
            Background::Color(rgb(p.well))
        } else {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, darken(p.well, 0.14))
                    .add_stop(1.0, rgb(p.well)),
            ))
        };
        let r = if p.square_corners {
            0.0
        } else if t.is_firefly() {
            12.0
        } else {
            p.radius_panel
        };
        container::Style {
            background: Some(bg),
            border: Border {
                color: if t.design == DesignId::Contrast {
                    rgb(p.line)
                } else {
                    rgba(t.line_soft, 0.8)
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

/// Ambient telemetry pill for top-bar CPU/GPU readout (deep dusk plum on Firefly, warm parchment well on Paper).
pub fn ambient_pill_style(t: DesignTokens) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let bg_hex = if t.is_firefly() {
            0x1a1624
        } else if t.p.is_light {
            t.p.well
        } else {
            t.p.well
        };
        let border_col = if t.is_firefly() {
            rgba(t.p.line, 0.65)
        } else if t.design == DesignId::Contrast {
            rgb(t.p.line)
        } else if t.p.is_light {
            rgb(t.p.line)
        } else {
            rgba(0x464852, 0.55)
        };
        container::Style {
            background: Some(Background::Color(rgb(bg_hex))),
            border: Border {
                color: border_col,
                width: if t.design == DesignId::Contrast { 2.0 } else { 1.0 },
                radius: Radius::from(if t.p.square_corners {
                    0.0
                } else if t.is_firefly() {
                    10.0
                } else {
                    6.0
                }),
            },
            text_color: Some(rgb(t.p.ink)),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Segmented control capsule container (`Silent | Balanced | Perf`, `0.5x | 1x | 2x`).
pub fn segmented_rail_style(t: DesignTokens) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let p = t.p;
        container::Style {
            background: Some(Background::Color(rgb(p.well))),
            border: Border {
                color: rgba(t.line_soft, 0.9),
                width: if t.design == DesignId::Contrast { 2.0 } else { 1.0 },
                radius: Radius::from(if p.square_corners {
                    0.0
                } else if t.is_firefly() || t.pill_controls {
                    999.0
                } else {
                    p.radius_ctl + 2.0
                }),
            },
            text_color: Some(rgb(p.ink)),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

pub fn segmented_item_btn(
    t: DesignTokens,
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let p = t.p;
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let radius = if p.square_corners {
            0.0
        } else if t.is_firefly() || t.pill_controls {
            999.0
        } else {
            p.radius_ctl
        };
        let (bg, txt, border_col) = if active {
            if t.is_firefly() {
                let top = mix_color(rgb(0x4a384b), rgb(p.brass), 0.16);
                let bot = mix_color(rgb(0x3d2f41), rgb(p.brass), 0.10);
                (
                    Background::Gradient(Gradient::Linear(
                        Linear::new(Radians(PI))
                            .add_stop(0.0, top)
                            .add_stop(1.0, bot),
                    )),
                    rgb(p.brass_hi),
                    rgba(p.brass, 0.48),
                )
            } else {
                (
                    Background::Color(rgb(p.raised)),
                    rgb(p.brass_hi),
                    rgb(p.brass),
                )
            }
        } else if hovered {
            (
                Background::Color(rgba(p.raised, 0.55)),
                rgb(p.ink),
                Color::TRANSPARENT,
            )
        } else {
            (
                Background::Color(Color::TRANSPARENT),
                rgb(p.ink_3),
                Color::TRANSPARENT,
            )
        };
        button::Style {
            background: Some(bg),
            text_color: txt,
            border: Border {
                color: border_col,
                width: if active { 1.0 } else { 0.0 },
                radius: Radius::from(radius),
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

pub fn banner_style(t: DesignTokens, accent: u32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(rgba(
            accent,
            if t.p.is_light { 0.12 } else { 0.13 },
        ))),
        border: Border {
            color: rgba(accent, 0.72),
            width: 1.2,
            radius: Radius::from(t.p.radius_panel.min(12.0)),
        },
        text_color: Some(rgb(t.p.ink)),
        shadow: Shadow::default(),
        snap: true,
    }
}

pub fn active_card_box(t: DesignTokens, active: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let p = t.p;
        if !active {
            return panel_style(t)(theme);
        }
        let bg = if t.is_firefly() {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, rgb(0x49374a))
                    .add_stop(1.0, rgb(0x3a2c3f)),
            ))
        } else {
            Background::Color(mix_color(rgb(p.panel), rgb(p.brass), 0.10))
        };
        container::Style {
            background: Some(bg),
            border: Border {
                color: if t.is_firefly() {
                    rgba(t.pink_rose, 0.65)
                } else {
                    rgba(p.brass, 0.75)
                },
                width: 1.4,
                radius: Radius::from(p.radius_panel),
            },
            text_color: Some(rgb(p.ink)),
            shadow: if t.flat_surface {
                Shadow::default()
            } else {
                Shadow {
                    color: rgba(if t.is_firefly() { t.pink_rose } else { p.brass }, 0.16),
                    offset: Vector::new(0.0, 2.0),
                    blur_radius: 14.0,
                }
            },
            snap: true,
        }
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
                radius: Radius::from(t.p.radius_panel),
                ..Default::default()
            },
            text_color: Some(rgb(0xfff4e4)),
            ..Default::default()
        }
    }
}

/// Hairline badge pill (used for "IN USE", "Whole rig", automation sentence tokens, index chips).
pub fn chip_style(t: DesignTokens, accent: Option<u32>) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let (bg, border_c, txt_c) = match accent {
            Some(c) => (rgba(c, 0.16), rgba(c, 0.55), rgb(c)),
            None => (rgb(t.p.well), rgba(t.line_soft, 0.85), rgb(t.p.ink_2)),
        };
        container::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                color: border_c,
                width: 1.0,
                radius: Radius::from(if t.p.square_corners {
                    0.0
                } else if t.is_firefly() || t.pill_controls {
                    999.0
                } else {
                    5.0
                }),
            },
            text_color: Some(txt_c),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

pub fn btn_secondary(
    t: DesignTokens,
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let p = t.p;
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let bg = if active {
            if t.is_firefly() {
                Background::Gradient(Gradient::Linear(
                    Linear::new(Radians(PI))
                        .add_stop(0.0, rgb(0x4b394c))
                        .add_stop(1.0, rgb(0x3f3144)),
                ))
            } else {
                Background::Color(mix_color(rgb(p.raised), rgb(p.brass), 0.14))
            }
        } else if t.flat_surface {
            Background::Color(rgb(if hovered { p.raised } else { p.panel }))
        } else {
            let top = if hovered {
                lighten(p.raised, 0.10)
            } else {
                lighten(p.raised, 0.04)
            };
            let bot = if hovered {
                rgb(p.raised)
            } else {
                mix_color(rgb(p.panel), rgb(p.raised), 0.6)
            };
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, top)
                    .add_stop(1.0, bot),
            ))
        };

        let border_col = if active {
            rgb(p.brass)
        } else if hovered {
            if t.is_firefly() {
                rgba(t.pink_rose, 0.45)
            } else {
                rgb(p.line)
            }
        } else if t.design == DesignId::Contrast {
            rgb(p.line)
        } else {
            rgba(p.line, 0.72)
        };

        let txt = if active {
            rgb(p.brass_hi)
        } else if hovered {
            rgb(p.ink)
        } else {
            rgb(p.ink_2)
        };

        button::Style {
            background: Some(bg),
            text_color: txt,
            border: Border {
                color: border_col,
                width: if t.design == DesignId::Contrast {
                    2.0
                } else if active {
                    1.3
                } else {
                    1.0
                },
                radius: Radius::from(t.ctl_radius()),
            },
            shadow: if active && !t.flat_surface {
                Shadow {
                    color: rgba(p.brass, 0.15),
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

/// Interactive card button (Profiles, Effect cards, Starter ideas, Settings Design cards).
pub fn card_button(
    t: DesignTokens,
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let p = t.p;
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let radius = if p.square_corners {
            0.0
        } else if t.is_firefly() {
            12.0
        } else {
            p.radius_panel
        };

        let bg = if active {
            if t.is_firefly() {
                Background::Gradient(Gradient::Linear(
                    Linear::new(Radians(PI))
                        .add_stop(0.0, mix_color(rgb(0x46344a), rgb(p.brass), 0.14))
                        .add_stop(1.0, mix_color(rgb(0x382a3e), rgb(p.brass), 0.08)),
                ))
            } else {
                Background::Color(mix_color(rgb(p.panel), rgb(p.brass), 0.10))
            }
        } else if hovered {
            Background::Color(rgb(p.raised))
        } else if t.is_firefly() {
            Background::Color(rgb(0x282135))
        } else {
            Background::Color(rgb(p.well))
        };

        let border_col = if active {
            rgb(p.brass)
        } else if hovered {
            if t.is_firefly() {
                rgba(t.pink_rose, 0.50)
            } else {
                rgb(p.line)
            }
        } else if t.design == DesignId::Contrast {
            rgb(p.line)
        } else {
            rgba(t.line_soft, 0.9)
        };

        button::Style {
            background: Some(bg),
            text_color: rgb(p.ink),
            border: Border {
                color: border_col,
                width: if t.design == DesignId::Contrast {
                    2.0
                } else if active {
                    1.5
                } else {
                    1.0
                },
                radius: Radius::from(radius),
            },
            shadow: if active && !t.flat_surface {
                Shadow {
                    color: rgba(p.brass, 0.18),
                    offset: Vector::new(0.0, 2.0),
                    blur_radius: 12.0,
                }
            } else {
                Shadow::default()
            },
            snap: true,
        }
    }
}

pub fn btn_primary(t: DesignTokens) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let p = t.p;
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let bg = if t.flat_surface {
            Background::Color(rgb(if hovered { p.brass_hi } else { p.brass }))
        } else {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, if hovered { lighten(p.brass_hi, 0.1) } else { rgb(p.brass_hi) })
                    .add_stop(1.0, rgb(p.brass)),
            ))
        };

        button::Style {
            background: Some(bg),
            text_color: rgb(t.primary_ink),
            border: Border {
                color: rgb(p.brass_hi),
                width: if t.design == DesignId::Contrast { 2.0 } else { 1.0 },
                radius: Radius::from(t.ctl_radius()),
            },
            shadow: if t.flat_surface {
                Shadow::default()
            } else {
                Shadow {
                    color: rgba(p.brass, if hovered { 0.34 } else { 0.22 }),
                    offset: Vector::new(0.0, 2.0),
                    blur_radius: 12.0,
                }
            },
            snap: true,
        }
    }
}

pub fn btn_danger(t: DesignTokens) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let p = t.p;
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(Background::Color(rgba(
                p.hot,
                if hovered { 0.26 } else { 0.15 },
            ))),
            text_color: rgb(p.hot),
            border: Border {
                color: rgba(p.hot, 0.8),
                width: 1.0,
                radius: Radius::from(t.ctl_radius()),
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Low-profile ghost chip button (transparent & borderless at rest, reveals subtle plum/brass wash on hover/active).
pub fn btn_ghost(
    t: DesignTokens,
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let p = t.p;
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let (bg, border_col, txt) = if active {
            (
                rgba(p.brass, 0.18),
                rgba(p.brass, 0.65),
                rgb(p.brass_hi),
            )
        } else if hovered {
            (
                rgba(p.raised, 0.72),
                rgba(t.pink_rose, 0.35),
                rgb(p.ink),
            )
        } else {
            (Color::TRANSPARENT, Color::TRANSPARENT, rgb(p.ink_3))
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: txt,
            border: Border {
                color: border_col,
                width: if active || hovered { 1.0 } else { 0.0 },
                radius: Radius::from(if p.square_corners { 0.0 } else { 999.0 }),
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Top navigation tab button (warm apricot-plum pill on Firefly/Spore/Kiln/Lumen, crisp underline elsewhere).
pub fn nav_tab_btn(
    t: DesignTokens,
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let p = t.p;
        let hovered = matches!(status, button::Status::Hovered);
        if t.pill_tabs {
            let (bg, txt, border_col) = if active {
                if t.is_firefly() {
                    (
                        Background::Gradient(Gradient::Linear(
                            Linear::new(Radians(PI))
                                .add_stop(0.0, mix_color(rgb(0x483649), rgb(p.brass), 0.16))
                                .add_stop(1.0, mix_color(rgb(0x3d2e40), rgb(p.brass), 0.10)),
                        )),
                        rgb(p.brass_hi),
                        rgba(p.brass, 0.44),
                    )
                } else {
                    (
                        Background::Color(rgba(p.brass, 0.16)),
                        rgb(p.brass_hi),
                        rgba(p.brass, 0.4),
                    )
                }
            } else if hovered {
                (
                    Background::Color(rgba(p.raised, 0.55)),
                    rgb(p.ink),
                    Color::TRANSPARENT,
                )
            } else {
                (
                    Background::Color(Color::TRANSPARENT),
                    rgb(p.ink_3),
                    Color::TRANSPARENT,
                )
            };
            return button::Style {
                background: Some(bg),
                text_color: txt,
                border: Border {
                    color: border_col,
                    width: if active { 1.0 } else { 0.0 },
                    radius: Radius::from(999.0),
                },
                shadow: if active && t.is_firefly() {
                    Shadow {
                        color: rgba(p.brass, 0.16),
                        offset: Vector::new(0.0, 1.0),
                        blur_radius: 10.0,
                    }
                } else {
                    Shadow::default()
                },
                snap: true,
            };
        }

        let txt = if active {
            rgb(p.ink)
        } else if hovered {
            rgb(p.ink_2)
        } else {
            rgb(p.ink_3)
        };
        button::Style {
            background: Some(Background::Color(Color::TRANSPARENT)),
            text_color: txt,
            border: Border::default(),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Circular Power button (`36x36` solid glowing apricot on Firefly when ON; metallic brass ring on Graphite).
pub fn power_circle_btn(
    t: DesignTokens,
    on: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let p = t.p;
        let hovered = matches!(status, button::Status::Hovered);
        let radius = if p.square_corners { 0.0 } else { 999.0 };

        if t.is_firefly() {
            let (bg, txt, border_c, shadow) = if on {
                (
                    Background::Gradient(Gradient::Linear(
                        Linear::new(Radians(PI))
                            .add_stop(
                                0.0,
                                if hovered {
                                    lighten(p.brass_hi, 0.12)
                                } else {
                                    rgb(p.brass_hi)
                                },
                            )
                            .add_stop(1.0, rgb(p.brass)),
                    )),
                    rgb(t.primary_ink),
                    rgb(p.brass_hi),
                    Shadow {
                        color: rgba(p.brass, if hovered { 0.52 } else { 0.36 }),
                        offset: Vector::new(0.0, 0.0),
                        blur_radius: 14.0,
                    },
                )
            } else {
                (
                    Background::Color(rgb(if hovered { p.raised } else { p.panel })),
                    rgb(p.ink_3),
                    rgb(p.line),
                    Shadow::default(),
                )
            };
            return button::Style {
                background: Some(bg),
                text_color: txt,
                border: Border {
                    color: border_c,
                    width: 1.2,
                    radius: Radius::from(radius),
                },
                shadow,
                snap: true,
            };
        }

        let bg = if on {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, mix_color(rgb(p.raised), rgb(p.brass), if hovered { 0.28 } else { 0.18 }))
                    .add_stop(1.0, mix_color(rgb(p.panel), rgb(p.brass), 0.10)),
            ))
        } else {
            Background::Color(rgb(if hovered { p.raised } else { p.well }))
        };

        button::Style {
            background: Some(bg),
            text_color: rgb(if on { p.brass_hi } else { p.ink_3 }),
            border: Border {
                color: rgb(if on { p.brass } else { p.line }),
                width: 1.5,
                radius: Radius::from(radius),
            },
            shadow: if on && !t.flat_surface {
                Shadow {
                    color: rgba(p.brass, 0.36),
                    offset: Vector::new(0.0, 0.0),
                    blur_radius: 14.0,
                }
            } else {
                Shadow::default()
            },
            snap: true,
        }
    }
}

/// Custom tactile Pill Toggle Switch (`42x22`) track button style.
pub fn toggle_track_btn(
    t: DesignTokens,
    on: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let p = t.p;
        let hovered = matches!(status, button::Status::Hovered);
        let radius = if p.square_corners { 0.0 } else { 999.0 };
        let bg = if on {
            if t.is_firefly() {
                Background::Gradient(Gradient::Linear(
                    Linear::new(Radians(FRAC_PI_2))
                        .add_stop(0.0, rgba(t.pink_rose, if hovered { 0.42 } else { 0.32 }))
                        .add_stop(1.0, rgba(p.brass, if hovered { 0.50 } else { 0.40 })),
                ))
            } else {
                Background::Color(rgba(p.brass, if hovered { 0.32 } else { 0.24 }))
            }
        } else {
            Background::Color(rgb(if hovered { p.raised } else { p.well }))
        };
        button::Style {
            background: Some(bg),
            text_color: Color::TRANSPARENT,
            border: Border {
                color: if on {
                    rgb(p.brass)
                } else {
                    rgba(p.line, 0.85)
                },
                width: 1.2,
                radius: Radius::from(radius),
            },
            shadow: if on && !t.flat_surface {
                Shadow {
                    color: rgba(p.brass, 0.22),
                    offset: Vector::new(0.0, 0.0),
                    blur_radius: 8.0,
                }
            } else {
                Shadow::default()
            },
            snap: true,
        }
    }
}

pub fn toggle_knob_style(t: DesignTokens, on: bool) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let p = t.p;
        let radius = if p.square_corners { 0.0 } else { 999.0 };
        container::Style {
            background: Some(Background::Color(rgb(if on {
                p.brass_hi
            } else {
                p.ink_3
            }))),
            border: Border {
                color: if on {
                    rgb(p.brass)
                } else {
                    rgba(p.ground, 0.5)
                },
                width: 1.0,
                radius: Radius::from(radius),
            },
            shadow: if on && !t.flat_surface {
                Shadow {
                    color: rgba(p.brass, 0.45),
                    offset: Vector::new(0.0, 0.0),
                    blur_radius: 6.0,
                }
            } else {
                Shadow::default()
            },
            ..Default::default()
        }
    }
}

pub fn swatch_btn(
    col: [u8; 3],
    selected: bool,
    t: DesignTokens,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered);
        let c = Color::from_rgb8(col[0], col[1], col[2]);
        button::Style {
            background: Some(Background::Color(c)),
            text_color: Color::TRANSPARENT,
            border: Border {
                color: if selected {
                    rgb(t.p.ink)
                } else if hovered {
                    rgb(t.p.brass_hi)
                } else {
                    rgba(t.p.line, 0.85)
                },
                width: if selected { 2.2 } else { 1.0 },
                radius: Radius::from(if t.p.square_corners { 0.0 } else { 999.0 }),
            },
            shadow: if selected {
                Shadow {
                    color: Color { a: 0.45, ..c },
                    offset: Vector::new(0.0, 0.0),
                    blur_radius: 8.0,
                }
            } else {
                Shadow::default()
            },
            snap: true,
        }
    }
}

pub fn slider_style(t: DesignTokens) -> impl Fn(&Theme, slider::Status) -> slider::Style {
    move |_, status| {
        let p = t.p;
        let active = matches!(status, slider::Status::Hovered | slider::Status::Dragged);
        let handle_col = if active { rgb(p.brass_hi) } else { rgb(p.brass) };
        let active_rail = if t.is_firefly() {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(FRAC_PI_2))
                    .add_stop(0.0, rgb(t.pink_rose))
                    .add_stop(1.0, rgb(p.brass)),
            ))
        } else {
            Background::Color(rgb(p.brass))
        };
        slider::Style {
            rail: slider::Rail {
                backgrounds: (active_rail, Background::Color(rgb(p.well))),
                width: 4.5,
                border: Border {
                    color: rgba(t.line_soft, 0.9),
                    width: 1.0,
                    radius: Radius::from(if p.square_corners { 0.0 } else { 999.0 }),
                },
            },
            handle: slider::Handle {
                shape: if p.square_corners {
                    slider::HandleShape::Rectangle {
                        width: 9,
                        border_radius: Radius::from(0.0),
                    }
                } else {
                    slider::HandleShape::Circle {
                        radius: if t.is_firefly() { 7.5 } else { 7.0 },
                    }
                },
                background: Background::Color(if t.is_firefly() {
                    rgb(p.brass_hi)
                } else {
                    handle_col
                }),
                border_width: 2.0,
                border_color: rgb(if t.is_firefly() { p.panel } else { p.ground }),
            },
        }
    }
}

pub fn text_input_style(t: DesignTokens) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |_, status| {
        let p = t.p;
        let focused = matches!(status, text_input::Status::Focused { .. });
        let hovered = matches!(status, text_input::Status::Hovered);
        text_input::Style {
            background: Background::Color(rgb(p.well)),
            border: Border {
                color: if focused {
                    rgb(p.brass)
                } else if hovered {
                    rgb(p.line)
                } else {
                    rgba(t.line_soft, 0.95)
                },
                width: if focused { 1.4 } else { 1.0 },
                radius: Radius::from(if p.square_corners { 0.0 } else { p.radius_ctl.min(10.0) }),
            },
            icon: rgb(p.ink_3),
            placeholder: rgb(p.ink_3),
            value: rgb(p.ink),
            selection: rgba(p.brass, 0.28),
        }
    }
}

pub fn checkbox_style(t: DesignTokens) -> impl Fn(&Theme, checkbox::Status) -> checkbox::Style {
    move |_, status| {
        let p = t.p;
        let (is_checked, hovered) = match status {
            checkbox::Status::Active { is_checked } => (is_checked, false),
            checkbox::Status::Hovered { is_checked } => (is_checked, true),
            checkbox::Status::Disabled { is_checked } => (is_checked, false),
        };
        checkbox::Style {
            background: Background::Color(if is_checked {
                rgb(p.brass)
            } else if hovered {
                rgb(p.raised)
            } else {
                rgb(p.well)
            }),
            icon_color: rgb(t.primary_ink),
            border: Border {
                color: if is_checked || hovered {
                    rgb(p.brass)
                } else {
                    rgb(p.line)
                },
                width: 1.2,
                radius: Radius::from(if p.square_corners { 0.0 } else { 5.0 }),
            },
            text_color: Some(rgb(p.ink)),
        }
    }
}

/// Dimmed translucent dusk backdrop behind modal overlays (Command Palette).
pub fn modal_backdrop_style(t: DesignTokens) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let base = if t.p.is_light { 0x1a1712 } else { 0x0e0a16 };
        container::Style {
            background: Some(Background::Color(rgba(base, 0.72))),
            ..Default::default()
        }
    }
}

/// Centered frosted glass modal frame for the Command Palette (`Ctrl+K`).
pub fn palette_modal_style(t: DesignTokens) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let p = t.p;
        let bg = if t.is_firefly() {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, rgb(0x362b47))
                    .add_stop(0.35, rgb(0x2a223a))
                    .add_stop(1.0, rgb(0x211a2f)),
            ))
        } else if t.p.is_light {
            Background::Color(rgb(p.panel))
        } else {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, lighten(p.panel, 0.05))
                    .add_stop(1.0, darken(p.panel, 0.08)),
            ))
        };
        container::Style {
            background: Some(bg),
            border: Border {
                color: if t.is_firefly() {
                    rgba(t.pink_rose, 0.55)
                } else {
                    rgba(p.brass, 0.65)
                },
                width: 1.4,
                radius: Radius::from(if p.square_corners { 0.0 } else { 16.0 }),
            },
            text_color: Some(rgb(p.ink)),
            shadow: Shadow {
                color: Color::from_rgba(0.04, 0.02, 0.08, 0.68),
                offset: Vector::new(0.0, 14.0),
                blur_radius: 34.0,
            },
            snap: true,
        }
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
        let (bg, border_c, txt) = if selected {
            (
                if t.is_firefly() {
                    Background::Gradient(Gradient::Linear(
                        Linear::new(Radians(FRAC_PI_2))
                            .add_stop(0.0, rgba(p.brass, 0.24))
                            .add_stop(1.0, rgba(t.pink_rose, 0.14)),
                    ))
                } else {
                    Background::Color(rgba(p.brass, 0.20))
                },
                rgba(p.brass, 0.60),
                rgb(p.ink),
            )
        } else if hovered {
            (
                Background::Color(rgba(p.raised, 0.68)),
                rgba(t.line_soft, 0.8),
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
                width: if selected { 1.0 } else { 0.0 },
                radius: Radius::from(if p.square_corners { 0.0 } else { 10.0 }),
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Sleek `Ctrl+K` quick-command keycap pill button in the top appbar.
pub fn shortcut_badge_btn(
    t: DesignTokens,
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let p = t.p;
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let bg = if active {
            rgba(p.brass, 0.24)
        } else if hovered {
            rgba(p.raised, 0.85)
        } else if t.is_firefly() {
            rgba(0x1e182b, 0.88)
        } else {
            rgb(p.well)
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: rgb(if active {
                p.brass_hi
            } else if hovered {
                p.ink
            } else {
                p.ink_2
            }),
            border: Border {
                color: if active {
                    rgba(p.brass, 0.72)
                } else if hovered {
                    rgba(t.pink_rose, 0.55)
                } else {
                    rgba(t.line_soft, 0.85)
                },
                width: 1.0,
                radius: Radius::from(if p.square_corners { 0.0 } else { 10.0 }),
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Frosted glass action pill floating cleanly on top of the Firefly masthead banner (`Customize`, `Sleep`).
pub fn pill_action_btn(
    t: DesignTokens,
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let p = t.p;
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let bg = if active {
            if t.is_firefly() {
                rgba(0x3a243d, 0.94)
            } else {
                rgba(p.brass, 0.28)
            }
        } else if hovered {
            if t.is_firefly() {
                rgba(0x2c1d3c, 0.94)
            } else {
                rgba(p.raised, 0.92)
            }
        } else if t.is_firefly() {
            rgba(0x181024, 0.88)
        } else {
            rgba(p.well, 0.92)
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: rgb(if active {
                p.brass_hi
            } else if t.is_firefly() {
                0xfff4ea
            } else if hovered {
                p.ink
            } else {
                p.ink_2
            }),
            border: Border {
                color: if active {
                    rgba(p.brass_hi, 0.88)
                } else if hovered {
                    rgba(t.pink_rose, 0.72)
                } else if t.is_firefly() {
                    rgba(t.pink_rose, 0.44)
                } else {
                    rgba(t.line_soft, 0.85)
                },
                width: 1.1,
                radius: Radius::from(if p.square_corners { 0.0 } else { 999.0 }),
            },
            shadow: if t.is_firefly() {
                Shadow {
                    color: rgba(if active { p.brass } else { 0x090512 }, if active { 0.28 } else { 0.42 }),
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

/// Elevated translucent Customize drawer panel on Overview.
pub fn drawer_panel_style(t: DesignTokens) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let p = t.p;
        let bg = if t.is_firefly() {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(PI))
                    .add_stop(0.0, rgb(0x3a2e4c))
                    .add_stop(1.0, rgb(0x2b233a)),
            ))
        } else if t.flat_surface {
            Background::Color(rgb(p.panel))
        } else {
            Background::Color(mix_color(rgb(p.panel), rgb(p.brass), 0.05))
        };
        container::Style {
            background: Some(bg),
            border: Border {
                color: if t.is_firefly() {
                    rgba(t.pink_rose, 0.52)
                } else {
                    rgba(p.brass, 0.58)
                },
                width: 1.2,
                radius: Radius::from(p.radius_panel),
            },
            text_color: Some(rgb(p.ink)),
            shadow: if t.flat_surface {
                Shadow::default()
            } else {
                Shadow {
                    color: rgba(if t.is_firefly() { t.pink_rose } else { p.brass }, 0.16),
                    offset: Vector::new(0.0, 4.0),
                    blur_radius: 16.0,
                }
            },
            snap: true,
        }
    }
}

/// Floating bottom-right Toast notification pill.
pub fn toast_pill_style(t: DesignTokens, accent: u32) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let p = t.p;
        let bg = if t.is_firefly() {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(FRAC_PI_2))
                    .add_stop(0.0, rgb(0x3b2d4a))
                    .add_stop(1.0, rgb(0x2a2238)),
            ))
        } else {
            Background::Color(rgb(p.raised))
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
                color: Color::from_rgba(0.04, 0.02, 0.08, 0.58),
                offset: Vector::new(0.0, 6.0),
                blur_radius: 20.0,
            },
            snap: true,
        }
    }
}

/// Subtle frameless click target used inside composite cards (e.g. Profile card with in-card Try pill).
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
            12.0
        } else {
            p.radius_panel
        };
        let bg = if active {
            if t.is_firefly() {
                Background::Gradient(Gradient::Linear(
                    Linear::new(Radians(PI))
                        .add_stop(0.0, mix_color(rgb(0x46344a), rgb(p.brass), 0.14))
                        .add_stop(1.0, mix_color(rgb(0x382a3e), rgb(p.brass), 0.08)),
                ))
            } else {
                Background::Color(mix_color(rgb(p.panel), rgb(p.brass), 0.10))
            }
        } else if t.is_firefly() {
            Background::Color(rgb(0x282135))
        } else {
            Background::Color(rgb(p.well))
        };
        container::Style {
            background: Some(bg),
            text_color: Some(rgb(p.ink)),
            border: Border {
                color: if active {
                    rgb(p.brass)
                } else if t.design == DesignId::Contrast {
                    rgb(p.line)
                } else {
                    rgba(t.line_soft, 0.9)
                },
                width: if t.design == DesignId::Contrast {
                    2.0
                } else if active {
                    1.5
                } else {
                    1.0
                },
                radius: Radius::from(radius),
            },
            shadow: if active && !t.flat_surface {
                Shadow {
                    color: rgba(p.brass, 0.18),
                    offset: Vector::new(0.0, 2.0),
                    blur_radius: 12.0,
                }
            } else {
                Shadow::default()
            },
            snap: true,
        }
    }
}

/// Subtle in-card pill button (used for the `Try` badge tucked neatly inside each Profile card).
pub fn in_card_try_pill(
    t: DesignTokens,
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let p = t.p;
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let (bg, border_c, txt) = if active {
            (rgba(p.brass, 0.24), rgba(p.brass, 0.78), rgb(p.brass_hi))
        } else if hovered {
            (rgba(p.raised, 0.92), rgba(t.pink_rose, 0.55), rgb(p.ink))
        } else {
            (
                rgba(p.well, if t.is_firefly() { 0.85 } else { 0.7 }),
                rgba(t.line_soft, 0.85),
                rgb(p.ink_3),
            )
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: txt,
            border: Border {
                color: border_c,
                width: 1.0,
                radius: Radius::from(if p.square_corners { 0.0 } else { 999.0 }),
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Subtle recessed strip for the live Activity Feed at the bottom of Overview.
pub fn activity_strip_style(t: DesignTokens) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let p = t.p;
        container::Style {
            background: Some(Background::Color(if t.is_firefly() {
                rgba(0x231d31, 0.92)
            } else {
                rgb(p.well)
            })),
            border: Border {
                color: rgba(t.line_soft, 0.78),
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
        // Bright rose pink should get dark plum ink for crisp contrast
        assert_eq!(t.primary_ink, 0x261722);

        // Deep saturated navy/indigo should get warm white ink
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
}
