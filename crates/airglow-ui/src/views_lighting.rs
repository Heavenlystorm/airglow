//! Full-viewport Widescreen Lighting Atelier (Interactive Rig Hero Stage with clickable Zone
//! Filter Pill Strip, taller 4×4 live-animated EffectThumbCanvas gallery, 36px multi-stop
//! Colour Harmony pills, vivid Saved Studio Look cards, 2-column Visual Quick Profiles Grid,
//! and Luminous 164px ColorWheel + pill-badged Effect Inspector) plus Studio workbench fallback.

use airglow_core::config::TempSource;
use airglow_core::effects::{BlendMode, EffectId, HARMONIES, LayerKind};
use iced::widget::{
    Space, button, checkbox, column, container, row, scrollable, slider, text, text_input,
};
use iced::{Alignment, Element, Length};

use crate::canvas_ambient::{AudioVisualizerCanvas, UiIcon, icon_canvas};
use crate::canvas_colorwheel::{ColorWheelCanvas, ColorWheelMessage};
use crate::canvas_stage::{CaseStageCanvas, StageMessage};
use crate::canvas_widgets::EffectThumbCanvas;
use crate::theme::{self, DesignTokens};
use crate::views::{
    CURATED_SWATCHES, color_dot, pill_toggle, preset_dot_color, truncate_str,
};
use crate::{AirglowApp, Message};

/// Illuminated hardware slider with a crisp tactile pill value badge on the right.
#[allow(clippy::too_many_arguments)]
fn inspector_pill_slider<'a>(
    t: DesignTokens,
    _font: iced::Font,
    label: &'a str,
    value_label: String,
    min: u16,
    max: u16,
    value: u16,
    accent: Option<u32>,
    on_change: impl Fn(u16) -> Message + 'a,
) -> Element<'a, Message> {
    let p = t.p;
    let badge_col = accent.unwrap_or(p.brass_hi);
    column![
        row![
            text(label)
                .size(12.5)
                .font(theme::ui_semibold(t.design))
                .color(theme::rgb(p.ink_2)),
            Space::new().width(Length::Fill),
            container(
                text(value_label)
                    .size(11)
                    .font(theme::mono_font())
                    .color(theme::rgb(badge_col)),
            )
            .padding([2, 9])
            .style(theme::chip_style(t, Some(accent.unwrap_or(p.brass)))),
        ]
        .align_y(Alignment::Center),
        slider(min as f32..=max as f32, value as f32, move |v| {
            on_change(v.round() as u16)
        })
        .style(theme::slider_style(t)),
    ]
    .spacing(5)
    .width(Length::Fill)
    .into()
}

/// Samples a vivid representative live RGB dot color for a zone from the engine's current frame.
fn live_zone_dot_rgb(app: &AirglowApp, zone_id: &str, fallback: [u8; 3]) -> [u8; 3] {
    if let Some(frame) = app.snapshot.frames.iter().find(|f| f.zone_id == zone_id)
        && !frame.colors.is_empty()
    {
        let mut best = frame.colors[0];
        let mut best_sum = 0u32;
        let mut r_acc = 0u32;
        let mut g_acc = 0u32;
        let mut b_acc = 0u32;
        for &c in &frame.colors {
            let s = c[0] as u32 + c[1] as u32 + c[2] as u32;
            if s > best_sum {
                best_sum = s;
                best = c;
            }
            r_acc += c[0] as u32;
            g_acc += c[1] as u32;
            b_acc += c[2] as u32;
        }
        let n = frame.colors.len() as u32;
        let avg = [(r_acc / n) as u8, (g_acc / n) as u8, (b_acc / n) as u8];
        if best_sum > 48 {
            return [
                ((avg[0] as u16 + best[0] as u16) / 2) as u8,
                ((avg[1] as u16 + best[1] as u16) / 2) as u8,
                ((avg[2] as u16 + best[2] as u16) / 2) as u8,
            ];
        }
    }
    fallback
}

/// Produces a clean, compact zone pill label (collapsing verbose sub-zone suffixes).
fn compact_zone_pill_name(raw: &str) -> String {
    let base = raw.split(':').next().unwrap_or(raw).trim();
    let cleaned = base
        .replace(" Gaming OC", "")
        .replace(" RGB Core", "")
        .replace(" RGB", "")
        .replace("header ", "");
    truncate_str(cleaned.trim(), 14)
}

/// Multi-stop gradient swatch pills (`height: 36px` tactile buttons) for 1-click Colour Harmonies.
fn harmony_palette_grid<'a>(
    app: &'a AirglowApp,
    t: DesignTokens,
    _font: iced::Font,
    per_row: usize,
) -> Element<'a, Message> {
    let p = t.p;
    let mut grid = column![].spacing(5);
    for (row_idx, chunk) in HARMONIES.chunks(per_row).enumerate() {
        let mut r = row![].spacing(6);
        for (col_idx, h) in chunk.iter().enumerate() {
            let idx = row_idx * per_row + col_idx;
            let grad_strip = iced::widget::canvas(
                EffectThumbCanvas {
                    effect: EffectId::Gradient,
                    color: h.colors[0],
                    secondary: h.colors[2],
                    palette: p,
                    accent_override: t.accent_override,
                    anim_t: app.anim_t,
                    selected: false,
                }
                .with_custom_colors(h.colors[0], h.colors[2]),
            )
            .width(Length::Fixed(30.0))
            .height(Length::Fixed(16.0));

            let swatch_badge = container(grad_strip).clip(true).style(move |_| {
                iced::widget::container::Style {
                    border: iced::Border {
                        color: iced::Color::from_rgba(1.0, 1.0, 1.0, 0.18),
                        width: 1.0,
                        radius: iced::border::Radius::from(if p.square_corners {
                            0.0
                        } else {
                            5.0
                        }),
                    },
                    ..Default::default()
                }
            });

            r = r.push(
                button(
                    row![
                        swatch_badge,
                        color_dot(h.colors[1], 7.0),
                        text(truncate_str(h.name, 14))
                            .size(11.5)
                            .font(theme::ui_semibold(t.design))
                            .color(theme::rgb(p.ink)),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding([5, 9])
                .height(Length::Fixed(34.0))
                .width(Length::Fill)
                .style(theme::card_button(t, false))
                .on_press(Message::ApplyHarmony(idx)),
            );
        }
        for _ in chunk.len()..per_row {
            r = r.push(Space::new().width(Length::Fill));
        }
        grid = grid.push(r);
    }
    grid.into()
}

pub fn view_lighting<'a>(
    app: &'a AirglowApp,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let mono = theme::mono_font();
    let eff = &app.snapshot.effective_scene;
    let scene = &app.snapshot.config.scene;
    let appr = &app.snapshot.config.appearance;
    let unit = app.snapshot.config.settings.temp_unit;

    let window_h = app.window_size.height.max(760.0);
    let stage_h = (window_h * 0.25).clamp(216.0, 286.0);
    let (active_effect, active_params) = app.active_lighting_target();

    // -----------------------------------------------------------------------
    // 1. LEFT/CENTER MAIN STAGE (`FillPortion(7)`)
    // -----------------------------------------------------------------------
    let stage = iced::widget::canvas(CaseStageCanvas {
        frames: &app.snapshot.frames,
        zone_overrides: &scene.zone_overrides,
        palette: p,
        accent_override: t.accent_override,
        rig_style: appr.rig_style,
        airflow_lines: appr.airflow_lines,
        spinning_fans: appr.spinning_fans,
        ambient_bloom: appr.ambient_bloom,
        lights_on: eff.on,
        show_labels: true,
        arrange_mode: app.arrange_mode,
        selected_zone: app.selected_zone.as_deref(),
        active_effect: eff.effect,
        temp_unit: unit,
        anim_t: app.anim_t,
        fan_duty: app.snapshot.fan_duty,
        cpu_temp: app.snapshot.sensors.cpu_temp,
        gpu_temp: app.snapshot.sensors.gpu_temp,
        ambient_rgb: app.snapshot.average_rgb,
        eyebrow: None,
        animate_lighting: true,
    })
    .width(Length::Fill)
    .height(Length::Fixed(stage_h));
    let stage_el = Element::from(stage).map(Message::Stage);

    let active_zones = app.snapshot.frames.iter().filter(|f| f.enabled).count();
    let total_leds: usize = app
        .snapshot
        .frames
        .iter()
        .filter(|f| f.enabled)
        .map(|f| f.colors.len())
        .sum();

    let arrange_icon_col = if app.arrange_mode {
        theme::rgb(t.primary_ink)
    } else {
        theme::rgb(p.brass_hi)
    };
    let arrange_btn = button(
        row![
            icon_canvas(UiIcon::Customize, arrange_icon_col, 14.0),
            text(if app.arrange_mode {
                "Done Arranging"
            } else {
                "Arrange"
            })
            .size(12.5)
            .font(theme::ui_semibold(t.design)),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .padding([6, 14])
    .style(move |th, st| {
        if app.arrange_mode {
            theme::btn_primary(t)(th, st)
        } else {
            theme::btn_secondary(t, false)(th, st)
        }
    })
    .on_press(Message::ToggleArrange);

    let mut stage_header = row![
        icon_canvas(UiIcon::Lighting, theme::rgb(p.brass_hi), 18.0),
        text("Lighting Stage")
            .size(18)
            .font(theme::display_semibold(t.design))
            .color(theme::rgb(p.ink)),
        container(
            text(format!(
                "{active_zones} zones · {total_leds} LEDs · {:.0} FPS",
                app.snapshot.fps.max(1.0)
            ))
            .size(10.5)
            .font(mono)
            .color(theme::rgb(t.data_primary)),
        )
        .padding([3, 9])
        .style(theme::chip_style(t, Some(t.data_primary))),
        Space::new().width(Length::Fill),
    ]
    .spacing(9)
    .align_y(Alignment::Center);

    if let Some(zone_cfg) = app.selected_zone_cfg() {
        let is_flashing = app.snapshot.identify_zone.as_deref() == Some(zone_cfg.id.as_str());
        let has_override = scene.zone_overrides.contains_key(&zone_cfg.id);
        stage_header = stage_header.push(
            button(
                row![
                    icon_canvas(UiIcon::Sparkle, theme::rgb(p.brass_hi), 13.0),
                    text(if is_flashing { "Blinking…" } else { "Identify" })
                        .size(11.5)
                        .font(font),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .padding([5, 11])
            .style(theme::btn_secondary(t, is_flashing))
            .on_press(Message::IdentifyZone(zone_cfg.id.clone())),
        );
        if has_override {
            stage_header = stage_header.push(
                button(text("Clear Override").size(11.5).font(font))
                    .padding([5, 11])
                    .style(theme::btn_danger(t))
                    .on_press(Message::ClearZoneOverride(zone_cfg.id.clone())),
            );
        }
    } else if !scene.zone_overrides.is_empty() {
        stage_header = stage_header.push(
            button(
                row![
                    icon_canvas(UiIcon::Reset, theme::rgb(p.ink_2), 12.5),
                    text(format!("Clear {} override(s)", scene.zone_overrides.len()))
                        .size(11.5)
                        .font(font),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .padding([5, 11])
            .style(theme::btn_secondary(t, false))
            .on_press(Message::ClearAllZoneOverrides),
        );
    }
    stage_header = stage_header.push(arrange_btn);

    // Clickable Zone Filter Pill Strip (`Whole Rig` + compact pills for key zones with live RGB dots)
    let whole_rig_active = app.selected_zone.is_none();
    let mut zone_strip = row![
        button(
            row![
                color_dot(app.snapshot.average_rgb, 8.5),
                text("Whole Rig")
                    .size(11.5)
                    .font(if whole_rig_active {
                        theme::ui_semibold(t.design)
                    } else {
                        font
                    }),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([5, 11])
        .style(theme::segmented_pill_style(t, whole_rig_active))
        .on_press(Message::SelectWholeRig),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let max_zone_pills = if app.window_size.width >= 1580.0 {
        9usize
    } else if app.window_size.width >= 1260.0 {
        7
    } else {
        5
    };
    let mut shown_labels: Vec<String> = Vec::with_capacity(max_zone_pills);
    for z in app.snapshot.config.zones.iter().filter(|z| z.enabled) {
        if shown_labels.len() >= max_zone_pills {
            break;
        }
        let short_lbl = compact_zone_pill_name(&z.name);
        let is_selected = app.selected_zone.as_deref() == Some(z.id.as_str());
        // Deduplicate multi-strip device sub-zones (unless this exact sub-zone is currently selected)
        if !is_selected && shown_labels.iter().any(|prev| prev == &short_lbl) {
            continue;
        }
        shown_labels.push(short_lbl.clone());

        let dot_rgb = live_zone_dot_rgb(app, &z.id, active_params.color);
        let has_ov = scene.zone_overrides.contains_key(&z.id);
        let zid = z.id.clone();
        let pill_text = if has_ov {
            format!("{short_lbl} •")
        } else {
            short_lbl
        };

        zone_strip = zone_strip.push(
            button(
                row![
                    color_dot(dot_rgb, 8.0),
                    text(pill_text).size(11.5).font(if is_selected {
                        theme::ui_semibold(t.design)
                    } else {
                        font
                    }),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .padding([5, 10])
            .style(theme::segmented_pill_style(t, is_selected))
            .on_press(Message::Stage(StageMessage::SelectZone(Some(zid)))),
        );
    }

    let zone_filter_bar = container(zone_strip)
        .padding(3)
        .width(Length::Fill)
        .style(theme::segmented_track_style(t));

    let rig_hero_card = container(
        column![
            stage_header,
            zone_filter_bar,
            container(stage_el)
                .width(Length::Fill)
                .clip(true)
                .style(theme::bento_stage_well_style(t)),
        ]
        .spacing(7),
    )
    .padding(t.pad(12))
    .width(Length::Fill)
    .style(theme::hero_panel_style(t));

    // Lush Live Effect Gallery (taller crisp artwork cards in a spacious 4-column grid)
    let effect_thumb_h = ((window_h - 860.0) * 0.03 + 38.0).clamp(38.0, 46.0);
    let mut effects_grid = column![].spacing(6);
    for chunk in EffectId::ALL.chunks(4) {
        let mut r = row![].spacing(7);
        for &e in chunk {
            let active = active_effect == e
                && (e != EffectId::Studio || scene.active_studio_effect.is_none());
            let thumb = iced::widget::canvas(EffectThumbCanvas {
                effect: e,
                color: active_params.color,
                secondary: active_params.secondary,
                palette: p,
                accent_override: t.accent_override,
                anim_t: app.anim_t,
                selected: active,
            })
            .width(Length::Fill)
            .height(Length::Fixed(effect_thumb_h));

            r = r.push(
                button(
                    column![
                        container(thumb).width(Length::Fill).clip(true).style(move |_| {
                            iced::widget::container::Style {
                                border: iced::Border {
                                    radius: iced::border::Radius::from(if p.square_corners {
                                        0.0
                                    } else {
                                        8.0
                                    }),
                                    ..Default::default()
                                },
                                ..Default::default()
                            }
                        }),
                        row![
                            text(e.label())
                                .size(13)
                                .font(theme::ui_semibold(t.design))
                                .color(theme::rgb(if active { p.brass_hi } else { p.ink })),
                        ]
                        .align_y(Alignment::Center),
                    ]
                    .spacing(4)
                    .align_x(Alignment::Center),
                )
                .padding([5, 6])
                .width(Length::Fill)
                .style(theme::card_button(t, active))
                .on_press(Message::SelectEffect(e)),
            );
        }
        effects_grid = effects_grid.push(r);
    }

    // Rich Colour Harmonies (2 rows of 5 multi-stop gradient buttons)
    let harmony_section = column![
        row![
            icon_canvas(UiIcon::Sparkle, theme::rgb(t.pink_rose), 13.0),
            text("COLOUR HARMONIES")
                .size(11)
                .font(theme::ui_semibold(t.design))
                .color(theme::rgb(t.pink_rose)),
            Space::new().width(Length::Fill),
            text("10 multi-stop studio blends")
                .size(11)
                .font(font)
                .color(theme::rgb(p.ink_3)),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
        harmony_palette_grid(app, t, font, 5),
    ]
    .spacing(5);

    let mut gallery_col = column![
        row![
            icon_canvas(UiIcon::Lighting, theme::rgb(p.brass_hi), 14.0),
            text("LIVE EFFECT GALLERY (16 ARTWORKS)")
                .size(11.5)
                .font(theme::ui_semibold(t.design))
                .color(theme::rgb(p.ink_2)),
            Space::new().width(Length::Fill),
            container(
                text("350ms hardware crossfade")
                    .size(10.5)
                    .font(mono)
                    .color(theme::rgb(p.ink_3)),
            )
            .padding([2, 8])
            .style(theme::chip_style(t, None)),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
        effects_grid,
        harmony_section,
    ]
    .spacing(7);

    // Vivid custom-colored Saved Studio Look cards
    if !app.snapshot.config.studio_effects.is_empty() {
        let mut studio_row = row![].spacing(7);
        let max_show = 4usize;
        for saved in app.snapshot.config.studio_effects.iter().take(max_show) {
            let active = active_effect == EffectId::Studio
                && scene.active_studio_effect.as_deref() == Some(saved.id.as_str())
                && app.selected_zone.is_none();
            let c1 = saved
                .stack
                .layers
                .first()
                .map(|l| l.color)
                .unwrap_or([165, 222, 206]);
            let c2 = saved
                .stack
                .layers
                .get(1)
                .map(|l| l.color)
                .or_else(|| saved.stack.layers.first().map(|l| l.secondary))
                .unwrap_or([242, 169, 196]);
            let thumb = iced::widget::canvas(
                EffectThumbCanvas {
                    effect: EffectId::Studio,
                    color: c1,
                    secondary: c2,
                    palette: p,
                    accent_override: t.accent_override,
                    anim_t: app.anim_t,
                    selected: active,
                }
                .with_custom_colors(c1, c2),
            )
            .width(Length::Fill)
            .height(Length::Fixed(32.0));

            studio_row = studio_row.push(
                button(
                    column![
                        container(thumb).width(Length::Fill).clip(true).style(move |_| {
                            iced::widget::container::Style {
                                border: iced::Border {
                                    radius: iced::border::Radius::from(if p.square_corners {
                                        0.0
                                    } else {
                                        7.0
                                    }),
                                    ..Default::default()
                                },
                                ..Default::default()
                            }
                        }),
                        row![
                            color_dot(c1, 7.5),
                            text(truncate_str(&saved.name, 16))
                                .size(12)
                                .font(theme::ui_semibold(t.design))
                                .color(theme::rgb(if active { p.brass_hi } else { p.ink })),
                            Space::new().width(Length::Fill),
                            text(format!("{}L", saved.stack.layers.len()))
                                .size(10)
                                .font(mono)
                                .color(theme::rgb(p.ink_3)),
                        ]
                        .spacing(5)
                        .align_y(Alignment::Center),
                    ]
                    .spacing(4),
                )
                .padding([5, 7])
                .width(Length::Fill)
                .style(theme::card_button(t, active))
                .on_press(Message::ApplyStudioEffect(saved.id.clone())),
            );
        }
        let count = app.snapshot.config.studio_effects.len().min(max_show);
        for _ in count..max_show {
            studio_row = studio_row.push(Space::new().width(Length::Fill));
        }
        gallery_col = gallery_col
            .push(
                row![
                    icon_canvas(UiIcon::Studio, theme::rgb(t.data_secondary), 13.0),
                    text("SAVED STUDIO LOOKS")
                        .size(11)
                        .font(theme::ui_semibold(t.design))
                        .color(theme::rgb(t.data_secondary)),
                    Space::new().width(Length::Fill),
                    button(
                        text("Open Studio Workbench →")
                            .size(11)
                            .font(theme::ui_semibold(t.design)),
                    )
                    .padding([3, 10])
                    .style(theme::btn_ghost(t, false))
                    .on_press(Message::SelectTab(crate::Tab::Studio)),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .push(studio_row);
    }

    let effects_gallery_card = container(
        scrollable(
            container(gallery_col)
                .padding(iced::Padding {
                    top: 0.0,
                    right: 12.0,
                    bottom: 0.0,
                    left: 0.0,
                })
                .width(Length::Fill),
        )
        .height(Length::Fill),
    )
    .padding(t.pad(13))
    .width(Length::Fill)
    .height(Length::Fill)
    .style(theme::rich_instrument_card_style(t, Some(t.pink_rose), false));

    let left_col = column![rig_hero_card, effects_gallery_card]
        .spacing(t.sp(11))
        .width(Length::FillPortion(7))
        .height(Length::Fill);

    // -----------------------------------------------------------------------
    // 2. RIGHT INSPECTOR DOCK (`FillPortion(4)`)
    // -----------------------------------------------------------------------
    // Visual Quick Profiles Grid (compact 2-column visual Preset Swatch Cards)
    let mut presets_grid = column![].spacing(7);
    for chunk in app.snapshot.config.presets.chunks(2) {
        let mut r = row![].spacing(7);
        for preset in chunk {
            let active = app.snapshot.config.active_preset.as_deref() == Some(&preset.id);
            let can_delete = app.snapshot.config.presets.len() > 1
                && !matches!(
                    preset.id.as_str(),
                    "silent" | "balanced" | "gaming" | "night" | "heatmap"
                );

            let thumb = iced::widget::canvas(EffectThumbCanvas {
                effect: preset.effect,
                color: preset.params.color,
                secondary: preset.params.secondary,
                palette: p,
                accent_override: t.accent_override,
                anim_t: app.anim_t,
                selected: active,
            })
            .width(Length::Fill)
            .height(Length::Fixed(30.0));

            let mut label_row = row![
                color_dot(preset_dot_color(preset.effect, preset.params.color), 8.0),
                text(truncate_str(&preset.name, 13))
                    .size(12.5)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(if active { p.brass_hi } else { p.ink })),
                Space::new().width(Length::Fill),
                text(truncate_str(preset.effect.label(), 11))
                    .size(10.5)
                    .font(font)
                    .color(theme::rgb(p.ink_3)),
            ]
            .spacing(5)
            .align_y(Alignment::Center);

            if active {
                label_row = label_row.push(icon_canvas(
                    UiIcon::Check,
                    theme::rgb(p.brass_hi),
                    12.0,
                ));
            }

            let card_btn = button(
                column![
                    container(thumb).width(Length::Fill).clip(true).style(move |_| {
                        iced::widget::container::Style {
                            border: iced::Border {
                                radius: iced::border::Radius::from(if p.square_corners {
                                    0.0
                                } else {
                                    7.0
                                }),
                                ..Default::default()
                            },
                            ..Default::default()
                        }
                    }),
                    label_row,
                ]
                .spacing(5),
            )
            .padding([6, 8])
            .width(Length::Fill)
            .style(theme::card_button(t, active))
            .on_press(Message::ApplyPreset(preset.id.clone()));

            if can_delete {
                r = r.push(
                    row![
                        card_btn,
                        button(text("×").size(12).font(font))
                            .padding([6, 8])
                            .style(theme::btn_secondary(t, false))
                            .on_press(Message::DeletePreset(preset.id.clone())),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center)
                    .width(Length::Fill),
                );
            } else {
                r = r.push(card_btn);
            }
        }
        if chunk.len() == 1 {
            let scene_thumb = iced::widget::canvas(EffectThumbCanvas {
                effect: active_effect,
                color: active_params.color,
                secondary: active_params.secondary,
                palette: p,
                accent_override: t.accent_override,
                anim_t: app.anim_t,
                selected: false,
            })
            .width(Length::Fill)
            .height(Length::Fixed(30.0));
            let is_custom = app.snapshot.config.active_preset.is_none();
            r = r.push(
                button(
                    column![
                        container(scene_thumb).width(Length::Fill).clip(true).style(move |_| {
                            iced::widget::container::Style {
                                border: iced::Border {
                                    radius: iced::border::Radius::from(if p.square_corners {
                                        0.0
                                    } else {
                                        7.0
                                    }),
                                    ..Default::default()
                                },
                                ..Default::default()
                            }
                        }),
                        row![
                            color_dot(app.snapshot.average_rgb, 8.0),
                            text(if is_custom {
                                "Custom Live"
                            } else {
                                "Open Studio"
                            })
                            .size(12.5)
                            .font(theme::ui_semibold(t.design))
                            .color(theme::rgb(if is_custom { p.brass_hi } else { p.ink_2 })),
                            Space::new().width(Length::Fill),
                            text(format!("{}%", scene.brightness))
                                .size(10.5)
                                .font(mono)
                                .color(theme::rgb(p.ink_3)),
                        ]
                        .spacing(5)
                        .align_y(Alignment::Center),
                    ]
                    .spacing(5),
                )
                .padding([6, 8])
                .width(Length::Fill)
                .style(theme::card_button(t, is_custom))
                .on_press(Message::SelectTab(crate::Tab::Studio)),
            );
        }
        presets_grid = presets_grid.push(r);
    }

    let active_preset_name = app
        .snapshot
        .config
        .active_preset
        .as_deref()
        .and_then(|id| app.snapshot.config.presets.iter().find(|pr| pr.id == id))
        .map(|pr| pr.name.as_str())
        .unwrap_or("Custom");

    let profiles_card = container(
        column![
            row![
                icon_canvas(UiIcon::Sparkle, theme::rgb(p.brass_hi), 15.0),
                text("Quick Profiles")
                    .size(15)
                    .font(theme::display_semibold(t.design))
                    .color(theme::rgb(p.ink)),
                Space::new().width(Length::Fill),
                container(
                    text(truncate_str(active_preset_name, 16))
                        .size(10.5)
                        .font(theme::ui_semibold(t.design))
                        .color(theme::rgb(p.brass_hi)),
                )
                .padding([2, 9])
                .style(theme::chip_style(t, Some(p.brass))),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
            presets_grid,
            row![
                text_input("New profile name…", &app.preset_name_draft)
                    .size(12.5)
                    .style(theme::text_input_style(t))
                    .on_input(Message::PresetNameDraftChanged)
                    .on_submit(Message::SaveCurrentPreset)
                    .width(Length::Fill),
                button(
                    row![
                        icon_canvas(UiIcon::Plus, theme::rgb(t.primary_ink), 12.5),
                        text("Save")
                            .size(12)
                            .font(theme::ui_semibold(t.design)),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding([7, 14])
                .style(theme::btn_primary(t))
                .on_press(Message::SaveCurrentPreset),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
        ]
        .spacing(9),
    )
    .padding(t.pad(14))
    .width(Length::Fill)
    .style(theme::rich_instrument_card_style(t, Some(p.brass), false));

    // Luminous Effect Inspector Card: 164px ColorWheel + #HEX + 24px circular swatches + Swap + pill sliders
    let mut swatches_col = column![].spacing(7);
    for chunk in CURATED_SWATCHES.chunks(6) {
        let mut sr = row![].spacing(7);
        for &col in chunk {
            let selected = active_params.color == col;
            let mut next = active_params.clone();
            next.color = col;
            if let Some(first) = next.colors.first_mut() {
                *first = col;
            }
            sr = sr.push(
                button(Space::new().width(24).height(24))
                    .padding(0)
                    .width(Length::Fixed(24.0))
                    .height(Length::Fixed(24.0))
                    .style(theme::swatch_btn(col, selected, t))
                    .on_press(Message::UpdateSceneParams(next)),
            );
        }
        swatches_col = swatches_col.push(sr);
    }

    let color_wheel_el = Element::from(
        iced::widget::canvas(ColorWheelCanvas {
            rgb: active_params.color,
            palette: p,
            accent_override: t.accent_override,
        })
        .width(Length::Fixed(164.0))
        .height(Length::Fixed(164.0)),
    )
    .map(|msg| match msg {
        ColorWheelMessage::PickRgb(rgb) => Message::LightingColorPicked(rgb),
    });

    let color_picker_block = row![
        color_wheel_el,
        column![
            text("PRIMARY COLOUR")
                .size(10.5)
                .font(theme::ui_semibold(t.design))
                .color(theme::rgb(p.ink_3)),
            row![
                color_dot(active_params.color, 14.0),
                text_input("#F48FB1", &app.lighting_hex_draft)
                    .size(12.5)
                    .font(mono)
                    .style(theme::text_input_style(t))
                    .on_input(Message::LightingHexDraftChanged)
                    .width(Length::Fill),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            text("Curated jewel swatches")
                .size(11)
                .font(font)
                .color(theme::rgb(p.ink_3)),
            swatches_col,
        ]
        .spacing(8)
        .width(Length::Fill),
    ]
    .spacing(14)
    .align_y(Alignment::Center);

    let is_multi_color = matches!(
        active_effect,
        EffectId::Gradient
            | EffectId::Comet
            | EffectId::Aurora
            | EffectId::FireflySwarm
            | EffectId::MeteorShower
            | EffectId::Heatmap
            | EffectId::Temperature
    );

    let secondary_row: Option<Element<'_, Message>> = if is_multi_color {
        let mut sec_swatches = row![].spacing(7).align_y(Alignment::Center);
        for &col in CURATED_SWATCHES.iter().take(9) {
            let selected = active_params.secondary == col;
            let mut next = active_params.clone();
            next.secondary = col;
            if next.colors.len() >= 2 {
                next.colors[1] = col;
            }
            sec_swatches = sec_swatches.push(
                button(Space::new().width(22).height(22))
                    .padding(0)
                    .width(Length::Fixed(22.0))
                    .height(Length::Fixed(22.0))
                    .style(theme::swatch_btn(col, selected, t))
                    .on_press(Message::UpdateSceneParams(next)),
            );
        }
        Some(
            container(
                column![
                    row![
                        color_dot(active_params.secondary, 10.0),
                        text("Secondary colour")
                            .size(12)
                            .font(theme::ui_semibold(t.design))
                            .color(theme::rgb(p.ink_2)),
                        Space::new().width(Length::Fill),
                        button(
                            row![
                                icon_canvas(UiIcon::Swap, theme::rgb(p.brass_hi), 13.0),
                                text("Swap")
                                    .size(11.5)
                                    .font(theme::ui_semibold(t.design)),
                            ]
                            .spacing(5)
                            .align_y(Alignment::Center),
                        )
                        .padding([4, 11])
                        .style(theme::btn_secondary(t, false))
                        .on_press(Message::SwapLightingColors),
                    ]
                    .spacing(7)
                    .align_y(Alignment::Center),
                    sec_swatches,
                ]
                .spacing(7),
            )
            .padding([9, 11])
            .width(Length::Fill)
            .style(theme::well_style(t))
            .into(),
        )
    } else {
        None
    };

    let color_picker_block: Element<'_, Message> = if let Some(sec) = secondary_row {
        column![color_picker_block, sec].spacing(10).into()
    } else {
        color_picker_block.into()
    };

    let target_badge = if let Some(zone_cfg) = app.selected_zone_cfg() {
        if scene.zone_overrides.contains_key(&zone_cfg.id) {
            format!("Override · {}", truncate_str(&zone_cfg.name, 15))
        } else {
            format!("Zone · {}", truncate_str(&zone_cfg.name, 16))
        }
    } else {
        "Whole Rig".to_string()
    };

    let mut inspector_col = column![
        row![
            icon_canvas(UiIcon::Sliders, theme::rgb(t.data_primary), 16.0),
            text(active_effect.label())
                .size(20)
                .font(theme::display_semibold(t.design))
                .color(theme::rgb(p.ink)),
            Space::new().width(Length::Fill),
            container(
                text(target_badge)
                    .size(11)
                    .font(theme::ui_semibold(t.design)),
            )
            .padding([4, 10])
            .style(theme::chip_style(
                t,
                if app.selected_zone.is_some() {
                    Some(p.brass_hi)
                } else {
                    Some(t.data_primary)
                },
            )),
            pill_toggle(t, eff.on, Message::TogglePower),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        text(active_effect.description())
            .size(12)
            .font(font)
            .color(theme::rgb(p.ink_3)),
    ]
    .spacing(10);

    // Live 24-band AudioVisualizerCanvas when audio effects are active
    if matches!(active_effect, EffectId::Spectrum | EffectId::Pulse) {
        let audio_viz = iced::widget::canvas(AudioVisualizerCanvas {
            bands: app.snapshot.audio_bands,
            bass: app.snapshot.audio_bass,
            beat: app.snapshot.audio_frame.beat,
            palette: p,
            accent_override: t.accent_override,
            active: eff.on,
        })
        .width(Length::Fill)
        .height(Length::Fixed(56.0));
        inspector_col = inspector_col.push(
            container(audio_viz)
                .padding(7)
                .width(Length::Fill)
                .style(theme::well_style(t)),
        );
    }

    match active_effect {
        EffectId::Studio => {
            inspector_col = inspector_col.push(
                button(
                    row![
                        icon_canvas(UiIcon::Studio, theme::rgb(t.primary_ink), 15.0),
                        text("Open Full Studio Layer Editor →")
                            .size(13)
                            .font(theme::ui_semibold(t.design)),
                    ]
                    .spacing(7)
                    .align_y(Alignment::Center),
                )
                .padding([10, 16])
                .width(Length::Fill)
                .style(theme::btn_primary(t))
                .on_press(Message::SelectTab(crate::Tab::Studio)),
            );
        }
        EffectId::Screen => {
            let p_mirror = active_params.clone();
            let p_sat = active_params.clone();
            inspector_col = inspector_col
                .push(
                    checkbox(active_params.mirror)
                        .label("Mirror screen left-to-right")
                        .style(theme::checkbox_style(t))
                        .on_toggle(move |v| {
                            let mut n = p_mirror.clone();
                            n.mirror = v;
                            Message::UpdateSceneParams(n)
                        }),
                )
                .push(inspector_pill_slider(
                    t,
                    font,
                    "Colour boost (saturation)",
                    format!("{}%", active_params.saturation),
                    0,
                    100,
                    active_params.saturation as u16,
                    Some(t.pink_rose),
                    move |v| {
                        let mut n = p_sat.clone();
                        n.saturation = v as u8;
                        Message::UpdateSceneParams(n)
                    },
                ));
        }
        EffectId::Spectrum => {
            let p_sens = active_params.clone();
            let p_hue = active_params.clone();
            let p_ang = active_params.clone();
            inspector_col = inspector_col
                .push(inspector_pill_slider(
                    t,
                    font,
                    "Audio sensitivity",
                    format!("{}%", active_params.sensitivity),
                    10,
                    100,
                    active_params.sensitivity as u16,
                    Some(t.data_primary),
                    move |v| {
                        let mut n = p_sens.clone();
                        n.sensitivity = v as u8;
                        Message::UpdateSceneParams(n)
                    },
                ))
                .push(inspector_pill_slider(
                    t,
                    font,
                    "Hue shift",
                    format!("{}°", active_params.hue_shift),
                    0,
                    359,
                    active_params.hue_shift,
                    Some(t.pink_rose),
                    move |v| {
                        let mut n = p_hue.clone();
                        n.hue_shift = v;
                        Message::UpdateSceneParams(n)
                    },
                ))
                .push(inspector_pill_slider(
                    t,
                    font,
                    "Direction",
                    format!("{}°", active_params.angle),
                    0,
                    359,
                    active_params.angle,
                    None,
                    move |v| {
                        let mut n = p_ang.clone();
                        n.angle = v;
                        Message::UpdateSceneParams(n)
                    },
                ));
        }
        EffectId::Pulse => {
            let p_sens = active_params.clone();
            let p_floor = active_params.clone();
            inspector_col = inspector_col
                .push(color_picker_block)
                .push(inspector_pill_slider(
                    t,
                    font,
                    "Beat sensitivity",
                    format!("{}%", active_params.sensitivity),
                    10,
                    100,
                    active_params.sensitivity as u16,
                    Some(t.pink_rose),
                    move |v| {
                        let mut n = p_sens.clone();
                        n.sensitivity = v as u8;
                        Message::UpdateSceneParams(n)
                    },
                ))
                .push(inspector_pill_slider(
                    t,
                    font,
                    "Idle floor glow",
                    format!("{}%", active_params.floor),
                    0,
                    60,
                    active_params.floor as u16,
                    Some(t.data_secondary),
                    move |v| {
                        let mut n = p_floor.clone();
                        n.floor = v as u8;
                        Message::UpdateSceneParams(n)
                    },
                ));
        }
        EffectId::Temperature | EffectId::Heatmap => {
            let p_cool = active_params.clone();
            let p_hot = active_params.clone();
            let mut src_btns = row![].spacing(3);
            for src in TempSource::ALL {
                let mut n = active_params.clone();
                n.temp_source = src;
                let short = match src {
                    TempSource::Max => "Hotter",
                    TempSource::Cpu => "CPU",
                    TempSource::Gpu => "GPU",
                    TempSource::Liquid => "Coolant",
                };
                src_btns = src_btns.push(
                    button(container(text(short).size(11.5).font(font)).center_x(Length::Fill))
                        .padding([5, 8])
                        .width(Length::Fill)
                        .style(theme::segmented_pill_style(
                            t,
                            active_params.temp_source == src,
                        ))
                        .on_press(Message::UpdateSceneParams(n)),
                );
            }
            inspector_col = inspector_col
                .push(color_picker_block)
                .push(
                    container(src_btns)
                        .padding(3)
                        .width(Length::Fill)
                        .style(theme::segmented_track_style(t)),
                )
                .push(inspector_pill_slider(
                    t,
                    font,
                    "Cool threshold",
                    unit.format_temp(active_params.cool_temp as f32),
                    20,
                    75,
                    active_params.cool_temp as u16,
                    Some(t.data_primary),
                    move |v| {
                        let mut n = p_cool.clone();
                        n.cool_temp = v as u8;
                        Message::UpdateSceneParams(n)
                    },
                ))
                .push(inspector_pill_slider(
                    t,
                    font,
                    "Hot threshold",
                    unit.format_temp(active_params.hot_temp as f32),
                    40,
                    100,
                    active_params.hot_temp as u16,
                    Some(p.hot),
                    move |v| {
                        let mut n = p_hot.clone();
                        n.hot_temp = v as u8;
                        Message::UpdateSceneParams(n)
                    },
                ));
        }
        EffectId::Off => {}
        _ => {
            let p_spd = active_params.clone();
            let p_ang = active_params.clone();

            inspector_col = inspector_col
                .push(color_picker_block)
                .push(inspector_pill_slider(
                    t,
                    font,
                    "Speed",
                    format!("{}%", active_params.speed),
                    0,
                    100,
                    active_params.speed as u16,
                    Some(t.data_primary),
                    move |v| {
                        let mut n = p_spd.clone();
                        n.speed = v as u8;
                        Message::UpdateSceneParams(n)
                    },
                ))
                .push(inspector_pill_slider(
                    t,
                    font,
                    "Direction",
                    format!("{}°", active_params.angle),
                    0,
                    359,
                    active_params.angle,
                    Some(t.data_secondary),
                    move |v| {
                        let mut n = p_ang.clone();
                        n.angle = v;
                        Message::UpdateSceneParams(n)
                    },
                ));
        }
    }

    // Master Brightness slider with luminous pill badge
    inspector_col = inspector_col.push(inspector_pill_slider(
        t,
        font,
        "Master Brightness",
        format!("{}%", scene.brightness),
        0,
        100,
        scene.brightness as u16,
        Some(p.brass_hi),
        |v| Message::SetBrightness(v as u8),
    ));

    let inspector_scroll = scrollable(inspector_col).height(Length::Fill);

    let inspector_footer = container(
        row![
            color_dot(app.snapshot.average_rgb, 8.0),
            text(if eff.on {
                format!(
                    "Live output · {} override(s)",
                    scene.zone_overrides.len()
                )
            } else {
                "Blackout standby (lights off)".to_string()
            })
            .size(11)
            .font(font)
            .color(theme::rgb(p.ink_3)),
            Space::new().width(Length::Fill),
            button(
                row![
                    icon_canvas(UiIcon::Expand, theme::rgb(p.brass_hi), 12.0),
                    text("Theater")
                        .size(11)
                        .font(theme::ui_semibold(t.design)),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .padding([3, 10])
            .style(theme::btn_ghost(t, app.theater_rig))
            .on_press(Message::ToggleTheaterRig),
        ]
        .spacing(7)
        .align_y(Alignment::Center),
    )
    .padding([7, 10])
    .width(Length::Fill)
    .style(theme::well_style(t));

    let inspector_card = container(
        column![inspector_scroll, inspector_footer]
            .spacing(10)
            .height(Length::Fill),
    )
    .padding(t.pad(15))
    .width(Length::Fill)
    .height(Length::Fill)
    .style(theme::rich_instrument_card_style(
        t,
        Some(t.data_secondary),
        false,
    ));

    let right_dock = column![profiles_card, inspector_card]
        .spacing(t.sp(14))
        .width(Length::FillPortion(4))
        .height(Length::Fill);

    container(
        row![left_col, right_dock]
            .spacing(t.sp(16))
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .padding([18, 26])
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

#[allow(dead_code)]
pub fn view_studio<'a>(
    app: &'a AirglowApp,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let appr = &app.snapshot.config.appearance;
    let unit = app.snapshot.config.settings.temp_unit;
    let stack_cfg = &app.snapshot.config.scene.studio_stack;
    let is_live = app.snapshot.config.scene.effect == EffectId::Studio;

    let can_undo = !app.studio_undo.is_empty();
    let can_redo = !app.studio_redo.is_empty();

    let mut speed_mult_row = row![].spacing(3);
    for (mult, label) in [(0.5_f32, "0.5×"), (1.0_f32, "1×"), (2.0_f32, "2×")] {
        let active = (app.snapshot.studio_speed_mult - mult).abs() < 0.05;
        speed_mult_row = speed_mult_row.push(
            button(text(label).size(11.5).font(font))
                .padding([5, 10])
                .style(theme::segmented_pill_style(t, active))
                .on_press(Message::SetStudioSpeedMult(mult)),
        );
    }

    let mut undo_btn = button(text("Undo").size(12).font(font))
        .padding([6, 12])
        .style(theme::btn_secondary(t, false));
    if can_undo {
        undo_btn = undo_btn.on_press(Message::StudioUndo);
    }

    let mut redo_btn = button(text("Redo").size(12).font(font))
        .padding([6, 12])
        .style(theme::btn_secondary(t, false));
    if can_redo {
        redo_btn = redo_btn.on_press(Message::StudioRedo);
    }

    let top_bar = container(
        row![
            icon_canvas(UiIcon::Studio, theme::rgb(p.brass_hi), 18.0),
            text(format!("Studio · {}", stack_cfg.name))
                .size(18)
                .font(theme::display_semibold(t.design)),
            Space::new().width(8),
            button(
                row![
                    icon_canvas(UiIcon::Sparkle, theme::rgb(t.pink_rose), 13.0),
                    text("Surprise me").size(12).font(font),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .padding([6, 12])
            .style(theme::btn_secondary(t, false))
            .on_press(Message::StudioSurpriseMe),
            undo_btn,
            redo_btn,
            Space::new().width(Length::Fill),
            row![
                pill_toggle(
                    t,
                    app.snapshot.studio_test_beat,
                    Message::ToggleStudioTestBeat(!app.snapshot.studio_test_beat),
                ),
                text("Test beat")
                    .size(12)
                    .font(font)
                    .color(theme::rgb(p.ink_2)),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
            button(
                text(if app.snapshot.studio_paused {
                    "Play"
                } else {
                    "Pause"
                })
                .size(12)
                .font(font),
            )
            .padding([6, 12])
            .style(theme::btn_secondary(t, app.snapshot.studio_paused))
            .on_press(Message::ToggleStudioPaused),
            container(speed_mult_row)
                .padding(3)
                .style(theme::segmented_track_style(t)),
            button(
                text(if is_live {
                    "Live on lights ✓"
                } else {
                    "Use on my lights"
                })
                .size(12)
                .font(theme::ui_semibold(t.design)),
            )
            .padding([7, 15])
            .style(theme::btn_primary(t))
            .on_press(Message::SelectEffect(EffectId::Studio)),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([12, 16])
    .style(theme::panel_style(t));

    let mut layer_cards = column![
        text("LAYERS (TOP DRAWS OVER BOTTOM)")
            .size(11)
            .font(theme::ui_semibold(t.design))
            .color(theme::rgb(p.ink_3)),
    ]
    .spacing(6);

    for layer in &stack_cfg.layers {
        let selected = app.selected_layer == Some(layer.id);
        let mut toggled = layer.clone();
        toggled.enabled = !toggled.enabled;

        let meta = if layer.react > 0 {
            format!(
                "{}% · {} · ♪{}%",
                layer.opacity,
                layer.blend.label(),
                layer.react
            )
        } else {
            format!("{}% · {}", layer.opacity, layer.blend.label())
        };

        layer_cards = layer_cards.push(
            row![
                button(
                    row![
                        color_dot(layer.color, 11.0),
                        column![
                            text(layer.kind.label())
                                .size(12.5)
                                .font(theme::ui_semibold(t.design)),
                            text(meta)
                                .size(10.5)
                                .font(font)
                                .color(theme::rgb(p.ink_3)),
                        ]
                        .spacing(1),
                        Space::new().width(Length::Fill),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .padding([8, 10])
                .style(theme::card_button(t, selected))
                .on_press(Message::SelectLayer(layer.id)),
                button(text(if layer.enabled { "On" } else { "Off" }).size(10.5).font(font))
                    .padding([8, 8])
                    .style(theme::btn_secondary(t, layer.enabled))
                    .on_press(Message::UpdateSelectedLayer(toggled)),
                button(text("↑").size(10.5).font(font))
                    .padding([8, 7])
                    .style(theme::btn_secondary(t, false))
                    .on_press(Message::MoveLayer(layer.id, true)),
                button(text("↓").size(10.5).font(font))
                    .padding([8, 7])
                    .style(theme::btn_secondary(t, false))
                    .on_press(Message::MoveLayer(layer.id, false)),
                button(text("×").size(10.5).font(font))
                    .padding([8, 8])
                    .style(theme::btn_secondary(t, false))
                    .on_press(Message::RemoveLayer(layer.id)),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        );
    }

    let add_row = |kinds: &[LayerKind]| {
        let mut r = row![].spacing(5);
        for &kind in kinds {
            r = r.push(
                button(
                    container(text(format!("+ {}", kind.label())).size(11).font(font))
                        .center_x(Length::Fill),
                )
                .padding([5, 7])
                .width(Length::Fill)
                .style(theme::btn_secondary(t, false))
                .on_press(Message::AddLayer(kind)),
            );
        }
        r
    };

    let add_visual_1 = add_row(&[
        LayerKind::Gradient,
        LayerKind::Plasma,
        LayerKind::Sparkles,
        LayerKind::Comet,
    ]);
    let add_visual_2 = add_row(&[
        LayerKind::Chase,
        LayerKind::Scanner,
        LayerKind::Wave,
        LayerKind::Rainbow,
    ]);
    let add_visual_3 = add_row(&[LayerKind::Solid, LayerKind::Breathing]);
    let add_music = add_row(&[LayerKind::Beat, LayerKind::Spectrum, LayerKind::Bass]);

    let mut library_col = column![
        text("SAVED STUDIO EFFECTS")
            .size(11)
            .font(theme::ui_semibold(t.design))
            .color(theme::rgb(p.ink_3)),
        row![
            text_input("Effect name", &app.studio_save_name)
                .style(theme::text_input_style(t))
                .on_input(Message::StudioSaveNameChanged)
                .width(Length::Fill),
            button(text("Save").size(11.5).font(theme::ui_semibold(t.design)))
                .padding([7, 13])
                .style(theme::btn_primary(t))
                .on_press(Message::SaveStudioEffect),
        ]
        .spacing(6),
    ]
    .spacing(7);

    for saved in &app.snapshot.config.studio_effects {
        let active = app.snapshot.config.scene.active_studio_effect.as_deref()
            == Some(saved.id.as_str());
        library_col = library_col.push(
            row![
                button(text(&saved.name).size(12).font(font))
                    .padding([6, 10])
                    .width(Length::Fill)
                    .style(theme::card_button(t, active))
                    .on_press(Message::LoadStudioEffect(saved.id.clone())),
                button(text("Dup").size(10.5).font(font))
                    .padding([6, 8])
                    .style(theme::btn_secondary(t, false))
                    .on_press(Message::DuplicateStudioEffect(saved.id.clone())),
                button(text("×").size(10.5).font(font))
                    .padding([6, 8])
                    .style(theme::btn_secondary(t, false))
                    .on_press(Message::DeleteStudioEffect(saved.id.clone())),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        );
    }

    let left_panel = column![
        container(
            scrollable(
                column![
                    layer_cards,
                    Space::new().height(4),
                    text("ADD VISUAL LAYER")
                        .size(10.5)
                        .font(theme::ui_semibold(t.design))
                        .color(theme::rgb(p.ink_3)),
                    add_visual_1,
                    add_visual_2,
                    add_visual_3,
                    text("ADD MUSIC LAYER")
                        .size(10.5)
                        .font(theme::ui_semibold(t.design))
                        .color(theme::rgb(t.pink_rose)),
                    add_music,
                ]
                .spacing(6),
            )
            .height(Length::Fill),
        )
        .padding(t.pad(14))
        .height(Length::FillPortion(6))
        .style(theme::panel_style(t)),
        container(scrollable(library_col).height(Length::Fill))
            .padding(t.pad(14))
            .height(Length::FillPortion(4))
            .style(theme::panel_style(t)),
    ]
    .spacing(t.sp(14))
    .width(Length::FillPortion(4))
    .height(Length::Fill);

    let preview_frames = if !app.snapshot.studio_preview_frames.is_empty()
        && app.snapshot.effective_scene.effect != EffectId::Studio
    {
        &app.snapshot.studio_preview_frames
    } else {
        &app.snapshot.frames
    };
    let active_zones = preview_frames.iter().filter(|f| f.enabled).count();
    let total_leds: usize = preview_frames
        .iter()
        .filter(|f| f.enabled)
        .map(|f| f.colors.len())
        .sum();
    let stage = iced::widget::canvas(CaseStageCanvas {
        frames: preview_frames,
        zone_overrides: &app.snapshot.config.scene.zone_overrides,
        palette: p,
        accent_override: t.accent_override,
        rig_style: appr.rig_style,
        airflow_lines: appr.airflow_lines,
        spinning_fans: appr.spinning_fans,
        ambient_bloom: appr.ambient_bloom,
        lights_on: true,
        show_labels: false,
        arrange_mode: false,
        selected_zone: None,
        active_effect: EffectId::Studio,
        temp_unit: unit,
        anim_t: app.anim_t,
        fan_duty: app.snapshot.fan_duty,
        cpu_temp: app.snapshot.sensors.cpu_temp,
        gpu_temp: app.snapshot.sensors.gpu_temp,
        ambient_rgb: app.snapshot.average_rgb,
        eyebrow: None,
        animate_lighting: true,
    })
    .width(Length::Fill)
    .height(Length::Fill);
    let stage_el = Element::from(stage).map(Message::Stage);

    let audio_active = app.snapshot.studio_test_beat
        || app.snapshot.audio_bass > 0.03
        || stack_cfg.layers.iter().any(|l| {
            l.enabled
                && (l.react > 0
                    || matches!(l.kind, LayerKind::Beat | LayerKind::Spectrum | LayerKind::Bass))
        });

    let audio_eq = iced::widget::canvas(AudioVisualizerCanvas {
        bands: app.snapshot.audio_bands,
        bass: app.snapshot.audio_bass,
        beat: app.snapshot.audio_frame.beat,
        palette: p,
        accent_override: t.accent_override,
        active: audio_active,
    })
    .width(Length::Fill)
    .height(Length::Fixed(52.0));

    let center_panel = container(
        column![
            row![
                text(format!(
                    "{:.0} fps · {active_zones} zones · {total_leds} LEDs",
                    app.snapshot.fps
                ))
                .size(11.5)
                .font(theme::mono_font())
                .color(theme::rgb(p.ink_3)),
                Space::new().width(Length::Fill),
                text(if is_live {
                    "Live on rig"
                } else if app.snapshot.studio_test_beat {
                    "Test beat active"
                } else {
                    "Real-rig preview"
                })
                .size(11)
                .font(font)
                .color(theme::rgb(if is_live || app.snapshot.studio_test_beat {
                    t.pink_rose
                } else {
                    t.data_primary
                })),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            container(stage_el)
                .width(Length::Fill)
                .height(Length::Fill)
                .clip(true)
                .style(theme::bento_stage_well_style(t)),
            container(audio_eq)
                .padding([6, 8])
                .style(theme::well_style(t)),
            inspector_pill_slider(
                t,
                font,
                "Master speed (all layers)",
                format!("{}%", stack_cfg.master_speed),
                10,
                200,
                stack_cfg.master_speed as u16,
                Some(p.brass_hi),
                Message::SetStudioMasterSpeed,
            ),
            text("ONE-CLICK COLOUR HARMONIES")
                .size(10.5)
                .font(theme::ui_semibold(t.design))
                .color(theme::rgb(t.pink_rose)),
            harmony_palette_grid(app, t, font, 2),
        ]
        .spacing(10)
        .height(Length::Fill),
    )
    .padding(t.pad(15))
    .width(Length::FillPortion(5))
    .height(Length::Fill)
    .style(theme::panel_style(t));

    let inspector_body: Element<'_, Message> = if let Some(layer) = app
        .selected_layer
        .and_then(|id| stack_cfg.layers.iter().find(|l| l.id == id))
        .or_else(|| stack_cfg.layers.first())
    {
        let mut blend_row = row![].spacing(3);
        for b in BlendMode::ALL {
            let mut next = layer.clone();
            next.blend = b;
            blend_row = blend_row.push(
                button(container(text(b.label()).size(11.5).font(font)).center_x(Length::Fill))
                    .padding([5, 6])
                    .width(Length::Fill)
                    .style(theme::segmented_pill_style(t, layer.blend == b))
                    .on_press(Message::UpdateSelectedLayer(next)),
            );
        }

        let studio_wheel = Element::from(
            iced::widget::canvas(ColorWheelCanvas {
                rgb: layer.color,
                palette: p,
                accent_override: t.accent_override,
            })
            .width(Length::Fixed(132.0))
            .height(Length::Fixed(132.0)),
        )
        .map(|msg| match msg {
            ColorWheelMessage::PickRgb(rgb) => Message::StudioColorPicked(rgb),
        });

        let mut swatches_primary_a = row![].spacing(6);
        for col in CURATED_SWATCHES.iter().take(6) {
            let mut next = layer.clone();
            next.color = *col;
            swatches_primary_a = swatches_primary_a.push(
                button(Space::new().width(20).height(20))
                    .padding(0)
                    .width(Length::Fixed(21.0))
                    .height(Length::Fixed(21.0))
                    .style(theme::swatch_btn(*col, layer.color == *col, t))
                    .on_press(Message::UpdateSelectedLayer(next)),
            );
        }
        let mut swatches_primary_b = row![].spacing(6);
        for col in CURATED_SWATCHES.iter().skip(6).take(6) {
            let mut next = layer.clone();
            next.color = *col;
            swatches_primary_b = swatches_primary_b.push(
                button(Space::new().width(20).height(20))
                    .padding(0)
                    .width(Length::Fixed(21.0))
                    .height(Length::Fixed(21.0))
                    .style(theme::swatch_btn(*col, layer.color == *col, t))
                    .on_press(Message::UpdateSelectedLayer(next)),
            );
        }

        let mut swatches_secondary = row![].spacing(6);
        for col in CURATED_SWATCHES.iter().take(9) {
            let mut next = layer.clone();
            next.secondary = *col;
            swatches_secondary = swatches_secondary.push(
                button(Space::new().width(20).height(20))
                    .padding(0)
                    .width(Length::Fixed(21.0))
                    .height(Length::Fixed(21.0))
                    .style(theme::swatch_btn(*col, layer.secondary == *col, t))
                    .on_press(Message::UpdateSelectedLayer(next)),
            );
        }

        let mut rev_next = layer.clone();
        rev_next.reverse = !rev_next.reverse;

        scrollable(
            column![
                text(format!("LAYER · {}", layer.kind.label().to_uppercase()))
                    .size(12)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(p.brass_hi)),
                text("Blend mode").size(12).font(font),
                container(blend_row)
                    .padding(3)
                    .width(Length::Fill)
                    .style(theme::segmented_track_style(t)),
                text("Primary colour").size(12).font(font),
                row![
                    studio_wheel,
                    column![
                        row![
                            color_dot(layer.color, 12.0),
                            text_input("#F48FB1", &app.studio_hex_draft)
                                .size(12)
                                .font(theme::mono_font())
                                .style(theme::text_input_style(t))
                                .on_input(Message::StudioHexDraftChanged)
                                .width(Length::Fill),
                        ]
                        .spacing(6)
                        .align_y(Alignment::Center),
                        swatches_primary_a,
                        swatches_primary_b,
                    ]
                    .spacing(7)
                    .width(Length::Fill),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
                text("Secondary colour").size(12).font(font),
                swatches_secondary,
                inspector_pill_slider(
                    t,
                    font,
                    "Opacity",
                    format!("{}%", layer.opacity),
                    0,
                    100,
                    layer.opacity as u16,
                    Some(p.brass_hi),
                    |v| {
                        let mut n = layer.clone();
                        n.opacity = v as u8;
                        Message::UpdateSelectedLayer(n)
                    },
                ),
                inspector_pill_slider(
                    t,
                    font,
                    "Speed",
                    format!("{}%", layer.speed),
                    0,
                    100,
                    layer.speed as u16,
                    Some(t.data_primary),
                    |v| {
                        let mut n = layer.clone();
                        n.speed = v as u8;
                        Message::UpdateSelectedLayer(n)
                    },
                ),
                inspector_pill_slider(
                    t,
                    font,
                    "Scale / Tail",
                    format!("{}%", layer.scale),
                    5,
                    100,
                    layer.scale as u16,
                    None,
                    |v| {
                        let mut n = layer.clone();
                        n.scale = v as u8;
                        Message::UpdateSelectedLayer(n)
                    },
                ),
                inspector_pill_slider(
                    t,
                    font,
                    "Direction",
                    format!("{}°", layer.angle),
                    0,
                    359,
                    layer.angle,
                    Some(t.data_secondary),
                    |v| {
                        let mut n = layer.clone();
                        n.angle = v;
                        Message::UpdateSelectedLayer(n)
                    },
                ),
                inspector_pill_slider(
                    t,
                    font,
                    "Music reactivity",
                    format!("{}%", layer.react),
                    0,
                    100,
                    layer.react as u16,
                    Some(t.pink_rose),
                    |v| {
                        let mut n = layer.clone();
                        n.react = v as u8;
                        Message::UpdateSelectedLayer(n)
                    },
                ),
                checkbox(layer.reverse)
                    .label("Reverse direction")
                    .style(theme::checkbox_style(t))
                    .on_toggle(move |_| Message::UpdateSelectedLayer(rev_next.clone())),
            ]
            .spacing(10),
        )
        .height(Length::Fill)
        .into()
    } else {
        text("Add a layer on the left or press Surprise me to start composing.")
            .size(13)
            .font(font)
            .color(theme::rgb(p.ink_3))
            .into()
    };

    let right_panel = container(inspector_body)
        .padding(t.pad(15))
        .width(Length::FillPortion(4))
        .height(Length::Fill)
        .style(theme::panel_style(t));

    container(
        column![
            top_bar,
            row![left_panel, center_panel, right_panel]
                .spacing(t.sp(14))
                .height(Length::Fill),
        ]
        .spacing(t.sp(14))
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .padding([18, 26])
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
