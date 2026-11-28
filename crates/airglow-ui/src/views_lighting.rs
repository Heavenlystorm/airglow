//! Lighting view (4×4 = 16 live-animated effect artworks grid, 10-palette Color Harmony bar,
//! live Audio Visualizer, per-zone overrides, saved Studio looks, profile Apply/Try/Save/Delete)
//! and Studio Layer Mode workbench.

use airglow_core::config::TempSource;
use airglow_core::effects::{BlendMode, EffectId, HARMONIES, LayerKind};
use iced::widget::{Space, button, checkbox, column, container, row, text, text_input};
use iced::{Alignment, Element, Length};

use crate::canvas_ambient::AudioVisualizerCanvas;
use crate::canvas_colorwheel::{ColorWheelCanvas, ColorWheelMessage};
use crate::canvas_stage::CaseStageCanvas;
use crate::canvas_widgets::EffectThumbCanvas;
use crate::theme::{self, DesignTokens};
use crate::views::{
    CURATED_SWATCHES, color_dot, labeled_slider, pill_toggle, preset_dot_color, truncate_str,
};
use crate::{AirglowApp, Message};

fn harmony_palette_grid<'a>(
    t: DesignTokens,
    font: iced::Font,
    per_row: usize,
) -> Element<'a, Message> {
    let mut grid = column![].spacing(6);
    for (row_idx, chunk) in HARMONIES.chunks(per_row).enumerate() {
        let mut r = row![].spacing(6);
        for (col_idx, h) in chunk.iter().enumerate() {
            let idx = row_idx * per_row + col_idx;
            r = r.push(
                button(
                    row![
                        color_dot(h.colors[0], 8.0),
                        color_dot(h.colors[1], 8.0),
                        color_dot(h.colors[2], 8.0),
                        text(h.name).size(11).font(font),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                )
                .padding([5, 9])
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
    let eff = &app.snapshot.effective_scene;
    let scene = &app.snapshot.config.scene;
    let appr = &app.snapshot.config.appearance;
    let unit = app.snapshot.config.settings.temp_unit;

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
    })
    .width(Length::Fill)
    .height(Length::Fixed(274.0));
    let stage_el = Element::from(stage).map(Message::Stage);

    let mut top_toolbar = row![
        button(
            text(if app.arrange_mode {
                "Done arranging"
            } else {
                "Arrange"
            })
            .size(12)
            .font(theme::ui_semibold(t.design)),
        )
        .padding([6, 13])
        .style(theme::btn_secondary(t, app.arrange_mode))
        .on_press(Message::ToggleArrange),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    if let Some(zone_cfg) = app.selected_zone_cfg() {
        let has_override = scene.zone_overrides.contains_key(&zone_cfg.id);
        top_toolbar = top_toolbar
            .push(
                container(
                    text(format!("Selected: {}", zone_cfg.name))
                        .size(11.5)
                        .font(theme::ui_semibold(t.design)),
                )
                .padding([5, 10])
                .style(theme::chip_style(t, Some(p.brass_hi))),
            )
            .push(
                button(text("Identify").size(12).font(font))
                    .padding([5, 11])
                    .style(theme::btn_secondary(
                        t,
                        app.snapshot.identify_zone.as_deref() == Some(zone_cfg.id.as_str()),
                    ))
                    .on_press(Message::IdentifyZone(zone_cfg.id.clone())),
            );
        if has_override {
            top_toolbar = top_toolbar.push(
                button(text("Clear override").size(12).font(font))
                    .padding([5, 11])
                    .style(theme::btn_danger(t))
                    .on_press(Message::ClearZoneOverride(zone_cfg.id.clone())),
            );
        }
        top_toolbar = top_toolbar.push(
            button(text("Whole rig").size(12).font(font))
                .padding([5, 11])
                .style(theme::btn_secondary(t, false))
                .on_press(Message::SelectWholeRig),
        );
    } else {
        top_toolbar = top_toolbar.push(
            text("Hover any zone for X-ray details, click to override, or turn on Arrange to snap-drag.")
                .size(12)
                .font(font)
                .color(theme::rgb(p.ink_3)),
        );
    }

    if !scene.zone_overrides.is_empty() {
        top_toolbar = top_toolbar.push(Space::new().width(Length::Fill)).push(
            button(
                text(format!("Clear {} override(s)", scene.zone_overrides.len()))
                    .size(11)
                    .font(font),
            )
            .padding([5, 10])
            .style(theme::btn_secondary(t, false))
            .on_press(Message::ClearAllZoneOverrides),
        );
    }

    let (active_effect, active_params) = app.active_lighting_target();

    // 4x4 Visual Effect Gallery — all 16 live-animated built-in artworks with zero label truncation
    let mut effects_grid = column![].spacing(6);
    for chunk in EffectId::ALL.chunks(4) {
        let mut r = row![].spacing(6);
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
            .height(Length::Fixed(36.0));

            r = r.push(
                button(
                    column![
                        container(thumb).clip(true).style(move |_| {
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
                        text(e.label())
                            .size(11)
                            .font(if active {
                                theme::ui_semibold(t.design)
                            } else {
                                font
                            })
                            .color(theme::rgb(if active { p.brass_hi } else { p.ink_2 })),
                    ]
                    .spacing(4)
                    .align_x(Alignment::Center),
                )
                .padding(5)
                .width(Length::Fill)
                .style(theme::card_button(t, active))
                .on_press(Message::SelectEffect(e)),
            );
        }
        effects_grid = effects_grid.push(r);
    }

    // Curated 10 Color Harmony Palettes Bar (2 rows of 5)
    let harmony_section = column![
        row![
            text("COLOUR HARMONIES (1-CLICK PALETTES)")
                .size(10.5)
                .font(theme::ui_semibold(t.design))
                .color(theme::rgb(t.pink_rose)),
            Space::new().width(Length::Fill),
            text("10 curated multi-colour blends")
                .size(10.5)
                .font(font)
                .color(theme::rgb(p.ink_3)),
        ]
        .align_y(Alignment::Center),
        harmony_palette_grid(t, font, 5),
    ]
    .spacing(7);

    let mut effects_panel_col = column![
        row![
            text("EFFECTS (16 LIVE ARTWORKS)")
                .size(11)
                .font(theme::ui_semibold(t.design))
                .color(theme::rgb(p.ink_3)),
            Space::new().width(Length::Fill),
            text("Smooth 350ms hardware crossfade")
                .size(10.5)
                .font(font)
                .color(theme::rgb(p.ink_3)),
        ]
        .align_y(Alignment::Center),
        effects_grid,
        Space::new().height(4),
        harmony_section,
    ]
    .spacing(9);

    if !app.snapshot.config.studio_effects.is_empty() {
        let mut studio_row = row![].spacing(8);
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
            .height(Length::Fixed(40.0));

            studio_row = studio_row.push(
                button(
                    column![
                        container(thumb).clip(true).style(move |_| {
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
                        text(truncate_str(&saved.name, 16))
                            .size(11)
                            .font(if active {
                                theme::ui_semibold(t.design)
                            } else {
                                font
                            })
                            .color(theme::rgb(if active { p.brass_hi } else { p.ink_2 })),
                    ]
                    .spacing(5)
                    .align_x(Alignment::Center),
                )
                .padding(6)
                .width(Length::Fill)
                .style(theme::card_button(t, active))
                .on_press(Message::ApplyStudioEffect(saved.id.clone())),
            );
        }
        let count = app.snapshot.config.studio_effects.len().min(max_show);
        for _ in count..max_show {
            studio_row = studio_row.push(Space::new().width(Length::Fill));
        }
        effects_panel_col = effects_panel_col
            .push(Space::new().height(2))
            .push(
                text("SAVED STUDIO LOOKS")
                    .size(10.5)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(t.pink_rose)),
            )
            .push(studio_row);
    }

    let left_col = column![
        container(
            column![
                top_toolbar,
                container(stage_el).clip(true).style(move |_| {
                    iced::widget::container::Style {
                        border: iced::Border {
                            radius: iced::border::Radius::from(if p.square_corners {
                                0.0
                            } else {
                                12.0
                            }),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                }),
            ]
            .spacing(10),
        )
        .padding(t.pad(12))
        .style(theme::panel_style(t)),
        container(effects_panel_col)
            .padding(t.pad(12))
            .style(theme::panel_style(t)),
    ]
    .spacing(t.sp(10))
    .width(Length::FillPortion(7));

    // Right inspector sidebar: Profiles + Effect Inspector (+ Audio Visualizer when audio effect active)
    let mut presets_list = column![].spacing(6);
    for preset in &app.snapshot.config.presets {
        let active = app.snapshot.config.active_preset.as_deref() == Some(&preset.id);
        let trying = app
            .snapshot
            .trial
            .as_ref()
            .is_some_and(|tr| tr.preset_id == preset.id);
        let can_delete = app.snapshot.config.presets.len() > 1
            && !matches!(
                preset.id.as_str(),
                "silent" | "balanced" | "gaming" | "night" | "heatmap"
            );

        let mut prow = row![
            button(
                row![
                    color_dot(preset_dot_color(preset.effect, preset.params.color), 10.0),
                    text(&preset.name)
                        .size(12.5)
                        .font(if active {
                            theme::ui_semibold(t.design)
                        } else {
                            font
                        }),
                    Space::new().width(Length::Fill),
                    text(preset.effect.label())
                        .size(10.5)
                        .font(font)
                        .color(theme::rgb(p.ink_3)),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .padding([7, 10])
            .width(Length::Fill)
            .style(theme::card_button(t, active))
            .on_press(Message::ApplyPreset(preset.id.clone())),
            button(text(if trying { "Trying" } else { "Try" }).size(11).font(font))
                .padding([7, 9])
                .style(theme::btn_secondary(t, trying))
                .on_press(Message::TryPreset(preset.id.clone())),
        ]
        .spacing(5)
        .align_y(Alignment::Center);

        if can_delete {
            prow = prow.push(
                button(text("×").size(11).font(font))
                    .padding([7, 9])
                    .style(theme::btn_secondary(t, false))
                    .on_press(Message::DeletePreset(preset.id.clone())),
            );
        }
        presets_list = presets_list.push(prow);
    }

    // Swatches (2 neat rows of 6 crisp 22px circles) + Interactive 132x132 HSV Color Wheel + #HEX input
    let mut swatches_col = column![].spacing(6);
    for chunk in CURATED_SWATCHES.chunks(6) {
        let mut sr = row![].spacing(6);
        for &col in chunk {
            let selected = active_params.color == col;
            let mut next = active_params.clone();
            next.color = col;
            if let Some(first) = next.colors.first_mut() {
                *first = col;
            }
            sr = sr.push(
                button(Space::new().width(16).height(16))
                    .padding(0)
                    .width(Length::Fixed(21.0))
                    .height(Length::Fixed(21.0))
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
        .width(Length::Fixed(132.0))
        .height(Length::Fixed(132.0)),
    )
    .map(|msg| match msg {
        ColorWheelMessage::PickRgb(rgb) => Message::LightingColorPicked(rgb),
    });

    let color_picker_block = row![
        color_wheel_el,
        column![
            row![
                color_dot(active_params.color, 13.0),
                text_input("#F48FB1", &app.lighting_hex_draft)
                    .size(12)
                    .font(theme::mono_font())
                    .style(theme::text_input_style(t))
                    .on_input(Message::LightingHexDraftChanged)
                    .width(Length::Fill),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
            text("Curated swatches")
                .size(10.5)
                .font(font)
                .color(theme::rgb(p.ink_3)),
            swatches_col,
        ]
        .spacing(7)
        .width(Length::Fill),
    ]
    .spacing(12)
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
        let mut sec_swatches = row![].spacing(6).align_y(Alignment::Center);
        for &col in CURATED_SWATCHES.iter().take(8) {
            let selected = active_params.secondary == col;
            let mut next = active_params.clone();
            next.secondary = col;
            if next.colors.len() >= 2 {
                next.colors[1] = col;
            }
            sec_swatches = sec_swatches.push(
                button(Space::new().width(14).height(14))
                    .padding(0)
                    .width(Length::Fixed(18.0))
                    .height(Length::Fixed(18.0))
                    .style(theme::swatch_btn(col, selected, t))
                    .on_press(Message::UpdateSceneParams(next)),
            );
        }
        Some(
            column![
                row![
                    color_dot(active_params.secondary, 9.0),
                    text("Secondary colour")
                        .size(11)
                        .font(font)
                        .color(theme::rgb(p.ink_2)),
                    Space::new().width(Length::Fill),
                    button(
                        text("⇄ Swap")
                            .size(10.5)
                            .font(theme::ui_semibold(t.design)),
                    )
                    .padding([3, 9])
                    .style(theme::btn_secondary(t, false))
                    .on_press(Message::SwapLightingColors),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
                sec_swatches,
            ]
            .spacing(5)
            .into(),
        )
    } else {
        None
    };

    let color_picker_block: Element<'_, Message> = if let Some(sec) = secondary_row {
        column![color_picker_block, sec].spacing(9).into()
    } else {
        color_picker_block.into()
    };

    let target_badge = if let Some(zone_cfg) = app.selected_zone_cfg() {
        if scene.zone_overrides.contains_key(&zone_cfg.id) {
            format!("Override: {}", truncate_str(&zone_cfg.name, 14))
        } else {
            format!("Editing: {}", truncate_str(&zone_cfg.name, 14))
        }
    } else {
        "Whole rig".to_string()
    };

    let mut inspector_col = column![
        row![
            text(active_effect.label())
                .size(19)
                .font(theme::display_semibold(t.design)),
            Space::new().width(Length::Fill),
            container(text(target_badge).size(11).font(font))
                .padding([4, 9])
                .style(theme::chip_style(
                    t,
                    if app.selected_zone.is_some() {
                        Some(p.brass_hi)
                    } else {
                        None
                    }
                )),
        ]
        .align_y(Alignment::Center),
        text(active_effect.description())
            .size(12)
            .font(font)
            .color(theme::rgb(p.ink_3)),
    ]
    .spacing(10);

    // Show live 24-band AudioVisualizerCanvas when audio effects are active!
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
        .height(Length::Fixed(52.0));
        inspector_col = inspector_col.push(
            container(audio_viz)
                .padding(6)
                .style(theme::well_style(t)),
        );
    }

    match active_effect {
        EffectId::Studio => {
            inspector_col = inspector_col.push(
                button(
                    container(
                        text("Open Studio Editor →")
                            .size(13)
                            .font(theme::ui_semibold(t.design)),
                    )
                    .center_x(Length::Fill),
                )
                .padding([9, 14])
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
                .push(labeled_slider(
                    t,
                    font,
                    "Colour boost (saturation)",
                    format!("{}%", active_params.saturation),
                    0,
                    100,
                    active_params.saturation as u16,
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
                .push(labeled_slider(
                    t,
                    font,
                    "Sensitivity",
                    format!("{}%", active_params.sensitivity),
                    10,
                    100,
                    active_params.sensitivity as u16,
                    move |v| {
                        let mut n = p_sens.clone();
                        n.sensitivity = v as u8;
                        Message::UpdateSceneParams(n)
                    },
                ))
                .push(labeled_slider(
                    t,
                    font,
                    "Hue shift",
                    format!("{}°", active_params.hue_shift),
                    0,
                    359,
                    active_params.hue_shift,
                    move |v| {
                        let mut n = p_hue.clone();
                        n.hue_shift = v;
                        Message::UpdateSceneParams(n)
                    },
                ))
                .push(labeled_slider(
                    t,
                    font,
                    "Direction",
                    format!("{}°", active_params.angle),
                    0,
                    359,
                    active_params.angle,
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
                .push(labeled_slider(
                    t,
                    font,
                    "Beat sensitivity",
                    format!("{}%", active_params.sensitivity),
                    10,
                    100,
                    active_params.sensitivity as u16,
                    move |v| {
                        let mut n = p_sens.clone();
                        n.sensitivity = v as u8;
                        Message::UpdateSceneParams(n)
                    },
                ))
                .push(labeled_slider(
                    t,
                    font,
                    "Idle floor glow",
                    format!("{}%", active_params.floor),
                    0,
                    60,
                    active_params.floor as u16,
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
            let mut src_btns = row![].spacing(2);
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
                    button(container(text(short).size(11).font(font)).center_x(Length::Fill))
                        .padding([5, 6])
                        .width(Length::Fill)
                        .style(theme::segmented_item_btn(t, active_params.temp_source == src))
                        .on_press(Message::UpdateSceneParams(n)),
                );
            }
            inspector_col = inspector_col
                .push(color_picker_block)
                .push(
                    container(src_btns)
                        .padding(3)
                        .width(Length::Fill)
                        .style(theme::segmented_rail_style(t)),
                )
                .push(labeled_slider(
                    t,
                    font,
                    "Cool threshold",
                    unit.format_temp(active_params.cool_temp as f32),
                    20,
                    75,
                    active_params.cool_temp as u16,
                    move |v| {
                        let mut n = p_cool.clone();
                        n.cool_temp = v as u8;
                        Message::UpdateSceneParams(n)
                    },
                ))
                .push(labeled_slider(
                    t,
                    font,
                    "Hot threshold",
                    unit.format_temp(active_params.hot_temp as f32),
                    40,
                    100,
                    active_params.hot_temp as u16,
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
                .push(labeled_slider(
                    t,
                    font,
                    "Speed",
                    format!("{}", active_params.speed),
                    0,
                    100,
                    active_params.speed as u16,
                    move |v| {
                        let mut n = p_spd.clone();
                        n.speed = v as u8;
                        Message::UpdateSceneParams(n)
                    },
                ))
                .push(labeled_slider(
                    t,
                    font,
                    "Direction",
                    format!("{}°", active_params.angle),
                    0,
                    359,
                    active_params.angle,
                    move |v| {
                        let mut n = p_ang.clone();
                        n.angle = v;
                        Message::UpdateSceneParams(n)
                    },
                ));
        }
    }

    // Master Brightness slider
    inspector_col = inspector_col.push(labeled_slider(
        t,
        font,
        "Brightness",
        format!("{}%", scene.brightness),
        0,
        100,
        scene.brightness as u16,
        |v| Message::SetBrightness(v as u8),
    ));

    let sidebar = column![
        container(
            column![
                text("Profiles")
                    .size(13.5)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(p.ink_2)),
                presets_list,
                row![
                    text_input("New profile name", &app.preset_name_draft)
                        .style(theme::text_input_style(t))
                        .on_input(Message::PresetNameDraftChanged)
                        .width(Length::Fill),
                    button(text("+ Save").size(11.5).font(theme::ui_semibold(t.design)))
                        .padding([7, 11])
                        .style(theme::btn_primary(t))
                        .on_press(Message::SaveCurrentPreset),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            ]
            .spacing(10),
        )
        .padding(t.pad(15))
        .style(theme::panel_style(t)),
        container(inspector_col)
            .padding(t.pad(15))
            .style(theme::panel_style(t)),
    ]
    .spacing(t.sp(14))
    .width(Length::FillPortion(4));

    row![left_col, sidebar].spacing(t.sp(14)).into()
}

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

    let mut speed_mult_row = row![].spacing(2);
    for (mult, label) in [(0.5_f32, "0.5×"), (1.0_f32, "1×"), (2.0_f32, "2×")] {
        let active = (app.snapshot.studio_speed_mult - mult).abs() < 0.05;
        speed_mult_row = speed_mult_row.push(
            button(text(label).size(11).font(font))
                .padding([5, 9])
                .style(theme::segmented_item_btn(t, active))
                .on_press(Message::SetStudioSpeedMult(mult)),
        );
    }

    let undo_btn = if can_undo {
        button(text("Undo").size(12).font(font))
            .padding([6, 11])
            .style(theme::btn_secondary(t, false))
            .on_press(Message::StudioUndo)
    } else {
        button(text("Undo").size(12).font(font))
            .padding([6, 11])
            .style(theme::btn_secondary(t, false))
    };

    let redo_btn = if can_redo {
        button(text("Redo").size(12).font(font))
            .padding([6, 11])
            .style(theme::btn_secondary(t, false))
            .on_press(Message::StudioRedo)
    } else {
        button(text("Redo").size(12).font(font))
            .padding([6, 11])
            .style(theme::btn_secondary(t, false))
    };

    let top_bar = container(
        row![
            text(format!("Studio · {}", stack_cfg.name))
                .size(18)
                .font(theme::display_semibold(t.design)),
            Space::new().width(8),
            button(text("Surprise me").size(12).font(font))
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
            .padding([6, 11])
            .style(theme::btn_secondary(t, app.snapshot.studio_paused))
            .on_press(Message::ToggleStudioPaused),
            container(speed_mult_row)
                .padding(2)
                .style(theme::segmented_rail_style(t)),
            button(
                text(if is_live {
                    "Live on lights ✓"
                } else {
                    "Use on my lights"
                })
                .size(12)
                .font(theme::ui_semibold(t.design)),
            )
            .padding([7, 14])
            .style(theme::btn_primary(t))
            .on_press(Message::SelectEffect(EffectId::Studio)),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([12, 16])
    .style(theme::panel_style(t));

    // Left column: Layer stack + Add Layer + Saved Studio Library
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
                button(text(if layer.enabled { "On" } else { "Off" }).size(10).font(font))
                    .padding([8, 8])
                    .style(theme::btn_secondary(t, layer.enabled))
                    .on_press(Message::UpdateSelectedLayer(toggled)),
                button(text("↑").size(10).font(font))
                    .padding([8, 6])
                    .style(theme::btn_secondary(t, false))
                    .on_press(Message::MoveLayer(layer.id, true)),
                button(text("↓").size(10).font(font))
                    .padding([8, 6])
                    .style(theme::btn_secondary(t, false))
                    .on_press(Message::MoveLayer(layer.id, false)),
                button(text("×").size(10).font(font))
                    .padding([8, 7])
                    .style(theme::btn_secondary(t, false))
                    .on_press(Message::RemoveLayer(layer.id)),
            ]
            .spacing(3)
            .align_y(Alignment::Center),
        );
    }

    let add_row = |kinds: &[LayerKind]| {
        let mut r = row![].spacing(4);
        for &kind in kinds {
            r = r.push(
                button(
                    container(text(format!("+ {}", kind.label())).size(10.5).font(font))
                        .center_x(Length::Fill),
                )
                .padding([5, 6])
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

    // Saved Studio effects library
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
                .padding([7, 12])
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
                button(text("Dup").size(10).font(font))
                    .padding([6, 8])
                    .style(theme::btn_secondary(t, false))
                    .on_press(Message::DuplicateStudioEffect(saved.id.clone())),
                button(text("×").size(10).font(font))
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
        .padding(t.pad(14))
        .style(theme::panel_style(t)),
        container(library_col)
            .padding(t.pad(14))
            .style(theme::panel_style(t)),
    ]
    .spacing(t.sp(14))
    .width(Length::FillPortion(4));

    // Center: Real-Rig Live Studio Preview (uses studio_preview_frames when Studio isn't active on live lights)
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
    })
    .width(Length::Fill)
    .height(Length::Fixed(274.0));
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
    .height(Length::Fixed(46.0));

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
            container(stage_el).clip(true).style(move |_| {
                iced::widget::container::Style {
                    border: iced::Border {
                        radius: iced::border::Radius::from(if p.square_corners {
                            0.0
                        } else {
                            12.0
                        }),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            }),
            container(audio_eq)
                .padding([5, 8])
                .style(theme::well_style(t)),
            labeled_slider(
                t,
                font,
                "Master speed (all layers)",
                format!("{}%", stack_cfg.master_speed),
                10,
                200,
                stack_cfg.master_speed as u16,
                Message::SetStudioMasterSpeed,
            ),
            text("ONE-CLICK COLOUR HARMONIES")
                .size(10.5)
                .font(theme::ui_semibold(t.design))
                .color(theme::rgb(t.pink_rose)),
            harmony_palette_grid(t, font, 2),
        ]
        .spacing(10),
    )
    .padding(t.pad(15))
    .width(Length::FillPortion(5))
    .style(theme::panel_style(t));

    // Right: Layer inspector
    let inspector_body: Element<'_, Message> = if let Some(layer) = app
        .selected_layer
        .and_then(|id| stack_cfg.layers.iter().find(|l| l.id == id))
        .or_else(|| stack_cfg.layers.first())
    {
        let mut blend_row = row![].spacing(2);
        for b in BlendMode::ALL {
            let mut next = layer.clone();
            next.blend = b;
            blend_row = blend_row.push(
                button(container(text(b.label()).size(11).font(font)).center_x(Length::Fill))
                    .padding([5, 6])
                    .width(Length::Fill)
                    .style(theme::segmented_item_btn(t, layer.blend == b))
                    .on_press(Message::UpdateSelectedLayer(next)),
            );
        }

        let studio_wheel = Element::from(
            iced::widget::canvas(ColorWheelCanvas {
                rgb: layer.color,
                palette: p,
                accent_override: t.accent_override,
            })
            .width(Length::Fixed(104.0))
            .height(Length::Fixed(104.0)),
        )
        .map(|msg| match msg {
            ColorWheelMessage::PickRgb(rgb) => Message::StudioColorPicked(rgb),
        });

        let mut swatches_primary_a = row![].spacing(5);
        for col in CURATED_SWATCHES.iter().take(6) {
            let mut next = layer.clone();
            next.color = *col;
            swatches_primary_a = swatches_primary_a.push(
                button(Space::new().width(15).height(15))
                    .padding(0)
                    .width(Length::Fixed(19.0))
                    .height(Length::Fixed(19.0))
                    .style(theme::swatch_btn(*col, layer.color == *col, t))
                    .on_press(Message::UpdateSelectedLayer(next)),
            );
        }
        let mut swatches_primary_b = row![].spacing(5);
        for col in CURATED_SWATCHES.iter().skip(6).take(6) {
            let mut next = layer.clone();
            next.color = *col;
            swatches_primary_b = swatches_primary_b.push(
                button(Space::new().width(15).height(15))
                    .padding(0)
                    .width(Length::Fixed(19.0))
                    .height(Length::Fixed(19.0))
                    .style(theme::swatch_btn(*col, layer.color == *col, t))
                    .on_press(Message::UpdateSelectedLayer(next)),
            );
        }

        let mut swatches_secondary = row![].spacing(6);
        for col in CURATED_SWATCHES.iter().take(9) {
            let mut next = layer.clone();
            next.secondary = *col;
            swatches_secondary = swatches_secondary.push(
                button(Space::new().width(15).height(15))
                    .padding(0)
                    .width(Length::Fixed(19.0))
                    .height(Length::Fixed(19.0))
                    .style(theme::swatch_btn(*col, layer.secondary == *col, t))
                    .on_press(Message::UpdateSelectedLayer(next)),
            );
        }

        let mut rev_next = layer.clone();
        rev_next.reverse = !rev_next.reverse;

        column![
            text(format!("LAYER · {}", layer.kind.label().to_uppercase()))
                .size(12)
                .font(theme::ui_semibold(t.design))
                .color(theme::rgb(p.brass_hi)),
            text("Blend mode").size(12).font(font),
            container(blend_row)
                .padding(3)
                .width(Length::Fill)
                .style(theme::segmented_rail_style(t)),
            text("Primary colour").size(12).font(font),
            row![
                studio_wheel,
                column![
                    row![
                        color_dot(layer.color, 12.0),
                        text_input("#F48FB1", &app.studio_hex_draft)
                            .size(11.5)
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
                .spacing(6)
                .width(Length::Fill),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            text("Secondary colour").size(12).font(font),
            swatches_secondary,
            labeled_slider(
                t,
                font,
                "Opacity",
                format!("{}%", layer.opacity),
                0,
                100,
                layer.opacity as u16,
                |v| {
                    let mut n = layer.clone();
                    n.opacity = v as u8;
                    Message::UpdateSelectedLayer(n)
                },
            ),
            labeled_slider(
                t,
                font,
                "Speed",
                format!("{}", layer.speed),
                0,
                100,
                layer.speed as u16,
                |v| {
                    let mut n = layer.clone();
                    n.speed = v as u8;
                    Message::UpdateSelectedLayer(n)
                },
            ),
            labeled_slider(
                t,
                font,
                "Scale / Tail",
                format!("{}", layer.scale),
                5,
                100,
                layer.scale as u16,
                |v| {
                    let mut n = layer.clone();
                    n.scale = v as u8;
                    Message::UpdateSelectedLayer(n)
                },
            ),
            labeled_slider(
                t,
                font,
                "Direction",
                format!("{}°", layer.angle),
                0,
                359,
                layer.angle,
                |v| {
                    let mut n = layer.clone();
                    n.angle = v;
                    Message::UpdateSelectedLayer(n)
                },
            ),
            labeled_slider(
                t,
                font,
                "Music reactivity",
                format!("{}%", layer.react),
                0,
                100,
                layer.react as u16,
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
        .spacing(9)
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
        .style(theme::panel_style(t));

    column![top_bar, row![left_panel, center_panel, right_panel].spacing(t.sp(14))]
        .spacing(t.sp(14))
        .into()
}
