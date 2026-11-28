//! Instant Command Palette (`Ctrl+K`), Frosted Keyboard Shortcuts Sheet (`?` / `Ctrl+/`),
//! Overview Customize Drawer, and Floating Toast Overlay — upgraded with crisp `UiIcon`
//! vectors, tactile milled pills, and `Ctrl + / Ctrl - / Ctrl 0` UI Zoom controls.

use airglow_core::config::{ACCENT_SWATCHES, DesignId, RigStyle, UI_SCALE_PRESETS};
use airglow_core::effects::{EffectId, HARMONIES};
use iced::widget::{
    Space, button, column, container, mouse_area, row, scrollable, text, text_input,
};
use iced::{Alignment, Element, Length};

use crate::canvas_ambient::{UiIcon, icon_canvas};
use crate::theme::{self, DesignTokens};
use crate::views::{color_dot, pill_toggle, preset_dot_color, status_dot};
use crate::{AirglowApp, Message, Tab};

pub const PALETTE_INPUT_ID: &str = "airglow_command_palette_input";

#[derive(Debug, Clone)]
pub struct PaletteItem {
    pub title: String,
    pub subtitle: String,
    pub category: &'static str,
    pub icon: UiIcon,
    pub swatch: Option<[u8; 3]>,
    pub message: Message,
}

fn tab_ui_icon(tab: Tab) -> UiIcon {
    match tab {
        Tab::Overview => UiIcon::Overview,
        Tab::Lighting => UiIcon::Lighting,
        Tab::Studio => UiIcon::Studio,
        Tab::Fans => UiIcon::Fans,
        Tab::Automations => UiIcon::Automations,
        Tab::Devices => UiIcon::Devices,
        Tab::Settings => UiIcon::Settings,
    }
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
    let mut all = Vec::with_capacity(112);

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
        icon: UiIcon::Power,
        swatch: Some(if lights_on {
            [110, 100, 120]
        } else {
            [255, 195, 139]
        }),
        message: Message::TogglePower,
    });

    all.push(PaletteItem {
        title: if app.is_fullscreen {
            "Exit Fullscreen (F11)".to_string()
        } else {
            "Toggle Fullscreen (F11)".to_string()
        },
        subtitle: "Switch between crisp windowed mode and true fullscreen".to_string(),
        category: "Viewport",
        icon: UiIcon::Expand,
        swatch: Some([165, 222, 206]),
        message: Message::ToggleFullscreen,
    });

    all.push(PaletteItem {
        title: if app.theater_rig {
            "Exit Theater Rig Mode (Shift+F11)".to_string()
        } else {
            "Theater Rig Mode (Shift+F11)".to_string()
        },
        subtitle: "Cinematic full-viewport Living Rig with floating CPU/GPU/Power HUD".to_string(),
        category: "Viewport",
        icon: UiIcon::Lighting,
        swatch: Some([255, 195, 139]),
        message: Message::ToggleTheaterRig,
    });

    all.push(PaletteItem {
        title: if app.edit_tiles_mode {
            "Finish Editing Dashboard Tiles".to_string()
        } else {
            "Customize Dashboard Tiles".to_string()
        },
        subtitle: "Drag-and-drop reorder, corner-drag resize, or restyle Overview Bento tiles"
            .to_string(),
        category: "Workshop",
        icon: UiIcon::Customize,
        swatch: Some([244, 143, 177]),
        message: Message::ToggleEditTilesMode,
    });

    all.push(PaletteItem {
        title: if app.snapshot.config.appearance.overview_stage_animated {
            "Pause Overview Lighting Canvas Animation".to_string()
        } else {
            "Resume Overview Lighting Canvas Animation".to_string()
        },
        subtitle: "Switch Overview rig preview between Live animation and low-GPU Static mode"
            .to_string(),
        category: "Performance",
        icon: UiIcon::Sparkle,
        swatch: Some([165, 222, 206]),
        message: Message::ToggleOverviewStageAnimation,
    });

    all.push(PaletteItem {
        title: "Add Overview Tile".to_string(),
        subtitle: "Open the Tile Catalog (Controls, 12 Sensors & live LHM sensors)".to_string(),
        category: "Workshop",
        icon: UiIcon::Plus,
        swatch: Some([197, 155, 255]),
        message: Message::OpenAddTileFromPalette,
    });

    all.push(PaletteItem {
        title: "Check for Updates".to_string(),
        subtitle: format!(
            "Check GitHub Releases for newer Airglow builds (v{})",
            airglow_core::updates::current_version()
        ),
        category: "Updates",
        icon: UiIcon::Refresh,
        swatch: Some([107, 203, 247]),
        message: Message::CheckForUpdates,
    });

    all.push(PaletteItem {
        title: "Keyboard Shortcuts".to_string(),
        subtitle: "View all quick keys & UI Zoom shortcuts (? or Ctrl+/)".to_string(),
        category: "Action",
        icon: UiIcon::Sparkle,
        swatch: Some([209, 185, 240]),
        message: Message::ToggleShortcuts,
    });

    all.push(PaletteItem {
        title: if app.compact_window {
            "Expand Full Airglow Window".to_string()
        } else {
            "Switch to Compact Mini-Mode".to_string()
        },
        subtitle: "430×404 desktop companion widget (Ctrl+M)".to_string(),
        category: "Viewport",
        icon: UiIcon::Minimize,
        swatch: Some([244, 143, 177]),
        message: Message::ToggleCompactMode,
    });

    all.push(PaletteItem {
        title: if app.customize_open {
            "Close Control Center Drawer".to_string()
        } else {
            "Open Control Center Drawer".to_string()
        },
        subtitle: "Accent colour, UI scale, rig style, FPS power mode & header pins".to_string(),
        category: "Action",
        icon: UiIcon::Sliders,
        swatch: Some([244, 143, 177]),
        message: Message::OpenCustomizeFromPalette,
    });

    all.push(PaletteItem {
        title: "Surprise Me (Studio)".to_string(),
        subtitle: "Generate a fresh multi-layer Studio lighting scene".to_string(),
        category: "Studio",
        icon: UiIcon::Studio,
        swatch: Some([197, 155, 255]),
        message: Message::StudioSurpriseMe,
    });

    // UI Scale / HiDPI Zoom commands (`Ctrl + / Ctrl - / Ctrl 0`)
    for &(pct, raw_lbl) in UI_SCALE_PRESETS {
        let label = if pct == 0 {
            "UI Scale: Auto (HiDPI) — Ctrl+0".to_string()
        } else {
            format!("UI Scale: {raw_lbl} Zoom")
        };
        let sub = if pct == 0 {
            "Automatic 1.25× on 1440p & 1.50× on 4K displays".to_string()
        } else {
            "Set manual crisp vector UI scale factor (Ctrl + / Ctrl -)".to_string()
        };
        all.push(PaletteItem {
            title: label,
            subtitle: sub,
            category: "Scale",
            icon: UiIcon::Expand,
            swatch: None,
            message: Message::SetUiScalePct(pct),
        });
    }

    if let Some(rem) = app.snapshot.sleep_timer_remaining_s {
        all.push(PaletteItem {
            title: "Cancel Sleep Timer".to_string(),
            subtitle: format!("Counting down ({}m {:02}s left)", rem / 60, rem % 60),
            category: "Sleep",
            icon: UiIcon::Reset,
            swatch: Some([255, 138, 117]),
            message: Message::CancelSleepTimer,
        });
    }
    for mins in [15u16, 30, 60] {
        all.push(PaletteItem {
            title: format!("Sleep Timer: {mins} min"),
            subtitle: "Smoothly dim over the final 2 minutes, then turn lights off".to_string(),
            category: "Sleep",
            icon: UiIcon::Power,
            swatch: Some([209, 185, 240]),
            message: Message::StartSleepTimer(mins),
        });
    }

    all.push(PaletteItem {
        title: "Rescan Hardware".to_string(),
        subtitle: "Refresh USB HID controllers, OpenRGB SDK & LHM fans".to_string(),
        category: "Devices",
        icon: UiIcon::Refresh,
        swatch: Some([165, 222, 206]),
        message: Message::RescanHardware,
    });

    // Tabs (all 7)
    for (idx, tab) in Tab::ALL.iter().enumerate() {
        all.push(PaletteItem {
            title: format!("Go to {}", tab.label()),
            subtitle: format!("Switch workspace view (shortcut {})", idx + 1),
            category: "Tab",
            icon: tab_ui_icon(*tab),
            swatch: None,
            message: Message::SelectTab(*tab),
        });
    }

    // Settings Sections
    for sec in crate::SettingsSection::ALL {
        all.push(PaletteItem {
            title: format!("Settings: {}", sec.label()),
            subtitle: sec.subtitle().to_string(),
            category: "Settings",
            icon: UiIcon::Settings,
            swatch: None,
            message: Message::JumpSettingsSection(sec),
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
            icon: UiIcon::Customize,
            swatch: Some(swatch),
            message: Message::ApplyLook(look.id.clone()),
        });
    }

    // Profiles
    for preset in &app.snapshot.config.presets {
        all.push(PaletteItem {
            title: format!("Profile: {}", preset.name),
            subtitle: format!("{} · {}% brightness", preset.effect.label(), preset.brightness),
            category: "Profile",
            icon: UiIcon::Sparkle,
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
            icon: UiIcon::Lighting,
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
            icon: UiIcon::Sparkle,
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
            icon: UiIcon::Fans,
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
            icon: UiIcon::Customize,
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
            icon: UiIcon::Sparkle,
            swatch: Some(*rgb),
            message: Message::SetAccentOverride(Some(*rgb)),
        });
    }
    all.push(PaletteItem {
        title: "Accent: Design Default".to_string(),
        subtitle: "Reset accent colour override".to_string(),
        category: "Accent",
        icon: UiIcon::Reset,
        swatch: None,
        message: Message::SetAccentOverride(None),
    });

    if app.palette_query.trim().is_empty() {
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
        container(icon_canvas(UiIcon::Search, theme::rgb(p.brass_hi), 17.0))
            .padding(6)
            .style(theme::chip_style(t, Some(p.brass))),
        text_input(
            "Jump to tab, effect, UI scale, design, harmony, profile, or sleep timer…",
            &app.palette_query
        )
        .id(iced::widget::Id::new(PALETTE_INPUT_ID))
        .size(14)
        .padding([9, 12])
        .style(theme::text_input_style(t))
        .on_input(Message::PaletteQueryChanged)
        .on_submit(Message::PaletteExecuteSelected)
        .width(Length::Fill),
        button(text("ESC").size(10.5).font(mono))
            .padding([5, 10])
            .style(theme::shortcut_badge_btn(t, false))
            .on_press(Message::CloseCommandPalette),
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
            let icon_col = if selected {
                theme::rgb(p.brass_hi)
            } else {
                theme::rgb(p.ink_2)
            };

            let mut r = row![
                container(icon_canvas(item.icon, icon_col, 15.0))
                    .padding(5)
                    .style(theme::chip_style(
                        t,
                        if selected { Some(p.brass) } else { None }
                    )),
            ]
            .spacing(10)
            .align_y(Alignment::Center);

            if let Some(rgb) = item.swatch {
                r = r.push(color_dot(rgb, 9.5));
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
                    .padding([3, 9])
                    .style(theme::chip_style(
                        t,
                        if selected { Some(p.brass_hi) } else { None },
                    )),
                );
            if selected {
                r = r.push(
                    container(
                        text("↵")
                            .size(11)
                            .font(mono)
                            .color(theme::rgb(p.brass_hi)),
                    )
                    .padding([2, 7])
                    .style(theme::chip_style(t, Some(p.brass))),
                );
            }

            list_col = list_col.push(
                button(r)
                    .padding([7, 12])
                    .width(Length::Fill)
                    .height(Length::Fixed(50.0))
                    .style(theme::palette_row_btn(t, selected))
                    .on_press(Message::PaletteExecuteIndex(idx)),
            );
        }
    }

    let visible_rows = items.len().clamp(1, 7);
    let list_height = if items.is_empty() {
        76.0
    } else {
        (visible_rows as f32) * 50.0 + ((visible_rows.saturating_sub(1)) as f32) * 4.0
    };

    let footer = row![
        text("↑↓ Navigate  ·  ↵ Run  ·  Ctrl +/-/0 UI Zoom  ·  Esc Close")
            .size(11)
            .font(font)
            .color(theme::rgb(p.ink_3)),
        Space::new().width(Length::Fill),
        container(
            text(format!("{} cmd(s)", items.len()))
                .size(10.5)
                .font(mono)
                .color(theme::rgb(p.ink_3)),
        )
        .padding([2, 8])
        .style(theme::chip_style(t, None)),
    ]
    .align_y(Alignment::Center);

    let divider = container(Space::new().width(Length::Fill).height(1)).style(move |_| {
        iced::widget::container::Style {
            background: Some(iced::Background::Color(theme::rgba(t.line_soft, 0.75))),
            ..Default::default()
        }
    });

    let modal_w = if app.compact_window { 404.0 } else { 640.0 };
    let top_pad = if app.compact_window { 24.0 } else { 68.0 };
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

/// Frosted 2-column Keyboard Shortcuts modal (`?` or `Ctrl+/`), documenting UI Zoom (`Ctrl + / - / 0`).
pub fn view_shortcuts_modal<'a>(t: DesignTokens, font: iced::Font) -> Element<'a, Message> {
    let p = t.p;
    let mono = theme::mono_font();

    let shortcut_row = |keys: &'static str, desc: &'static str| {
        container(
            row![
                container(
                    text(keys)
                        .size(11)
                        .font(mono)
                        .color(theme::rgb(p.brass_hi)),
                )
                .padding([3, 9])
                .style(theme::chip_style(t, Some(p.brass))),
                Space::new().width(Length::Fill),
                text(desc).size(12).font(font).color(theme::rgb(p.ink)),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding([6, 10])
        .width(Length::Fill)
        .style(theme::well_style(t))
    };

    let left_col = column![
        row![
            icon_canvas(UiIcon::Expand, theme::rgb(t.pink_rose), 13.5),
            text("NAVIGATION & VIEWPORT")
                .size(10.5)
                .font(theme::ui_semibold(t.design))
                .color(theme::rgb(t.pink_rose)),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
        shortcut_row("1 – 7  /  Ctrl+1–7", "Switch workspace tabs"),
        shortcut_row("Ctrl +  /  Ctrl -", "Zoom UI scale up / down"),
        shortcut_row("Ctrl + 0", "Reset UI scale to Auto (HiDPI)"),
        shortcut_row("F11", "Toggle Fullscreen"),
        shortcut_row("Shift + F11", "Theater Rig mode"),
        shortcut_row("Ctrl + M", "Compact Mini-Mode"),
    ]
    .spacing(8)
    .width(Length::Fill);

    let right_col = column![
        row![
            icon_canvas(UiIcon::Sparkle, theme::rgb(t.data_primary), 13.5),
            text("COMMAND PALETTE & STUDIO")
                .size(10.5)
                .font(theme::ui_semibold(t.design))
                .color(theme::rgb(t.data_primary)),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
        shortcut_row("Ctrl + K", "Open Command Palette"),
        shortcut_row("?  /  Ctrl + /", "Keyboard Shortcuts sheet"),
        shortcut_row("↑  /  ↓  /  Enter", "Navigate & run command"),
        shortcut_row("Ctrl + Z", "Studio Undo (30 steps)"),
        shortcut_row("Ctrl + Y  /  ⇧⌘Z", "Studio Redo"),
        shortcut_row("Ctrl+Alt+Shift+L / P / A", "Global Lights / Profile / Open"),
        shortcut_row("Esc", "Close active overlay"),
    ]
    .spacing(8)
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
                    container(icon_canvas(UiIcon::Sparkle, theme::rgb(p.brass_hi), 18.0))
                        .padding(7)
                        .style(theme::chip_style(t, Some(p.brass))),
                    column![
                        text("Keyboard Shortcuts")
                            .size(20)
                            .font(theme::display_semibold(t.design))
                            .color(theme::rgb(p.ink)),
                        text("Instant hands-on-keyboard control & HiDPI viewport zoom across Airglow.")
                            .size(11.5)
                            .font(font)
                            .color(theme::rgb(p.ink_3)),
                    ]
                    .spacing(2)
                    .width(Length::Fill),
                    button(text("ESC").size(10.5).font(mono))
                        .padding([5, 11])
                        .style(theme::shortcut_badge_btn(t, false))
                        .on_press(Message::ToggleShortcuts),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
                divider,
                row![left_col, right_col].spacing(14),
            ]
            .spacing(14),
        )
        .padding(20)
        .width(Length::Fixed(650.0))
        .style(theme::palette_modal_style(t)),
    )
    .on_press(Message::Noop);

    mouse_area(
        container(
            column![
                Space::new().height(Length::Fixed(92.0)),
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

/// Interactive collapsible Control Center / Customize Drawer right on Overview.
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
            button(Space::new().width(20).height(20))
                .padding(0)
                .width(Length::Fixed(23.0))
                .height(Length::Fixed(23.0))
                .style(theme::swatch_btn(*rgb, selected, t))
                .on_press(Message::SetAccentOverride(Some(*rgb))),
        );
    }
    accent_row = accent_row.push(
        button(text("Default").size(11).font(font))
            .padding([4, 10])
            .style(theme::btn_secondary(t, appr.accent_override.is_none()))
            .on_press(Message::SetAccentOverride(None)),
    );

    // UI Scale segmented rail
    let mut scale_rail = row![].spacing(2);
    for &(pct, lbl) in UI_SCALE_PRESETS {
        scale_rail = scale_rail.push(
            button(text(lbl).size(11).font(font))
                .padding([4, 9])
                .style(theme::segmented_pill_style(t, appr.ui_scale_pct == pct))
                .on_press(Message::SetUiScalePct(pct)),
        );
    }

    // Rig Style segmented rail
    let mut rig_rail = row![].spacing(2);
    for style in RigStyle::ALL {
        rig_rail = rig_rail.push(
            button(text(style.label()).size(11).font(font))
                .padding([4, 9])
                .style(theme::segmented_pill_style(t, appr.rig_style == style))
                .on_press(Message::SetRigStyle(style)),
        );
    }

    // GPU Power Mode segmented rail
    let mut fps_rail = row![].spacing(2);
    for (fps, label) in [(15u8, "15fps"), (30, "30fps"), (60, "60fps")] {
        fps_rail = fps_rail.push(
            button(text(label).size(11).font(font))
                .padding([4, 9])
                .style(theme::segmented_pill_style(t, appr.ui_fps_cap == fps))
                .on_press(Message::SetUiFpsCap(fps)),
        );
    }

    // Dashboard Tile Workshop quick actions
    let tile_row = row![
        button(
            row![
                icon_canvas(UiIcon::Customize, theme::rgb(p.brass_hi), 12.5),
                text(if app.edit_tiles_mode {
                    "Done editing"
                } else {
                    "Edit tiles"
                })
                .size(11.5)
                .font(theme::ui_semibold(t.design)),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
        )
        .padding([5, 11])
        .style(theme::btn_secondary(t, app.edit_tiles_mode))
        .on_press(Message::ToggleEditTilesMode),
        button(
            row![
                icon_canvas(UiIcon::Plus, theme::rgb(t.primary_ink), 12.0),
                text("Add tile")
                    .size(11.5)
                    .font(theme::ui_semibold(t.design)),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
        )
        .padding([5, 12])
        .style(theme::btn_primary(t))
        .on_press(Message::ToggleAddTileCatalog),
        button(
            row![
                icon_canvas(UiIcon::Expand, theme::rgb(p.ink_2), 12.0),
                text("Theater").size(11.5).font(font),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
        )
        .padding([5, 11])
        .style(theme::btn_secondary(t, app.theater_rig))
        .on_press(Message::ToggleTheaterRig),
        button(
            text(if app.snapshot.config.fullscreen_fill_width {
                "✓ Full-bleed width"
            } else {
                "Fluid centered"
            })
            .size(11.5)
            .font(font),
        )
        .padding([5, 11])
        .style(theme::btn_secondary(
            t,
            app.snapshot.config.fullscreen_fill_width
        ))
        .on_press(Message::SetFullscreenFillWidth(
            !app.snapshot.config.fullscreen_fill_width
        )),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

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
            text("Motes").size(11.5).font(font),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
        row![
            pill_toggle(
                t,
                appr.overview_stage_animated,
                Message::ToggleOverviewStageAnimation
            ),
            text("Overview canvas").size(11.5).font(font),
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
                icon_canvas(UiIcon::Sliders, theme::rgb(p.brass_hi), 15.0),
                text("Control Center")
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
                    row![
                        icon_canvas(UiIcon::Check, theme::rgb(t.primary_ink), 12.0),
                        text("Done")
                            .size(11.5)
                            .font(theme::ui_semibold(t.design)),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding([5, 13])
                .style(theme::btn_primary(t))
                .on_press(Message::ToggleCustomizeDrawer),
            ]
            .spacing(7)
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
                    text("UI SCALE")
                        .size(10.5)
                        .font(theme::ui_semibold(t.design))
                        .color(theme::rgb(p.ink_3)),
                    container(scale_rail)
                        .padding(2)
                        .style(theme::segmented_track_style(t)),
                ]
                .spacing(6)
                .width(Length::FillPortion(4)),
                column![
                    text("RIG STYLE & FPS")
                        .size(10.5)
                        .font(theme::ui_semibold(t.design))
                        .color(theme::rgb(p.ink_3)),
                    row![
                        container(rig_rail)
                            .padding(2)
                            .style(theme::segmented_track_style(t)),
                        container(fps_rail)
                            .padding(2)
                            .style(theme::segmented_track_style(t)),
                    ]
                    .spacing(6),
                ]
                .spacing(6)
                .width(Length::FillPortion(5)),
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
        assert!(
            palette_match_score("firefly p", "Look: Firefly Pink", "Look", "Firefly")
                .is_some()
        );
        assert!(
            palette_match_score("125%", "UI Scale: 125% Zoom", "Scale", "HiDPI")
                .is_some()
        );
    }
}
