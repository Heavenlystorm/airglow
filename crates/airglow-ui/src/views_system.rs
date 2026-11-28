//! Full-viewport System Atelier Views:
//! - `Fans` (Cooling Cockpit: hero interactive FanCurveCanvas, segmented presets, Thermal Recorder,
//!   tactile Fan Channel cards with `Curve | Steady | Auto`, and Acoustic Hysteresis & Safety Floor)
//! - `Automations` (Smart Rules Studio: 4 tinted visual Smart Recipe cards + Active Rules | Rule Composer split)
//! - `Devices` (Hardware Topology & Sensor Deck: 4 36px telemetry cards + Connected Controllers | Searchable Live Sensor Explorer)
//! - `Settings` (Full-Width Balanced 2-Pane Atelier: 230px `UiIcon` sidebar + 1480px 2-column Bento settings cards + `UI Scale` selector)

use airglow_core::automations::ALL_DAYS_MASK;
use airglow_core::config::{
    ACCENT_SWATCHES, ColorOrder, DesignId, FanChannelMode, MIN_FAN_DUTY, OverviewTileKind,
    RigStyle, SAFETY_TEMP, TempSource, TempUnit, UI_SCALE_PRESETS, UpdateChannel, ZoneConfig,
};
use airglow_core::geometry::ZoneLayout;
use airglow_core::sensors;
use airglow_core::updates::{GITHUB_RELEASES_WEB, UpdateStatus, current_version};
use iced::widget::{
    Space, button, checkbox, column, container, image, row, scrollable, slider, stack, text,
    text_input,
};
use iced::{Alignment, ContentFit, Element, Length};

use crate::canvas_ambient::{FireflyParticlesCanvas, UiIcon, icon_canvas};
use crate::canvas_curves::{CurveMessage, FanCurveCanvas};
use crate::canvas_widgets::{
    BarMeterCanvas, DesignPreviewCanvas, SegmentedBarCanvas, SparklineCanvas,
};
use crate::theme::{self, DesignTokens};
use crate::views::{
    cap_first, color_dot, labeled_slider, pill_toggle, short_hw_name, status_dot,
    thermal_temp_color, truncate_str, view_conflict_banner,
};
use crate::{
    AirglowApp, AutoActionDraft, AutoTriggerDraft, Message, PulseSpan, SettingsSection, StarterIdea,
};

// ===========================================================================
// 1. FANS — FULL-VIEWPORT COOLING COCKPIT
// ===========================================================================

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

    let active_name = &app.snapshot.effective_curve;
    let points = app
        .snapshot
        .config
        .fan_curves
        .get(active_name)
        .map(Vec::as_slice)
        .unwrap_or(&[]);

    // Segmented Curve Preset Bar (`Silent | Balanced | Performance`)
    let mut curve_tabs = row![].spacing(3);
    for (id, label) in [
        ("silent", "Silent"),
        ("balanced", "Balanced"),
        ("performance", "Performance"),
    ] {
        let active = active_name == id;
        curve_tabs = curve_tabs.push(
            button(
                text(label).size(12.5).font(if active {
                    theme::ui_semibold(t.design)
                } else {
                    font
                }),
            )
            .padding([6, 15])
            .style(theme::segmented_pill_style(t, active))
            .on_press(Message::SelectFanCurve(id.to_string())),
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
    .height(Length::Fill);
    let mapped_canvas = Element::from(curve_canvas).map(|m| match m {
        CurveMessage::PointsChanged(pts) => Message::CurveEdited(pts),
    });

    // Temp Source segmented pills
    let mut src_pills = row![].spacing(3);
    for src in TempSource::ALL {
        let active = app.snapshot.config.settings.temp_source == src;
        let short = match src {
            TempSource::Max => "Hotter (CPU/GPU)",
            TempSource::Cpu => "CPU Package",
            TempSource::Gpu => "GPU Core",
            TempSource::Liquid => "Coolant",
        };
        src_pills = src_pills.push(
            button(
                text(short).size(11.5).font(if active {
                    theme::ui_semibold(t.design)
                } else {
                    font
                }),
            )
            .padding([5, 11])
            .style(theme::segmented_pill_style(t, active))
            .on_press(Message::SelectTempSource(src)),
        );
    }

    let now_temp_str = app
        .snapshot
        .fan_temp
        .map(|v| unit.format_temp(v))
        .unwrap_or_else(|| "—".to_string());

    let curve_hero_card = container(
        column![
            row![
                icon_canvas(UiIcon::Fans, theme::rgb(t.data_primary), 20.0),
                column![
                    row![
                        text("Cooling Cockpit")
                            .size(21)
                            .font(theme::display_semibold(t.design))
                            .color(theme::rgb(p.ink)),
                        container(
                            text(format!(
                                "{now_temp_str} → {}% duty",
                                app.snapshot.fan_duty
                            ))
                            .size(11)
                            .font(mono)
                            .color(theme::rgb(t.data_primary)),
                        )
                        .padding([3, 10])
                        .style(theme::chip_style(t, Some(t.data_primary))),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                    text(format!(
                        "Following {} · Drag control nodes, double-click to add, right-click to remove",
                        app.snapshot.config.settings.temp_source.label()
                    ))
                    .size(11.5)
                    .font(font)
                    .color(theme::rgb(p.ink_3)),
                ]
                .spacing(2),
                Space::new().width(Length::Fill),
                container(curve_tabs)
                    .padding(3)
                    .style(theme::segmented_track_style(t)),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            container(mapped_canvas)
                .width(Length::Fill)
                .height(Length::Fill)
                .clip(true)
                .style(theme::bento_stage_well_style(t)),
            row![
                text("Sensor source:")
                    .size(12)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(p.ink_2)),
                container(src_pills)
                    .padding(3)
                    .style(theme::segmented_track_style(t)),
                Space::new().width(Length::Fill),
                button(
                    row![
                        icon_canvas(UiIcon::Reset, theme::rgb(p.ink_2), 13.0),
                        text("Reset Curve").size(12).font(font),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding([6, 13])
                .style(theme::btn_secondary(t, false))
                .on_press(Message::ResetFanCurve(active_name.clone())),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ]
        .spacing(11)
        .height(Length::Fill),
    )
    .padding(t.pad(16))
    .width(Length::Fill)
    .height(Length::Fill)
    .style(theme::rich_instrument_card_style(
        t,
        Some(t.data_primary),
        false,
    ));

    // Live Thermal Session Recorder Card
    let rec_on = app.snapshot.cooling_recording;
    let rec_status = if rec_on {
        format!(
            "Recording live telemetry · {} samples captured ({} saved sessions)",
            app.snapshot.cooling_sample_count, app.snapshot.cooling_session_count
        )
    } else {
        format!(
            "Ready to capture thermal & RPM traces · {} saved session(s)",
            app.snapshot.cooling_session_count
        )
    };

    let mut recorder_col = column![
        row![
            status_dot(if rec_on { p.hot } else { t.data_primary }, 8.5),
            column![
                text("LIVE THERMAL SESSION RECORDER")
                    .size(11)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(if rec_on { p.hot } else { p.ink_2 })),
                text(rec_status)
                    .size(12)
                    .font(font)
                    .color(theme::rgb(p.ink_3)),
            ]
            .spacing(2)
            .width(Length::Fill),
            button(
                text(if rec_on {
                    "■ Stop Recording"
                } else {
                    "● Start Recording"
                })
                .size(12)
                .font(theme::ui_semibold(t.design)),
            )
            .padding([7, 14])
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
        .spacing(10)
        .align_y(Alignment::Center),
    ]
    .spacing(8);

    if !app.snapshot.cooling_sessions.is_empty() {
        let mut sess_row = row![].spacing(8);
        for sess in app.snapshot.cooling_sessions.iter().take(3) {
            let cpu_s = sess
                .cpu_max
                .map(|v| format!("CPU {}", unit.format_temp(v as f32)))
                .unwrap_or_else(|| "CPU —".to_string());
            let gpu_s = sess
                .gpu_max
                .map(|v| format!("GPU {}", unit.format_temp(v as f32)))
                .unwrap_or_else(|| "GPU —".to_string());
            let name = if sess.annotation.is_empty() {
                format!("Run {}", &sess.id.chars().take(6).collect::<String>())
            } else {
                truncate_str(&sess.annotation, 16)
            };
            sess_row = sess_row.push(
                container(
                    column![
                        text(name)
                            .size(11.5)
                            .font(theme::ui_semibold(t.design))
                            .color(theme::rgb(p.ink)),
                        text(format!(
                            "{:.0}s · {cpu_s} · {gpu_s}",
                            sess.duration_seconds
                        ))
                        .size(10.5)
                        .font(mono)
                        .color(theme::rgb(p.ink_3)),
                    ]
                    .spacing(2),
                )
                .padding([6, 10])
                .width(Length::Fill)
                .style(theme::well_style(t)),
            );
        }
        recorder_col = recorder_col.push(sess_row);
    }

    let recorder_card = container(recorder_col)
        .padding(t.pad(14))
        .width(Length::Fill)
        .style(theme::rich_instrument_card_style(t, Some(p.brass), false));

    let left_col = column![curve_hero_card, recorder_card]
        .spacing(t.sp(14))
        .width(Length::FillPortion(7))
        .height(Length::Fill);

    // -----------------------------------------------------------------------
    // RIGHT COLUMN (`FillPortion(5)`): Telemetry Strip + Tactile Channel Cards + Hysteresis
    // -----------------------------------------------------------------------
    let mini_thermal_pill = |label: String,
                             val: String,
                             sub: String,
                             samples: &'a [f32],
                             min_v: f32,
                             max_v: f32,
                             accent: u32,
                             val_col: u32| {
        let spark = iced::widget::canvas(SparklineCanvas {
            samples,
            min_val: min_v,
            max_val: max_v,
            color: accent,
            palette: p,
            accent_override: t.accent_override,
            anim_t: app.anim_t,
        })
        .width(Length::Fill)
        .height(Length::Fixed(34.0));

        container(
            column![
                row![
                    text(truncate_str(&label, 15))
                        .size(11.5)
                        .font(font)
                        .color(theme::rgb(p.ink_2)),
                    Space::new().width(Length::Fill),
                    text(sub).size(10.5).font(mono).color(theme::rgb(p.ink_3)),
                ]
                .align_y(Alignment::Center),
                text(val)
                    .size(24)
                    .font(theme::ui_bold(t.design))
                    .color(theme::rgb(val_col)),
                spark,
            ]
            .spacing(3),
        )
        .padding(t.pad(11))
        .width(Length::Fill)
        .style(theme::rich_instrument_card_style(t, Some(accent), false))
    };

    let cpu_str = sensors::format_temp(s.cpu_temp, unit);
    let gpu_str = sensors::format_temp(s.gpu_temp, unit);
    let pwr_str = s
        .total_power_w
        .map(|w| format!("{w:.0} W"))
        .unwrap_or_else(|| "— W".to_string());

    let telemetry_strip = row![
        mini_thermal_pill(
            short_hw_name(&s.cpu_name),
            cpu_str,
            s.cpu_power_w
                .map(|w| format!("{w:.0}W"))
                .unwrap_or_else(|| "CPU".to_string()),
            &app.snapshot.cpu_temp_history,
            25.0,
            95.0,
            t.data_primary,
            thermal_temp_color(s.cpu_temp, t, p.ink),
        ),
        mini_thermal_pill(
            short_hw_name(&s.gpu_name),
            gpu_str,
            s.gpu_power_w
                .map(|w| format!("{w:.0}W"))
                .unwrap_or_else(|| "GPU".to_string()),
            &app.snapshot.gpu_temp_history,
            25.0,
            95.0,
            t.data_secondary,
            thermal_temp_color(s.gpu_temp, t, p.ink),
        ),
        mini_thermal_pill(
            "System Power".to_string(),
            pwr_str,
            if app.snapshot.power_peak_w > 0.0 {
                format!("Pk {:.0}W", app.snapshot.power_peak_w)
            } else {
                "CPU+GPU".to_string()
            },
            &app.snapshot.cpu_power_history,
            0.0,
            240.0,
            p.brass_hi,
            p.ink,
        ),
    ]
    .spacing(t.sp(10));

    // Tactile Fan Channel Cards (`Curve | Steady | Auto`)
    let mut channels_list = column![].spacing(9);
    if !app.snapshot.lhm_channels.is_empty() {
        for ch in &app.snapshot.lhm_channels {
            let cid_auto = ch.id.clone();
            let cid_curve = ch.id.clone();
            let cid_steady = ch.id.clone();
            let cid_slider = ch.id.clone();

            let is_auto = matches!(ch.mode, FanChannelMode::Auto);
            let is_curve = matches!(ch.mode, FanChannelMode::Curve);
            let steady_duty = match ch.mode {
                FanChannelMode::Steady { duty } => Some(duty),
                _ => None,
            };

            let duty_val = ch
                .duty
                .unwrap_or(app.snapshot.fan_duty as f32)
                .clamp(0.0, 100.0);
            let rpm_str = ch
                .rpm
                .map(|r| format!("{r} RPM"))
                .unwrap_or_else(|| "— RPM".to_string());
            let badge_accent = if ch.is_pump {
                t.pink_rose
            } else {
                t.data_primary
            };

            let default_steady = duty_val.round().clamp(20.0, 100.0) as u8;
            let mode_seg = container(
                row![
                    button(text("Curve").size(11).font(font))
                        .padding([4, 10])
                        .style(theme::segmented_pill_style(t, is_curve))
                        .on_press(Message::SetFanChannelMode(cid_curve, FanChannelMode::Curve)),
                    button(text("Steady").size(11).font(font))
                        .padding([4, 10])
                        .style(theme::segmented_pill_style(t, steady_duty.is_some()))
                        .on_press(Message::SetFanChannelMode(
                            cid_steady,
                            FanChannelMode::Steady {
                                duty: steady_duty.unwrap_or(default_steady),
                            },
                        )),
                    button(text("Auto").size(11).font(font))
                        .padding([4, 10])
                        .style(theme::segmented_pill_style(t, is_auto))
                        .on_press(Message::SetFanChannelMode(cid_auto, FanChannelMode::Auto)),
                ]
                .spacing(2),
            )
            .padding(2)
            .style(theme::segmented_track_style(t));

            let meter = iced::widget::canvas(BarMeterCanvas {
                fraction: (duty_val / 100.0).clamp(0.02, 1.0),
                accent: badge_accent,
                palette: p,
                accent_override: t.accent_override,
                anim_t: app.anim_t,
            })
            .width(Length::Fill)
            .height(Length::Fixed(18.0));

            let mut ch_col = column![
                row![
                    container(icon_canvas(UiIcon::Fans, theme::rgb(badge_accent), 15.0))
                        .padding(6)
                        .style(theme::chip_style(t, Some(badge_accent))),
                    column![
                        row![
                            text(if ch.is_pump {
                                format!("{} · Pump", ch.name)
                            } else {
                                ch.name.clone()
                            })
                            .size(13.5)
                            .font(theme::ui_semibold(t.design))
                            .color(theme::rgb(p.ink)),
                        ]
                        .align_y(Alignment::Center),
                        text(truncate_str(&ch.hardware, 28))
                            .size(10.5)
                            .font(font)
                            .color(theme::rgb(p.ink_3)),
                    ]
                    .spacing(1)
                    .width(Length::Fill),
                    mode_seg,
                ]
                .spacing(9)
                .align_y(Alignment::Center),
                row![
                    text(rpm_str)
                        .size(13)
                        .font(mono)
                        .color(theme::rgb(p.ink))
                        .width(Length::Fixed(86.0)),
                    meter,
                    container(
                        text(format!("{duty_val:.0}%"))
                            .size(11)
                            .font(mono)
                            .color(theme::rgb(badge_accent)),
                    )
                    .padding([2, 8])
                    .style(theme::chip_style(t, Some(badge_accent))),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            ]
            .spacing(8);

            if let Some(duty) = steady_duty {
                ch_col = ch_col.push(
                    row![
                        text("Fixed duty")
                            .size(11)
                            .font(font)
                            .color(theme::rgb(p.ink_3)),
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
                            .size(11)
                            .font(mono)
                            .color(theme::rgb(p.brass_hi)),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                );
            }
            if let Some(err) = &ch.error {
                ch_col = ch_col.push(
                    text(format!("Notice: {err}"))
                        .size(11)
                        .font(font)
                        .color(theme::rgb(p.warm)),
                );
            }

            channels_list = channels_list.push(
                container(ch_col)
                    .padding(11)
                    .width(Length::Fill)
                    .style(theme::well_style(t)),
            );
        }
    } else {
        for (idx, (label, rpm)) in app.snapshot.fan_rpms.iter().enumerate() {
            let is_pump = label.to_ascii_lowercase().contains("pump")
                || label.to_ascii_lowercase().contains("aio");
            let badge_accent = if is_pump {
                t.pink_rose
            } else if idx % 2 == 0 {
                t.data_primary
            } else {
                t.data_secondary
            };
            let est_duty = if *rpm > 0 {
                ((*rpm as f32 / 2100.0) * 100.0).clamp(20.0, 100.0)
            } else {
                app.snapshot.fan_duty as f32
            };
            let meter = iced::widget::canvas(BarMeterCanvas {
                fraction: (est_duty / 100.0).clamp(0.05, 1.0),
                accent: badge_accent,
                palette: p,
                accent_override: t.accent_override,
                anim_t: app.anim_t,
            })
            .width(Length::Fill)
            .height(Length::Fixed(18.0));

            let mode_badge = container(
                row![
                    container(
                        text("Curve")
                            .size(10.5)
                            .font(theme::ui_semibold(t.design))
                            .color(theme::rgb(p.brass_hi)),
                    )
                    .padding([3, 9])
                    .style(theme::chip_style(t, Some(p.brass))),
                    text("Steady")
                        .size(10.5)
                        .font(font)
                        .color(theme::rgb(p.ink_3)),
                    text("Auto")
                        .size(10.5)
                        .font(font)
                        .color(theme::rgb(p.ink_3)),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .padding([3, 8])
            .style(theme::segmented_track_style(t));

            channels_list = channels_list.push(
                container(
                    column![
                        row![
                            container(icon_canvas(
                                UiIcon::Fans,
                                theme::rgb(badge_accent),
                                14.5
                            ))
                            .padding(5)
                            .style(theme::chip_style(t, Some(badge_accent))),
                            column![
                                text(label.clone())
                                    .size(13)
                                    .font(theme::ui_semibold(t.design))
                                    .color(theme::rgb(p.ink)),
                                text(format!("Following {} curve", cap_first(active_name)))
                                    .size(10.5)
                                    .font(font)
                                    .color(theme::rgb(p.ink_3)),
                            ]
                            .spacing(1)
                            .width(Length::Fill),
                            mode_badge,
                        ]
                        .spacing(9)
                        .align_y(Alignment::Center),
                        row![
                            text(format!("{rpm} RPM"))
                                .size(13)
                                .font(mono)
                                .color(theme::rgb(p.ink))
                                .width(Length::Fixed(86.0)),
                            meter,
                            container(
                                text(format!("{}%", app.snapshot.fan_duty))
                                    .size(11)
                                    .font(mono)
                                    .color(theme::rgb(badge_accent)),
                            )
                            .padding([2, 8])
                            .style(theme::chip_style(t, Some(badge_accent))),
                        ]
                        .spacing(10)
                        .align_y(Alignment::Center),
                    ]
                    .spacing(7),
                )
                .padding(11)
                .width(Length::Fill)
                .style(theme::well_style(t)),
            );
        }

        channels_list = channels_list.push(
            container(
                row![
                    status_dot(p.warm, 7.0),
                    text("Enable LibreHardwareMonitor Remote Web Server in Settings → Sensors & Devices to unlock per-header Steady/Auto overrides.")
                        .size(11)
                        .font(font)
                        .color(theme::rgb(p.ink_3))
                        .width(Length::Fill),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .padding([8, 10])
            .style(theme::well_style(t)),
        );
    }

    let channels_card = container(
        column![
            row![
                icon_canvas(UiIcon::Sliders, theme::rgb(t.data_secondary), 15.0),
                text("FAN & PUMP CHANNELS")
                    .size(11.5)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(p.ink_2)),
                Space::new().width(Length::Fill),
                status_dot(if s.lhm_connected { p.ok } else { t.data_primary }, 7.0),
                text(if s.lhm_connected {
                    format!("{} LHM headers", app.snapshot.lhm_channels.len())
                } else {
                    format!("{} rig channels", app.snapshot.fan_rpms.len())
                })
                .size(11)
                .font(mono)
                .color(theme::rgb(p.ink_3)),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
            scrollable(channels_list).height(Length::Fill),
        ]
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

    // Acoustic Hysteresis (0–8°C) & Safety Floor Instrument Card
    let hysteresis_card = container(
        column![
            row![
                icon_canvas(UiIcon::Fans, theme::rgb(p.brass_hi), 14.5),
                text("ACOUSTIC HYSTERESIS & HARDWARE SAFETY")
                    .size(11)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(p.ink_2)),
                Space::new().width(Length::Fill),
                container(
                    text(format!("{hyst:.1} °C deadband"))
                        .size(11)
                        .font(mono)
                        .color(theme::rgb(p.brass_hi)),
                )
                .padding([2, 9])
                .style(theme::chip_style(t, Some(p.brass))),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
            row![
                text("0°C").size(10.5).font(mono).color(theme::rgb(p.ink_3)),
                slider(0.0..=8.0, hyst, |v| {
                    let snapped = (v * 2.0).round() / 2.0;
                    Message::SetFanHysteresis(snapped)
                })
                .step(0.5)
                .style(theme::slider_style(t))
                .width(Length::Fill),
                text("8°C").size(10.5).font(mono).color(theme::rgb(p.ink_3)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            row![
                container(
                    row![
                        status_dot(p.ok, 6.5),
                        text(format!("Anti-stall floor ≥ {MIN_FAN_DUTY}%"))
                            .size(11)
                            .font(mono)
                            .color(theme::rgb(p.ink_2)),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .padding([4, 9])
                .width(Length::Fill)
                .style(theme::well_style(t)),
                container(
                    row![
                        status_dot(p.hot, 6.5),
                        text(format!(
                            "Critical ≥ {} → 100%",
                            unit.format_temp(SAFETY_TEMP)
                        ))
                        .size(11)
                        .font(mono)
                        .color(theme::rgb(p.ink_2)),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .padding([4, 9])
                .width(Length::Fill)
                .style(theme::well_style(t)),
            ]
            .spacing(8),
        ]
        .spacing(9),
    )
    .padding(t.pad(14))
    .width(Length::Fill)
    .style(theme::rich_instrument_card_style(t, Some(p.brass), false));

    let right_col = column![telemetry_strip, channels_card, hysteresis_card]
        .spacing(t.sp(12))
        .width(Length::FillPortion(5))
        .height(Length::Fill);

    container(
        row![left_col, right_col]
            .spacing(t.sp(16))
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .padding([18, 26])
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

// ===========================================================================
// 2. AUTOMATIONS — SMART RULES STUDIO
// ===========================================================================

pub fn view_automations<'a>(
    app: &'a AirglowApp,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let mono = theme::mono_font();
    let status = &app.snapshot.automation_status;
    let paused = app.snapshot.config.automations_paused;
    let unit = app.snapshot.config.settings.temp_unit;

    let active_count = status.active_ids.len();
    let header_bar = row![
        icon_canvas(UiIcon::Automations, theme::rgb(p.brass_hi), 22.0),
        column![
            row![
                text("Smart Rules Studio")
                    .size(23)
                    .font(theme::display_semibold(t.design))
                    .color(theme::rgb(p.ink)),
                container(
                    text(if paused {
                        "PAUSED".to_string()
                    } else if active_count > 0 {
                        format!("{active_count} ACTIVE NOW")
                    } else {
                        format!("{} ARMED", app.snapshot.config.automations.len())
                    })
                    .size(10.5)
                    .font(mono)
                    .color(theme::rgb(if paused {
                        p.warm
                    } else if active_count > 0 {
                        p.ok
                    } else {
                        t.data_primary
                    })),
                )
                .padding([3, 9])
                .style(theme::chip_style(
                    t,
                    Some(if paused { p.warm } else { t.data_primary })
                )),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            text("Context-aware lighting & cooling rules — highest priority active rule takes control and cleanly restores when done.")
                .size(12)
                .font(font)
                .color(theme::rgb(p.ink_3)),
        ]
        .spacing(2)
        .width(Length::Fill),
        image(&app.moth_art)
            .content_fit(ContentFit::Contain)
            .width(Length::Fixed(48.0))
            .height(Length::Fixed(44.0)),
        button(
            row![
                icon_canvas(
                    UiIcon::Power,
                    theme::rgb(if paused { t.primary_ink } else { p.brass_hi }),
                    13.5,
                ),
                text(if paused {
                    "Resume Engine"
                } else {
                    "Pause Rules"
                })
                .size(12)
                .font(theme::ui_semibold(t.design)),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([7, 15])
        .style(move |th, st| {
            if paused {
                theme::btn_primary(t)(th, st)
            } else {
                theme::btn_secondary(t, false)(th, st)
            }
        })
        .on_press(Message::SetAutomationsPaused(!paused)),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    // Top Row: 4 Rich Visual Smart Recipe Cards
    let recipe_card = |icon: UiIcon,
                       badge: &'static str,
                       tint: u32,
                       title: &'static str,
                       flow: &'static str,
                       desc: &'static str,
                       idea: StarterIdea| {
        container(
            column![
                row![
                    container(icon_canvas(icon, theme::rgb(tint), 15.0))
                        .padding(5)
                        .style(theme::chip_style(t, Some(tint))),
                    text(badge)
                        .size(10.5)
                        .font(theme::ui_semibold(t.design))
                        .color(theme::rgb(tint)),
                    Space::new().width(Length::Fill),
                    button(
                        row![
                            icon_canvas(UiIcon::Plus, theme::rgb(p.brass_hi), 11.5),
                            text("Add recipe")
                                .size(11)
                                .font(theme::ui_semibold(t.design)),
                        ]
                        .spacing(4)
                        .align_y(Alignment::Center),
                    )
                    .padding([4, 10])
                    .style(theme::btn_secondary(t, false))
                    .on_press(Message::AddStarterAutomation(idea)),
                ]
                .spacing(7)
                .align_y(Alignment::Center),
                text(title)
                    .size(15)
                    .font(theme::display_semibold(t.design))
                    .color(theme::rgb(p.ink)),
                container(
                    text(flow)
                        .size(10.5)
                        .font(mono)
                        .color(theme::rgb(tint)),
                )
                .padding([3, 8])
                .style(theme::well_style(t)),
                text(desc)
                    .size(11.5)
                    .font(font)
                    .color(theme::rgb(p.ink_3)),
            ]
            .spacing(6),
        )
        .padding(t.pad(13))
        .width(Length::Fill)
        .style(theme::rich_instrument_card_style(t, Some(tint), false))
    };

    let recipes_row = row![
        recipe_card(
            UiIcon::Lighting,
            "SCHEDULE",
            t.pink_rose,
            "Sunset Dim",
            "23:00–07:00  →  Dim 20%",
            "Softly lowers rig brightness every evening until morning.",
            StarterIdea::SunsetDim,
        ),
        recipe_card(
            UiIcon::Sparkle,
            "APP LAUNCH",
            p.brass_hi,
            "Gaming Boost",
            "Cyberpunk2077.exe  →  Gaming",
            "Activates vivid Gaming profile automatically on launch.",
            StarterIdea::GamingBoost,
        ),
        recipe_card(
            UiIcon::Fans,
            "ACOUSTIC",
            t.data_primary,
            "Quiet When Idle",
            "Idle ≥ 15 min  →  Silent Curve",
            "Drops case fans to whisper-quiet ramp while away.",
            StarterIdea::QuietWhenIdle,
        ),
        recipe_card(
            UiIcon::Sliders,
            "THERMAL GUARD",
            p.warm,
            "Thermal Boost",
            "Temp > 75°C  →  Performance",
            "Ramps cooling headroom under sustained heavy loads.",
            StarterIdea::ThermalBoost,
        ),
    ]
    .spacing(t.sp(12));

    // Left Main Split (`FillPortion(6)`): Active Automation Rules
    let mut rules_list = column![].spacing(9);
    if app.snapshot.config.automations.is_empty() {
        rules_list = rules_list.push(
            container(
                column![
                    text("No automation rules configured yet")
                        .size(14)
                        .font(theme::ui_semibold(t.design))
                        .color(theme::rgb(p.ink)),
                    text("Click any Smart Recipe above or compose a custom rule on the right.")
                        .size(12)
                        .font(font)
                        .color(theme::rgb(p.ink_3)),
                ]
                .spacing(4),
            )
            .padding(18)
            .width(Length::Fill)
            .style(theme::well_style(t)),
        );
    } else {
        for (idx, rule) in app.snapshot.config.automations.iter().enumerate() {
            let is_light_win = status.light_winner.as_deref() == Some(&rule.id);
            let is_fan_win = status.fan_winner.as_deref() == Some(&rule.id);
            let is_active = status.active_ids.iter().any(|id| id == &rule.id);
            let in_control = is_light_win || is_fan_win;

            let (pill_lbl, pill_col) = if paused {
                ("PAUSED", p.warm)
            } else if !rule.enabled {
                ("DISABLED", p.ink_3)
            } else if is_light_win && is_fan_win {
                ("CONTROLLING RIG", p.brass_hi)
            } else if is_light_win {
                ("CONTROLLING LIGHTS", p.brass_hi)
            } else if is_fan_win {
                ("CONTROLLING FANS", t.data_primary)
            } else if is_active {
                ("ACTIVE · OVERRIDDEN", p.ok)
            } else {
                ("STANDBY", p.ink_3)
            };

            let trig_text = rule.trigger.describe();
            let act_text = rule.action.describe(&app.snapshot.config.presets);

            let rank_badge = container(
                text(format!("#{}", idx + 1))
                    .size(11.5)
                    .font(mono)
                    .color(theme::rgb(if in_control { p.brass_hi } else { p.ink_2 })),
            )
            .padding([5, 9])
            .style(theme::chip_style(
                t,
                if in_control { Some(p.brass_hi) } else { None },
            ));

            rules_list = rules_list.push(
                container(
                    row![
                        rank_badge,
                        column![
                            row![
                                text(trig_text)
                                    .size(13.5)
                                    .font(theme::ui_semibold(t.design))
                                    .color(theme::rgb(p.ink)),
                                Space::new().width(Length::Fill),
                                container(
                                    text(pill_lbl)
                                        .size(10)
                                        .font(mono)
                                        .color(theme::rgb(pill_col)),
                                )
                                .padding([2, 8])
                                .style(theme::chip_style(
                                    t,
                                    if in_control || is_active {
                                        Some(pill_col)
                                    } else {
                                        None
                                    },
                                )),
                            ]
                            .spacing(8)
                            .align_y(Alignment::Center),
                            text(format!("→ {act_text}"))
                                .size(12)
                                .font(font)
                                .color(theme::rgb(if in_control {
                                    p.brass_hi
                                } else {
                                    t.data_primary
                                })),
                        ]
                        .spacing(4)
                        .width(Length::Fill),
                        pill_toggle(t, rule.enabled, Message::ToggleAutomation(rule.id.clone())),
                        button(text("↑").size(12).font(font))
                            .padding([5, 9])
                            .style(theme::btn_secondary(t, false))
                            .on_press(Message::MoveAutomation(rule.id.clone(), true)),
                        button(text("↓").size(12).font(font))
                            .padding([5, 9])
                            .style(theme::btn_secondary(t, false))
                            .on_press(Message::MoveAutomation(rule.id.clone(), false)),
                        button(text("×").size(12).font(font))
                            .padding([5, 9])
                            .style(theme::btn_secondary(t, false))
                            .on_press(Message::DeleteAutomation(rule.id.clone())),
                    ]
                    .spacing(9)
                    .align_y(Alignment::Center),
                )
                .padding(t.pad(12))
                .width(Length::Fill)
                .style(theme::active_card_box(t, in_control)),
            );
        }
    }

    let light_ctrl_lbl = status
        .light_winner
        .as_deref()
        .and_then(|id| app.snapshot.config.automations.iter().find(|r| r.id == id))
        .map(|r| truncate_str(&r.action.describe(&app.snapshot.config.presets), 24))
        .unwrap_or_else(|| "Manual Scene".to_string());
    let fan_ctrl_lbl = status
        .fan_winner
        .as_deref()
        .and_then(|id| app.snapshot.config.automations.iter().find(|r| r.id == id))
        .map(|r| truncate_str(&r.action.describe(&app.snapshot.config.presets), 24))
        .unwrap_or_else(|| format!("{} Curve", cap_first(&app.snapshot.effective_curve)));
    let hot_temp = app
        .snapshot
        .sensors
        .cpu_temp
        .into_iter()
        .chain(app.snapshot.sensors.gpu_temp)
        .fold(None::<f32>, |acc, v| Some(acc.map_or(v, |a| a.max(v))));

    let evaluator_guard_card = container(
        column![
            row![
                icon_canvas(UiIcon::Sliders, theme::rgb(t.pink_rose), 14.0),
                text("LIVE ENGINE EVALUATOR & GUARD")
                    .size(11)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(t.pink_rose)),
                Space::new().width(Length::Fill),
                container(
                    text(if paused { "PAUSED" } else { "250MS POLL" })
                        .size(10)
                        .font(mono)
                        .color(theme::rgb(if paused { p.warm } else { p.ok })),
                )
                .padding([2, 8])
                .style(theme::chip_style(
                    t,
                    Some(if paused { p.warm } else { p.ok })
                )),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
            row![
                container(
                    column![
                        text("IDLE TIMER")
                            .size(9.5)
                            .font(theme::ui_semibold(t.design))
                            .color(theme::rgb(p.ink_3)),
                        text(format!("{}s", app.snapshot.idle_secs))
                            .size(14)
                            .font(mono)
                            .color(theme::rgb(t.data_primary)),
                    ]
                    .spacing(2),
                )
                .padding([7, 10])
                .width(Length::Fill)
                .style(theme::well_style(t)),
                container(
                    column![
                        text("THERMAL PEAK")
                            .size(9.5)
                            .font(theme::ui_semibold(t.design))
                            .color(theme::rgb(p.ink_3)),
                        text(
                            hot_temp
                                .map(|v| unit.format_temp(v))
                                .unwrap_or_else(|| "—".to_string())
                        )
                        .size(14)
                        .font(mono)
                        .color(theme::rgb(p.brass_hi)),
                    ]
                    .spacing(2),
                )
                .padding([7, 10])
                .width(Length::Fill)
                .style(theme::well_style(t)),
                container(
                    column![
                        text("WORKSTATION")
                            .size(9.5)
                            .font(theme::ui_semibold(t.design))
                            .color(theme::rgb(p.ink_3)),
                        text(if app.snapshot.workstation_locked {
                            "Locked"
                        } else {
                            "Unlocked"
                        })
                        .size(14)
                        .font(mono)
                        .color(theme::rgb(p.ok)),
                    ]
                    .spacing(2),
                )
                .padding([7, 10])
                .width(Length::Fill)
                .style(theme::well_style(t)),
            ]
            .spacing(8),
            row![
                container(
                    row![
                        status_dot(p.brass_hi, 6.5),
                        text(format!("Lights: {light_ctrl_lbl}"))
                            .size(11.5)
                            .font(font)
                            .color(theme::rgb(p.ink_2)),
                    ]
                    .spacing(7)
                    .align_y(Alignment::Center),
                )
                .padding([6, 10])
                .width(Length::Fill)
                .style(theme::well_style(t)),
                container(
                    row![
                        status_dot(t.data_primary, 6.5),
                        text(format!("Fans: {fan_ctrl_lbl}"))
                            .size(11.5)
                            .font(font)
                            .color(theme::rgb(p.ink_2)),
                    ]
                    .spacing(7)
                    .align_y(Alignment::Center),
                )
                .padding([6, 10])
                .width(Length::Fill)
                .style(theme::well_style(t)),
            ]
            .spacing(8),
        ]
        .spacing(8),
    )
    .padding(t.pad(13))
    .width(Length::Fill)
    .style(theme::rich_instrument_card_style(t, Some(t.pink_rose), false));

    let left_rules_card = container(
        column![
            row![
                icon_canvas(UiIcon::Automations, theme::rgb(t.data_primary), 15.0),
                text("ACTIVE PRIORITY STACK")
                    .size(11.5)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(p.ink_2)),
                Space::new().width(Length::Fill),
                text(format!(
                    "{} rule(s) · {} active",
                    app.snapshot.config.automations.len(),
                    active_count
                ))
                .size(11)
                .font(mono)
                .color(theme::rgb(p.ink_3)),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
            scrollable(rules_list).height(Length::Fill),
            evaluator_guard_card,
        ]
        .spacing(10)
        .height(Length::Fill),
    )
    .padding(t.pad(15))
    .width(Length::FillPortion(6))
    .height(Length::Fill)
    .style(theme::rich_instrument_card_style(
        t,
        Some(t.data_primary),
        false,
    ));

    // Right Main Split (`FillPortion(5)`): Spacious Rule Composer Card
    let trig_seg_1 = row![
        button(text("Time Window").size(11.5).font(font))
            .padding([6, 10])
            .width(Length::Fill)
            .style(theme::segmented_pill_style(
                t,
                app.auto_trigger == AutoTriggerDraft::TimeWindow,
            ))
            .on_press(Message::SetAutoTriggerDraft(AutoTriggerDraft::TimeWindow)),
        button(text("App (.exe)").size(11.5).font(font))
            .padding([6, 10])
            .width(Length::Fill)
            .style(theme::segmented_pill_style(
                t,
                app.auto_trigger == AutoTriggerDraft::AppRunning,
            ))
            .on_press(Message::SetAutoTriggerDraft(AutoTriggerDraft::AppRunning)),
        button(text("Idle").size(11.5).font(font))
            .padding([6, 10])
            .width(Length::Fill)
            .style(theme::segmented_pill_style(
                t,
                app.auto_trigger == AutoTriggerDraft::Idle,
            ))
            .on_press(Message::SetAutoTriggerDraft(AutoTriggerDraft::Idle)),
        button(text("Locked").size(11.5).font(font))
            .padding([6, 10])
            .width(Length::Fill)
            .style(theme::segmented_pill_style(
                t,
                app.auto_trigger == AutoTriggerDraft::Locked,
            ))
            .on_press(Message::SetAutoTriggerDraft(AutoTriggerDraft::Locked)),
        button(text("Temp >").size(11.5).font(font))
            .padding([6, 10])
            .width(Length::Fill)
            .style(theme::segmented_pill_style(
                t,
                app.auto_trigger == AutoTriggerDraft::TempAbove,
            ))
            .on_press(Message::SetAutoTriggerDraft(AutoTriggerDraft::TempAbove)),
    ]
    .spacing(3);

    let trig_detail: Element<'_, Message> = match app.auto_trigger {
        AutoTriggerDraft::TimeWindow => {
            let mut day_row = row![
                button(text("Daily").size(11).font(font))
                    .padding([4, 9])
                    .style(theme::btn_secondary(t, app.auto_days_mask == ALL_DAYS_MASK))
                    .on_press(Message::SetAutoDaysMask(ALL_DAYS_MASK)),
            ]
            .spacing(4);
            for (idx, lbl) in ["M", "T", "W", "T", "F", "S", "S"].iter().enumerate() {
                let on = (app.auto_days_mask & (1u8 << idx)) != 0;
                day_row = day_row.push(
                    button(text(*lbl).size(11).font(font))
                        .padding([4, 8])
                        .style(theme::btn_secondary(t, on))
                        .on_press(Message::ToggleAutoDay(idx as u8)),
                );
            }
            column![
                row![
                    text("Start:").size(12).font(font),
                    text_input("23:00", &app.auto_start_draft)
                        .font(mono)
                        .style(theme::text_input_style(t))
                        .on_input(Message::SetAutoStartDraft)
                        .width(Length::Fixed(80.0)),
                    text("End:").size(12).font(font),
                    text_input("07:00", &app.auto_end_draft)
                        .font(mono)
                        .style(theme::text_input_style(t))
                        .on_input(Message::SetAutoEndDraft)
                        .width(Length::Fixed(80.0)),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
                day_row,
            ]
            .spacing(8)
            .into()
        }
        AutoTriggerDraft::AppRunning => column![
            text("Process executable name (case-insensitive):")
                .size(11.5)
                .font(font)
                .color(theme::rgb(p.ink_3)),
            text_input("Cyberpunk2077.exe", &app.auto_exe_draft)
                .font(mono)
                .style(theme::text_input_style(t))
                .on_input(Message::SetAutoExeDraft)
                .width(Length::Fill),
        ]
        .spacing(6)
        .into(),
        AutoTriggerDraft::Idle => row![
            text("Trigger after").size(12).font(font),
            text_input("10", &app.auto_idle_draft)
                .font(mono)
                .style(theme::text_input_style(t))
                .on_input(Message::SetAutoIdleDraft)
                .width(Length::Fixed(70.0)),
            text(format!("minutes idle (now {}s)", app.snapshot.idle_secs))
                .size(12)
                .font(font)
                .color(theme::rgb(p.ink_3)),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into(),
        AutoTriggerDraft::Locked => text(
            "Activates the moment Windows workstation locks (Win+L) and restores upon unlock.",
        )
        .size(12)
        .font(font)
        .color(theme::rgb(p.ink_2))
        .into(),
        AutoTriggerDraft::TempAbove => {
            let mut src_row = row![].spacing(3);
            for src in TempSource::ALL {
                let short = match src {
                    TempSource::Max => "Hotter",
                    TempSource::Cpu => "CPU",
                    TempSource::Gpu => "GPU",
                    TempSource::Liquid => "Coolant",
                };
                src_row = src_row.push(
                    button(text(short).size(11).font(font))
                        .padding([4, 9])
                        .style(theme::segmented_pill_style(t, app.auto_temp_source == src))
                        .on_press(Message::SetAutoTempSource(src)),
                );
            }
            let parsed_c = app.auto_temp_draft.trim().parse::<u8>().unwrap_or(80);
            let equiv = if unit == TempUnit::F {
                format!("°C ({})", unit.format_temp(parsed_c as f32))
            } else {
                "°C (3°C cooldown hysteresis)".to_string()
            };
            column![
                container(src_row)
                    .padding(2)
                    .style(theme::segmented_track_style(t)),
                row![
                    text("Exceeds").size(12).font(font),
                    text_input("80", &app.auto_temp_draft)
                        .font(mono)
                        .style(theme::text_input_style(t))
                        .on_input(Message::SetAutoTempDraft)
                        .width(Length::Fixed(70.0)),
                    text(equiv).size(11.5).font(font).color(theme::rgb(p.ink_3)),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ]
            .spacing(8)
            .into()
        }
    };

    let act_seg = row![
        button(text("Profile").size(11.5).font(font))
            .padding([6, 10])
            .width(Length::Fill)
            .style(theme::segmented_pill_style(
                t,
                app.auto_action == AutoActionDraft::ApplyPreset,
            ))
            .on_press(Message::SetAutoActionDraft(AutoActionDraft::ApplyPreset)),
        button(text("Lights Off").size(11.5).font(font))
            .padding([6, 10])
            .width(Length::Fill)
            .style(theme::segmented_pill_style(
                t,
                app.auto_action == AutoActionDraft::LightsOff,
            ))
            .on_press(Message::SetAutoActionDraft(AutoActionDraft::LightsOff)),
        button(text("Dim %").size(11.5).font(font))
            .padding([6, 10])
            .width(Length::Fill)
            .style(theme::segmented_pill_style(
                t,
                app.auto_action == AutoActionDraft::Dim,
            ))
            .on_press(Message::SetAutoActionDraft(AutoActionDraft::Dim)),
        button(text("Fan Curve").size(11.5).font(font))
            .padding([6, 10])
            .width(Length::Fill)
            .style(theme::segmented_pill_style(
                t,
                app.auto_action == AutoActionDraft::FanCurve,
            ))
            .on_press(Message::SetAutoActionDraft(AutoActionDraft::FanCurve)),
    ]
    .spacing(3);

    let act_detail: Element<'_, Message> = match app.auto_action {
        AutoActionDraft::ApplyPreset => {
            let mut grid = column![].spacing(6);
            for chunk in app.snapshot.config.presets.chunks(3) {
                let mut r = row![].spacing(6);
                for preset in chunk {
                    let active = app.auto_preset_draft == preset.id;
                    r = r.push(
                        button(
                            text(truncate_str(&preset.name, 13))
                                .size(11.5)
                                .font(if active {
                                    theme::ui_semibold(t.design)
                                } else {
                                    font
                                }),
                        )
                        .padding([6, 10])
                        .width(Length::Fill)
                        .style(theme::btn_secondary(t, active))
                        .on_press(Message::SetAutoPresetDraft(preset.id.clone())),
                    );
                }
                for _ in chunk.len()..3 {
                    r = r.push(Space::new().width(Length::Fill));
                }
                grid = grid.push(r);
            }
            grid.into()
        }
        AutoActionDraft::LightsOff => text(
            "Smoothly fades all addressable LED zones to black while this rule stays active.",
        )
        .size(12)
        .font(font)
        .color(theme::rgb(p.ink_2))
        .into(),
        AutoActionDraft::Dim => labeled_slider(
            t,
            font,
            "Target brightness cap",
            format!("{}%", app.auto_dim_draft),
            5,
            80,
            app.auto_dim_draft as u16,
            |v| Message::SetAutoDimDraft(v as u8),
        ),
        AutoActionDraft::FanCurve => {
            let mut r = row![].spacing(6);
            for c in ["silent", "balanced", "performance"] {
                let active = app.auto_curve_draft == c;
                r = r.push(
                    button(
                        container(text(cap_first(c)).size(11.5).font(font))
                            .center_x(Length::Fill),
                    )
                    .padding([6, 10])
                    .width(Length::Fill)
                    .style(theme::btn_secondary(t, active))
                    .on_press(Message::SetAutoCurveDraft(c.to_string())),
                );
            }
            r.into()
        }
    };

    let composer_card = container(
        column![
            row![
                icon_canvas(UiIcon::Plus, theme::rgb(p.brass_hi), 15.0),
                text("RULE COMPOSER")
                    .size(11.5)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(p.brass_hi)),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
            text("1. WHEN CONDITION MATCHES")
                .size(10.5)
                .font(theme::ui_semibold(t.design))
                .color(theme::rgb(p.ink_3)),
            container(trig_seg_1)
                .padding(3)
                .width(Length::Fill)
                .style(theme::segmented_track_style(t)),
            container(trig_detail)
                .padding(12)
                .width(Length::Fill)
                .style(theme::well_style(t)),
            text("2. THEN APPLY OVERRIDE")
                .size(10.5)
                .font(theme::ui_semibold(t.design))
                .color(theme::rgb(p.ink_3)),
            container(act_seg)
                .padding(3)
                .width(Length::Fill)
                .style(theme::segmented_track_style(t)),
            container(act_detail)
                .padding(12)
                .width(Length::Fill)
                .style(theme::well_style(t)),
            Space::new().height(Length::Fill),
            button(
                container(
                    row![
                        icon_canvas(UiIcon::Plus, theme::rgb(t.primary_ink), 14.0),
                        text("Add Automation Rule")
                            .size(13)
                            .font(theme::ui_semibold(t.design)),
                    ]
                    .spacing(7)
                    .align_y(Alignment::Center),
                )
                .center_x(Length::Fill),
            )
            .padding([10, 18])
            .width(Length::Fill)
            .style(theme::btn_primary(t))
            .on_press(Message::AddAutomation),
        ]
        .spacing(10)
        .height(Length::Fill),
    )
    .padding(t.pad(16))
    .width(Length::FillPortion(5))
    .height(Length::Fill)
    .style(theme::rich_instrument_card_style(t, Some(p.brass), false));

    let main_split = row![left_rules_card, composer_card]
        .spacing(t.sp(16))
        .width(Length::Fill)
        .height(Length::Fill);

    container(
        column![header_bar, recipes_row, main_split]
            .spacing(t.sp(14))
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .padding([18, 26])
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

// ===========================================================================
// 3. DEVICES — HARDWARE TOPOLOGY & LIVE SENSOR DECK
// ===========================================================================

fn color_order_dots<'a>(order: ColorOrder) -> Element<'a, Message> {
    const R: [u8; 3] = [245, 98, 98];
    const G: [u8; 3] = [108, 224, 148];
    const B: [u8; 3] = [96, 168, 252];
    let triplet = match order {
        ColorOrder::Rgb => [R, G, B],
        ColorOrder::Grb => [G, R, B],
        ColorOrder::Bgr => [B, G, R],
        ColorOrder::Brg => [B, R, G],
        ColorOrder::Rbg => [R, B, G],
        ColorOrder::Gbr => [G, B, R],
    };
    row![
        color_dot(triplet[0], 6.0),
        color_dot(triplet[1], 6.0),
        color_dot(triplet[2], 6.0),
    ]
    .spacing(3)
    .align_y(Alignment::Center)
    .into()
}

fn format_sensor_explorer_val(val: f32, unit_str: &str) -> String {
    match unit_str {
        "°C" | "°F" | "%" | "W" => format!("{val:.1} {unit_str}"),
        "RPM" | "MHz" => format!("{val:.0} {unit_str}"),
        "V" => format!("{val:.3} V"),
        _ if unit_str.is_empty() => format!("{val:.1}"),
        _ => format!("{val:.1} {unit_str}"),
    }
}

fn normalize_sensor_fraction(val: f32, sensor_type: &str, unit_str: &str) -> f32 {
    let st = sensor_type.to_ascii_lowercase();
    if st.contains("load") || st.contains("control") || unit_str == "%" {
        (val / 100.0).clamp(0.02, 1.0)
    } else if st.contains("temp") || unit_str.contains('°') {
        ((val - 20.0) / 75.0).clamp(0.04, 1.0)
    } else if st.contains("power") || unit_str == "W" {
        (val / 250.0).clamp(0.03, 1.0)
    } else if st.contains("fan") || unit_str == "RPM" {
        (val / 2400.0).clamp(0.03, 1.0)
    } else if st.contains("clock") || unit_str == "MHz" {
        (val / 5500.0).clamp(0.05, 1.0)
    } else if st.contains("voltage") || unit_str == "V" {
        (val / 1.6).clamp(0.05, 1.0)
    } else {
        (val / 100.0).clamp(0.06, 1.0)
    }
}

pub fn view_devices<'a>(
    app: &'a AirglowApp,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let mono = theme::mono_font();
    let s = &app.snapshot.sensors;
    let unit = app.snapshot.config.settings.temp_unit;

    let total_zones = app.snapshot.config.zones.len();
    let enabled_zones = app
        .snapshot
        .config
        .zones
        .iter()
        .filter(|z| z.enabled)
        .count();
    let total_leds: usize = app
        .snapshot
        .config
        .zones
        .iter()
        .filter(|z| z.enabled)
        .map(|z| z.layout.total_leds())
        .sum();
    let lhm_sensor_count = s.readings.len();

    // Top Row: 4 Luminous Telemetry Summary Cards with 36px tabular numerals
    let summary_card = |icon: UiIcon,
                        label: &'static str,
                        big_val: String,
                        pill_txt: String,
                        sub: String,
                        accent: u32| {
        container(
            column![
                row![
                    container(icon_canvas(icon, theme::rgb(accent), 15.0))
                        .padding(5)
                        .style(theme::chip_style(t, Some(accent))),
                    text(label)
                        .size(11)
                        .font(theme::ui_semibold(t.design))
                        .color(theme::rgb(p.ink_2)),
                    Space::new().width(Length::Fill),
                    container(
                        text(pill_txt)
                            .size(10)
                            .font(mono)
                            .color(theme::rgb(accent)),
                    )
                    .padding([2, 8])
                    .style(theme::chip_style(t, Some(accent))),
                ]
                .spacing(7)
                .align_y(Alignment::Center),
                text(big_val)
                    .size(36)
                    .font(theme::ui_bold(t.design))
                    .color(theme::rgb(p.ink)),
                text(sub)
                    .size(11.5)
                    .font(font)
                    .color(theme::rgb(p.ink_3)),
            ]
            .spacing(4),
        )
        .padding(t.pad(14))
        .width(Length::Fill)
        .style(theme::rich_instrument_card_style(t, Some(accent), false))
    };

    let top_summary = row![
        summary_card(
            UiIcon::Devices,
            "USB HID CONTROLLERS",
            format!("{}", app.snapshot.hid_devices.len()),
            if app.snapshot.hid_devices.is_empty() {
                "SIMULATED".to_string()
            } else {
                "NATIVE USB".to_string()
            },
            if app.snapshot.hid_devices.is_empty() {
                "Live 2D virtual rig active".to_string()
            } else {
                "Direct low-latency HID streams".to_string()
            },
            p.ok,
        ),
        summary_card(
            UiIcon::Lighting,
            "OPENRGB SDK",
            format!("{}", app.snapshot.openrgb.controller_count),
            if app.snapshot.openrgb.connected {
                "ONLINE".to_string()
            } else {
                "STANDBY".to_string()
            },
            truncate_str(&app.snapshot.openrgb.message, 34),
            t.data_secondary,
        ),
        summary_card(
            UiIcon::Sparkle,
            "ACTIVE ZONES & LEDS",
            format!("{total_leds}"),
            format!("{enabled_zones}/{total_zones} ZONES"),
            format!("{:.0} FPS hardware crossfade stream", app.snapshot.fps.max(1.0)),
            p.brass_hi,
        ),
        summary_card(
            UiIcon::Sliders,
            "LHM SENSORS",
            if lhm_sensor_count > 0 {
                format!("{lhm_sensor_count}")
            } else {
                "12".to_string()
            },
            if s.lhm_connected {
                "LIVE BRIDGE".to_string()
            } else {
                "SYSINFO".to_string()
            },
            if s.lhm_connected {
                "LibreHardwareMonitor JSON active".to_string()
            } else {
                "Native Windows telemetry active".to_string()
            },
            t.data_primary,
        ),
    ]
    .spacing(t.sp(12));

    // Left Main Split (`FillPortion(6)`): Connected RGB & Fan Controllers + Zone Geometry Deck
    let mut hw_scroll_col = column![].spacing(10);

    // Connected USB HID / OpenRGB controller cards
    if app.snapshot.hid_devices.is_empty() && app.snapshot.openrgb.controllers.is_empty() {
        hw_scroll_col = hw_scroll_col.push(
            container(
                row![
                    container(icon_canvas(UiIcon::Devices, theme::rgb(p.ok), 16.0))
                        .padding(7)
                        .style(theme::chip_style(t, Some(p.ok))),
                    column![
                        row![
                            text("Airglow Virtual Rig Controller")
                                .size(13.5)
                                .font(theme::ui_semibold(t.design)),
                            container(
                                text("2D TOPOLOGY")
                                    .size(9.5)
                                    .font(mono)
                                    .color(theme::rgb(p.ok)),
                            )
                            .padding([2, 7])
                            .style(theme::chip_style(t, Some(p.ok))),
                        ]
                        .spacing(8)
                        .align_y(Alignment::Center),
                        text(format!(
                            "{enabled_zones} zones mapped · {total_leds} addressable RGB pixels · Connect NZXT / ASUS USB or OpenRGB to stream live"
                        ))
                        .size(11.5)
                        .font(font)
                        .color(theme::rgb(p.ink_3)),
                    ]
                    .spacing(2)
                    .width(Length::Fill),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding(12)
            .width(Length::Fill)
            .style(theme::well_style(t)),
        );
    } else {
        for dev in &app.snapshot.hid_devices {
            let health_col = if dev.online { p.ok } else { p.warm };
            hw_scroll_col = hw_scroll_col.push(
                container(
                    row![
                        container(icon_canvas(UiIcon::Devices, theme::rgb(health_col), 16.0))
                            .padding(7)
                            .style(theme::chip_style(t, Some(health_col))),
                        column![
                            row![
                                text(&dev.name)
                                    .size(13.5)
                                    .font(theme::ui_semibold(t.design)),
                                container(
                                    text("USB HID")
                                        .size(9.5)
                                        .font(mono)
                                        .color(theme::rgb(t.data_primary)),
                                )
                                .padding([2, 7])
                                .style(theme::chip_style(t, Some(t.data_primary))),
                            ]
                            .spacing(8)
                            .align_y(Alignment::Center),
                            text(format!("{} · {}", dev.kind, dev.note))
                                .size(11.5)
                                .font(font)
                                .color(theme::rgb(p.ink_3)),
                        ]
                        .spacing(2)
                        .width(Length::Fill),
                        status_dot(health_col, 7.5),
                        text(if dev.online { "Healthy" } else { "Busy" })
                            .size(11.5)
                            .font(mono)
                            .color(theme::rgb(health_col)),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                )
                .padding(12)
                .width(Length::Fill)
                .style(theme::well_style(t)),
            );
        }
        for ctrl in &app.snapshot.openrgb.controllers {
            hw_scroll_col = hw_scroll_col.push(
                container(
                    row![
                        container(icon_canvas(
                            UiIcon::Lighting,
                            theme::rgb(t.data_secondary),
                            15.0,
                        ))
                        .padding(6)
                        .style(theme::chip_style(t, Some(t.data_secondary))),
                        column![
                            row![
                                text(truncate_str(&ctrl.name, 26))
                                    .size(13)
                                    .font(theme::ui_semibold(t.design)),
                                container(
                                    text("OPENRGB TCP")
                                        .size(9.5)
                                        .font(mono)
                                        .color(theme::rgb(t.data_secondary)),
                                )
                                .padding([2, 7])
                                .style(theme::chip_style(t, Some(t.data_secondary))),
                            ]
                            .spacing(8)
                            .align_y(Alignment::Center),
                            text(format!("Controller index #{}", ctrl.index))
                                .size(11)
                                .font(mono)
                                .color(theme::rgb(p.ink_3)),
                        ]
                        .spacing(2)
                        .width(Length::Fill),
                        status_dot(p.ok, 7.0),
                        text("Online")
                            .size(11.5)
                            .font(mono)
                            .color(theme::rgb(p.ok)),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                )
                .padding(11)
                .width(Length::Fill)
                .style(theme::well_style(t)),
            );
        }
    }

    hw_scroll_col = hw_scroll_col.push(
        text("ADDRESSABLE ZONE TOPOLOGY & COLOUR ORDER")
            .size(11)
            .font(theme::ui_semibold(t.design))
            .color(theme::rgb(p.ink_3)),
    );

    for z in &app.snapshot.config.zones {
        let zid = z.id.clone();
        let zid_id = z.id.clone();
        let zid_ord = z.id.clone();
        let zid_exp = z.id.clone();
        let flashing = app.snapshot.identify_zone.as_deref() == Some(&z.id);
        let expanded = app.expanded_zone.as_deref() == Some(&z.id);
        let led_count = z.layout.total_leds();

        let header_row = row![
            checkbox(z.enabled)
                .label(truncate_str(&z.name, 26))
                .style(theme::checkbox_style(t))
                .on_toggle(move |_| Message::ToggleZone(zid.clone())),
            container(
                text(format!("{led_count} LEDs · {}", z.layout.shape_label()))
                    .size(10.5)
                    .font(mono)
                    .color(theme::rgb(p.brass_hi)),
            )
            .padding([2, 8])
            .style(theme::chip_style(t, Some(p.brass))),
            Space::new().width(Length::Fill),
            button(
                row![
                    color_order_dots(z.color_order),
                    text(format!("{:?}", z.color_order).to_uppercase())
                        .size(10.5)
                        .font(mono),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .padding([4, 9])
            .style(theme::btn_secondary(t, false))
            .on_press(Message::CycleZoneColorOrder(zid_ord)),
            button(
                text(if flashing { "Blinking…" } else { "Identify" })
                    .size(11)
                    .font(font),
            )
            .padding([4, 10])
            .style(theme::btn_secondary(t, flashing))
            .on_press(Message::IdentifyZone(zid_id)),
            button(
                text(if expanded { "Close" } else { "Setup" })
                    .size(11)
                    .font(font),
            )
            .padding([4, 10])
            .style(theme::btn_secondary(t, expanded))
            .on_press(Message::ToggleExpandZone(zid_exp)),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        let mut zone_col = column![header_row].spacing(9);
        if expanded {
            zone_col = zone_col.push(zone_setup_editor(z, t, font));
        }

        hw_scroll_col = hw_scroll_col.push(
            container(zone_col)
                .padding([9, 11])
                .width(Length::Fill)
                .style(theme::well_style(t)),
        );
    }

    let left_controllers_card = container(
        column![
            row![
                icon_canvas(UiIcon::Devices, theme::rgb(p.brass_hi), 16.0),
                text("CONNECTED RGB & FAN CONTROLLERS")
                    .size(11.5)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(p.ink_2)),
                Space::new().width(Length::Fill),
                button(
                    row![
                        icon_canvas(UiIcon::Refresh, theme::rgb(t.primary_ink), 13.0),
                        text("Rescan")
                            .size(12)
                            .font(theme::ui_semibold(t.design)),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .padding([6, 14])
                .style(theme::btn_primary(t))
                .on_press(Message::RescanHardware),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            scrollable(hw_scroll_col).height(Length::Fill),
        ]
        .spacing(11)
        .height(Length::Fill),
    )
    .padding(t.pad(15))
    .width(Length::FillPortion(6))
    .height(Length::Fill)
    .style(theme::rich_instrument_card_style(t, Some(p.brass), false));

    // Right Main Split (`FillPortion(5)`): Searchable Live Sensor Explorer + RGB Conflict Guard
    let q = app.sensor_search_query.trim().to_ascii_lowercase();
    let mut sensor_rows = column![].spacing(7);
    let mut match_count = 0usize;

    if !s.readings.is_empty() {
        let filtered: Vec<&sensors::SensorReading> = s
            .readings
            .iter()
            .filter(|r| {
                if q.is_empty() {
                    true
                } else {
                    let hay = format!(
                        "{} {} {} {} {}",
                        r.name, r.hardware, r.sensor_type, r.hw_kind, r.id
                    )
                    .to_ascii_lowercase();
                    q.split_whitespace().all(|tok| hay.contains(tok))
                }
            })
            .collect();
        match_count = filtered.len();

        for reading in filtered.into_iter().take(60) {
            let frac = normalize_sensor_fraction(reading.value, &reading.sensor_type, &reading.unit);
            let bar_accent = if reading.sensor_type.eq_ignore_ascii_case("Temperature") {
                t.pink_rose
            } else if reading.sensor_type.eq_ignore_ascii_case("Load") {
                t.data_primary
            } else if reading.sensor_type.eq_ignore_ascii_case("Power") {
                p.brass_hi
            } else {
                t.data_secondary
            };
            let sparkbar = iced::widget::canvas(BarMeterCanvas {
                fraction: frac,
                accent: bar_accent,
                palette: p,
                accent_override: t.accent_override,
                anim_t: app.anim_t,
            })
            .width(Length::Fixed(86.0))
            .height(Length::Fixed(16.0));

            let pin_kind = OverviewTileKind::CustomSensor {
                sensor_id: reading.id.clone(),
                label: reading.name.clone(),
                unit: reading.unit.clone(),
            };

            sensor_rows = sensor_rows.push(
                container(
                    row![
                        column![
                            text(truncate_str(&reading.name, 26))
                                .size(12.5)
                                .font(theme::ui_semibold(t.design))
                                .color(theme::rgb(p.ink)),
                            text(truncate_str(
                                &format!("{} · {}", reading.hardware, reading.sensor_type),
                                36
                            ))
                            .size(10.5)
                            .font(font)
                            .color(theme::rgb(p.ink_3)),
                        ]
                        .spacing(1)
                        .width(Length::Fill),
                        sparkbar,
                        text(format_sensor_explorer_val(reading.value, &reading.unit))
                            .size(12)
                            .font(mono)
                            .color(theme::rgb(bar_accent))
                            .width(Length::Fixed(84.0)),
                        button(text("+ Pin").size(10.5).font(theme::ui_semibold(t.design)))
                            .padding([4, 9])
                            .style(theme::btn_secondary(t, false))
                            .on_press(Message::AddOverviewTile(pin_kind)),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .padding([7, 10])
                .width(Length::Fill)
                .style(theme::well_style(t)),
            );
        }
    } else {
        // Built-in live telemetry explorer rows when LHM web server is offline
        let builtin_rows: Vec<(&str, String, String, f32, u32, OverviewTileKind)> = vec![
            (
                "Processor Temperature",
                short_hw_name(&s.cpu_name),
                sensors::format_temp(s.cpu_temp, unit),
                ((s.cpu_temp.unwrap_or(45.0) - 20.0) / 75.0).clamp(0.05, 1.0),
                t.data_primary,
                OverviewTileKind::CpuTemp,
            ),
            (
                "Processor Load",
                short_hw_name(&s.cpu_name),
                format!("{:.0}%", s.cpu_load),
                (s.cpu_load / 100.0).clamp(0.03, 1.0),
                t.data_primary,
                OverviewTileKind::CpuLoad,
            ),
            (
                "Graphics Temperature",
                short_hw_name(&s.gpu_name),
                sensors::format_temp(s.gpu_temp, unit),
                ((s.gpu_temp.unwrap_or(40.0) - 20.0) / 75.0).clamp(0.05, 1.0),
                t.data_secondary,
                OverviewTileKind::GpuTemp,
            ),
            (
                "Graphics Load",
                short_hw_name(&s.gpu_name),
                format!("{:.0}%", s.gpu_load),
                (s.gpu_load / 100.0).clamp(0.03, 1.0),
                t.data_secondary,
                OverviewTileKind::GpuLoad,
            ),
            (
                "System Memory Usage",
                s.ram_total_gb
                    .map(|tot| format!("{:.1} / {tot:.0} GB", s.ram_used_gb.unwrap_or(0.0)))
                    .unwrap_or_else(|| "System RAM".to_string()),
                format!("{:.0}%", s.ram_load),
                (s.ram_load / 100.0).clamp(0.05, 1.0),
                t.pink_rose,
                OverviewTileKind::RamUsage,
            ),
            (
                "Total CPU + GPU Power",
                "Package + Board Power".to_string(),
                s.total_power_w
                    .map(|w| format!("{w:.0} W"))
                    .unwrap_or_else(|| "— W".to_string()),
                (s.total_power_w.unwrap_or(65.0) / 280.0).clamp(0.05, 1.0),
                p.brass_hi,
                OverviewTileKind::PowerDraw,
            ),
            (
                "NVMe Drive Temperature",
                "Primary Storage".to_string(),
                sensors::format_temp(s.ssd_temp, unit),
                ((s.ssd_temp.unwrap_or(38.0) - 20.0) / 65.0).clamp(0.05, 1.0),
                p.ok,
                OverviewTileKind::DriveTemp,
            ),
            (
                "Network Download",
                "Active Adapter".to_string(),
                sensors::format_throughput(s.net_down_bps),
                (s.net_down_bps.unwrap_or(0.0) / (5.0 * 1024.0 * 1024.0)).clamp(0.04, 1.0),
                t.data_primary,
                OverviewTileKind::NetDown,
            ),
        ];

        for (title, hw_sub, val_txt, frac, bar_col, kind) in builtin_rows {
            if !q.is_empty() {
                let hay = format!("{title} {hw_sub}").to_ascii_lowercase();
                if !q.split_whitespace().all(|tok| hay.contains(tok)) {
                    continue;
                }
            }
            match_count += 1;
            let sparkbar = iced::widget::canvas(BarMeterCanvas {
                fraction: frac,
                accent: bar_col,
                palette: p,
                accent_override: t.accent_override,
                anim_t: app.anim_t,
            })
            .width(Length::Fixed(86.0))
            .height(Length::Fixed(16.0));

            sensor_rows = sensor_rows.push(
                container(
                    row![
                        column![
                            text(title)
                                .size(12.5)
                                .font(theme::ui_semibold(t.design))
                                .color(theme::rgb(p.ink)),
                            text(hw_sub)
                                .size(10.5)
                                .font(font)
                                .color(theme::rgb(p.ink_3)),
                        ]
                        .spacing(1)
                        .width(Length::Fill),
                        sparkbar,
                        text(val_txt)
                            .size(12)
                            .font(mono)
                            .color(theme::rgb(bar_col))
                            .width(Length::Fixed(80.0)),
                        button(text("+ Pin").size(10.5).font(theme::ui_semibold(t.design)))
                            .padding([4, 9])
                            .style(theme::btn_secondary(t, false))
                            .on_press(Message::AddOverviewTile(kind)),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .padding([7, 10])
                .width(Length::Fill)
                .style(theme::well_style(t)),
            );
        }
    }

    let sensor_explorer_card = container(
        column![
            row![
                icon_canvas(UiIcon::Search, theme::rgb(t.data_primary), 15.0),
                text("LIVE SENSOR EXPLORER")
                    .size(11.5)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(p.ink_2)),
                Space::new().width(Length::Fill),
                container(
                    text(format!("{match_count} live"))
                        .size(10.5)
                        .font(mono)
                        .color(theme::rgb(t.data_primary)),
                )
                .padding([2, 9])
                .style(theme::chip_style(t, Some(t.data_primary))),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
            row![
                text_input(
                    "Filter sensors (CPU, GPU, Vcore, Fan, Power, NVMe, Clock)…",
                    &app.sensor_search_query
                )
                .size(12.5)
                .style(theme::text_input_style(t))
                .on_input(Message::SensorSearchChanged)
                .width(Length::Fill),
            ]
            .align_y(Alignment::Center),
            scrollable(sensor_rows).height(Length::Fill),
        ]
        .spacing(10)
        .height(Length::Fill),
    )
    .padding(t.pad(15))
    .width(Length::Fill)
    .height(Length::Fill)
    .style(theme::rich_instrument_card_style(
        t,
        Some(t.data_primary),
        false,
    ));

    // RGB Conflict Guard Card anchored at bottom of Right column
    let conflict_guard_card: Element<'a, Message> =
        if let Some(banner) = view_conflict_banner(app, t, font) {
            banner
        } else {
            container(
                row![
                    container(icon_canvas(UiIcon::Check, theme::rgb(p.ok), 14.0))
                        .padding(5)
                        .style(theme::chip_style(t, Some(p.ok))),
                    column![
                        text("RGB Conflict Guard · All Clear")
                            .size(12.5)
                            .font(theme::ui_semibold(t.design))
                            .color(theme::rgb(p.ok)),
                        text("Zero competing RGB services (iCUE, CAM, SignalRGB, Armoury Crate).")
                            .size(11)
                            .font(font)
                            .color(theme::rgb(p.ink_3)),
                    ]
                    .spacing(1)
                    .width(Length::Fill),
                    button(text("Scan").size(11.5).font(font))
                        .padding([5, 11])
                        .style(theme::btn_secondary(t, false))
                        .on_press(Message::RescanHardware),
                ]
                .spacing(9)
                .align_y(Alignment::Center),
            )
            .padding(t.pad(12))
            .width(Length::Fill)
            .style(theme::rich_instrument_card_style(t, Some(p.ok), false))
            .into()
        };

    let right_col = column![sensor_explorer_card, conflict_guard_card]
        .spacing(t.sp(12))
        .width(Length::FillPortion(5))
        .height(Length::Fill);

    let main_split = row![left_controllers_card, right_col]
        .spacing(t.sp(16))
        .width(Length::Fill)
        .height(Length::Fill);

    container(
        column![top_summary, main_split]
            .spacing(t.sp(14))
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .padding([18, 26])
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn zone_setup_editor<'a>(
    z: &'a ZoneConfig,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let (cx, cy) = z.layout.center();
    let cur_leds = z.layout.total_leds().max(1);

    let to_fans = {
        let mut c = z.clone();
        c.layout = ZoneLayout::Fans {
            fans: 1,
            leds_per_fan: cur_leds.clamp(1, 48),
            x: cx,
            y: cy,
            angle: 0.0,
            size: 0.21,
            reverse: false,
        };
        c
    };
    let to_strip = {
        let mut c = z.clone();
        c.layout = ZoneLayout::Strip {
            leds: cur_leds.clamp(1, 120),
            x: cx,
            y: cy,
            angle: 0.0,
            length: 0.65,
            reverse: false,
        };
        c
    };
    let to_matrix = {
        let mut c = z.clone();
        c.layout = ZoneLayout::Matrix {
            leds: cur_leds.clamp(1, 144),
            rows: 4,
            cols: (cur_leds / 4).clamp(1, 24),
            x: cx,
            y: cy,
            angle: 0.0,
            length: 0.55,
            reverse: false,
            map: Vec::new(),
        };
        c
    };

    let shape_row = row![
        text("Geometry:").size(12).font(font),
        container(
            row![
                button(text("Fans").size(11).font(font))
                    .padding([4, 10])
                    .style(theme::segmented_pill_style(
                        t,
                        matches!(z.layout, ZoneLayout::Fans { .. })
                    ))
                    .on_press(Message::UpdateZone(to_fans)),
                button(text("Strip").size(11).font(font))
                    .padding([4, 10])
                    .style(theme::segmented_pill_style(
                        t,
                        matches!(z.layout, ZoneLayout::Strip { .. })
                    ))
                    .on_press(Message::UpdateZone(to_strip)),
                button(text("Matrix").size(11).font(font))
                    .padding([4, 10])
                    .style(theme::segmented_pill_style(
                        t,
                        matches!(z.layout, ZoneLayout::Matrix { .. })
                    ))
                    .on_press(Message::UpdateZone(to_matrix)),
            ]
            .spacing(2),
        )
        .padding(2)
        .style(theme::segmented_track_style(t)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let controls: Element<'_, Message> = match &z.layout {
        ZoneLayout::Fans {
            fans,
            leds_per_fan,
            angle,
            size,
            reverse,
            ..
        } => {
            let z1 = z.clone();
            let z2 = z.clone();
            let z3 = z.clone();
            let z4 = z.clone();
            let mut z_rev = z.clone();
            if let ZoneLayout::Fans { reverse: r, .. } = &mut z_rev.layout {
                *r = !*reverse;
            }
            column![
                row![
                    labeled_slider(
                        t,
                        font,
                        "Fan count",
                        format!("{fans}"),
                        1,
                        6,
                        *fans as u16,
                        move |v| {
                            let mut c = z1.clone();
                            if let ZoneLayout::Fans { fans, .. } = &mut c.layout {
                                *fans = v as usize;
                            }
                            Message::UpdateZone(c)
                        }
                    ),
                    labeled_slider(
                        t,
                        font,
                        "LEDs per fan",
                        format!("{leds_per_fan}"),
                        1,
                        48,
                        *leds_per_fan as u16,
                        move |v| {
                            let mut c = z2.clone();
                            if let ZoneLayout::Fans { leds_per_fan, .. } = &mut c.layout {
                                *leds_per_fan = v as usize;
                            }
                            Message::UpdateZone(c)
                        }
                    ),
                ]
                .spacing(14),
                row![
                    labeled_slider(
                        t,
                        font,
                        "Angle",
                        format!("{:.0}°", angle),
                        0,
                        359,
                        (*angle as u16) % 360,
                        move |v| {
                            let mut c = z3.clone();
                            if let ZoneLayout::Fans { angle, .. } = &mut c.layout {
                                *angle = v as f32;
                            }
                            Message::UpdateZone(c)
                        }
                    ),
                    labeled_slider(
                        t,
                        font,
                        "Fan size",
                        format!("{:.0}%", size * 100.0),
                        10,
                        45,
                        (size * 100.0).round() as u16,
                        move |v| {
                            let mut c = z4.clone();
                            if let ZoneLayout::Fans { size, .. } = &mut c.layout {
                                *size = v as f32 / 100.0;
                            }
                            Message::UpdateZone(c)
                        }
                    ),
                ]
                .spacing(14),
                checkbox(*reverse)
                    .label("Reverse LED order")
                    .style(theme::checkbox_style(t))
                    .on_toggle(move |_| Message::UpdateZone(z_rev.clone())),
            ]
            .spacing(10)
            .into()
        }
        ZoneLayout::Strip {
            leds,
            angle,
            length,
            reverse,
            ..
        } => {
            let z1 = z.clone();
            let z2 = z.clone();
            let z3 = z.clone();
            let mut z_rev = z.clone();
            if let ZoneLayout::Strip { reverse: r, .. } = &mut z_rev.layout {
                *r = !*reverse;
            }
            column![
                row![
                    labeled_slider(
                        t,
                        font,
                        "LED count",
                        format!("{leds}"),
                        1,
                        120,
                        *leds as u16,
                        move |v| {
                            let mut c = z1.clone();
                            if let ZoneLayout::Strip { leds, .. } = &mut c.layout {
                                *leds = v as usize;
                            }
                            Message::UpdateZone(c)
                        }
                    ),
                    labeled_slider(
                        t,
                        font,
                        "Angle",
                        format!("{:.0}°", angle),
                        0,
                        359,
                        (*angle as u16) % 360,
                        move |v| {
                            let mut c = z2.clone();
                            if let ZoneLayout::Strip { angle, .. } = &mut c.layout {
                                *angle = v as f32;
                            }
                            Message::UpdateZone(c)
                        }
                    ),
                    labeled_slider(
                        t,
                        font,
                        "Length",
                        format!("{:.0}%", length * 100.0),
                        15,
                        140,
                        (length * 100.0).round() as u16,
                        move |v| {
                            let mut c = z3.clone();
                            if let ZoneLayout::Strip { length, .. } = &mut c.layout {
                                *length = v as f32 / 100.0;
                            }
                            Message::UpdateZone(c)
                        }
                    ),
                ]
                .spacing(14),
                checkbox(*reverse)
                    .label("Reverse LED order")
                    .style(theme::checkbox_style(t))
                    .on_toggle(move |_| Message::UpdateZone(z_rev.clone())),
            ]
            .spacing(10)
            .into()
        }
        ZoneLayout::Matrix {
            leds,
            rows,
            cols,
            angle,
            length,
            reverse,
            ..
        } => {
            let z1 = z.clone();
            let z2 = z.clone();
            let z3 = z.clone();
            let z4 = z.clone();
            let z5 = z.clone();
            let mut z_rev = z.clone();
            if let ZoneLayout::Matrix { reverse: r, .. } = &mut z_rev.layout {
                *r = !*reverse;
            }
            column![
                row![
                    labeled_slider(
                        t,
                        font,
                        "LED count",
                        format!("{leds}"),
                        1,
                        256,
                        *leds as u16,
                        move |v| {
                            let mut c = z1.clone();
                            if let ZoneLayout::Matrix { leds, .. } = &mut c.layout {
                                *leds = v as usize;
                            }
                            Message::UpdateZone(c)
                        }
                    ),
                    labeled_slider(
                        t,
                        font,
                        "Rows",
                        format!("{rows}"),
                        1,
                        12,
                        *rows as u16,
                        move |v| {
                            let mut c = z2.clone();
                            if let ZoneLayout::Matrix { rows, .. } = &mut c.layout {
                                *rows = v as usize;
                            }
                            Message::UpdateZone(c)
                        }
                    ),
                    labeled_slider(
                        t,
                        font,
                        "Cols",
                        format!("{cols}"),
                        1,
                        24,
                        *cols as u16,
                        move |v| {
                            let mut c = z3.clone();
                            if let ZoneLayout::Matrix { cols, .. } = &mut c.layout {
                                *cols = v as usize;
                            }
                            Message::UpdateZone(c)
                        }
                    ),
                ]
                .spacing(14),
                row![
                    labeled_slider(
                        t,
                        font,
                        "Angle",
                        format!("{:.0}°", angle),
                        0,
                        359,
                        (*angle as u16) % 360,
                        move |v| {
                            let mut c = z4.clone();
                            if let ZoneLayout::Matrix { angle, .. } = &mut c.layout {
                                *angle = v as f32;
                            }
                            Message::UpdateZone(c)
                        }
                    ),
                    labeled_slider(
                        t,
                        font,
                        "Width",
                        format!("{:.0}%", length * 100.0),
                        15,
                        140,
                        (length * 100.0).round() as u16,
                        move |v| {
                            let mut c = z5.clone();
                            if let ZoneLayout::Matrix { length, .. } = &mut c.layout {
                                *length = v as f32 / 100.0;
                            }
                            Message::UpdateZone(c)
                        }
                    ),
                ]
                .spacing(14),
                checkbox(*reverse)
                    .label("Reverse LED order")
                    .style(theme::checkbox_style(t))
                    .on_toggle(move |_| Message::UpdateZone(z_rev.clone())),
            ]
            .spacing(10)
            .into()
        }
    };

    container(column![shape_row, controls].spacing(10))
        .padding(12)
        .style(theme::panel_style(t))
        .into()
}

// ===========================================================================
// 4. SETTINGS — FULL-WIDTH BALANCED 2-PANE ATELIER
// ===========================================================================

fn settings_section_ui_icon(sec: SettingsSection) -> UiIcon {
    match sec {
        SettingsSection::Appearance => UiIcon::Customize,
        SettingsSection::Updates => UiIcon::Refresh,
        SettingsSection::General => UiIcon::Sliders,
        SettingsSection::LightingCooling => UiIcon::Lighting,
        SettingsSection::SensorsDevices => UiIcon::Devices,
        SettingsSection::BackupDiagnostics => UiIcon::Settings,
    }
}

pub fn view_settings<'a>(
    app: &'a AirglowApp,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let mono = theme::mono_font();
    let update_available = matches!(
        app.snapshot.update_status,
        UpdateStatus::Available { .. } | UpdateStatus::ReadyToRestart { .. }
    );

    // Left Category Sidebar (`230px` crisp modern navigation rail with `UiIcon` vectors)
    let mut nav_col = column![
        container(
            row![
                image(&app.moth_art)
                    .content_fit(ContentFit::Contain)
                    .width(Length::Fixed(32.0))
                    .height(Length::Fixed(30.0)),
                column![
                    text("Settings")
                        .size(17)
                        .font(theme::display_semibold(t.design))
                        .color(theme::rgb(p.ink)),
                    text(format!("Airglow v{}", current_version()))
                        .size(10.5)
                        .font(mono)
                        .color(theme::rgb(p.ink_3)),
                ]
                .spacing(1),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding([4, 6]),
        Space::new().height(4),
    ]
    .spacing(6);

    for sec in SettingsSection::ALL {
        let active = app.settings_section == sec;
        let show_update_dot = sec == SettingsSection::Updates && update_available;
        let icon_col = if active {
            theme::rgb(p.brass_hi)
        } else {
            theme::rgb(p.ink_3)
        };
        let mut label_row = row![
            icon_canvas(settings_section_ui_icon(sec), icon_col, 16.0),
            column![
                text(sec.label())
                    .size(12.5)
                    .font(if active {
                        theme::ui_semibold(t.design)
                    } else {
                        font
                    })
                    .color(theme::rgb(if active { p.ink } else { p.ink_2 })),
                text(sec.subtitle())
                    .size(10)
                    .font(font)
                    .color(theme::rgb(p.ink_3)),
            ]
            .spacing(1)
            .width(Length::Fill),
        ]
        .spacing(9)
        .align_y(Alignment::Center);

        if show_update_dot {
            label_row = label_row.push(status_dot(p.brass_hi, 7.0));
        }

        nav_col = nav_col.push(
            button(label_row)
                .padding([9, 11])
                .width(Length::Fill)
                .style(theme::settings_nav_item_style(t, active))
                .on_press(Message::SelectSettingsSection(sec)),
        );
    }

    let app_cpu = app.snapshot.sensors.app_cpu_pct;
    let app_mem = app.snapshot.sensors.app_mem_mb;
    nav_col = nav_col
        .push(Space::new().height(Length::Fill))
        .push(
            container(
                column![
                    row![
                        status_dot(p.ok, 6.5),
                        text("Engine Footprint")
                            .size(11)
                            .font(theme::ui_semibold(t.design))
                            .color(theme::rgb(p.ink_2)),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                    text(format!("{app_cpu:.1}% CPU · {app_mem:.0} MB RAM"))
                        .size(10.5)
                        .font(mono)
                        .color(theme::rgb(t.data_primary)),
                ]
                .spacing(3),
            )
            .padding([9, 10])
            .width(Length::Fill)
            .style(theme::well_style(t)),
        );

    let sidebar = container(nav_col.height(Length::Fill))
        .padding(t.pad(12))
        .width(Length::Fixed(230.0))
        .height(Length::Fill)
        .style(theme::rich_instrument_card_style(t, Some(p.brass), false));

    let content: Element<'a, Message> = match app.settings_section {
        SettingsSection::Appearance => view_settings_appearance(app, t, font),
        SettingsSection::Updates => view_settings_updates(app, t, font),
        SettingsSection::General => view_settings_general(app, t, font),
        SettingsSection::LightingCooling => view_settings_lighting_cooling(app, t, font),
        SettingsSection::SensorsDevices => view_settings_sensors_devices(app, t, font),
        SettingsSection::BackupDiagnostics => view_settings_backup(app, t, font),
    };

    let right_pane = container(
        scrollable(
            container(content)
                .max_width(1480.0)
                .width(Length::Fill),
        )
        .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill);

    container(
        row![sidebar, right_pane]
            .spacing(t.sp(18))
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .padding([18, 26])
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn view_settings_appearance<'a>(
    app: &'a AirglowApp,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let mono = theme::mono_font();
    let settings = &app.snapshot.config.settings;
    let appr = &app.snapshot.config.appearance;

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
        .width(Length::Fixed(74.0))
        .height(Length::Fixed(68.0));

    let active_accent_name = match appr.accent_override {
        Some(rgb) => ACCENT_SWATCHES
            .iter()
            .find(|(_, c)| *c == rgb)
            .map(|(n, _)| *n)
            .unwrap_or("Custom"),
        None => "Theme default",
    };

    let hero_banner = container(
        stack![
            dusk_bg,
            scrim,
            particles,
            container(
                row![
                    column![
                        row![
                            text("Appearance Studio")
                                .size(24)
                                .font(theme::display_semibold(t.design))
                                .color(theme::rgb(0xfff4e4)),
                            Space::new().width(8),
                            container(
                                text(format!(
                                    "{} · {}",
                                    settings.design.label(),
                                    active_accent_name
                                ))
                                .size(11)
                                .font(theme::ui_semibold(t.design)),
                            )
                            .padding([4, 10])
                            .style(theme::chip_style(t, Some(p.brass_hi))),
                        ]
                        .align_y(Alignment::Center),
                        text("Combine any of 13 curated interface designs with live custom accents, Saved Looks, Rig Styles, and Auto-HiDPI scaling.")
                            .size(12)
                            .font(font)
                            .color(theme::rgb(0xe2d4df)),
                    ]
                    .spacing(4)
                    .width(Length::Fill),
                    moth_img,
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding([12, 20])
            .width(Length::Fill)
            .height(Length::Fill),
        ]
        .width(Length::Fill)
        .height(Length::Fixed(96.0)),
    )
    .width(Length::Fill)
    .clip(true)
    .style(theme::hero_panel_style(t));

    // 2-Column Bento Row: Live Accent Override (left) | Saved Studio Looks (right)
    let mut accent_rows = column![].spacing(7);
    let mut row_a = row![
        button(
            row![
                status_dot(settings.design.palette().brass, 9.5),
                text("Default").size(11.5).font(font),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([6, 10])
        .width(Length::Fill)
        .style(theme::card_button(t, appr.accent_override.is_none()))
        .on_press(Message::SetAccentOverride(None)),
    ]
    .spacing(7);

    for (idx, (name, rgb)) in ACCENT_SWATCHES.iter().enumerate() {
        let selected = appr.accent_override == Some(*rgb);
        let btn = button(
            row![
                color_dot(*rgb, 10.0),
                text(*name)
                    .size(11.5)
                    .font(if selected {
                        theme::ui_semibold(t.design)
                    } else {
                        font
                    }),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([6, 10])
        .width(Length::Fill)
        .style(theme::card_button(t, selected))
        .on_press(Message::SetAccentOverride(Some(*rgb)));

        if idx < 3 {
            row_a = row_a.push(btn);
        }
    }
    accent_rows = accent_rows.push(row_a);

    if ACCENT_SWATCHES.len() > 3 {
        let mut row_b = row![].spacing(7);
        for (name, rgb) in ACCENT_SWATCHES.iter().skip(3) {
            let selected = appr.accent_override == Some(*rgb);
            row_b = row_b.push(
                button(
                    row![
                        color_dot(*rgb, 10.0),
                        text(*name)
                            .size(11.5)
                            .font(if selected {
                                theme::ui_semibold(t.design)
                            } else {
                                font
                            }),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .padding([6, 10])
                .width(Length::Fill)
                .style(theme::card_button(t, selected))
                .on_press(Message::SetAccentOverride(Some(*rgb))),
            );
        }
        accent_rows = accent_rows.push(row_b);
    }

    let custom_hex_preview = airglow_core::config::parse_hex_rgb(&app.accent_hex_draft)
        .or(appr.accent_override)
        .unwrap_or_else(|| theme::hex_to_rgb_arr(settings.design.palette().brass));

    let accent_card = container(
        column![
            row![
                icon_canvas(UiIcon::Sparkle, theme::rgb(t.pink_rose), 14.5),
                text("LIVE ACCENT OVERRIDE")
                    .size(11.5)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(t.pink_rose)),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
            accent_rows,
            row![
                text("Custom #HEX:")
                    .size(12)
                    .font(font)
                    .color(theme::rgb(p.ink_2)),
                color_dot(custom_hex_preview, 12.0),
                text_input("#F48FB1", &app.accent_hex_draft)
                    .font(mono)
                    .style(theme::text_input_style(t))
                    .on_input(Message::AccentHexDraftChanged)
                    .on_submit(Message::ApplyAccentHex)
                    .width(Length::Fill),
                button(
                    text("Apply")
                        .size(11.5)
                        .font(theme::ui_semibold(t.design)),
                )
                .padding([6, 13])
                .style(theme::btn_secondary(t, false))
                .on_press(Message::ApplyAccentHex),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ]
        .spacing(10),
    )
    .padding(t.pad(16))
    .width(Length::Fill)
    .style(theme::rich_instrument_card_style(t, Some(t.pink_rose), false));

    let mut looks_grid = column![].spacing(7);
    for chunk in app.snapshot.config.saved_looks.chunks(2) {
        let mut r = row![].spacing(7);
        for look in chunk {
            let is_active = settings.design == look.design
                && appr.accent_override == look.accent_override
                && appr.rig_style == look.rig_style;
            let dot_rgb = look
                .accent_override
                .unwrap_or_else(|| theme::hex_to_rgb_arr(look.design.palette().brass));
            let is_builtin = matches!(
                look.id.as_str(),
                "firefly-pink" | "firefly-classic" | "midnight-blueprint" | "kiln-ember"
            );

            let mut top_row = row![
                color_dot(dot_rgb, 10.0),
                text(truncate_str(&look.name, 16))
                    .size(12)
                    .font(theme::ui_semibold(t.design))
                    .width(Length::Fill),
            ]
            .spacing(6)
            .align_y(Alignment::Center);

            if is_active {
                top_row = top_row.push(status_dot(p.brass_hi, 6.0));
            }

            let card_btn = button(
                column![
                    top_row,
                    text(format!(
                        "{} · {} rig",
                        look.design.label(),
                        look.rig_style.label()
                    ))
                    .size(10.5)
                    .font(font)
                    .color(theme::rgb(p.ink_3)),
                ]
                .spacing(3),
            )
            .padding([7, 10])
            .width(Length::Fill)
            .style(theme::card_button(t, is_active))
            .on_press(Message::ApplyLook(look.id.clone()));

            if is_builtin {
                r = r.push(card_btn);
            } else {
                r = r.push(
                    row![
                        card_btn,
                        button(text("×").size(11).font(font))
                            .padding([7, 8])
                            .style(theme::btn_secondary(t, false))
                            .on_press(Message::DeleteLook(look.id.clone())),
                    ]
                    .spacing(4)
                    .width(Length::Fill),
                );
            }
        }
        if chunk.len() == 1 {
            r = r.push(Space::new().width(Length::Fill));
        }
        looks_grid = looks_grid.push(r);
    }

    let looks_card = container(
        column![
            row![
                icon_canvas(UiIcon::Customize, theme::rgb(p.brass_hi), 14.5),
                text("SAVED THEME LOOKS")
                    .size(11.5)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(p.brass_hi)),
                Space::new().width(Length::Fill),
                text_input("Name this look…", &app.look_name_draft)
                    .size(12)
                    .style(theme::text_input_style(t))
                    .on_input(Message::LookNameDraftChanged)
                    .on_submit(Message::SaveLook)
                    .width(Length::Fixed(156.0)),
                button(
                    text("+ Save")
                        .size(11.5)
                        .font(theme::ui_semibold(t.design)),
                )
                .padding([6, 12])
                .style(theme::btn_primary(t))
                .on_press(Message::SaveLook),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
            looks_grid,
        ]
        .spacing(10),
    )
    .padding(t.pad(16))
    .width(Length::Fill)
    .style(theme::rich_instrument_card_style(t, Some(p.brass), false));

    let top_bento_row = row![accent_card, looks_card].spacing(t.sp(14));

    // Rig Visuals, GPU Power Mode, Viewport & UI Scale (`Auto (HiDPI) | 100% | 115% | 125% | 140%`)
    let mut scale_seg = row![].spacing(3);
    for &(pct, raw_lbl) in UI_SCALE_PRESETS {
        let lbl = if pct == 0 { "Auto (HiDPI)" } else { raw_lbl };
        let active = appr.ui_scale_pct == pct;
        scale_seg = scale_seg.push(
            button(
                text(lbl).size(11.5).font(if active {
                    theme::ui_semibold(t.design)
                } else {
                    font
                }),
            )
            .padding([5, 11])
            .style(theme::segmented_pill_style(t, active))
            .on_press(Message::SetUiScalePct(pct)),
        );
    }

    let mut fps_seg = row![].spacing(3);
    for (fps, label) in [
        (15u8, "Eco (15 FPS)"),
        (30, "Balanced (30 FPS)"),
        (60, "Smooth (60 FPS)"),
    ] {
        let active = appr.ui_fps_cap == fps;
        fps_seg = fps_seg.push(
            button(text(label).size(11.5).font(font))
                .padding([5, 11])
                .style(theme::segmented_pill_style(t, active))
                .on_press(Message::SetUiFpsCap(fps)),
        );
    }

    let mut rig_seg = row![].spacing(3);
    for style in RigStyle::ALL {
        let active = appr.rig_style == style;
        rig_seg = rig_seg.push(
            button(text(style.label()).size(11.5).font(font))
                .padding([5, 11])
                .style(theme::segmented_pill_style(t, active))
                .on_press(Message::SetRigStyle(style)),
        );
    }

    let perf_card = container(
        column![
            row![
                icon_canvas(UiIcon::Sliders, theme::rgb(t.data_primary), 15.0),
                text("RIG VISUALS, GPU POWER MODE & VIEWPORT")
                    .size(11.5)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(p.ink_2)),
                Space::new().width(Length::Fill),
                container(
                    text("Shortcuts: Ctrl + / Ctrl - / Ctrl 0")
                        .size(10.5)
                        .font(mono)
                        .color(theme::rgb(t.data_primary)),
                )
                .padding([3, 9])
                .style(theme::chip_style(t, Some(t.data_primary))),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
            row![
                column![
                    text("UI Scale (HiDPI & Widescreen)")
                        .size(12.5)
                        .font(theme::ui_semibold(t.design)),
                    text("Auto scales 2560×1440 to 1.25× and 4K to 1.50× cleanly.")
                        .size(11)
                        .font(font)
                        .color(theme::rgb(p.ink_3)),
                ]
                .spacing(1)
                .width(Length::Fill),
                container(scale_seg)
                    .padding(3)
                    .style(theme::segmented_track_style(t)),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
            row![
                text("UI frame rate:").size(12).font(font),
                container(fps_seg)
                    .padding(3)
                    .style(theme::segmented_track_style(t)),
                Space::new().width(14),
                text("Rig style:").size(12).font(font),
                container(rig_seg)
                    .padding(3)
                    .style(theme::segmented_track_style(t)),
                Space::new().width(Length::Fill),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            row![
                row![
                    pill_toggle(t, appr.airflow_lines, Message::ToggleAirflowLines),
                    text("Airflow ribbons").size(12).font(font),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
                row![
                    pill_toggle(t, appr.spinning_fans, Message::ToggleSpinningFans),
                    text("Spinning fans").size(12).font(font),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
                row![
                    pill_toggle(t, appr.ambient_bloom, Message::ToggleAmbientBloom),
                    text("LED bloom").size(12).font(font),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
                row![
                    pill_toggle(t, appr.firefly_particles, Message::ToggleFireflyParticles),
                    text("Atmospheric motes").size(12).font(font),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
                row![
                    pill_toggle(t, appr.compact_density, Message::ToggleCompactDensity),
                    text("Compact density").size(12).font(font),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
                row![
                    pill_toggle(
                        t,
                        app.snapshot.config.fullscreen_fill_width,
                        Message::SetFullscreenFillWidth(!app.snapshot.config.fullscreen_fill_width),
                    ),
                    text("Ultrawide edge-to-edge").size(12).font(font),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            ]
            .spacing(16)
            .align_y(Alignment::Center),
        ]
        .spacing(12),
    )
    .padding(t.pad(16))
    .width(Length::Fill)
    .style(theme::rich_instrument_card_style(
        t,
        Some(t.data_primary),
        false,
    ));

    // 13-Design Visual Gallery
    let mut grid = column![].spacing(12);
    for chunk in DesignId::ALL.chunks(3) {
        let mut r = row![].spacing(12);
        for &d in chunk {
            let selected = settings.design == d;
            let preview = iced::widget::canvas(DesignPreviewCanvas {
                design: d,
                accent_override: t.accent_override,
                selected,
                anim_t: app.anim_t,
            })
            .width(Length::Fill)
            .height(Length::Fixed(94.0));

            let mut title_row = row![
                text(d.label())
                    .size(14)
                    .font(theme::display_semibold(t.design))
                    .color(theme::rgb(if selected { p.brass_hi } else { p.ink })),
                Space::new().width(Length::Fill),
            ]
            .align_y(Alignment::Center);

            if selected {
                title_row = title_row.push(
                    container(
                        text("IN USE")
                            .size(9.5)
                            .font(theme::ui_semibold(t.design)),
                    )
                    .padding([2, 8])
                    .style(theme::chip_style(t, Some(p.brass))),
                );
            }

            r = r.push(
                button(
                    column![
                        container(preview).clip(true).style(move |_| {
                            iced::widget::container::Style {
                                border: iced::Border {
                                    radius: iced::border::Radius::from(
                                        if d.palette().square_corners {
                                            0.0
                                        } else {
                                            10.0
                                        },
                                    ),
                                    ..Default::default()
                                },
                                ..Default::default()
                            }
                        }),
                        title_row,
                        text(d.tagline())
                            .size(11)
                            .font(font)
                            .color(theme::rgb(p.ink_3)),
                    ]
                    .spacing(7),
                )
                .padding(10)
                .width(Length::Fill)
                .style(theme::card_button(t, selected))
                .on_press(Message::SetDesign(d)),
            );
        }
        for _ in chunk.len()..3 {
            r = r.push(Space::new().width(Length::Fill));
        }
        grid = grid.push(r);
    }

    column![
        hero_banner,
        top_bento_row,
        perf_card,
        container(
            column![
                text("INTERFACE DESIGNS (ALL 13)")
                    .size(11.5)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(p.ink_3)),
                grid,
            ]
            .spacing(12),
        )
        .padding(t.pad(16))
        .width(Length::Fill)
        .style(theme::panel_style(t)),
    ]
    .spacing(t.sp(14))
    .into()
}

fn view_settings_updates<'a>(
    app: &'a AirglowApp,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let mono = theme::mono_font();
    let cfg = &app.snapshot.config;
    let status = &app.snapshot.update_status;
    let s = &app.snapshot.sensors;

    let mut ch_seg = row![].spacing(3);
    for ch in UpdateChannel::ALL {
        let active = cfg.update_channel == ch;
        ch_seg = ch_seg.push(
            button(text(ch.label()).size(11.5).font(font))
                .padding([5, 14])
                .style(theme::segmented_pill_style(t, active))
                .on_press(Message::SetUpdateChannel(ch)),
        );
    }

    let is_busy = matches!(
        status,
        UpdateStatus::Checking | UpdateStatus::Downloading { .. }
    );

    let status_panel: Element<'a, Message> = match status {
        UpdateStatus::Idle => container(
            row![
                status_dot(t.data_primary, 8.0),
                column![
                    text("Ready to check GitHub Releases")
                        .size(13.5)
                        .font(theme::ui_semibold(t.design))
                        .color(theme::rgb(p.ink)),
                    text("Direct HTTPS check against Heavenlystorm/airglow — zero telemetry or background bloat.")
                        .size(11.5)
                        .font(font)
                        .color(theme::rgb(p.ink_3)),
                ]
                .spacing(2),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding(14)
        .width(Length::Fill)
        .style(theme::well_style(t))
        .into(),

        UpdateStatus::Checking => container(
            row![
                status_dot(p.brass_hi, 8.0),
                column![
                    text("Checking GitHub Releases…")
                        .size(13.5)
                        .font(theme::ui_semibold(t.design))
                        .color(theme::rgb(p.brass_hi)),
                    text(format!(
                        "Channel: {} · Contacting api.github.com",
                        cfg.update_channel.label()
                    ))
                    .size(11.5)
                    .font(font)
                    .color(theme::rgb(p.ink_3)),
                ]
                .spacing(2),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding(14)
        .width(Length::Fill)
        .style(theme::banner_style(t, p.brass))
        .into(),

        UpdateStatus::UpToDate {
            current,
            checked_at,
        } => container(
            row![
                status_dot(p.ok, 8.5),
                column![
                    text(format!("Up to date — Airglow v{current}"))
                        .size(14)
                        .font(theme::ui_semibold(t.design))
                        .color(theme::rgb(p.ok)),
                    text(format!(
                        "Running the latest {} release · Verified at {checked_at}",
                        cfg.update_channel.label().to_ascii_lowercase()
                    ))
                    .size(11.5)
                    .font(font)
                    .color(theme::rgb(p.ink_2)),
                ]
                .spacing(2),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding(14)
        .width(Length::Fill)
        .style(theme::banner_style(t, p.ok))
        .into(),

        UpdateStatus::Available {
            current,
            latest,
            notes,
            html_url,
            exe_url,
            is_prerelease,
            ..
        } => {
            let mut head_row = row![
                status_dot(p.brass_hi, 9.0),
                text(format!("Airglow {latest} available!"))
                    .size(16)
                    .font(theme::display_semibold(t.design))
                    .color(theme::rgb(p.brass_hi)),
            ]
            .spacing(8)
            .align_y(Alignment::Center);

            if *is_prerelease {
                head_row = head_row.push(
                    container(text("BETA").size(9.5).font(mono).color(theme::rgb(p.warm)))
                        .padding([2, 7])
                        .style(theme::chip_style(t, Some(p.warm))),
                );
            }

            let mut actions = row![].spacing(8).align_y(Alignment::Center);
            if exe_url.is_some() {
                actions = actions.push(
                    button(
                        text("Download & Stage Update")
                            .size(12)
                            .font(theme::ui_semibold(t.design)),
                    )
                    .padding([8, 16])
                    .style(theme::btn_primary(t))
                    .on_press(Message::DownloadAndStageUpdate),
                );
            }
            actions = actions.push(
                button(text("Open Release Notes").size(12).font(font))
                    .padding([8, 14])
                    .style(theme::btn_secondary(t, false))
                    .on_press(Message::OpenUrl(html_url.clone())),
            );

            let notes_text = if notes.trim().is_empty() {
                "No release notes published for this tag.".to_string()
            } else {
                notes.clone()
            };

            container(
                column![
                    row![
                        head_row,
                        Space::new().width(Length::Fill),
                        text(format!("v{current} → {latest}"))
                            .size(11.5)
                            .font(mono)
                            .color(theme::rgb(p.ink_2)),
                    ]
                    .align_y(Alignment::Center),
                    container(
                        scrollable(
                            text(notes_text)
                                .size(11.5)
                                .font(font)
                                .color(theme::rgb(p.ink_2)),
                        )
                        .height(Length::Fixed(110.0)),
                    )
                    .padding(10)
                    .width(Length::Fill)
                    .style(theme::well_style(t)),
                    actions,
                ]
                .spacing(10),
            )
            .padding(14)
            .width(Length::Fill)
            .style(theme::banner_style(t, p.brass))
            .into()
        }

        UpdateStatus::Downloading {
            latest,
            progress_pct,
        } => {
            let bar = iced::widget::canvas(BarMeterCanvas {
                fraction: (*progress_pct as f32 / 100.0).clamp(0.0, 1.0),
                accent: p.brass_hi,
                palette: p,
                accent_override: t.accent_override,
                anim_t: app.anim_t,
            })
            .width(Length::Fill)
            .height(Length::Fixed(18.0));

            container(
                column![
                    row![
                        status_dot(p.brass_hi, 8.0),
                        text(format!("Downloading Airglow {latest}…"))
                            .size(13.5)
                            .font(theme::ui_semibold(t.design))
                            .color(theme::rgb(p.ink)),
                        Space::new().width(Length::Fill),
                        text(format!("{progress_pct}%"))
                            .size(13)
                            .font(mono)
                            .color(theme::rgb(p.brass_hi)),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                    bar,
                    text("Verifying SHA-256 checksum automatically after download completes.")
                        .size(11)
                        .font(font)
                        .color(theme::rgb(p.ink_3)),
                ]
                .spacing(8),
            )
            .padding(14)
            .width(Length::Fill)
            .style(theme::banner_style(t, p.brass))
            .into()
        }

        UpdateStatus::ReadyToRestart { latest, .. } => container(
            row![
                status_dot(p.ok, 9.0),
                column![
                    text(format!("Airglow {latest} is ready to install"))
                        .size(14.5)
                        .font(theme::display_semibold(t.design))
                        .color(theme::rgb(p.ok)),
                    text("Binary staged and SHA-256 verified. Restart Airglow to swap cleanly.")
                        .size(11.5)
                        .font(font)
                        .color(theme::rgb(p.ink_2)),
                ]
                .spacing(2)
                .width(Length::Fill),
                button(
                    text("Restart to Update")
                        .size(12.5)
                        .font(theme::ui_semibold(t.design)),
                )
                .padding([8, 16])
                .style(theme::btn_primary(t))
                .on_press(Message::ApplyStagedUpdateAndRestart),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .padding(14)
        .width(Length::Fill)
        .style(theme::banner_style(t, p.ok))
        .into(),

        UpdateStatus::Error(err) => container(
            row![
                status_dot(p.warm, 8.5),
                column![
                    text("Update check encountered an issue")
                        .size(13.5)
                        .font(theme::ui_semibold(t.design))
                        .color(theme::rgb(p.warm)),
                    text(err)
                        .size(11.5)
                        .font(font)
                        .color(theme::rgb(p.ink_2)),
                ]
                .spacing(2)
                .width(Length::Fill),
                button(text("Retry").size(11.5).font(theme::ui_semibold(t.design)))
                    .padding([6, 13])
                    .style(theme::btn_secondary(t, false))
                    .on_press(Message::CheckForUpdates),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding(14)
        .width(Length::Fill)
        .style(theme::banner_style(t, p.warm))
        .into(),
    };

    let updater_hero_card = container(
        column![
            row![
                image(&app.moth_art)
                    .content_fit(ContentFit::Contain)
                    .width(Length::Fixed(54.0))
                    .height(Length::Fixed(50.0)),
                column![
                    row![
                        text(format!("Airglow v{}", current_version()))
                            .size(22)
                            .font(theme::display_semibold(t.design))
                            .color(theme::rgb(p.ink)),
                        Space::new().width(8),
                        container(
                            text(cfg.update_channel.label().to_uppercase())
                                .size(10)
                                .font(mono)
                                .color(theme::rgb(p.brass_hi)),
                        )
                        .padding([3, 8])
                        .style(theme::chip_style(t, Some(p.brass))),
                    ]
                    .align_y(Alignment::Center),
                    text("Native Rust + Iced 0.14 GPU UI · Pure-Rust HID & OpenRGB engine · Zero telemetry")
                        .size(12)
                        .font(font)
                        .color(theme::rgb(p.ink_3)),
                ]
                .spacing(3)
                .width(Length::Fill),
                button(
                    row![
                        icon_canvas(UiIcon::Refresh, theme::rgb(t.primary_ink), 13.5),
                        text(if is_busy {
                            "Checking…"
                        } else {
                            "Check for updates"
                        })
                        .size(12)
                        .font(theme::ui_semibold(t.design)),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .padding([8, 16])
                .style(theme::btn_primary(t))
                .on_press(Message::CheckForUpdates),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
            status_panel,
            row![
                row![
                    pill_toggle(
                        t,
                        cfg.auto_check_updates,
                        Message::ToggleAutoCheckUpdates(!cfg.auto_check_updates)
                    ),
                    column![
                        text("Check for updates automatically")
                            .size(12.5)
                            .font(theme::ui_semibold(t.design)),
                        text("Runs one quiet background check 10 seconds after launch.")
                            .size(11)
                            .font(font)
                            .color(theme::rgb(p.ink_3)),
                    ]
                    .spacing(1),
                ]
                .spacing(10)
                .align_y(Alignment::Center)
                .width(Length::Fill),
                row![
                    text("Release channel:")
                        .size(12)
                        .font(font)
                        .color(theme::rgb(p.ink_2)),
                    container(ch_seg)
                        .padding(3)
                        .style(theme::segmented_track_style(t)),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        ]
        .spacing(14),
    )
    .padding(t.pad(18))
    .width(Length::Fill)
    .style(theme::hero_panel_style(t));

    // 2-Column Bento Row: Release Highlights (left) | System & Runtime Info (right)
    let highlight_item = |title: &'static str, desc: &'static str, col: u32| {
        container(
            row![
                status_dot(col, 7.0),
                column![
                    text(title)
                        .size(12.5)
                        .font(theme::ui_semibold(t.design))
                        .color(theme::rgb(p.ink)),
                    text(desc)
                        .size(11)
                        .font(font)
                        .color(theme::rgb(p.ink_3)),
                ]
                .spacing(1)
                .width(Length::Fill),
            ]
            .spacing(9)
            .align_y(Alignment::Start),
        )
        .padding([8, 11])
        .width(Length::Fill)
        .style(theme::well_style(t))
    };

    let release_highlights_card = container(
        column![
            row![
                icon_canvas(UiIcon::Sparkle, theme::rgb(t.pink_rose), 15.0),
                text("RELEASE HIGHLIGHTS")
                    .size(11.5)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(t.pink_rose)),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
            highlight_item(
                "Auto-HiDPI Widescreen Scaling & Tactile Chrome",
                "Automatic 1.25× 1440p & 1.50× 4K scaling, Ctrl +/-/0 zoom, and milled hardware controls.",
                p.brass_hi,
            ),
            highlight_item(
                "60 FPS 2D Bento Grid with Live Drag & Resize",
                "Grab any Overview tile header to reorder or drag corners to resize 1×1 through 4×3.",
                t.data_primary,
            ),
            highlight_item(
                "Collision-Free Widescreen Rig & Cooling Cockpit",
                "De-duplicated RGB callout pills, 164px HSV color wheel, and thermal regime fan curves.",
                t.pink_rose,
            ),
            highlight_item(
                "SHA-256 Verified Native GitHub Auto-Updater",
                "Stages verified binaries locally and swaps seamlessly on restart with zero external installers.",
                p.ok,
            ),
            row![
                button(
                    text("Open GitHub Releases ↗")
                        .size(12)
                        .font(theme::ui_semibold(t.design)),
                )
                .padding([7, 14])
                .style(theme::btn_secondary(t, false))
                .on_press(Message::OpenUrl(GITHUB_RELEASES_WEB.to_string())),
                button(
                    text("? Keyboard Shortcuts")
                        .size(12)
                        .font(theme::ui_semibold(t.design)),
                )
                .padding([7, 14])
                .style(theme::btn_secondary(t, app.show_shortcuts))
                .on_press(Message::ToggleShortcuts),
            ]
            .spacing(8),
        ]
        .spacing(9),
    )
    .padding(t.pad(16))
    .width(Length::Fill)
    .style(theme::rich_instrument_card_style(t, Some(t.pink_rose), false));

    let kv_row = |k: &'static str, v: String, v_col: u32| {
        container(
            row![
                text(k).size(11.5).font(font).color(theme::rgb(p.ink_3)),
                Space::new().width(Length::Fill),
                text(v).size(11.5).font(mono).color(theme::rgb(v_col)),
            ]
            .align_y(Alignment::Center),
        )
        .padding([7, 11])
        .width(Length::Fill)
        .style(theme::well_style(t))
    };

    let scale_lbl = if cfg.appearance.ui_scale_pct == 0 {
        "Auto (HiDPI)".to_string()
    } else {
        format!("{}%", cfg.appearance.ui_scale_pct)
    };
    let active_zones = app.snapshot.frames.iter().filter(|f| f.enabled).count();
    let total_leds: usize = app
        .snapshot
        .frames
        .iter()
        .filter(|f| f.enabled)
        .map(|f| f.colors.len())
        .sum();

    let ram_seg_bar = iced::widget::canvas(SegmentedBarCanvas {
        fraction: (s.ram_load / 100.0).clamp(0.04, 1.0),
        accent: t.data_secondary,
        palette: p,
        accent_override: t.accent_override,
        segments: 24,
    })
    .width(Length::Fill)
    .height(Length::Fixed(10.0));

    let system_info_card = container(
        column![
            row![
                icon_canvas(UiIcon::Devices, theme::rgb(t.data_primary), 15.0),
                text("SYSTEM & RUNTIME TELEMETRY")
                    .size(11.5)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(t.data_primary)),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
            kv_row("Processor", truncate_str(&short_hw_name(&s.cpu_name), 32), p.ink),
            kv_row("Graphics", truncate_str(&short_hw_name(&s.gpu_name), 32), p.ink),
            kv_row(
                "System Memory",
                s.ram_total_gb
                    .map(|tot| format!("{:.1} / {tot:.0} GB ({:.0}%)", s.ram_used_gb.unwrap_or(0.0), s.ram_load))
                    .unwrap_or_else(|| format!("{:.0}% used", s.ram_load)),
                t.data_secondary,
            ),
            ram_seg_bar,
            kv_row(
                "RGB Engine Stream",
                format!("{active_zones} zones · {total_leds} LEDs · {:.0} FPS", app.snapshot.fps.max(1.0)),
                p.brass_hi,
            ),
            kv_row(
                "Logical Viewport & Scale",
                format!("{:.0}×{:.0} · {scale_lbl}", app.window_size.width, app.window_size.height),
                t.data_primary,
            ),
            kv_row(
                "Process Footprint",
                format!("{:.1}% CPU · {:.0} MB working set", s.app_cpu_pct, s.app_mem_mb),
                p.ok,
            ),
            row![
                button(text("Open Config Folder").size(12).font(font))
                    .padding([7, 14])
                    .style(theme::btn_secondary(t, false))
                    .on_press(Message::OpenConfigFolder),
                button(text("Export Diagnostics").size(12).font(font))
                    .padding([7, 14])
                    .style(theme::btn_secondary(t, false))
                    .on_press(Message::ExportDiagnostics),
            ]
            .spacing(8),
        ]
        .spacing(8),
    )
    .padding(t.pad(16))
    .width(Length::Fill)
    .style(theme::rich_instrument_card_style(
        t,
        Some(t.data_primary),
        false,
    ));

    let security_card = container(
        column![
            row![
                icon_canvas(UiIcon::Check, theme::rgb(p.ok), 15.0),
                text("SHA-256 UPDATE SECURITY & INTEGRITY")
                    .size(11.5)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(p.ok)),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
            kv_row("Release Verification", "SHA256SUMS.txt enforced".to_string(), p.ok),
            kv_row(
                "Staging Path",
                "%LOCALAPPDATA%\\Airglow\\Airglow.new.exe".to_string(),
                p.ink_2,
            ),
            kv_row(
                "Transport",
                "Rustls TLS 1.3 · api.github.com".to_string(),
                t.data_primary,
            ),
        ]
        .spacing(8),
    )
    .padding(t.pad(16))
    .width(Length::Fill)
    .style(theme::rich_instrument_card_style(t, Some(p.ok), false));

    let drivers_links_card = container(
        column![
            row![
                icon_canvas(UiIcon::Lighting, theme::rgb(t.data_secondary), 15.0),
                text("HARDWARE DRIVERS & COMMUNITY LINKS")
                    .size(11.5)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(t.data_secondary)),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
            kv_row(
                "Direct USB HID",
                format!("{} native controller(s)", app.snapshot.hid_devices.len()),
                p.brass_hi,
            ),
            kv_row(
                "OpenRGB SDK Bridge",
                if app.snapshot.openrgb.connected {
                    format!("Connected ({} devices)", app.snapshot.openrgb.controller_count)
                } else {
                    "Standby (127.0.0.1:6742)".to_string()
                },
                t.data_secondary,
            ),
            kv_row(
                "Sensor Bridge",
                if s.lhm_connected {
                    format!("LHM Active ({} sensors)", s.readings.len())
                } else {
                    "Windows Sysinfo Active".to_string()
                },
                t.data_primary,
            ),
        ]
        .spacing(8),
    )
    .padding(t.pad(16))
    .width(Length::Fill)
    .style(theme::rich_instrument_card_style(
        t,
        Some(t.data_secondary),
        false,
    ));

    column![
        updater_hero_card,
        row![release_highlights_card, system_info_card].spacing(t.sp(14)),
        row![security_card, drivers_links_card].spacing(t.sp(14)),
    ]
    .spacing(t.sp(14))
    .into()
}

fn view_settings_general<'a>(
    app: &'a AirglowApp,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let settings = &app.snapshot.config.settings;
    let mono = theme::mono_font();

    let temp_seg = container(
        row![
            button(text("Celsius (°C)").size(12).font(font))
                .padding([5, 13])
                .style(theme::segmented_pill_style(t, settings.temp_unit == TempUnit::C))
                .on_press(Message::SetTempUnit(TempUnit::C)),
            button(text("Fahrenheit (°F)").size(12).font(font))
                .padding([5, 13])
                .style(theme::segmented_pill_style(t, settings.temp_unit == TempUnit::F))
                .on_press(Message::SetTempUnit(TempUnit::F)),
        ]
        .spacing(3),
    )
    .padding(3)
    .style(theme::segmented_track_style(t));

    let mut pulse_seg = row![].spacing(3);
    for sp in PulseSpan::ALL {
        pulse_seg = pulse_seg.push(
            button(text(sp.label()).size(11.5).font(font))
                .padding([5, 12])
                .style(theme::segmented_pill_style(t, app.pulse_span == sp))
                .on_press(Message::SetPulseSpan(sp)),
        );
    }

    let total_w = app.snapshot.sensors.total_power_w.unwrap_or(0.0);
    let est_hr = (total_w / 1000.0) * settings.energy_price_eur_kwh;
    let est_mo = est_hr * 6.0 * 30.0;

    let units_card = container(
        column![
            row![
                icon_canvas(UiIcon::Sliders, theme::rgb(t.data_primary), 15.0),
                text("UNITS, ENERGY TARIFF & HISTORY")
                    .size(11.5)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(t.data_primary)),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
            row![
                text("Temperature unit:").size(13).font(font),
                Space::new().width(Length::Fill),
                temp_seg,
            ]
            .align_y(Alignment::Center),
            container(
                column![
                    row![
                        column![
                            text("Electricity tariff (€/kWh)")
                                .size(12.5)
                                .font(theme::ui_semibold(t.design)),
                            text(format!(
                                "Now {total_w:.0} W ≈ €{est_hr:.3}/hr (€{est_mo:.1}/mo @ 6h/day)"
                            ))
                            .size(11)
                            .font(mono)
                            .color(theme::rgb(p.ink_3)),
                        ]
                        .spacing(2)
                        .width(Length::Fill),
                        text("€").size(13).font(mono).color(theme::rgb(p.ink_2)),
                        text_input("0.3797", &app.tariff_draft)
                            .font(mono)
                            .style(theme::text_input_style(t))
                            .on_input(Message::SetTariffDraft)
                            .width(Length::Fixed(96.0)),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                ],
            )
            .padding(11)
            .width(Length::Fill)
            .style(theme::well_style(t)),
            row![
                column![
                    text("Telemetry sparkline window")
                        .size(12.5)
                        .font(theme::ui_semibold(t.design)),
                    text("Default span for CPU/GPU/Power history charts.")
                        .size(11)
                        .font(font)
                        .color(theme::rgb(p.ink_3)),
                ]
                .spacing(2)
                .width(Length::Fill),
                container(pulse_seg)
                    .padding(3)
                    .style(theme::segmented_track_style(t)),
            ]
            .align_y(Alignment::Center),
        ]
        .spacing(13),
    )
    .padding(t.pad(18))
    .width(Length::Fill)
    .style(theme::rich_instrument_card_style(
        t,
        Some(t.data_primary),
        false,
    ));

    let toggle_card_row = |on: bool, title: &'static str, desc: &'static str, msg: Message| {
        container(
            row![
                pill_toggle(t, on, msg),
                column![
                    text(title)
                        .size(12.5)
                        .font(theme::ui_semibold(t.design))
                        .color(theme::rgb(p.ink)),
                    text(desc)
                        .size(11)
                        .font(font)
                        .color(theme::rgb(p.ink_3)),
                ]
                .spacing(1)
                .width(Length::Fill),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding([9, 11])
        .width(Length::Fill)
        .style(theme::well_style(t))
    };

    let windows_card = container(
        column![
            row![
                icon_canvas(UiIcon::Settings, theme::rgb(p.brass_hi), 15.0),
                text("WINDOWS STARTUP & TRAY LIFECYCLE")
                    .size(11.5)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(p.brass_hi)),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
            toggle_card_row(
                settings.start_with_windows,
                "Launch Airglow automatically with Windows",
                "Starts silently in the system tray and restores your saved RGB & cooling scene.",
                Message::ToggleStartWithWindows(!settings.start_with_windows),
            ),
            toggle_card_row(
                app.minimize_to_tray_on_close,
                "Keep engine running in system tray on window close",
                "Closing the main window drops UI GPU usage to 0% while hardware control continues.",
                Message::ToggleMinimizeToTray(!app.minimize_to_tray_on_close),
            ),
            toggle_card_row(
                settings.demo_mode,
                "Simulated telemetry fallback when LHM is offline",
                "Keeps temperature effects and curves responsive using native OS metrics.",
                Message::ToggleDemoMode(!settings.demo_mode),
            ),
        ]
        .spacing(10),
    )
    .padding(t.pad(18))
    .width(Length::Fill)
    .style(theme::rich_instrument_card_style(t, Some(p.brass), false));

    let workspaces_summary = container(
        row![
            icon_canvas(UiIcon::Overview, theme::rgb(t.data_secondary), 16.0),
            column![
                text(format!(
                    "Active Bento Workspace: {} ({} tiles)",
                    app.snapshot.config.active_workspace().name,
                    app.snapshot.config.active_workspace().tiles.len()
                ))
                .size(13)
                .font(theme::ui_semibold(t.design)),
                text("Tip: Drag any Overview card header at 60 FPS to reorder, or drag its bottom-right corner to resize.")
                    .size(11.5)
                    .font(font)
                    .color(theme::rgb(p.ink_3)),
            ]
            .spacing(2)
            .width(Length::Fill),
            button(text("Open Overview →").size(12).font(theme::ui_semibold(t.design)))
                .padding([7, 14])
                .style(theme::btn_secondary(t, false))
                .on_press(Message::SelectTab(crate::Tab::Overview)),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    )
    .padding(t.pad(15))
    .width(Length::Fill)
    .style(theme::panel_style(t));

    column![
        text("General & Telemetry")
            .size(24)
            .font(theme::display_semibold(t.design)),
        row![units_card, windows_card].spacing(t.sp(14)),
        workspaces_summary,
    ]
    .spacing(t.sp(14))
    .into()
}

fn view_settings_lighting_cooling<'a>(
    app: &'a AirglowApp,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let settings = &app.snapshot.config.settings;
    let mono = theme::mono_font();
    let hyst = settings.fan_hysteresis_c;

    let mut sleep_btns = row![].spacing(6).align_y(Alignment::Center);
    for mins in [15u16, 30, 60] {
        sleep_btns = sleep_btns.push(
            button(text(format!("{mins} min")).size(11.5).font(font))
                .padding([5, 11])
                .style(theme::btn_secondary(t, false))
                .on_press(Message::StartSleepTimer(mins)),
        );
    }
    if app.snapshot.sleep_timer_remaining_s.is_some() {
        sleep_btns = sleep_btns.push(
            button(text("Cancel").size(11.5).font(font))
                .padding([5, 11])
                .style(theme::btn_danger(t))
                .on_press(Message::CancelSleepTimer),
        );
    }

    let lighting_card = container(
        column![
            row![
                icon_canvas(UiIcon::Lighting, theme::rgb(t.pink_rose), 15.0),
                text("LIGHTING POWER, SLEEP & CROSSFADE")
                    .size(11.5)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(t.pink_rose)),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
            container(
                row![
                    pill_toggle(
                        t,
                        settings.lights_off_on_sleep,
                        Message::ToggleLightsOffSleep(!settings.lights_off_on_sleep),
                    ),
                    column![
                        text("Blackout automatically on PC sleep")
                            .size(12.5)
                            .font(theme::ui_semibold(t.design)),
                        text("Fades zones off before suspend and restores smoothly on wake.")
                            .size(11)
                            .font(font)
                            .color(theme::rgb(p.ink_3)),
                    ]
                    .spacing(1)
                    .width(Length::Fill),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding([9, 11])
            .style(theme::well_style(t)),
            container(
                row![
                    pill_toggle(
                        t,
                        settings.lights_off_on_lock,
                        Message::ToggleLightsOffLock(!settings.lights_off_on_lock),
                    ),
                    column![
                        text("Turn lights off while Windows is locked (Win+L)")
                            .size(12.5)
                            .font(theme::ui_semibold(t.design)),
                        text("Saves OLED & LED wear whenever your workstation is locked.")
                            .size(11)
                            .font(font)
                            .color(theme::rgb(p.ink_3)),
                    ]
                    .spacing(1)
                    .width(Length::Fill),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding([9, 11])
            .style(theme::well_style(t)),
            container(
                row![
                    column![
                        text("Quick Sleep Fade Timer")
                            .size(12.5)
                            .font(theme::ui_semibold(t.design)),
                        text(match app.snapshot.sleep_timer_remaining_s {
                            Some(rem) => format!("Counting down: {:02}m {:02}s remaining", rem / 60, rem % 60),
                            None => "Dims gently over the final 2 minutes.".to_string(),
                        })
                        .size(11)
                        .font(font)
                        .color(theme::rgb(p.ink_3)),
                    ]
                    .spacing(1)
                    .width(Length::Fill),
                    sleep_btns,
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .padding([9, 11])
            .style(theme::well_style(t)),
        ]
        .spacing(10),
    )
    .padding(t.pad(18))
    .width(Length::Fill)
    .style(theme::rich_instrument_card_style(t, Some(t.pink_rose), false));

    let mut curve_seg = row![].spacing(3);
    for name in ["silent", "balanced", "performance"] {
        let active = app.snapshot.effective_curve == name;
        curve_seg = curve_seg.push(
            button(text(cap_first(name)).size(11.5).font(font))
                .padding([5, 12])
                .style(theme::segmented_pill_style(t, active))
                .on_press(Message::SelectFanCurve(name.to_string())),
        );
    }

    let mut src_seg = row![].spacing(3);
    for src in TempSource::ALL {
        let short = match src {
            TempSource::Max => "Hotter",
            TempSource::Cpu => "CPU",
            TempSource::Gpu => "GPU",
            TempSource::Liquid => "Coolant",
        };
        src_seg = src_seg.push(
            button(text(short).size(11.5).font(font))
                .padding([5, 10])
                .style(theme::segmented_pill_style(t, settings.temp_source == src))
                .on_press(Message::SelectTempSource(src)),
        );
    }

    let lhm_any_controlled = app
        .snapshot
        .lhm_channels
        .iter()
        .any(|c| !matches!(c.mode, FanChannelMode::Auto));

    let cooling_card = container(
        column![
            row![
                icon_canvas(UiIcon::Fans, theme::rgb(t.data_primary), 15.0),
                text("COOLING CURVES, SENSOR SOURCE & HYSTERESIS")
                    .size(11.5)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(t.data_primary)),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
            row![
                text("Default fan curve:").size(12.5).font(font),
                Space::new().width(Length::Fill),
                container(curve_seg)
                    .padding(3)
                    .style(theme::segmented_track_style(t)),
            ]
            .align_y(Alignment::Center),
            row![
                text("Thermal input:").size(12.5).font(font),
                Space::new().width(Length::Fill),
                container(src_seg)
                    .padding(3)
                    .style(theme::segmented_track_style(t)),
            ]
            .align_y(Alignment::Center),
            container(
                column![
                    row![
                        text("Acoustic cooldown hysteresis")
                            .size(12.5)
                            .font(theme::ui_semibold(t.design)),
                        Space::new().width(Length::Fill),
                        container(
                            text(format!("{hyst:.1} °C"))
                                .size(11)
                                .font(mono)
                                .color(theme::rgb(p.brass_hi)),
                        )
                        .padding([2, 9])
                        .style(theme::chip_style(t, Some(p.brass))),
                    ]
                    .align_y(Alignment::Center),
                    slider(0.0..=8.0, hyst, |v| {
                        let snapped = (v * 2.0).round() / 2.0;
                        Message::SetFanHysteresis(snapped)
                    })
                    .step(0.5)
                    .style(theme::slider_style(t)),
                ]
                .spacing(6),
            )
            .padding([9, 11])
            .style(theme::well_style(t)),
            container(
                row![
                    pill_toggle(
                        t,
                        lhm_any_controlled,
                        Message::ToggleLhmFanControl(!lhm_any_controlled),
                    ),
                    column![
                        text("Motherboard & AIO header control via LHM")
                            .size(12.5)
                            .font(theme::ui_semibold(t.design)),
                        text(if app.snapshot.lhm_channels.is_empty() {
                            "Enable LHM Remote Web Server to manage board fan headers.".to_string()
                        } else {
                            format!("{} detected header(s) ready.", app.snapshot.lhm_channels.len())
                        })
                        .size(11)
                        .font(font)
                        .color(theme::rgb(p.ink_3)),
                    ]
                    .spacing(1)
                    .width(Length::Fill),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding([9, 11])
            .style(theme::well_style(t)),
        ]
        .spacing(10),
    )
    .padding(t.pad(18))
    .width(Length::Fill)
    .style(theme::rich_instrument_card_style(
        t,
        Some(t.data_primary),
        false,
    ));

    column![
        text("Lighting & Cooling")
            .size(24)
            .font(theme::display_semibold(t.design)),
        row![lighting_card, cooling_card].spacing(t.sp(14)),
    ]
    .spacing(t.sp(14))
    .into()
}

fn view_settings_sensors_devices<'a>(
    app: &'a AirglowApp,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let mono = theme::mono_font();
    let settings = &app.snapshot.config.settings;
    let s = &app.snapshot.sensors;

    let lhm_card = container(
        column![
            row![
                icon_canvas(UiIcon::Sliders, theme::rgb(t.data_primary), 15.0),
                text("LIBREHARDWAREMONITOR BRIDGE")
                    .size(11.5)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(t.data_primary)),
                Space::new().width(Length::Fill),
                status_dot(if s.lhm_connected { p.ok } else { p.warm }, 7.5),
                text(if s.lhm_connected {
                    format!("Online · {} sensors", s.readings.len())
                } else {
                    "Sysinfo fallback".to_string()
                })
                .size(11)
                .font(mono)
                .color(theme::rgb(if s.lhm_connected { p.ok } else { p.warm })),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
            text("Connects to LibreHardwareMonitor's JSON web server for 200+ sensors and PWM fan headers.")
                .size(11.5)
                .font(font)
                .color(theme::rgb(p.ink_3)),
            row![
                text("Endpoint:").size(12.5).font(font),
                text_input("http://127.0.0.1:8085", &app.lhm_url_draft)
                    .font(mono)
                    .style(theme::text_input_style(t))
                    .on_input(Message::SetLhmUrl)
                    .width(Length::Fill),
                button(
                    row![
                        icon_canvas(UiIcon::Refresh, theme::rgb(t.primary_ink), 12.5),
                        text("Reconnect")
                            .size(12)
                            .font(theme::ui_semibold(t.design)),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding([7, 14])
                .style(theme::btn_primary(t))
                .on_press(Message::RescanHardware),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            container(
                row![
                    status_dot(
                        if app.snapshot.lhm_auth_configured {
                            p.ok
                        } else {
                            p.ink_3
                        },
                        6.5,
                    ),
                    text(if app.snapshot.lhm_auth_configured {
                        "HTTP Basic Auth credentials loaded from lhm-auth.json"
                    } else {
                        "Optional HTTP Basic Auth: place lhm-auth.json in config folder"
                    })
                    .size(11)
                    .font(font)
                    .color(theme::rgb(p.ink_3)),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .padding([8, 11])
            .width(Length::Fill)
            .style(theme::well_style(t)),
        ]
        .spacing(12),
    )
    .padding(t.pad(18))
    .width(Length::Fill)
    .style(theme::rich_instrument_card_style(
        t,
        Some(t.data_primary),
        false,
    ));

    let orgb_card = container(
        column![
            row![
                icon_canvas(UiIcon::Lighting, theme::rgb(t.data_secondary), 15.0),
                text("OPENRGB SDK SERVER")
                    .size(11.5)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(t.data_secondary)),
                Space::new().width(Length::Fill),
                status_dot(
                    if app.snapshot.openrgb.connected {
                        p.ok
                    } else {
                        p.ink_3
                    },
                    7.5,
                ),
                text(if app.snapshot.openrgb.connected {
                    format!("{} ctrl(s)", app.snapshot.openrgb.controller_count)
                } else {
                    "Standby".to_string()
                })
                .size(11)
                .font(mono)
                .color(theme::rgb(if app.snapshot.openrgb.connected {
                    p.ok
                } else {
                    p.ink_3
                })),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
            text("Bridges motherboards, GPUs, RAM sticks, and keyboards via OpenRGB SDK TCP protocol.")
                .size(11.5)
                .font(font)
                .color(theme::rgb(p.ink_3)),
            row![
                pill_toggle(
                    t,
                    settings.openrgb_enabled,
                    Message::ToggleOpenRgb(!settings.openrgb_enabled),
                ),
                text("Enable SDK bridge")
                    .size(12.5)
                    .font(theme::ui_semibold(t.design)),
                Space::new().width(Length::Fill),
                button(text("Rescan").size(12).font(font))
                    .padding([6, 13])
                    .style(theme::btn_secondary(t, false))
                    .on_press(Message::RescanHardware),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            row![
                text("Host:").size(12).font(font),
                text_input("127.0.0.1", &app.openrgb_host_draft)
                    .font(mono)
                    .style(theme::text_input_style(t))
                    .on_input(Message::SetOpenRgbHost)
                    .width(Length::Fill),
                text("Port:").size(12).font(font),
                text_input("6742", &app.openrgb_port_draft)
                    .font(mono)
                    .style(theme::text_input_style(t))
                    .on_input(Message::SetOpenRgbPort)
                    .width(Length::Fixed(82.0)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ]
        .spacing(12),
    )
    .padding(t.pad(18))
    .width(Length::Fill)
    .style(theme::rich_instrument_card_style(
        t,
        Some(t.data_secondary),
        false,
    ));

    let conflict_card: Element<'a, Message> =
        if let Some(banner) = view_conflict_banner(app, t, font) {
            banner
        } else {
            container(
                row![
                    container(icon_canvas(UiIcon::Check, theme::rgb(p.ok), 15.0))
                        .padding(6)
                        .style(theme::chip_style(t, Some(p.ok))),
                    column![
                        text("RGB Conflict Guard — All Clear")
                            .size(13.5)
                            .font(theme::ui_semibold(t.design))
                            .color(theme::rgb(p.ok)),
                        text("No competing vendor RGB daemons (iCUE, NZXT CAM, SignalRGB, Armoury Crate, MSI Center) detected.")
                            .size(11.5)
                            .font(font)
                            .color(theme::rgb(p.ink_3)),
                    ]
                    .spacing(2)
                    .width(Length::Fill),
                    button(text("Scan Processes").size(12).font(font))
                        .padding([7, 14])
                        .style(theme::btn_secondary(t, false))
                        .on_press(Message::RescanHardware),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding(t.pad(16))
            .width(Length::Fill)
            .style(theme::panel_style(t))
            .into()
        };

    column![
        text("Sensors & Devices")
            .size(24)
            .font(theme::display_semibold(t.design)),
        row![lhm_card, orgb_card].spacing(t.sp(14)),
        conflict_card,
    ]
    .spacing(t.sp(14))
    .into()
}

fn view_settings_backup<'a>(
    app: &'a AirglowApp,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let mono = theme::mono_font();
    let cfg = &app.snapshot.config;

    let export_import_card = container(
        column![
            row![
                icon_canvas(UiIcon::Check, theme::rgb(p.brass_hi), 15.0),
                text("PORTABLE RIG BACKUP & RESTORE (.JSON)")
                    .size(11.5)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(p.brass_hi)),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
            text(format!(
                "Snapshots {} workspace(s), {} profile(s), {} zone(s), {} automation(s), and all fan curves into one JSON file.",
                cfg.overview_workspaces.len(),
                cfg.presets.len(),
                cfg.zones.len(),
                cfg.automations.len()
            ))
            .size(11.5)
            .font(font)
            .color(theme::rgb(p.ink_3)),
            button(
                container(
                    row![
                        icon_canvas(UiIcon::Plus, theme::rgb(t.primary_ink), 13.5),
                        text("Export Full Rig Backup Bundle")
                            .size(12.5)
                            .font(theme::ui_semibold(t.design)),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .center_x(Length::Fill),
            )
            .padding([9, 16])
            .width(Length::Fill)
            .style(theme::btn_primary(t))
            .on_press(Message::ExportRigBundle),
            container(
                column![
                    text("Restore from backup file path")
                        .size(12)
                        .font(theme::ui_semibold(t.design)),
                    row![
                        text_input(
                            "C:\\Users\\...\\Desktop\\airglow-rig-backup-....json",
                            &app.import_backup_path_draft
                        )
                        .font(mono)
                        .style(theme::text_input_style(t))
                        .on_input(Message::ImportBackupPathChanged)
                        .on_submit(Message::ImportRigBundle)
                        .width(Length::Fill),
                        button(
                            text("Import")
                                .size(12)
                                .font(theme::ui_semibold(t.design)),
                        )
                        .padding([7, 14])
                        .style(theme::btn_secondary(t, false))
                        .on_press(Message::ImportRigBundle),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                ]
                .spacing(7),
            )
            .padding(11)
            .width(Length::Fill)
            .style(theme::well_style(t)),
        ]
        .spacing(12),
    )
    .padding(t.pad(18))
    .width(Length::Fill)
    .style(theme::rich_instrument_card_style(t, Some(p.brass), false));

    let diagnostics_card = container(
        column![
            row![
                icon_canvas(UiIcon::Settings, theme::rgb(t.data_primary), 15.0),
                text("DIAGNOSTICS, MIGRATION & WORKSPACE RESET")
                    .size(11.5)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(t.data_primary)),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
            container(
                row![
                    column![
                        text("Local Configuration Directory")
                            .size(12.5)
                            .font(theme::ui_semibold(t.design)),
                        text("Inspect config.json, cooling CSV logs, and optional lhm-auth.json.")
                            .size(11)
                            .font(font)
                            .color(theme::rgb(p.ink_3)),
                    ]
                    .spacing(1)
                    .width(Length::Fill),
                    button(text("Open Folder").size(12).font(theme::ui_semibold(t.design)))
                        .padding([7, 14])
                        .style(theme::btn_secondary(t, false))
                        .on_press(Message::OpenConfigFolder),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .padding(10)
            .width(Length::Fill)
            .style(theme::well_style(t)),
            container(
                row![
                    column![
                        text("Scrubbed Hardware Diagnostics")
                            .size(12.5)
                            .font(theme::ui_semibold(t.design)),
                        text("Exports a sanitized JSON report with zero usernames or tokens.")
                            .size(11)
                            .font(font)
                            .color(theme::rgb(p.ink_3)),
                    ]
                    .spacing(1)
                    .width(Length::Fill),
                    button(text("Export Report").size(12).font(font))
                        .padding([7, 14])
                        .style(theme::btn_secondary(t, false))
                        .on_press(Message::ExportDiagnostics),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .padding(10)
            .width(Length::Fill)
            .style(theme::well_style(t)),
            row![
                button(text("Import Legacy Python Config").size(12).font(font))
                    .padding([7, 14])
                    .width(Length::Fill)
                    .style(theme::btn_secondary(t, false))
                    .on_press(Message::ImportPythonSettings),
                button(
                    row![
                        icon_canvas(UiIcon::Reset, theme::rgb(p.hot), 13.0),
                        text("Reset Active Workspace")
                            .size(12)
                            .font(theme::ui_semibold(t.design)),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .padding([7, 14])
                .style(theme::btn_danger(t))
                .on_press(Message::ResetActiveWorkspace),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ]
        .spacing(11),
    )
    .padding(t.pad(18))
    .width(Length::Fill)
    .style(theme::rich_instrument_card_style(
        t,
        Some(t.data_primary),
        false,
    ));

    column![
        text("Backup & Diagnostics")
            .size(24)
            .font(theme::display_semibold(t.design)),
        row![export_import_card, diagnostics_card].spacing(t.sp(14)),
    ]
    .spacing(t.sp(14))
    .into()
}
