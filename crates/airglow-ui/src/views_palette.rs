//! Instant Command Palette (`Ctrl+K`), Overview Customize Drawer, and Floating Toast Overlay.

use airglow_core::config::{ACCENT_SWATCHES, DesignId, RigStyle};
use airglow_core::effects::{EffectId, HARMONIES};
use iced::widget::{
    Space, button, column, container, mouse_area, row, scrollable, text, text_input,
};
use iced::{Alignment, Element, Length};

use crate::theme::{self, DesignTokens};
use crate::views::{color_dot, pill_toggle, preset_dot_color, status_dot};
use crate::{AirglowApp, Message, Tab};

pub const PALETTE_INPUT_ID: &str = "airglow_command_palette_input";

#[derive(Debug, Clone)]
pub struct PaletteItem {
    pub title: String,
    pub subtitle: String,
    pub category: &'static str,
    pub swatch: Option<[u8; 3]>,
    pub message: Message,
}

fn token_score(token: &str, title: &str, category: &str, subtitle: &str) -> Option<i32> {
    if title == token {
        return Some(1000);
    }
    if title.starts_with(token) {
        return Some(860 - title.len() as i32);
    }
    if title
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w.starts_with(token))
    {
        return Some(760 - title.len() as i32);
    }
    if let Some(idx) = title.find(token) {
        return Some(640 - idx as i32 * 3);
    }
    if category == token || category.starts_with(token) {
        return Some(520);
    }
    if subtitle
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w.starts_with(token))
    {
        return Some(440);
    }
    if let Some(idx) = subtitle.find(token) {
        return Some(340 - (idx as i32).min(80));
    }
    None
}

fn palette_match_score(query: &str, title: &str, category: &str, subtitle: &str) -> Option<i32> {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return Some(100);
    }
    let t = title.to_ascii_lowercase();
    let c = category.to_ascii_lowercase();
    let s = subtitle.to_ascii_lowercase();

    // Direct full-phrase boost
    let phrase_bonus = if t.contains(&q) {
        260
    } else if s.contains(&q) {
        100
    } else {
        0
    };

    let mut total = phrase_bonus;
    let mut matched_any = false;
    for tok in q.split_whitespace() {
        matched_any = true;
        let sc = token_score(tok, &t, &c, &s)?;
        total += sc;
    }
    if matched_any { Some(total) } else { Some(100) }
}

pub fn filtered_palette_items(app: &AirglowApp) -> Vec<PaletteItem> {
    let mut all = Vec::with_capacity(96);

    // Quick actions
    let lights_on = app.snapshot.effective_scene.on;
    all.push(PaletteItem {
        title: if lights_on {
            "Turn Lights Off".to_string()
        } else {
            "Turn Lights On".to_string()
        },
        subtitle: "Toggle all RGB zones smoothly".to_string(),
        category: "Action",
        swatch: Some(if lights_on {
            [110, 100, 120]
        } else {
            [255, 195, 139]
        }),
        message: Message::TogglePower,
    });

    all.push(PaletteItem {
        title: "Keyboard Shortcuts".to_string(),
        subtitle: "View all quick keys (? or Ctrl+/)".to_string(),
        category: "Action",
        swatch: Some([209, 185, 240]),
        message: Message::ToggleShortcuts,
    });

    all.push(PaletteItem {
        title: if app.compact_window {
            "Expand Full Airglow Window".to_string()
        } else {
            "Switch to Compact Mini-Mode".to_string()
        },
        subtitle: "430×410 desktop companion widget (Ctrl+M)".to_string(),
        category: "Action",
        swatch: Some([244, 143, 177]),
        message: Message::ToggleCompactMode,
    });

    all.push(PaletteItem {
        title: if app.customize_open {
            "Close Customize Drawer".to_string()
        } else {
            "Customize Workspace".to_string()
        },
        subtitle: "Toggle tiles, header pins, accent colour, rig style & FPS".to_string(),
        category: "Action",
        swatch: Some([244, 143, 177]),
        message: Message::OpenCustomizeFromPalette,
    });

    all.push(PaletteItem {
        title: "Surprise Me (Studio)".to_string(),
        subtitle: "Generate a fresh multi-layer Studio lighting scene".to_string(),
        category: "Action",
        swatch: Some([197, 155, 255]),
        message: Message::StudioSurpriseMe,
    });

    if let Some(rem) = app.snapshot.sleep_timer_remaining_s {
        all.push(PaletteItem {
            title: "Cancel Sleep Timer".to_string(),
            subtitle: format!("Currently counting down ({}m {:02}s left)", rem / 60, rem % 60),
            category: "Sleep",
            swatch: Some([255, 138, 117]),
            message: Message::CancelSleepTimer,
        });
    }
    for mins in [15u16, 30, 60] {
        all.push(PaletteItem {
            title: format!("Sleep Timer: {mins} min"),
            subtitle: "Smoothly dim over the final 2 minutes, then turn lights off".to_string(),
            category: "Sleep",
            swatch: Some([209, 185, 240]),
            message: Message::StartSleepTimer(mins),
        });
    }

    all.push(PaletteItem {
        title: "Rescan Hardware".to_string(),
        subtitle: "Refresh USB HID controllers, OpenRGB SDK & LHM fans".to_string(),
        category: "Action",
        swatch: Some([165, 222, 206]),
        message: Message::RescanHardware,
    });

    // Tabs (all 7)
    for (idx, tab) in Tab::ALL.iter().enumerate() {
        all.push(PaletteItem {
            title: format!("Go to {}", tab.label()),
            subtitle: format!("Switch tab (shortcut {})", idx + 1),
            category: "Tab",
            swatch: None,
            message: Message::SelectTab(*tab),
        });
    }

    // Saved Looks
    for look in &app.snapshot.config.saved_looks {
        let swatch = look
            .accent_override
            .unwrap_or_else(|| theme::hex_to_rgb_arr(look.design.palette().brass));
        all.push(PaletteItem {
            title: format!("Look: {}", look.name),
            subtitle: format!("{} · {} rig", look.design.label(), look.rig_style.label()),
            category: "Look",
            swatch: Some(swatch),
            message: Message::ApplyLook(look.id.clone()),
        });
    }

    // Built-in & saved Profiles
    for preset in &app.snapshot.config.presets {
        all.push(PaletteItem {
            title: format!("Profile: {}", preset.name),
            subtitle: format!("{} · {}% brightness", preset.effect.label(), preset.brightness),
            category: "Profile",
            swatch: Some(preset_dot_color(preset.effect, preset.params.color)),
            message: Message::ApplyPreset(preset.id.clone()),
        });
    }

    // All 16 Effects
    for eff in EffectId::ALL {
        all.push(PaletteItem {
            title: format!("Effect: {}", eff.label()),
            subtitle: eff.description().to_string(),
            category: "Effect",
            swatch: Some(preset_dot_color(eff, app.snapshot.config.scene.params.color)),
            message: Message::SelectEffect(eff),
        });
    }

    // 10 Color Harmonies
    for (idx, h) in HARMONIES.iter().enumerate() {
        all.push(PaletteItem {
            title: format!("Palette: {}", h.name),
            subtitle: "Apply 3-colour lighting harmony".to_string(),
            category: "Harmony",
            swatch: Some(h.colors[0]),
            message: Message::ApplyHarmony(idx),
        });
    }

    // Fan curves
    for (id, label, desc) in [
        ("silent", "Silent", "Whisper-quiet acoustic curve"),
        ("balanced", "Balanced", "Everyday quiet-to-cool ramp"),
        ("performance", "Performance", "Aggressive thermal headroom"),
    ] {
        all.push(PaletteItem {
            title: format!("Fan Curve: {label}"),
            subtitle: desc.to_string(),
            category: "Fans",
            swatch: Some([165, 222, 206]),
            message: Message::SelectFanCurve(id.to_string()),
        });
    }

    // All 13 Designs
    for d in DesignId::ALL {
        all.push(PaletteItem {
            title: format!("Design: {}", d.label()),
            subtitle: d.tagline().to_string(),
            category: "Design",
            swatch: Some(theme::hex_to_rgb_arr(d.palette().brass)),
            message: Message::SetDesign(d),
        });
    }

    // Accent swatches
    for (name, rgb) in ACCENT_SWATCHES {
        all.push(PaletteItem {
            title: format!("Accent: {name}"),
            subtitle: "Live accent colour override across all themes".to_string(),
            category: "Accent",
            swatch: Some(*rgb),
            message: Message::SetAccentOverride(Some(*rgb)),
        });
    }
    all.push(PaletteItem {
        title: "Accent: Design Default".to_string(),
        subtitle: "Reset accent colour override".to_string(),
        category: "Accent",
        swatch: None,
        message: Message::SetAccentOverride(None),
    });

    if app.palette_query.trim().is_empty() {
        // Curated high-utility default list when first opened
        all.truncate(16);
        return all;
    }

    let mut scored: Vec<(i32, PaletteItem)> = all
        .into_iter()
        .filter_map(|item| {
            palette_match_score(
                &app.palette_query,
                &item.title,
                item.category,
                &item.subtitle,
            )
            .map(|sc| (sc, item))
        })
        .collect();

    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.into_iter().take(16).map(|(_, it)| it).collect()
}

pub fn view_command_palette<'a>(
    app: &'a AirglowApp,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let mono = theme::mono_font();
    let items = filtered_palette_items(app);
    let sel_idx = if items.is_empty() {
        0
    } else {
        app.palette_selected.min(items.len() - 1)
    };

    let search_bar = row![
        text("⌘")
            .size(16)
            .font(theme::ui_semibold(t.design))
            .color(theme::rgb(p.brass_hi)),
        text_input(
            "Jump to tab, effect, design, harmony, profile, or sleep timer…",
            &app.palette_query
        )
        .id(iced::widget::Id::new(PALETTE_INPUT_ID))
        .size(14)
        .padding([8, 10])
        .style(theme::text_input_style(t))
        .on_input(Message::PaletteQueryChanged)
        .on_submit(Message::PaletteExecuteSelected)
        .width(Length::Fill),
        container(text("ESC").size(10.5).font(mono).color(theme::rgb(p.ink_3)))
            .padding([4, 8])
            .style(theme::chip_style(t, None)),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    let mut list_col = column![].spacing(4);
    if items.is_empty() {
        list_col = list_col.push(
            container(
                text(format!("No commands matching \"{}\"", app.palette_query))
                    .size(13)
                    .font(font)
                    .color(theme::rgb(p.ink_3)),
            )
            .padding([22, 12])
            .center_x(Length::Fill),
        );
    } else {
        for (idx, item) in items.iter().enumerate() {
            let selected = idx == sel_idx;
            let mut r = row![].spacing(10).align_y(Alignment::Center);
            if let Some(rgb) = item.swatch {
                r = r.push(color_dot(rgb, 10.0));
            } else {
                r = r.push(status_dot(
                    if selected { p.brass_hi } else { p.ink_3 },
                    7.0,
                ));
            }
            r = r
                .push(
                    column![
                        text(item.title.clone()).size(13).font(if selected {
                            theme::ui_semibold(t.design)
                        } else {
                            font
                        }),
                        text(item.subtitle.clone())
                            .size(11)
                            .font(font)
                            .color(theme::rgb(p.ink_3)),
                    ]
                    .spacing(1)
                    .width(Length::Fill),
                )
                .push(
                    container(
                        text(item.category)
                            .size(10)
                            .font(theme::ui_semibold(t.design)),
                    )
                    .padding([3, 8])
                    .style(theme::chip_style(
                        t,
                        if selected { Some(p.brass_hi) } else { None },
                    )),
                );
            if selected {
                r = r.push(
                    text("↵")
                        .size(12)
                        .font(mono)
                        .color(theme::rgb(p.brass_hi)),
                );
            }

            list_col = list_col.push(
                button(r)
                    .padding([7, 12])
                    .width(Length::Fill)
                    .height(Length::Fixed(48.0))
                    .style(theme::palette_row_btn(t, selected))
                    .on_press(Message::PaletteExecuteIndex(idx)),
            );
        }
    }

    let visible_rows = items.len().clamp(1, 6);
    let list_height = if items.is_empty() {
        72.0
    } else {
        (visible_rows as f32) * 48.0 + ((visible_rows.saturating_sub(1)) as f32) * 4.0
    };

    let footer = row![
        text("↑↓ Navigate  ·  ↵ Run  ·  Esc Close  ·  Ctrl+M Mini")
            .size(11)
            .font(font)
            .color(theme::rgb(p.ink_3)),
        Space::new().width(Length::Fill),
        text(format!("{} command(s)", items.len()))
            .size(11)
            .font(mono)
            .color(theme::rgb(p.ink_3)),
    ]
    .align_y(Alignment::Center);

    let divider = container(Space::new().width(Length::Fill).height(1)).style(move |_| {
        iced::widget::container::Style {
            background: Some(iced::Background::Color(theme::rgba(t.line_soft, 0.75))),
            ..Default::default()
        }
    });

    let modal_w = if app.compact_window { 398.0 } else { 610.0 };
    let top_pad = if app.compact_window { 24.0 } else { 72.0 };
    let modal_card = mouse_area(
        container(
            column![
                search_bar,
                divider,
                scrollable(list_col).height(Length::Fixed(list_height)),
                footer,
            ]
            .spacing(10),
        )
        .padding(16)
        .width(Length::Fixed(modal_w))
        .style(theme::palette_modal_style(t)),
    )
    .on_press(Message::Noop);

    mouse_area(
        container(
            column![
                Space::new().height(Length::Fixed(top_pad)),
                container(modal_card).center_x(Length::Fill),
                Space::new().height(Length::Fill),
            ]
            .width(Length::Fill)
            .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::modal_backdrop_style(t)),
    )
    .on_press(Message::CloseCommandPalette)
    .into()
}

/// Frosted 2-column Keyboard Shortcuts modal (`?` or `Ctrl+/`).
pub fn view_shortcuts_modal<'a>(t: DesignTokens, font: iced::Font) -> Element<'a, Message> {
    let p = t.p;
    let mono = theme::mono_font();

    let shortcut_row = |keys: &'static str, desc: &'static str| {
        row![
            container(
                text(keys)
                    .size(11)
                    .font(mono)
                    .color(theme::rgb(p.brass_hi)),
            )
            .padding([4, 9])
            .style(theme::chip_style(t, Some(p.brass))),
            Space::new().width(Length::Fill),
            text(desc).size(12.5).font(font).color(theme::rgb(p.ink)),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
    };

    let left_col = column![
        text("NAVIGATION & OVERLAYS")
            .size(10.5)
            .font(theme::ui_semibold(t.design))
            .color(theme::rgb(t.pink_rose)),
        shortcut_row("1 – 7", "Switch workspace tabs"),
        shortcut_row("Ctrl + K", "Command Palette"),
        shortcut_row("Ctrl + M", "Compact Mini-Mode"),
        shortcut_row("?  /  Ctrl + /", "Keyboard shortcuts"),
        shortcut_row("Esc", "Close active overlay"),
    ]
    .spacing(10)
    .width(Length::Fill);

    let right_col = column![
        text("STUDIO & PALETTE")
            .size(10.5)
            .font(theme::ui_semibold(t.design))
            .color(theme::rgb(t.data_primary)),
        shortcut_row("Ctrl + Z", "Studio Undo (30 steps)"),
        shortcut_row("Ctrl + Y", "Studio Redo"),
        shortcut_row("↑  /  ↓", "Navigate palette commands"),
        shortcut_row("Enter", "Run selected command"),
        shortcut_row("Ctrl + 1–7", "Jump tab anytime"),
    ]
    .spacing(10)
    .width(Length::Fill);

    let divider = container(Space::new().width(Length::Fill).height(1)).style(move |_| {
        iced::widget::container::Style {
            background: Some(iced::Background::Color(theme::rgba(t.line_soft, 0.75))),
            ..Default::default()
        }
    });

    let modal_card = mouse_area(
        container(
            column![
                row![
                    column![
                        text("Keyboard Shortcuts")
                            .size(20)
                            .font(theme::display_semibold(t.design))
                            .color(theme::rgb(p.ink)),
                        text("Fast hands-on-keyboard control across every Airglow view.")
                            .size(11.5)
                            .font(font)
                            .color(theme::rgb(p.ink_3)),
                    ]
                    .spacing(2)
                    .width(Length::Fill),
                    button(text("ESC").size(10.5).font(mono))
                        .padding([4, 9])
                        .style(theme::shortcut_badge_btn(t, false))
                        .on_press(Message::ToggleShortcuts),
                ]
                .align_y(Alignment::Center),
                divider,
                row![left_col, Space::new().width(18), right_col].spacing(8),
            ]
            .spacing(14),
        )
        .padding(20)
        .width(Length::Fixed(580.0))
        .style(theme::palette_modal_style(t)),
    )
    .on_press(Message::Noop);

    mouse_area(
        container(
            column![
                Space::new().height(Length::Fixed(110.0)),
                container(modal_card).center_x(Length::Fill),
                Space::new().height(Length::Fill),
            ]
            .width(Length::Fill)
            .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::modal_backdrop_style(t)),
    )
    .on_press(Message::ToggleShortcuts)
    .into()
}

/// Interactive collapsible Customize Drawer right on Overview.
pub fn view_customize_drawer<'a>(
    app: &'a AirglowApp,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let appr = &app.snapshot.config.appearance;

    // Live Accent swatches
    let mut accent_row = row![].spacing(7).align_y(Alignment::Center);
    for (name, rgb) in ACCENT_SWATCHES {
        let selected = appr.accent_override == Some(*rgb);
        let _ = name;
        accent_row = accent_row.push(
            button(Space::new().width(18).height(18))
                .padding(0)
                .width(Length::Fixed(23.0))
                .height(Length::Fixed(23.0))
                .style(theme::swatch_btn(*rgb, selected, t))
                .on_press(Message::SetAccentOverride(Some(*rgb))),
        );
    }
    accent_row = accent_row.push(
        button(text("Default").size(11).font(font))
            .padding([4, 9])
            .style(theme::btn_secondary(t, appr.accent_override.is_none()))
            .on_press(Message::SetAccentOverride(None)),
    );

    // Rig Style segmented rail
    let mut rig_rail = row![].spacing(2);
    for style in RigStyle::ALL {
        rig_rail = rig_rail.push(
            button(text(style.label()).size(11.5).font(font))
                .padding([5, 10])
                .style(theme::segmented_item_btn(t, appr.rig_style == style))
                .on_press(Message::SetRigStyle(style)),
        );
    }

    // GPU Power Mode (UI FPS Cap) segmented rail
    let mut fps_rail = row![].spacing(2);
    for (fps, label) in [(15u8, "Eco 15fps"), (30, "Balanced 30fps"), (60, "Smooth 60fps")] {
        fps_rail = fps_rail.push(
            button(text(label).size(11).font(font))
                .padding([5, 9])
                .style(theme::segmented_item_btn(t, appr.ui_fps_cap == fps))
                .on_press(Message::SetUiFpsCap(fps)),
        );
    }

    // Overview Tile visibility chips
    let mut tile_row = row![].spacing(6).align_y(Alignment::Center);
    for (key, label) in [
        ("profiles", "Profiles"),
        ("power", "Power"),
        ("fans", "Fans"),
        ("memory", "Memory"),
        ("drive", "Drive"),
        ("download", "Download"),
        ("cost", "Cost"),
        ("hardware", "Hardware"),
    ] {
        let vis = appr.is_tile_visible(key);
        tile_row = tile_row.push(
            button(
                text(if vis {
                    format!("✓ {label}")
                } else {
                    label.to_string()
                })
                .size(11)
                .font(font),
            )
            .padding([4, 9])
            .style(theme::btn_secondary(t, vis))
            .on_press(Message::ToggleOverviewTile(key.to_string())),
        );
    }

    // Header pinned sensor chips
    let mut pin_row = row![].spacing(6).align_y(Alignment::Center);
    for (key, label) in [
        ("cpu_temp", "CPU °"),
        ("gpu_temp", "GPU °"),
        ("total_power", "Watts"),
        ("fan_duty", "Fans %"),
        ("ram_load", "RAM %"),
    ] {
        let pinned = appr.is_header_pinned(key);
        pin_row = pin_row.push(
            button(
                text(if pinned {
                    format!("✓ {label}")
                } else {
                    label.to_string()
                })
                .size(11)
                .font(font),
            )
            .padding([4, 9])
            .style(theme::btn_secondary(t, pinned))
            .on_press(Message::ToggleHeaderPin(key.to_string())),
        );
    }

    let toggles_row = row![
        row![
            pill_toggle(t, appr.airflow_lines, Message::ToggleAirflowLines),
            text("Airflow").size(11.5).font(font),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
        row![
            pill_toggle(t, appr.spinning_fans, Message::ToggleSpinningFans),
            text("Spinning fans").size(11.5).font(font),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
        row![
            pill_toggle(t, appr.ambient_bloom, Message::ToggleAmbientBloom),
            text("Bloom").size(11.5).font(font),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
        row![
            pill_toggle(t, appr.firefly_particles, Message::ToggleFireflyParticles),
            text("Fireflies").size(11.5).font(font),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
        row![
            pill_toggle(t, appr.compact_density, Message::ToggleCompactDensity),
            text("Compact").size(11.5).font(font),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    ]
    .spacing(14)
    .align_y(Alignment::Center);

    container(
        column![
            row![
                text("Customize workspace")
                    .size(15)
                    .font(theme::display_semibold(t.design))
                    .color(theme::rgb(p.ink)),
                Space::new().width(8),
                text("Instant live preview · saved automatically")
                    .size(11.5)
                    .font(font)
                    .color(theme::rgb(p.ink_3)),
                Space::new().width(Length::Fill),
                button(
                    text("Done")
                        .size(11.5)
                        .font(theme::ui_semibold(t.design)),
                )
                .padding([5, 13])
                .style(theme::btn_primary(t))
                .on_press(Message::ToggleCustomizeDrawer),
            ]
            .align_y(Alignment::Center),
            row![
                column![
                    text("LIVE ACCENT COLOUR")
                        .size(10.5)
                        .font(theme::ui_semibold(t.design))
                        .color(theme::rgb(p.ink_3)),
                    accent_row,
                ]
                .spacing(6)
                .width(Length::FillPortion(5)),
                column![
                    text("RIG VISUAL STYLE")
                        .size(10.5)
                        .font(theme::ui_semibold(t.design))
                        .color(theme::rgb(p.ink_3)),
                    container(rig_rail)
                        .padding(2)
                        .style(theme::segmented_rail_style(t)),
                ]
                .spacing(6)
                .width(Length::FillPortion(4)),
                column![
                    text("GPU POWER MODE")
                        .size(10.5)
                        .font(theme::ui_semibold(t.design))
                        .color(theme::rgb(p.ink_3)),
                    container(fps_rail)
                        .padding(2)
                        .style(theme::segmented_rail_style(t)),
                ]
                .spacing(6)
                .width(Length::FillPortion(4)),
            ]
            .spacing(12)
            .align_y(Alignment::End),
            toggles_row,
            row![
                column![
                    text("OVERVIEW TILES")
                        .size(10.5)
                        .font(theme::ui_semibold(t.design))
                        .color(theme::rgb(p.ink_3)),
                    tile_row,
                ]
                .spacing(5)
                .width(Length::FillPortion(7)),
                column![
                    text("HEADER PINNED SENSORS")
                        .size(10.5)
                        .font(theme::ui_semibold(t.design))
                        .color(theme::rgb(p.ink_3)),
                    pin_row,
                ]
                .spacing(5)
                .width(Length::FillPortion(5)),
            ]
            .spacing(12),
        ]
        .spacing(10),
    )
    .padding(t.pad(14))
    .width(Length::Fill)
    .style(theme::drawer_panel_style(t))
    .into()
}

/// Sleek floating bottom-right Toast notification pill (auto-fades after 3.5s).
pub fn view_toast_overlay<'a>(
    message: &'a str,
    accent: u32,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let pill = container(
        row![
            status_dot(accent, 8.0),
            text(message)
                .size(12.5)
                .font(theme::ui_semibold(t.design))
                .color(theme::rgb(t.p.ink)),
            button(text("×").size(12).font(font))
                .padding([2, 6])
                .style(theme::btn_ghost(t, false))
                .on_press(Message::DismissToast),
        ]
        .spacing(9)
        .align_y(Alignment::Center),
    )
    .padding([9, 15])
    .style(theme::toast_pill_style(t, accent));

    container(
        column![
            Space::new().height(Length::Fill),
            row![Space::new().width(Length::Fill), pill],
        ]
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .padding([20, 24])
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::palette_match_score;

    #[test]
    fn palette_matcher_matches_pink_cleanly_without_scattered_noise() {
        assert!(
            palette_match_score("pink", "Accent: Rose Pink", "Accent", "Live accent")
                .is_some()
        );
        assert!(
            palette_match_score("pink", "Look: Firefly Pink", "Look", "Firefly · Living rig")
                .is_some()
        );
        assert!(
            palette_match_score("pink", "Design: Spore", "Design", "Warm espresso and oyster pink")
                .is_some()
        );
        // Must NOT match unrelated designs via scattered p-i-n-k letters
        assert!(
            palette_match_score("pink", "Design: Paper", "Design", "Daylight paper and dark ink")
                .is_none()
        );
        assert!(
            palette_match_score(
                "pink",
                "Design: Darkroom",
                "Design",
                "Print-black with amber safelight"
            )
            .is_none()
        );
        assert!(
            palette_match_score(
                "pink",
                "Design: Graphite",
                "Design",
                "Charcoal workshop, warm ivory and brass"
            )
            .is_none()
        );
        // Multi-word prefix search works
        assert!(
            palette_match_score("firefly p", "Look: Firefly Pink", "Look", "Firefly")
                .is_some()
        );
    }
}
