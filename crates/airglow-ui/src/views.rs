//! Shared UI instrument helpers (pill toggle, segmented rail, hero readouts, dusk masthead)
//! plus the Overview (with Customize drawer, Firefly particles, upgraded Cost card & Activity strip)
//! and Fans (with Thermal Peak badges, Acoustic Hysteresis slider & 7 LHM channels) views.

use airglow_core::config::{FanChannelMode, TempSource, TempUnit};
use airglow_core::effects::EffectId;
use airglow_core::sensors;
use iced::widget::{Space, button, column, container, image, row, slider, stack, text};
use iced::{Alignment, ContentFit, Element, Length};

use crate::canvas_ambient::FireflyParticlesCanvas;
use crate::canvas_curves::{CurveMessage, FanCurveCanvas};
use crate::canvas_stage::CaseStageCanvas;
use crate::canvas_widgets::{
    PowerGraphCanvas, PulseGraphCanvas, RingGaugeCanvas, SegmentedBarCanvas, SparklineCanvas,
};
use crate::theme::{self, DesignTokens};
use crate::views_palette;
use crate::{AirglowApp, Message, PulseSpan};

pub const CURATED_SWATCHES: [[u8; 3]; 12] = [
    [255, 255, 255],
    [239, 90, 72],
    [255, 122, 26],
    [242, 185, 75],
    [124, 197, 142],
    [0, 224, 200],
    [0, 155, 222],
    [47, 91, 255],
    [138, 61, 255],
    [255, 47, 180],
    [255, 160, 122],
    [186, 224, 255],
];

pub fn preset_dot_color(effect: EffectId, color: [u8; 3]) -> [u8; 3] {
    match effect {
        EffectId::Rainbow | EffectId::Cycle => [255, 70, 160],
        EffectId::Temperature | EffectId::Heatmap => [70, 185, 255],
        EffectId::Aurora => [110, 232, 196],
        EffectId::FireflySwarm => [255, 195, 139],
        EffectId::MeteorShower => [244, 143, 177],
        EffectId::Screen => [255, 145, 60],
        EffectId::Spectrum => [0, 224, 160],
        EffectId::Pulse => [160, 80, 255],
        EffectId::Studio => [0, 224, 200],
        EffectId::Off => [70, 74, 80],
        _ => color,
    }
}

pub fn thermal_temp_color(temp_c: Option<f32>, t: DesignTokens, normal: u32) -> u32 {
    match temp_c {
        Some(v) if v >= 90.0 => t.p.hot,
        Some(v) if v >= 80.0 => t.p.warm,
        _ => normal,
    }
}

pub fn pill_toggle<'a>(t: DesignTokens, on: bool, on_press: Message) -> Element<'a, Message> {
    let knob = container(Space::new().width(16).height(16)).style(theme::toggle_knob_style(t, on));
    let track_inner = if on {
        row![Space::new().width(Length::Fill), knob]
    } else {
        row![knob, Space::new().width(Length::Fill)]
    }
    .align_y(Alignment::Center);

    button(track_inner)
        .width(Length::Fixed(42.0))
        .height(Length::Fixed(22.0))
        .padding([2, 3])
        .style(theme::toggle_track_btn(t, on))
        .on_press(on_press)
        .into()
}

pub fn view_conflict_banner<'a>(
    app: &'a AirglowApp,
    t: DesignTokens,
    font: iced::Font,
) -> Option<Element<'a, Message>> {
    if app.snapshot.conflicts.is_empty() {
        return None;
    }
    let p = t.p;

    let mut items = column![
        text("CONFLICTING RGB SOFTWARE DETECTED")
            .size(11)
            .font(theme::ui_semibold(t.design))
            .color(theme::rgb(p.warm)),
    ]
    .spacing(6);

    for app_c in &app.snapshot.conflicts {
        let mut r = row![
            text(format!(
                "{} is running — can fight Airglow over {}.",
                app_c.name, app_c.conflicts_with
            ))
            .size(12)
            .font(font)
            .color(theme::rgb(p.ink))
            .width(Length::Fill),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        if app_c.is_service {
            r = r.push(
                text("Windows service (stop in Services)")
                    .size(11)
                    .font(font)
                    .color(theme::rgb(p.ink_3)),
            );
        } else {
            r = r.push(
                button(text(format!("Close {}", app_c.name)).size(11).font(font))
                    .padding([5, 11])
                    .style(theme::btn_danger(t))
                    .on_press(Message::CloseConflictingApp(app_c.name.clone())),
            );
        }
        items = items.push(r);
    }

    Some(
        container(items)
            .padding([10, 14])
            .style(theme::banner_style(t, p.warm))
            .into(),
    )
}

fn slice_tail(data: &[f32], max_len: usize) -> &[f32] {
    if data.len() > max_len {
        &data[data.len() - max_len..]
    } else {
        data
    }
}

fn sleep_pill_label(remaining_s: Option<u32>) -> String {
    match remaining_s {
        Some(rem) => format!("☾ {:02}:{:02}", rem / 60, rem % 60),
        None => "☾ Sleep".to_string(),
    }
}

fn view_overview_masthead<'a>(
    app: &'a AirglowApp,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let s = &app.snapshot.sensors;
    let appr = &app.snapshot.config.appearance;
    let unit = app.snapshot.config.settings.temp_unit;
    let display_font = theme::display_font(t.design);
    let mono = theme::mono_font();

    let active_zones = app.snapshot.frames.iter().filter(|f| f.enabled).count();
    let total_leds: usize = app
        .snapshot
        .frames
        .iter()
        .filter(|f| f.enabled)
        .map(|f| f.colors.len())
        .sum();
    let lhm_badge = if s.lhm_connected {
        "LHM Live"
    } else {
        "Demo telemetry"
    };

    // Quick header action pills (Customize, Sleep timer cycle, Ctrl+K)
    let sleep_active = app.snapshot.sleep_timer_remaining_s.is_some();
    let quick_pills = row![
        button(
            text(if app.customize_open {
                "✦ Customize ✓"
            } else {
                "✦ Customize"
            })
            .size(11.5)
            .font(theme::ui_semibold(t.design)),
        )
        .padding([5, 11])
        .style(theme::pill_action_btn(t, app.customize_open))
        .on_press(Message::ToggleCustomizeDrawer),
        button(
            text(sleep_pill_label(app.snapshot.sleep_timer_remaining_s))
                .size(11.5)
                .font(if sleep_active { mono } else { font }),
        )
        .padding([5, 11])
        .style(theme::pill_action_btn(t, sleep_active))
        .on_press(Message::CycleSleepTimer),
    ]
    .spacing(7)
    .align_y(Alignment::Center);

    // Living-Light Pulse Strip with working 1 min | 5 min | 10 min buttons
    let cpu_t = sensors::format_temp(s.cpu_temp, unit);
    let gpu_t = sensors::format_temp(s.gpu_temp, unit);
    let max_n = app.pulse_span.max_samples();
    let cpu_slice = slice_tail(&app.snapshot.cpu_temp_history, max_n);
    let gpu_slice = slice_tail(&app.snapshot.gpu_temp_history, max_n);

    let pulse_canvas = iced::widget::canvas(PulseGraphCanvas {
        series_a: cpu_slice,
        series_b: gpu_slice,
        color_a: t.data_primary,
        color_b: t.data_secondary,
        temp_unit: unit,
        palette: p,
        accent_override: t.accent_override,
        anim_t: app.anim_t,
    })
    .width(Length::Fill)
    .height(Length::Fixed(26.0));

    let mut span_rail = row![].spacing(2);
    for sp in PulseSpan::ALL {
        let active = app.pulse_span == sp;
        span_rail = span_rail.push(
            button(text(sp.label()).size(10.5).font(font))
                .padding([3, 8])
                .style(theme::segmented_item_btn(t, active))
                .on_press(Message::SetPulseSpan(sp)),
        );
    }

    let pulse_band = container(
        row![
            row![
                dash_swatch(t.data_primary),
                text("CPU").size(11).font(font).color(theme::rgb(p.ink_3)),
                text(cpu_t)
                    .size(13)
                    .font(mono)
                    .color(theme::rgb(thermal_temp_color(s.cpu_temp, t, p.ink))),
                Space::new().width(10),
                dash_swatch(t.data_secondary),
                text("GPU").size(11).font(font).color(theme::rgb(p.ink_3)),
                text(gpu_t)
                    .size(13)
                    .font(mono)
                    .color(theme::rgb(thermal_temp_color(s.gpu_temp, t, p.ink))),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
            pulse_canvas,
            container(span_rail)
                .padding(2)
                .style(theme::segmented_rail_style(t)),
        ]
        .spacing(16)
        .align_y(Alignment::Center),
    )
    .padding([5, 16])
    .width(Length::Fill);

    let divider = container(Space::new().width(Length::Fill).height(1)).style(move |_| {
        iced::widget::container::Style {
            background: Some(iced::Background::Color(theme::rgba(t.line_soft, 0.8))),
            ..Default::default()
        }
    });

    if !t.is_firefly() {
        let compact_top = container(
            row![
                column![
                    text("My workspace")
                        .size(22)
                        .font(display_font)
                        .color(theme::rgb(p.ink)),
                    row![
                        status_dot(if s.lhm_connected { p.ok } else { p.warm }, 7.0),
                        text(format!(
                            "{lhm_badge} · {active_zones} zones · {total_leds} LEDs · {}% fans",
                            app.snapshot.fan_duty
                        ))
                        .size(11.5)
                        .font(font)
                        .color(theme::rgb(p.ink_3)),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                ]
                .spacing(2),
                Space::new().width(Length::Fill),
                quick_pills,
            ]
            .align_y(Alignment::Center),
        )
        .padding([10, 18])
        .width(Length::Fill);

        return container(column![compact_top, divider, pulse_band].spacing(0))
            .width(Length::Fill)
            .style(theme::panel_style(t))
            .into();
    }

    // Firefly Dusk Bellflower & Lantern Moth Hero Masthead with Bioluminescent Particles
    let dusk_bg = image(&app.dusk_art)
        .content_fit(ContentFit::Cover)
        .width(Length::Fill)
        .height(Length::Fill);

    let scrim = container(Space::new().width(Length::Fill).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::hero_scrim_style(t));

    let particles = iced::widget::canvas(FireflyParticlesCanvas {
        palette: p,
        accent_override: t.accent_override,
        anim_t: app.anim_t,
        enabled: appr.firefly_particles,
        show_lantern_aura: true,
    })
    .width(Length::Fill)
    .height(Length::Fill);

    let moth_img = image(&app.moth_art)
        .content_fit(ContentFit::Contain)
        .width(Length::Fixed(78.0))
        .height(Length::Fixed(72.0));

    let hero_fg = container(
        row![
            column![
                text("My workspace")
                    .size(28)
                    .font(display_font)
                    .color(theme::rgb(p.ink)),
                row![
                    status_dot(if s.lhm_connected { p.ok } else { p.warm }, 8.0),
                    text(format!(
                        "{lhm_badge} · {active_zones} zones · {total_leds} LEDs · {}% fans",
                        app.snapshot.fan_duty
                    ))
                    .size(12)
                    .font(font)
                    .color(theme::rgb(p.ink_2)),
                ]
                .spacing(7)
                .align_y(Alignment::Center),
            ]
            .spacing(3),
            Space::new().width(Length::Fill),
            moth_img,
            Space::new().width(Length::Fixed(20.0)),
            quick_pills,
        ]
        .align_y(Alignment::Center),
    )
    .padding([8, 20])
    .width(Length::Fill)
    .height(Length::Fill);

    let top_band = stack![dusk_bg, scrim, particles, hero_fg]
        .width(Length::Fill)
        .height(Length::Fixed(88.0));

    container(column![top_band, divider, pulse_band].spacing(0))
        .width(Length::Fill)
        .clip(true)
        .style(theme::panel_style(t))
        .into()
}

pub fn view_overview<'a>(
    app: &'a AirglowApp,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let s = &app.snapshot.sensors;
    let eff = &app.snapshot.effective_scene;
    let appr = &app.snapshot.config.appearance;
    let unit = app.snapshot.config.settings.temp_unit;
    let mono = theme::mono_font();
    let gap = t.sp(8);

    let masthead = view_overview_masthead(app, t, font);

    // Top-left hero Living Rig tile
    let stage = iced::widget::canvas(CaseStageCanvas {
        frames: &app.snapshot.frames,
        zone_overrides: &eff.zone_overrides,
        palette: p,
        accent_override: t.accent_override,
        rig_style: appr.rig_style,
        airflow_lines: appr.airflow_lines,
        spinning_fans: appr.spinning_fans,
        ambient_bloom: appr.ambient_bloom,
        lights_on: eff.on,
        show_labels: false,
        arrange_mode: false,
        selected_zone: None,
        active_effect: eff.effect,
        temp_unit: unit,
        anim_t: app.anim_t,
        fan_duty: app.snapshot.fan_duty,
        cpu_temp: s.cpu_temp,
        gpu_temp: s.gpu_temp,
        ambient_rgb: app.snapshot.average_rgb,
        eyebrow: Some("LIVING RIG"),
    })
    .width(Length::Fill)
    .height(Length::Fixed(204.0));
    let stage_el = Element::from(stage).map(Message::Stage);

    let owner_label = if let Some(rule_id) = &app.snapshot.automation_status.light_winner {
        format!("Controlled by automation ({rule_id})")
    } else if !eff.zone_overrides.is_empty() {
        format!(
            "Your own settings · {} zone override(s)",
            eff.zone_overrides.len()
        )
    } else {
        "Your own settings".to_string()
    };

    let active_effect_title = if eff.effect == EffectId::Studio {
        eff.studio_stack.name.clone()
    } else {
        eff.effect.label().to_string()
    };

    let rig_tile = container(
        column![
            container(stage_el)
                .clip(true)
                .style(move |_| iced::widget::container::Style {
                    border: iced::Border {
                        radius: iced::border::Radius::from(if p.square_corners {
                            0.0
                        } else if t.is_firefly() {
                            12.0
                        } else {
                            6.0
                        }),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
            row![
                column![
                    text(active_effect_title)
                        .size(16)
                        .font(theme::display_semibold(t.design)),
                    text(owner_label)
                        .size(12)
                        .font(font)
                        .color(theme::rgb(p.ink_3)),
                ]
                .spacing(2)
                .width(Length::Fill),
                row![
                    slider(
                        0.0..=100.0,
                        app.snapshot.config.scene.brightness as f32,
                        |v| Message::SetBrightness(v.round() as u8)
                    )
                    .style(theme::slider_style(t))
                    .width(Length::Fixed(148.0)),
                    text(format!("{}%", app.snapshot.config.scene.brightness))
                        .size(11.5)
                        .font(mono)
                        .color(theme::rgb(p.ink_2))
                        .width(Length::Fixed(34.0)),
                    pill_toggle(t, eff.on, Message::TogglePower),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            ]
            .align_y(Alignment::Center)
            .spacing(10),
        ]
        .spacing(t.sp(10)),
    )
    .padding(t.pad(12))
    .width(Length::FillPortion(6))
    .style(theme::panel_style(t));

    // Top-right 2x2 sensor grid
    let (cpu_num, cpu_u) = split_temp(s.cpu_temp, unit);
    let (gpu_num, gpu_u) = split_temp(s.gpu_temp, unit);
    let cpu_sub = temp_range_label(&app.snapshot.cpu_temp_history, unit);
    let gpu_sub = temp_range_label(&app.snapshot.gpu_temp_history, unit);

    let cpu_temp_tile = spark_metric_tile(
        t,
        font,
        "Processor temperature",
        &cpu_num,
        cpu_u,
        &cpu_sub,
        &app.snapshot.cpu_temp_history,
        25.0,
        95.0,
        t.data_primary,
        thermal_temp_color(s.cpu_temp, t, p.ink),
        app.anim_t,
        34,
        None,
    );
    let gpu_temp_tile = spark_metric_tile(
        t,
        font,
        "Graphics temperature",
        &gpu_num,
        gpu_u,
        &gpu_sub,
        &app.snapshot.gpu_temp_history,
        25.0,
        95.0,
        if t.is_firefly() {
            t.data_secondary
        } else {
            t.data_primary
        },
        thermal_temp_color(s.gpu_temp, t, p.ink),
        app.anim_t,
        34,
        None,
    );

    let cpu_load_tile = ring_metric_tile(
        t,
        font,
        "Processor load",
        &short_hw_name(&s.cpu_name),
        s.cpu_load,
        if t.is_firefly() {
            t.data_secondary
        } else {
            p.ink
        },
    );
    let gpu_load_tile = ring_metric_tile(
        t,
        font,
        "Graphics load",
        &short_hw_name(&s.gpu_name),
        s.gpu_load,
        if t.is_firefly() { t.pink_rose } else { p.ink },
    );

    let right_grid = column![
        row![cpu_temp_tile, gpu_temp_tile].spacing(gap),
        row![cpu_load_tile, gpu_load_tile].spacing(gap),
    ]
    .spacing(gap)
    .width(Length::FillPortion(5));

    let top_row = row![rig_tile, right_grid].spacing(gap);

    // Middle row: Profiles + Power draw graph (each toggleable in Customize!)
    let show_profiles = appr.is_tile_visible("profiles");
    let show_power = appr.is_tile_visible("power");

    let mid_row: Option<Element<'a, Message>> = if show_profiles || show_power {
        let mut r = row![].spacing(gap);
        if show_profiles {
            let mut presets_row = row![].spacing(8);
            for preset in app.snapshot.config.presets.iter().take(5) {
                let active = app.snapshot.config.active_preset.as_deref() == Some(&preset.id);
                let trying = app
                    .snapshot
                    .trial
                    .as_ref()
                    .is_some_and(|tr| tr.preset_id == preset.id);
                let dot_col = preset_dot_color(preset.effect, preset.params.color);

                let top_dots = row![
                    color_dot(dot_col, 11.0),
                    Space::new().width(Length::Fill),
                    if active {
                        status_dot(p.brass_hi, 6.0)
                    } else {
                        Space::new().width(6).height(6).into()
                    },
                ]
                .align_y(Alignment::Center);

                presets_row = presets_row.push(
                    container(
                        column![
                            button(
                                column![
                                    top_dots,
                                    Space::new().height(4),
                                    text(&preset.name)
                                        .size(12.5)
                                        .font(theme::ui_semibold(t.design)),
                                ]
                                .spacing(2)
                                .width(Length::Fill),
                            )
                            .padding(0)
                            .width(Length::Fill)
                            .style(theme::btn_frameless(t))
                            .on_press(Message::ApplyPreset(preset.id.clone())),
                            row![
                                button(
                                    text(truncate_str(preset.effect.label(), 11))
                                        .size(10.5)
                                        .font(font)
                                        .color(theme::rgb(p.ink_3)),
                                )
                                .padding(0)
                                .width(Length::Fill)
                                .style(theme::btn_frameless(t))
                                .on_press(Message::ApplyPreset(preset.id.clone())),
                                button(
                                    text(if trying { "Trying" } else { "Try" })
                                        .size(9.5)
                                        .font(theme::ui_semibold(t.design)),
                                )
                                .padding([2, 7])
                                .style(theme::in_card_try_pill(t, trying))
                                .on_press(Message::TryPreset(preset.id.clone())),
                            ]
                            .spacing(4)
                            .align_y(Alignment::Center),
                        ]
                        .spacing(5)
                        .width(Length::Fill),
                    )
                    .padding([8, 9])
                    .width(Length::Fill)
                    .style(theme::profile_card_frame(t, active || trying)),
                );
            }

            let profiles_tile = container(
                column![
                    row![
                        text("Profiles")
                            .size(13)
                            .font(theme::ui_semibold(t.design))
                            .color(theme::rgb(p.ink_2)),
                        Space::new().width(Length::Fill),
                        button(text("Lighting →").size(11).font(font))
                            .padding([3, 9])
                            .style(theme::btn_secondary(t, false))
                            .on_press(Message::SelectTab(crate::Tab::Lighting)),
                    ]
                    .align_y(Alignment::Center),
                    presets_row,
                ]
                .spacing(8),
            )
            .padding(t.pad(13))
            .width(Length::FillPortion(6))
            .style(theme::panel_style(t));

            r = r.push(profiles_tile);
        }

        if show_power {
            let cpu_w = s
                .cpu_power_w
                .map(|v| format!("{v:.0} W"))
                .unwrap_or_else(|| "—".to_string());
            let gpu_w = s
                .gpu_power_w
                .map(|v| format!("{v:.0} W"))
                .unwrap_or_else(|| "—".to_string());
            let power_chart = iced::widget::canvas(PowerGraphCanvas {
                cpu_w: &app.snapshot.cpu_power_history,
                gpu_w: &app.snapshot.gpu_power_history,
                palette: p,
                accent_override: t.accent_override,
                anim_t: app.anim_t,
            })
            .width(Length::Fill)
            .height(Length::Fixed(74.0));

            let power_tile = container(
                column![
                    row![
                        text("Power draw")
                            .size(13)
                            .font(theme::ui_semibold(t.design))
                            .color(theme::rgb(p.ink_2)),
                        Space::new().width(Length::Fill),
                        dash_swatch(t.data_primary),
                        text(format!("CPU {cpu_w}"))
                            .size(11.5)
                            .font(mono)
                            .color(theme::rgb(t.data_primary)),
                        Space::new().width(8),
                        dash_swatch(t.data_secondary),
                        text(format!("GPU {gpu_w}"))
                            .size(11.5)
                            .font(mono)
                            .color(theme::rgb(t.data_secondary)),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                    power_chart,
                ]
                .spacing(8),
            )
            .padding(t.pad(13))
            .width(Length::FillPortion(5))
            .style(theme::panel_style(t));

            r = r.push(power_tile);
        }
        Some(r.into())
    } else {
        None
    };

    // Instrumentation Row A: Case fans, Memory in use, Drive temperature, Download speed
    let mut row_a_tiles: Vec<Element<'a, Message>> = Vec::new();

    if appr.is_tile_visible("fans") {
        let mut curve_seg = row![].spacing(2);
        for name in ["silent", "balanced", "performance"] {
            let active = app.snapshot.effective_curve == name;
            let label = match name {
                "silent" => "Silent",
                "balanced" => "Balanced",
                _ => "Perf",
            };
            curve_seg = curve_seg.push(
                button(container(text(label).size(11).font(font)).center_x(Length::Fill))
                    .padding([5, 4])
                    .width(Length::Fill)
                    .style(theme::segmented_item_btn(t, active))
                    .on_press(Message::SelectFanCurve(name.to_string())),
            );
        }

        let avg_rpm = if !app.snapshot.fan_rpms.is_empty() {
            let sum: u32 = app.snapshot.fan_rpms.iter().map(|(_, r)| *r as u32).sum();
            let avg = sum / app.snapshot.fan_rpms.len() as u32;
            format!("{avg} rpm across {} fans", app.snapshot.fan_rpms.len())
        } else if !app.snapshot.lhm_channels.is_empty() {
            let rpms: Vec<u16> = app
                .snapshot
                .lhm_channels
                .iter()
                .filter_map(|c| c.rpm)
                .collect();
            if rpms.is_empty() {
                format!("{} LHM fan channels", app.snapshot.lhm_channels.len())
            } else {
                let avg = rpms.iter().map(|&r| r as u32).sum::<u32>() / rpms.len() as u32;
                format!("{avg} rpm across {} LHM channels", rpms.len())
            }
        } else {
            "No tach signal".to_string()
        };

        row_a_tiles.push(
            container(
                column![
                    row![
                        text("Case fans")
                            .size(12)
                            .font(font)
                            .color(theme::rgb(p.ink_2)),
                        Space::new().width(Length::Fill),
                        text(cap_first(&app.snapshot.effective_curve))
                            .size(11)
                            .font(font)
                            .color(theme::rgb(p.brass_hi)),
                    ],
                    hero_value_row(
                        &app.snapshot.fan_duty.to_string(),
                        "%",
                        30,
                        13,
                        p.ink,
                        p.ink_3,
                        t,
                    ),
                    text(avg_rpm)
                        .size(11)
                        .font(font)
                        .color(theme::rgb(p.ink_3)),
                    container(curve_seg)
                        .padding(2)
                        .width(Length::Fill)
                        .style(theme::segmented_rail_style(t)),
                ]
                .spacing(5),
            )
            .padding(t.pad(13))
            .width(Length::Fill)
            .style(theme::panel_style(t))
            .into(),
        );
    }

    if appr.is_tile_visible("memory") {
        let ram_bar = iced::widget::canvas(SegmentedBarCanvas {
            fraction: s.ram_load / 100.0,
            accent: if t.is_firefly() {
                t.data_primary
            } else {
                p.ink
            },
            palette: p,
            accent_override: t.accent_override,
            segments: 24,
        })
        .width(Length::Fill)
        .height(Length::Fixed(13.0));

        let ram_detail = match (s.ram_used_gb, s.ram_total_gb) {
            (Some(u), Some(t_gb)) => format!("{u:.1} / {t_gb:.0} GB"),
            _ => "0% to 100%".to_string(),
        };

        row_a_tiles.push(
            container(
                column![
                    row![
                        text("Memory in use")
                            .size(12)
                            .font(font)
                            .color(theme::rgb(p.ink_2)),
                        Space::new().width(Length::Fill),
                        text("Windows")
                            .size(11)
                            .font(font)
                            .color(theme::rgb(p.ink_3)),
                    ],
                    hero_value_row(
                        &format!("{:.0}", s.ram_load),
                        "%",
                        30,
                        13,
                        p.ink,
                        p.ink_3,
                        t,
                    ),
                    ram_bar,
                    text(ram_detail)
                        .size(11)
                        .font(font)
                        .color(theme::rgb(p.ink_3)),
                ]
                .spacing(6),
            )
            .padding(t.pad(13))
            .width(Length::Fill)
            .style(theme::panel_style(t))
            .into(),
        );
    }

    if appr.is_tile_visible("drive") {
        let (ssd_num, ssd_u) = split_temp(s.ssd_temp, unit);
        let ssd_sub = temp_range_label(&app.snapshot.ssd_temp_history, unit);
        row_a_tiles.push(spark_metric_tile(
            t,
            font,
            "Drive temperature",
            &ssd_num,
            ssd_u,
            &ssd_sub,
            &app.snapshot.ssd_temp_history,
            20.0,
            75.0,
            t.data_primary,
            thermal_temp_color(s.ssd_temp, t, p.ink),
            app.anim_t,
            30,
            None,
        ));
    }

    if appr.is_tile_visible("download") {
        let (dl_num, dl_u) = split_throughput(s.net_down_bps);
        let dl_peak = app
            .snapshot
            .net_down_history
            .iter()
            .copied()
            .fold(0.0_f32, f32::max);
        let dl_sub = if dl_peak > 0.0 {
            format!("Peak {}", sensors::format_throughput(Some(dl_peak)))
        } else {
            "Network activity".to_string()
        };
        row_a_tiles.push(spark_metric_tile(
            t,
            font,
            "Download speed",
            &dl_num,
            dl_u,
            &dl_sub,
            &app.snapshot.net_down_history,
            0.0,
            1024.0 * 1024.0,
            p.ok,
            p.ink,
            app.anim_t,
            30,
            None,
        ));
    }

    // Instrumentation Row B: Upgraded Cost & Efficiency Card + Hardware & Telemetry
    let mut row_b_tiles: Vec<Element<'a, Message>> = Vec::new();

    if appr.is_tile_visible("cost") {
        let total_w = s.total_power_w.unwrap_or(0.0);
        let tariff = app.snapshot.config.settings.energy_price_eur_kwh;
        let cost_hr = (total_w / 1000.0) * tariff;
        let cost_day = cost_hr * 8.0;
        let cost_mo = cost_day * 30.0;
        let peak_w = app.snapshot.power_peak_w.max(total_w);

        let cost_spark = iced::widget::canvas(SparklineCanvas {
            samples: &app.snapshot.cost_history,
            min_val: 0.0,
            max_val: 0.35,
            color: p.brass,
            palette: p,
            accent_override: t.accent_override,
            anim_t: app.anim_t,
        })
        .width(Length::Fixed(104.0))
        .height(Length::Fixed(42.0));

        let cost_card = container(
            column![
                row![
                    text("Cost & efficiency")
                        .size(12.5)
                        .font(font)
                        .color(theme::rgb(p.ink_2)),
                    Space::new().width(Length::Fill),
                    container(
                        text(format!("Peak {peak_w:.0} W"))
                            .size(10.5)
                            .font(mono)
                            .color(theme::rgb(p.brass_hi)),
                    )
                    .padding([2, 8])
                    .style(theme::chip_style(t, Some(p.brass))),
                ]
                .align_y(Alignment::Center),
                row![
                    column![
                        hero_value_row(
                            &format!("€{cost_hr:.3}"),
                            "/hr",
                            28,
                            13,
                            p.ink,
                            p.ink_3,
                            t,
                        ),
                        row![
                            text(format!("€{cost_day:.2}/day (8h)"))
                                .size(11.5)
                                .font(mono)
                                .color(theme::rgb(t.data_primary)),
                            text("·").size(11).font(font).color(theme::rgb(p.ink_3)),
                            text(format!("€{cost_mo:.1}/mo"))
                                .size(11.5)
                                .font(mono)
                                .color(theme::rgb(t.pink_rose)),
                            text("·").size(11).font(font).color(theme::rgb(p.ink_3)),
                            text(format!("{total_w:.0} W @ €{tariff:.2}/kWh"))
                                .size(11)
                                .font(font)
                                .color(theme::rgb(p.ink_3)),
                        ]
                        .spacing(6)
                        .align_y(Alignment::Center),
                    ]
                    .spacing(4)
                    .width(Length::Fill),
                    cost_spark,
                ]
                .spacing(10)
                .align_y(Alignment::End),
            ]
            .spacing(6),
        )
        .padding(t.pad(11))
        .width(Length::Fill)
        .style(theme::panel_style(t));

        row_b_tiles.push(cost_card.into());
    }

    if appr.is_tile_visible("hardware") {
        let hw_tile = container(
            column![
                row![
                    text("Hardware & telemetry")
                        .size(12.5)
                        .font(font)
                        .color(theme::rgb(p.ink_2)),
                    Space::new().width(Length::Fill),
                    row![
                        status_dot(if s.lhm_connected { p.ok } else { p.warm }, 7.0),
                        text(if s.lhm_connected {
                            if app.snapshot.lhm_auth_configured {
                                "LHM Live (Auth)"
                            } else {
                                "LHM Live"
                            }
                        } else {
                            "Demo fallback"
                        })
                        .size(11)
                        .font(font)
                        .color(theme::rgb(if s.lhm_connected { p.ok } else { p.warm })),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                ],
                row![
                    text(format!("{} USB HID", app.snapshot.hid_devices.len()))
                        .size(20)
                        .font(theme::display_semibold(t.design)),
                    Space::new().width(16),
                    text(format!(
                        "{} OpenRGB",
                        app.snapshot.openrgb.controller_count
                    ))
                    .size(20)
                    .font(theme::display_semibold(t.design))
                    .color(theme::rgb(t.data_secondary)),
                    Space::new().width(16),
                    text(format!(
                        "{} zones",
                        app.snapshot
                            .config
                            .zones
                            .iter()
                            .filter(|z| z.enabled)
                            .count()
                    ))
                    .size(20)
                    .font(theme::display_semibold(t.design))
                    .color(theme::rgb(p.brass_hi)),
                ]
                .align_y(Alignment::Center),
                text(format!(
                    "{} · {} sensors tracked",
                    app.snapshot.openrgb.message,
                    s.readings.len()
                ))
                .size(11)
                .font(font)
                .color(theme::rgb(p.ink_3)),
            ]
            .spacing(6),
        )
        .padding(t.pad(11))
        .width(Length::Fill)
        .style(theme::panel_style(t));

        row_b_tiles.push(hw_tile.into());
    }

    // Compact collapsible Live Activity Strip at bottom of Overview
    let activity_strip = view_activity_strip(app, t, font);

    let mut page = column![].spacing(gap);
    if let Some(conflict_banner) = view_conflict_banner(app, t, font) {
        page = page.push(conflict_banner);
    }
    page = page.push(masthead);
    if app.customize_open {
        page = page.push(views_palette::view_customize_drawer(app, t, font));
    }
    page = page.push(top_row);
    if let Some(mr) = mid_row {
        page = page.push(mr);
    }
    if !row_a_tiles.is_empty() {
        page = page.push(row(row_a_tiles).spacing(gap));
    }
    if !row_b_tiles.is_empty() {
        page = page.push(row(row_b_tiles).spacing(gap));
    }
    page = page.push(activity_strip);

    page.into()
}

/// Compact 430×410 Desktop Companion Mini-Mode widget (ideal for a second monitor while gaming).
pub fn view_compact_companion<'a>(
    app: &'a AirglowApp,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let s = &app.snapshot.sensors;
    let eff = &app.snapshot.effective_scene;
    let appr = &app.snapshot.config.appearance;
    let unit = app.snapshot.config.settings.temp_unit;
    let mono = theme::mono_font();
    let power_on = eff.on;

    let cpu_str = sensors::format_temp(s.cpu_temp, unit);
    let gpu_str = sensors::format_temp(s.gpu_temp, unit);
    let cpu_col = thermal_temp_color(s.cpu_temp, t, t.data_primary);
    let gpu_col = thermal_temp_color(s.gpu_temp, t, t.data_secondary);

    let temp_pill = container(
        row![
            text("CPU").size(9.5).font(font).color(theme::rgb(p.ink_3)),
            text(cpu_str).size(11).font(mono).color(theme::rgb(cpu_col)),
            Space::new().width(5),
            text("GPU").size(9.5).font(font).color(theme::rgb(p.ink_3)),
            text(gpu_str).size(11).font(mono).color(theme::rgb(gpu_col)),
        ]
        .spacing(4)
        .align_y(Alignment::Center),
    )
    .padding([4, 8])
    .style(theme::ambient_pill_style(t));

    let expand_btn = button(
        container(text("⤢").size(13).font(font))
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    )
    .width(Length::Fixed(28.0))
    .height(Length::Fixed(28.0))
    .padding(0)
    .style(theme::pill_action_btn(t, false))
    .on_press(Message::ToggleCompactMode);

    let power_btn = button(
        container(text(if power_on { "⏻" } else { "○" }).size(13).font(font))
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    )
    .width(Length::Fixed(30.0))
    .height(Length::Fixed(30.0))
    .padding(0)
    .style(theme::power_circle_btn(t, power_on))
    .on_press(Message::TogglePower);

    let header_fg = container(
        row![
            image(&app.moth_art)
                .content_fit(ContentFit::Contain)
                .width(Length::Fixed(30.0))
                .height(Length::Fixed(28.0)),
            text("Airglow")
                .size(16)
                .font(theme::display_semibold(t.design))
                .color(theme::rgb(p.ink)),
            Space::new().width(Length::Fill),
            temp_pill,
            expand_btn,
            power_btn,
        ]
        .spacing(7)
        .align_y(Alignment::Center),
    )
    .padding([6, 12])
    .width(Length::Fill)
    .height(Length::Fill);

    let top_header: Element<'a, Message> = if t.is_firefly() {
        let dusk_bg = image(&app.dusk_art)
            .content_fit(ContentFit::Cover)
            .width(Length::Fill)
            .height(Length::Fill);
        let scrim = container(Space::new().width(Length::Fill).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::hero_scrim_style(t));
        let particles = iced::widget::canvas(FireflyParticlesCanvas {
            palette: p,
            accent_override: t.accent_override,
            anim_t: app.anim_t,
            enabled: appr.firefly_particles,
            show_lantern_aura: false,
        })
        .width(Length::Fill)
        .height(Length::Fill);
        container(
            stack![dusk_bg, scrim, particles, header_fg]
                .width(Length::Fill)
                .height(Length::Fixed(50.0)),
        )
        .width(Length::Fill)
        .clip(true)
        .into()
    } else {
        container(header_fg)
            .width(Length::Fill)
            .height(Length::Fixed(50.0))
            .style(theme::appbar_style(t))
            .into()
    };

    let glow_line = iced::widget::canvas(crate::canvas_widgets::HeaderGlowCanvas {
        palette: p,
        accent_override: t.accent_override,
        ambient_rgb: app.snapshot.average_rgb,
        lights_on: power_on,
        anim_t: app.anim_t,
    })
    .width(Length::Fill)
    .height(Length::Fixed(2.0));

    // Compact Live Living Rig preview + Brightness slider
    let stage = iced::widget::canvas(CaseStageCanvas {
        frames: &app.snapshot.frames,
        zone_overrides: &eff.zone_overrides,
        palette: p,
        accent_override: t.accent_override,
        rig_style: appr.rig_style,
        airflow_lines: appr.airflow_lines,
        spinning_fans: appr.spinning_fans,
        ambient_bloom: appr.ambient_bloom,
        lights_on: eff.on,
        show_labels: false,
        arrange_mode: false,
        selected_zone: None,
        active_effect: eff.effect,
        temp_unit: unit,
        anim_t: app.anim_t,
        fan_duty: app.snapshot.fan_duty,
        cpu_temp: s.cpu_temp,
        gpu_temp: s.gpu_temp,
        ambient_rgb: app.snapshot.average_rgb,
        eyebrow: Some("LIVING RIG"),
    })
    .width(Length::Fill)
    .height(Length::Fixed(148.0));
    let stage_el = Element::from(stage).map(Message::Stage);

    let active_effect_title = if eff.effect == EffectId::Studio {
        eff.studio_stack.name.clone()
    } else {
        eff.effect.label().to_string()
    };

    let rig_card = container(
        column![
            container(stage_el)
                .clip(true)
                .style(move |_| iced::widget::container::Style {
                    border: iced::Border {
                        radius: iced::border::Radius::from(if p.square_corners { 0.0 } else { 10.0 }),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
            row![
                text(truncate_str(&active_effect_title, 16))
                    .size(12.5)
                    .font(theme::display_semibold(t.design))
                    .color(theme::rgb(p.ink)),
                Space::new().width(8),
                slider(
                    0.0..=100.0,
                    app.snapshot.config.scene.brightness as f32,
                    |v| Message::SetBrightness(v.round() as u8)
                )
                .style(theme::slider_style(t))
                .width(Length::Fill),
                text(format!("{}%", app.snapshot.config.scene.brightness))
                    .size(11)
                    .font(mono)
                    .color(theme::rgb(p.ink_2))
                    .width(Length::Fixed(32.0)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ]
        .spacing(7),
    )
    .padding(9)
    .width(Length::Fill)
    .style(theme::panel_style(t));

    // 5 one-click Profile chips (tight horizontal padding so "Meteor watch" fits cleanly)
    let mut profile_chips = row![].spacing(5);
    for preset in app.snapshot.config.presets.iter().take(5) {
        let active = app.snapshot.config.active_preset.as_deref() == Some(&preset.id);
        let dot_col = preset_dot_color(preset.effect, preset.params.color);
        let short_name = truncate_str(&preset.name, 13);
        profile_chips = profile_chips.push(
            button(
                column![
                    color_dot(dot_col, 7.5),
                    text(short_name)
                        .size(10)
                        .font(if active {
                            theme::ui_semibold(t.design)
                        } else {
                            font
                        })
                        .color(theme::rgb(if active { p.brass_hi } else { p.ink_2 })),
                ]
                .spacing(4)
                .align_x(Alignment::Center)
                .width(Length::Fill),
            )
            .padding([6, 2])
            .width(Length::Fill)
            .style(theme::card_button(t, active))
            .on_press(Message::ApplyPreset(preset.id.clone())),
        );
    }

    // Quick Fan Curve rail + Sleep timer pill
    let mut curve_seg = row![].spacing(2);
    for (id, label) in [
        ("silent", "Silent"),
        ("balanced", "Balanced"),
        ("performance", "Perf"),
    ] {
        let active = app.snapshot.effective_curve == id;
        curve_seg = curve_seg.push(
            button(container(text(label).size(10.5).font(font)).center_x(Length::Fill))
                .padding([4, 6])
                .width(Length::Fill)
                .style(theme::segmented_item_btn(t, active))
                .on_press(Message::SelectFanCurve(id.to_string())),
        );
    }

    let sleep_active = app.snapshot.sleep_timer_remaining_s.is_some();
    let bottom_bar = container(
        row![
            text(format!("{}%", app.snapshot.fan_duty))
                .size(11.5)
                .font(mono)
                .color(theme::rgb(t.data_primary))
                .width(Length::Fixed(32.0)),
            container(curve_seg)
                .padding(2)
                .width(Length::Fill)
                .style(theme::segmented_rail_style(t)),
            button(
                text(sleep_pill_label(app.snapshot.sleep_timer_remaining_s))
                    .size(10.5)
                    .font(if sleep_active { mono } else { font }),
            )
            .padding([5, 9])
            .style(theme::pill_action_btn(t, sleep_active))
            .on_press(Message::CycleSleepTimer),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([6, 9])
    .width(Length::Fill)
    .style(theme::panel_style(t));

    // Sleek 1-line compact telemetry & quick-command footer
    let active_zones = app.snapshot.frames.iter().filter(|f| f.enabled).count();
    let total_leds: usize = app
        .snapshot
        .frames
        .iter()
        .filter(|f| f.enabled)
        .map(|f| f.colors.len())
        .sum();
    let power_str = s
        .total_power_w
        .map(|w| format!("{w:.0} W"))
        .unwrap_or_else(|| "— W".to_string());

    let compact_footer = container(
        row![
            status_dot(if s.lhm_connected { p.ok } else { p.warm }, 6.5),
            text(format!("Power {power_str}"))
                .size(10.5)
                .font(mono)
                .color(theme::rgb(p.brass_hi)),
            text("·").size(10.5).font(font).color(theme::rgb(p.ink_3)),
            text(format!("{active_zones} zones · {total_leds} LEDs"))
                .size(10.5)
                .font(font)
                .color(theme::rgb(p.ink_3)),
            Space::new().width(Length::Fill),
            button(
                row![
                    text("⌘")
                        .size(10.5)
                        .font(theme::ui_semibold(t.design))
                        .color(theme::rgb(p.brass_hi)),
                    text("Ctrl+K").size(10).font(mono),
                ]
                .spacing(4)
                .align_y(Alignment::Center),
            )
            .padding([3, 8])
            .style(theme::shortcut_badge_btn(t, app.palette_open))
            .on_press(Message::ToggleCommandPalette),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .padding([5, 10])
    .width(Length::Fill)
    .style(theme::activity_strip_style(t));

    let body = container(
        column![rig_card, profile_chips, bottom_bar, compact_footer]
            .spacing(7)
            .width(Length::Fill),
    )
    .padding([8, 10])
    .width(Length::Fill)
    .height(Length::Fill);

    container(column![top_header, glow_line, body].spacing(0))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::window_ground_style(t))
        .into()
}

fn activity_kind_color(kind: &str, t: DesignTokens) -> u32 {
    match kind {
        "preset" | "lighting" | "look" => t.p.brass_hi,
        "automation" => t.data_secondary,
        "fan" | "thermal" => t.p.warm,
        "sleep" => t.pink_rose,
        "hardware" => t.data_primary,
        _ => t.p.ink_3,
    }
}

fn view_activity_strip<'a>(
    app: &'a AirglowApp,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let mono = theme::mono_font();
    let log = &app.snapshot.activity_log;

    let latest_summary: Element<'a, Message> = if let Some(last) = log.last() {
        row![
            status_dot(activity_kind_color(&last.kind, t), 6.0),
            text(&last.time_hm)
                .size(11)
                .font(mono)
                .color(theme::rgb(p.ink_3)),
            text(&last.text)
                .size(11.5)
                .font(font)
                .color(theme::rgb(p.ink_2)),
        ]
        .spacing(7)
        .align_y(Alignment::Center)
        .into()
    } else {
        text("Quiet session — recent lighting, fan, and automation events appear here.")
            .size(11.5)
            .font(font)
            .color(theme::rgb(p.ink_3))
            .into()
    };

    let header_row = row![
        text("ACTIVITY")
            .size(10.5)
            .font(theme::ui_semibold(t.design))
            .color(theme::rgb(p.ink_3)),
        Space::new().width(8),
        latest_summary,
        Space::new().width(Length::Fill),
        button(
            text(if app.activity_open {
                format!("Hide ({}) ▲", log.len())
            } else {
                format!("Timeline ({}) ▼", log.len())
            })
            .size(10.5)
            .font(font),
        )
        .padding([3, 9])
        .style(theme::btn_ghost(t, app.activity_open))
        .on_press(Message::ToggleActivityStrip),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    let mut col = column![header_row].spacing(6);
    if app.activity_open && !log.is_empty() {
        for ev in log.iter().rev().take(4) {
            col = col.push(
                row![
                    Space::new().width(6),
                    status_dot(activity_kind_color(&ev.kind, t), 6.0),
                    text(&ev.time_hm)
                        .size(11)
                        .font(mono)
                        .color(theme::rgb(p.ink_3))
                        .width(Length::Fixed(42.0)),
                    text(ev.kind.to_uppercase())
                        .size(9.5)
                        .font(theme::ui_semibold(t.design))
                        .color(theme::rgb(activity_kind_color(&ev.kind, t)))
                        .width(Length::Fixed(78.0)),
                    text(&ev.text)
                        .size(11.5)
                        .font(font)
                        .color(theme::rgb(p.ink)),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            );
        }
    }

    container(col)
        .padding([8, 14])
        .width(Length::Fill)
        .style(theme::activity_strip_style(t))
        .into()
}

pub fn view_fans<'a>(
    app: &'a AirglowApp,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let s = &app.snapshot.sensors;
    let unit = app.snapshot.config.settings.temp_unit;
    let mono = theme::mono_font();
    let hyst = app.snapshot.config.settings.fan_hysteresis_c;

    let (cpu_num, cpu_u) = split_temp(s.cpu_temp, unit);
    let (gpu_num, gpu_u) = split_temp(s.gpu_temp, unit);
    let total_w_num = s
        .total_power_w
        .map(|v| format!("{v:.0}"))
        .unwrap_or_else(|| "—".to_string());

    let cpu_peak_badge = if app.snapshot.cpu_temp_peak > 0.0 {
        Some(format!(
            "Peak {}",
            unit.format_temp(app.snapshot.cpu_temp_peak)
        ))
    } else {
        None
    };
    let gpu_peak_badge = if app.snapshot.gpu_temp_peak > 0.0 {
        Some(format!(
            "Peak {}",
            unit.format_temp(app.snapshot.gpu_temp_peak)
        ))
    } else {
        None
    };
    let power_peak_badge = if app.snapshot.power_peak_w > 0.0 {
        Some(format!("Peak {:.0} W", app.snapshot.power_peak_w))
    } else {
        None
    };

    let left_telemetry = column![
        spark_metric_tile(
            t,
            font,
            &short_hw_name(&s.cpu_name),
            &cpu_num,
            cpu_u,
            &s.cpu_power_w
                .map(|w| format!("{w:.0} W"))
                .unwrap_or_else(|| "Processor".to_string()),
            &app.snapshot.cpu_temp_history,
            25.0,
            95.0,
            t.data_primary,
            thermal_temp_color(s.cpu_temp, t, p.ink),
            app.anim_t,
            38,
            cpu_peak_badge,
        ),
        spark_metric_tile(
            t,
            font,
            &short_hw_name(&s.gpu_name),
            &gpu_num,
            gpu_u,
            &s.gpu_power_w
                .map(|w| format!("{w:.0} W"))
                .unwrap_or_else(|| "Graphics".to_string()),
            &app.snapshot.gpu_temp_history,
            25.0,
            95.0,
            t.data_secondary,
            thermal_temp_color(s.gpu_temp, t, p.ink),
            app.anim_t,
            38,
            gpu_peak_badge,
        ),
        spark_metric_tile(
            t,
            font,
            "Power draw",
            &total_w_num,
            "W",
            "processor and graphics",
            &app.snapshot.cpu_power_history,
            0.0,
            220.0,
            p.brass,
            p.ink,
            app.anim_t,
            38,
            power_peak_badge,
        ),
    ]
    .spacing(t.sp(14))
    .width(Length::FillPortion(3));

    let active_name = &app.snapshot.effective_curve;
    let points = app
        .snapshot
        .config
        .fan_curves
        .get(active_name)
        .map(Vec::as_slice)
        .unwrap_or(&[]);

    let mut curve_tabs = row![].spacing(2);
    for name in ["silent", "balanced", "performance"] {
        let active = active_name == name;
        curve_tabs = curve_tabs.push(
            button(text(cap_first(name)).size(12).font(font))
                .padding([6, 13])
                .style(theme::segmented_item_btn(t, active))
                .on_press(Message::SelectFanCurve(name.to_string())),
        );
    }

    let curve_canvas = iced::widget::canvas(FanCurveCanvas {
        points,
        current_temp: app.snapshot.fan_temp,
        current_duty: app.snapshot.fan_duty,
        temp_unit: unit,
        palette: p,
        accent_override: t.accent_override,
        hysteresis_c: hyst,
        anim_t: app.anim_t,
    })
    .width(Length::Fill)
    .height(Length::Fixed(300.0));
    let mapped_canvas = Element::from(curve_canvas).map(|m| match m {
        CurveMessage::PointsChanged(pts) => Message::CurveEdited(pts),
    });

    let mut src_row = row![].spacing(6).align_y(Alignment::Center);
    for src in TempSource::ALL {
        let active = app.snapshot.config.settings.temp_source == src;
        src_row = src_row.push(
            button(text(src.label()).size(11.5).font(font))
                .padding([5, 11])
                .style(theme::btn_secondary(t, active))
                .on_press(Message::SelectTempSource(src)),
        );
    }

    // Acoustic Hysteresis Slider (0.0 – 8.0 °C in 0.5°C steps)
    let hyst_row = container(
        row![
            column![
                text("Acoustic hysteresis")
                    .size(12)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(p.ink)),
                text("Cooldown deadband — instant ramp-up on heat, whisper-calm cooldown.")
                    .size(11)
                    .font(font)
                    .color(theme::rgb(p.ink_3)),
            ]
            .spacing(2)
            .width(Length::FillPortion(5)),
            slider(0.0..=8.0, hyst, |v| {
                let snapped = (v * 2.0).round() / 2.0;
                Message::SetFanHysteresis(snapped)
            })
            .step(0.5)
            .style(theme::slider_style(t))
            .width(Length::FillPortion(4)),
            container(
                text(format!("{hyst:.1} °C"))
                    .size(11.5)
                    .font(mono)
                    .color(theme::rgb(p.brass_hi)),
            )
            .padding([4, 9])
            .style(theme::chip_style(t, Some(p.brass))),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    )
    .padding([9, 12])
    .style(theme::well_style(t));

    let mut rpm_rows = column![].spacing(7);
    for (label, rpm) in &app.snapshot.fan_rpms {
        let bar = iced::widget::canvas(SegmentedBarCanvas {
            fraction: (*rpm as f32 / 2200.0).clamp(0.05, 1.0),
            accent: t.data_primary,
            palette: p,
            accent_override: t.accent_override,
            segments: 28,
        })
        .width(Length::Fill)
        .height(Length::Fixed(12.0));
        rpm_rows = rpm_rows.push(
            row![
                text(label).size(12).font(font).width(Length::Fixed(110.0)),
                bar,
                text(format!("{rpm} rpm"))
                    .size(12)
                    .font(mono)
                    .color(theme::rgb(p.ink_2))
                    .width(Length::Fixed(78.0)),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        );
    }

    let curve_card = container(
        column![
            row![
                column![
                    text("Fan curve")
                        .size(24)
                        .font(theme::display_semibold(t.design)),
                    text(format!(
                        "Following {} · target {:.0}% · commanded {}%",
                        app.snapshot.config.settings.temp_source.label(),
                        app.snapshot.fan_target,
                        app.snapshot.fan_duty,
                    ))
                    .size(12)
                    .font(font)
                    .color(theme::rgb(p.ink_3)),
                ]
                .spacing(3),
                Space::new().width(Length::Fill),
                container(curve_tabs)
                    .padding(3)
                    .style(theme::segmented_rail_style(t)),
            ]
            .align_y(Alignment::Center),
            container(mapped_canvas)
                .clip(true)
                .style(move |_| iced::widget::container::Style {
                    border: iced::Border {
                        radius: iced::border::Radius::from(if p.square_corners {
                            0.0
                        } else {
                            12.0
                        }),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
            row![
                text("Drag points, double-click to add, right-click a point to remove.")
                    .size(11.5)
                    .font(font)
                    .color(theme::rgb(p.ink_3)),
                Space::new().width(Length::Fill),
                button(text("Reset to default").size(12).font(font))
                    .padding([6, 12])
                    .style(theme::btn_secondary(t, false))
                    .on_press(Message::ResetFanCurve(active_name.clone())),
            ]
            .align_y(Alignment::Center),
            src_row,
            hyst_row,
            rpm_rows,
        ]
        .spacing(t.sp(11)),
    )
    .padding(t.pad(16))
    .style(theme::panel_style(t));

    // Motherboard & AIO LHM Fan Channels panel
    let mut lhm_col = column![
        row![
            text("MOTHERBOARD & AIO FAN CHANNELS (LIBREHARDWAREMONITOR)")
                .size(11)
                .font(theme::ui_semibold(t.design))
                .color(theme::rgb(p.ink_3)),
            Space::new().width(Length::Fill),
            row![
                status_dot(
                    if app.snapshot.sensors.lhm_connected {
                        p.ok
                    } else {
                        p.ink_3
                    },
                    7.0
                ),
                text(if app.snapshot.sensors.lhm_connected {
                    format!("{} detected", app.snapshot.lhm_channels.len())
                } else {
                    "LHM offline".to_string()
                })
                .size(11)
                .font(font)
                .color(theme::rgb(if app.snapshot.sensors.lhm_connected {
                    p.ok
                } else {
                    p.ink_3
                })),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(10);

    if app.snapshot.lhm_channels.is_empty() {
        lhm_col = lhm_col.push(
            text("Start LibreHardwareMonitor with its Remote Web Server enabled to control motherboard headers and AIO pumps directly. Channels stay on BIOS/Auto until you choose Curve or Steady.")
                .size(12)
                .font(font)
                .color(theme::rgb(p.ink_3)),
        );
    } else {
        for ch in &app.snapshot.lhm_channels {
            let cid = ch.id.clone();
            let cid_curve = ch.id.clone();
            let cid_steady = ch.id.clone();
            let cid_slider = ch.id.clone();

            let is_auto = matches!(ch.mode, FanChannelMode::Auto);
            let is_curve = matches!(ch.mode, FanChannelMode::Curve);
            let steady_duty = match ch.mode {
                FanChannelMode::Steady { duty } => Some(duty),
                _ => None,
            };

            let title = if ch.is_pump {
                format!("{} (Pump)", ch.name)
            } else {
                ch.name.clone()
            };

            let reading = format!(
                "{} · {} · {}",
                ch.hardware,
                ch.rpm
                    .map(|r| format!("{r} rpm"))
                    .unwrap_or_else(|| "— rpm".to_string()),
                ch.duty
                    .map(|d| format!("{d:.0}%"))
                    .unwrap_or_else(|| "—%".to_string()),
            );

            let default_steady = ch
                .duty
                .map(|d| d.round().clamp(20.0, 100.0) as u8)
                .unwrap_or(65);
            let mode_btns = container(
                row![
                    button(text("BIOS/Auto").size(11).font(font))
                        .padding([5, 9])
                        .style(theme::segmented_item_btn(t, is_auto))
                        .on_press(Message::SetFanChannelMode(cid, FanChannelMode::Auto)),
                    button(text("Curve").size(11).font(font))
                        .padding([5, 9])
                        .style(theme::segmented_item_btn(t, is_curve))
                        .on_press(Message::SetFanChannelMode(cid_curve, FanChannelMode::Curve)),
                    button(text("Steady").size(11).font(font))
                        .padding([5, 9])
                        .style(theme::segmented_item_btn(t, steady_duty.is_some()))
                        .on_press(Message::SetFanChannelMode(
                            cid_steady,
                            FanChannelMode::Steady {
                                duty: steady_duty.unwrap_or(default_steady),
                            },
                        )),
                ]
                .spacing(2),
            )
            .padding(2)
            .style(theme::segmented_rail_style(t));

            let mut ch_inner = column![
                row![
                    column![
                        text(title).size(13).font(theme::ui_semibold(t.design)),
                        text(reading)
                            .size(11)
                            .font(mono)
                            .color(theme::rgb(p.ink_3)),
                    ]
                    .spacing(2)
                    .width(Length::Fill),
                    mode_btns,
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ]
            .spacing(8);

            if let Some(duty) = steady_duty {
                ch_inner = ch_inner.push(
                    row![
                        text("Steady duty")
                            .size(11.5)
                            .font(font)
                            .color(theme::rgb(p.ink_2)),
                        slider(20.0..=100.0, duty as f32, move |v| {
                            Message::SetFanChannelMode(
                                cid_slider.clone(),
                                FanChannelMode::Steady {
                                    duty: v.round().clamp(20.0, 100.0) as u8,
                                },
                            )
                        })
                        .style(theme::slider_style(t))
                        .width(Length::Fill),
                        text(format!("{duty}%"))
                            .size(11.5)
                            .font(mono)
                            .color(theme::rgb(p.brass_hi))
                            .width(Length::Fixed(38.0)),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                );
            }

            if let Some(err) = &ch.error {
                ch_inner = ch_inner.push(
                    text(format!("Notice: {err}"))
                        .size(11)
                        .font(font)
                        .color(theme::rgb(p.warm)),
                );
            }

            lhm_col = lhm_col.push(container(ch_inner).padding(12).style(theme::well_style(t)));
        }
    }

    let lhm_card = container(lhm_col)
        .padding(t.pad(16))
        .style(theme::panel_style(t));

    // Cooling Recorder bar + recent sessions
    let rec_on = app.snapshot.cooling_recording;
    let rec_status = if rec_on {
        format!(
            "Recording… {} samples captured ({} saved sessions)",
            app.snapshot.cooling_sample_count, app.snapshot.cooling_session_count
        )
    } else {
        format!(
            "Idle · {} saved cooling session(s) stored locally",
            app.snapshot.cooling_session_count
        )
    };

    let mut recorder_col = column![
        row![
            column![
                text("COOLING RECORDER")
                    .size(11)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(if rec_on { p.hot } else { p.ink_3 })),
                text(rec_status)
                    .size(12)
                    .font(font)
                    .color(theme::rgb(p.ink_2)),
            ]
            .spacing(3)
            .width(Length::Fill),
            button(
                text(if rec_on {
                    "Stop recording"
                } else {
                    "Start recording"
                })
                .size(12)
                .font(theme::ui_semibold(t.design)),
            )
            .padding([7, 13])
            .style(move |th, st| {
                if rec_on {
                    theme::btn_danger(t)(th, st)
                } else {
                    theme::btn_primary(t)(th, st)
                }
            })
            .on_press(Message::ToggleCoolingRecording),
            button(text("Export CSV").size(12).font(font))
                .padding([7, 13])
                .style(theme::btn_secondary(t, false))
                .on_press(Message::ExportCoolingCsv),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(8);

    for sess in app.snapshot.cooling_sessions.iter().take(3) {
        let cpu_s = sess
            .cpu_max
            .map(|v| format!("CPU max {}", unit.format_temp(v as f32)))
            .unwrap_or_else(|| "CPU —".to_string());
        let gpu_s = sess
            .gpu_max
            .map(|v| format!("GPU max {}", unit.format_temp(v as f32)))
            .unwrap_or_else(|| "GPU —".to_string());
        recorder_col = recorder_col.push(
            container(
                row![
                    text(if sess.annotation.is_empty() {
                        format!("Session {}", &sess.id.chars().take(8).collect::<String>())
                    } else {
                        sess.annotation.clone()
                    })
                    .size(12)
                    .font(font),
                    Space::new().width(Length::Fill),
                    text(format!(
                        "{} samples · {:.0}s · {cpu_s} · {gpu_s}",
                        sess.sample_count, sess.duration_seconds
                    ))
                    .size(11)
                    .font(mono)
                    .color(theme::rgb(p.ink_3)),
                ]
                .align_y(Alignment::Center),
            )
            .padding([7, 11])
            .style(theme::well_style(t)),
        );
    }

    let recorder_card = container(recorder_col)
        .padding(t.pad(16))
        .style(theme::panel_style(t));

    let right_col = column![curve_card, lhm_card, recorder_card]
        .spacing(t.sp(14))
        .width(Length::FillPortion(7));

    row![left_telemetry, right_col].spacing(t.sp(14)).into()
}

fn hero_value_row<'a>(
    num: &str,
    unit: &str,
    num_size: u32,
    unit_size: u32,
    num_col: u32,
    unit_col: u32,
    t: DesignTokens,
) -> Element<'a, Message> {
    let val_font = if t.is_firefly() {
        theme::ui_font(t.design)
    } else {
        theme::display_font(t.design)
    };
    let mut r = row![
        text(num.to_string())
            .size(num_size)
            .font(val_font)
            .color(theme::rgb(num_col)),
    ]
    .spacing(4)
    .align_y(Alignment::End);
    if !unit.is_empty() {
        r = r.push(
            container(
                text(unit.to_string())
                    .size(unit_size)
                    .font(theme::ui_font(t.design))
                    .color(theme::rgb(unit_col)),
            )
            .padding([0, 0]),
        );
    }
    r.into()
}

#[allow(clippy::too_many_arguments)]
pub fn spark_metric_tile<'a>(
    t: DesignTokens,
    font: iced::Font,
    title: &str,
    big_num: &str,
    unit_lbl: &str,
    sub: &str,
    samples: &'a [f32],
    min_v: f32,
    max_v: f32,
    accent: u32,
    value_col: u32,
    anim_t: f32,
    num_size: u32,
    peak_badge: Option<String>,
) -> Element<'a, Message> {
    let p = t.p;
    let spark = iced::widget::canvas(SparklineCanvas {
        samples,
        min_val: min_v,
        max_val: max_v,
        color: accent,
        palette: p,
        accent_override: t.accent_override,
        anim_t,
    })
    .width(Length::Fixed(90.0))
    .height(Length::Fixed(38.0));

    let mut head = row![
        text(truncate_str(title, 24))
            .size(12)
            .font(font)
            .color(theme::rgb(p.ink_2)),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    if let Some(badge) = peak_badge {
        head = head.push(Space::new().width(Length::Fill)).push(
            container(
                text(badge)
                    .size(10)
                    .font(theme::mono_font())
                    .color(theme::rgb(accent)),
            )
            .padding([2, 7])
            .style(theme::chip_style(t, Some(accent))),
        );
    }

    container(
        column![
            head,
            row![
                column![
                    hero_value_row(big_num, unit_lbl, num_size, 13, value_col, p.ink_3, t),
                    text(sub.to_string())
                        .size(10.5)
                        .font(font)
                        .color(theme::rgb(p.ink_3)),
                ]
                .spacing(3)
                .width(Length::Fill),
                spark,
            ]
            .spacing(8)
            .align_y(Alignment::End),
        ]
        .spacing(6),
    )
    .padding(t.pad(13))
    .width(Length::Fill)
    .style(theme::panel_style(t))
    .into()
}

pub fn ring_metric_tile<'a>(
    t: DesignTokens,
    font: iced::Font,
    title: &'a str,
    subtitle: &str,
    pct: f32,
    accent: u32,
) -> Element<'a, Message> {
    let p = t.p;
    let gauge = iced::widget::canvas(RingGaugeCanvas {
        percent: pct,
        accent,
        palette: p,
        accent_override: t.accent_override,
    })
    .width(Length::Fixed(58.0))
    .height(Length::Fixed(58.0));

    container(
        column![
            row![
                text(title).size(12).font(font).color(theme::rgb(p.ink_2)),
                Space::new().width(Length::Fill),
                text(truncate_str(subtitle, 24))
                    .size(10.5)
                    .font(font)
                    .color(theme::rgb(p.ink_3)),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
            row![
                hero_value_row(&format!("{pct:.0}"), "%", 34, 14, p.ink, p.ink_3, t),
                Space::new().width(Length::Fill),
                gauge,
            ]
            .align_y(Alignment::Center),
        ]
        .spacing(4),
    )
    .padding(t.pad(13))
    .width(Length::Fill)
    .style(theme::panel_style(t))
    .into()
}

#[allow(clippy::too_many_arguments)]
pub fn labeled_slider<'a>(
    t: DesignTokens,
    font: iced::Font,
    label: &'a str,
    value_label: String,
    min: u16,
    max: u16,
    value: u16,
    on_change: impl Fn(u16) -> Message + 'a,
) -> Element<'a, Message> {
    let p = t.p;
    column![
        row![
            text(label).size(12).font(font).color(theme::rgb(p.ink_2)),
            Space::new().width(Length::Fill),
            text(value_label)
                .size(11.5)
                .font(theme::mono_font())
                .color(theme::rgb(p.ink_3)),
        ],
        slider(min as f32..=max as f32, value as f32, move |v| {
            on_change(v.round() as u16)
        })
        .style(theme::slider_style(t)),
    ]
    .spacing(5)
    .width(Length::Fill)
    .into()
}

pub fn color_dot<'a>(rgb_arr: [u8; 3], size: f32) -> Element<'a, Message> {
    let c = iced::Color::from_rgb8(rgb_arr[0], rgb_arr[1], rgb_arr[2]);
    container(Space::new().width(size).height(size))
        .style(move |_| iced::widget::container::Style {
            background: Some(iced::Background::Color(c)),
            border: iced::Border {
                radius: iced::border::Radius::from(999.0),
                ..Default::default()
            },
            shadow: iced::Shadow {
                color: iced::Color { a: 0.45, ..c },
                offset: iced::Vector::new(0.0, 0.0),
                blur_radius: 7.0,
            },
            ..Default::default()
        })
        .into()
}

pub fn status_dot<'a>(hex: u32, size: f32) -> Element<'a, Message> {
    let c = theme::rgb(hex);
    container(Space::new().width(size).height(size))
        .style(move |_| iced::widget::container::Style {
            background: Some(iced::Background::Color(c)),
            border: iced::Border {
                radius: iced::border::Radius::from(999.0),
                ..Default::default()
            },
            shadow: iced::Shadow {
                color: iced::Color { a: 0.55, ..c },
                offset: iced::Vector::new(0.0, 0.0),
                blur_radius: 6.0,
            },
            ..Default::default()
        })
        .into()
}

pub fn dash_swatch<'a>(hex: u32) -> Element<'a, Message> {
    container(Space::new().width(10).height(3))
        .style(move |_| iced::widget::container::Style {
            background: Some(iced::Background::Color(theme::rgb(hex))),
            border: iced::Border {
                radius: iced::border::Radius::from(999.0),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

fn split_temp(temp_c: Option<f32>, unit: TempUnit) -> (String, &'static str) {
    match temp_c {
        Some(v) => (format!("{:.0}", unit.convert(v)), unit.suffix()),
        None => ("—".to_string(), ""),
    }
}

fn split_throughput(bps: Option<f32>) -> (String, &'static str) {
    let Some(v) = bps else {
        return ("—".to_string(), "");
    };
    if v >= 1024.0 * 1024.0 {
        (format!("{:.1}", v / (1024.0 * 1024.0)), "MB/s")
    } else if v >= 1024.0 {
        (format!("{:.1}", v / 1024.0), "KB/s")
    } else {
        (format!("{v:.0}"), "B/s")
    }
}

pub fn temp_range_label(samples: &[f32], unit: TempUnit) -> String {
    if samples.is_empty() {
        return "Collecting…".to_string();
    }
    let min = samples.iter().copied().fold(f32::INFINITY, f32::min);
    let max = samples.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    format!(
        "{:.0}–{:.0}{} recent",
        unit.convert(min),
        unit.convert(max),
        unit.suffix()
    )
}

pub fn cap_first(s: &str) -> String {
    let mut ch = s.chars();
    match ch.next() {
        Some(first) => format!("{}{}", first.to_uppercase(), ch.as_str()),
        None => String::new(),
    }
}

pub fn short_hw_name(raw: &str) -> String {
    let s = raw.trim();
    for prefix in [
        "NVIDIA GeForce ",
        "AMD Radeon ",
        "Intel(R) Core(TM) ",
        "Intel Core ",
        "AMD ",
    ] {
        if let Some(rest) = s.strip_prefix(prefix) {
            return rest.trim().to_string();
        }
    }
    s.to_string()
}

pub fn truncate_str(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        s.to_string()
    } else {
        let head: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{head}…")
    }
}
