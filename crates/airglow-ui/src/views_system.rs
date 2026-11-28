//! Automations (Firefly companion moth + 4 one-click Smart Recipes + full rule builder),
//! Devices (HID, OpenRGB & per-zone setup editor), and Settings (Appearance Studio with
//! Firefly Dusk/Moth hero, Live Accent Override swatches, Saved Looks shelf, Performance &
//! GPU Power Mode, 13-design visual gallery, and hardware/diagnostics/migration controls).

use airglow_core::automations::ALL_DAYS_MASK;
use airglow_core::config::{
    ACCENT_SWATCHES, ColorOrder, DesignId, RigStyle, TempSource, TempUnit, ZoneConfig,
};
use airglow_core::geometry::ZoneLayout;
use iced::widget::{Space, button, checkbox, column, container, image, row, stack, text, text_input};
use iced::{Alignment, ContentFit, Element, Length};

use crate::canvas_ambient::FireflyParticlesCanvas;
use crate::canvas_widgets::DesignPreviewCanvas;
use crate::theme::{self, DesignTokens};
use crate::views::{
    cap_first, color_dot, labeled_slider, pill_toggle, status_dot, view_conflict_banner,
};
use crate::{AirglowApp, AutoActionDraft, AutoTriggerDraft, Message, StarterIdea};

pub fn view_automations<'a>(
    app: &'a AirglowApp,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let status = &app.snapshot.automation_status;
    let paused = app.snapshot.config.automations_paused;
    let unit = app.snapshot.config.settings.temp_unit;

    let top_bar = row![
        column![
            text("Automations")
                .size(26)
                .font(theme::display_semibold(t.design)),
            text("Rules that change your lighting and fans on their own. The highest active rule wins, and when it stops applying, your own settings come back.")
                .size(12.5)
                .font(font)
                .color(theme::rgb(p.ink_3)),
        ]
        .spacing(4)
        .width(Length::Fill),
        image(&app.moth_art)
            .content_fit(ContentFit::Contain)
            .width(Length::Fixed(66.0))
            .height(Length::Fixed(60.0)),
        button(
            text(if paused {
                "Resume automations"
            } else {
                "Pause automations"
            })
            .size(12)
            .font(theme::ui_semibold(t.design)),
        )
        .padding([8, 14])
        .style(theme::btn_secondary(t, paused))
        .on_press(Message::SetAutomationsPaused(!paused)),
    ]
    .spacing(14)
    .align_y(Alignment::Center);

    let mut rules_col = column![].spacing(10);
    for (idx, rule) in app.snapshot.config.automations.iter().enumerate() {
        let is_light_win = status.light_winner.as_deref() == Some(&rule.id);
        let is_fan_win = status.fan_winner.as_deref() == Some(&rule.id);
        let is_active = status.active_ids.iter().any(|id| id == &rule.id);
        let in_control = is_light_win || is_fan_win;

        let state_label = if paused {
            "Paused".to_string()
        } else if !rule.enabled {
            "Disabled".to_string()
        } else if is_light_win && is_fan_win {
            "In control of lights and fans now".to_string()
        } else if is_light_win {
            "In control of the lights now".to_string()
        } else if is_fan_win {
            "In control of the fans now".to_string()
        } else if is_active {
            "Active (overridden by higher rule)".to_string()
        } else {
            "Waiting".to_string()
        };

        let sentence = format!(
            "{}, {}.",
            rule.trigger.describe(),
            rule.action.describe(&app.snapshot.config.presets)
        );

        let idx_badge = container(
            text(format!("{}", idx + 1))
                .size(12)
                .font(theme::mono_font()),
        )
        .width(Length::Fixed(28.0))
        .height(Length::Fixed(28.0))
        .center_x(Length::Fixed(28.0))
        .center_y(Length::Fixed(28.0))
        .style(theme::chip_style(
            t,
            if in_control { Some(p.brass_hi) } else { None },
        ));

        rules_col = rules_col.push(
            container(
                row![
                    idx_badge,
                    column![
                        text(sentence).size(14).font(font),
                        text(state_label)
                            .size(11.5)
                            .font(font)
                            .color(theme::rgb(if in_control {
                                p.brass_hi
                            } else if is_active {
                                p.ok
                            } else {
                                p.ink_3
                            })),
                    ]
                    .spacing(4)
                    .width(Length::Fill),
                    pill_toggle(t, rule.enabled, Message::ToggleAutomation(rule.id.clone())),
                    button(text("↑").size(12).font(font))
                        .padding([6, 9])
                        .style(theme::btn_secondary(t, false))
                        .on_press(Message::MoveAutomation(rule.id.clone(), true)),
                    button(text("↓").size(12).font(font))
                        .padding([6, 9])
                        .style(theme::btn_secondary(t, false))
                        .on_press(Message::MoveAutomation(rule.id.clone(), false)),
                    button(text("×").size(12).font(font))
                        .padding([6, 10])
                        .style(theme::btn_secondary(t, false))
                        .on_press(Message::DeleteAutomation(rule.id.clone())),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding(t.pad(15))
            .style(theme::active_card_box(t, in_control)),
        );
    }

    // Full Rule Builder: all 5 triggers & all 4 actions
    let trig_btns = row![
        button(text("Time window").size(12).font(font))
            .padding([6, 11])
            .style(theme::btn_secondary(
                t,
                app.auto_trigger == AutoTriggerDraft::TimeWindow
            ))
            .on_press(Message::SetAutoTriggerDraft(AutoTriggerDraft::TimeWindow)),
        button(text("App running (.exe)").size(12).font(font))
            .padding([6, 11])
            .style(theme::btn_secondary(
                t,
                app.auto_trigger == AutoTriggerDraft::AppRunning
            ))
            .on_press(Message::SetAutoTriggerDraft(AutoTriggerDraft::AppRunning)),
        button(text("Idle (minutes)").size(12).font(font))
            .padding([6, 11])
            .style(theme::btn_secondary(
                t,
                app.auto_trigger == AutoTriggerDraft::Idle
            ))
            .on_press(Message::SetAutoTriggerDraft(AutoTriggerDraft::Idle)),
        button(text("PC locked").size(12).font(font))
            .padding([6, 11])
            .style(theme::btn_secondary(
                t,
                app.auto_trigger == AutoTriggerDraft::Locked
            ))
            .on_press(Message::SetAutoTriggerDraft(AutoTriggerDraft::Locked)),
        button(text("Temp above").size(12).font(font))
            .padding([6, 11])
            .style(theme::btn_secondary(
                t,
                app.auto_trigger == AutoTriggerDraft::TempAbove
            ))
            .on_press(Message::SetAutoTriggerDraft(AutoTriggerDraft::TempAbove)),
    ]
    .spacing(6);

    let trig_detail: Element<'_, Message> = match app.auto_trigger {
        AutoTriggerDraft::TimeWindow => {
            let mut day_row = row![
                button(text("Every day").size(11).font(font))
                    .padding([5, 9])
                    .style(theme::btn_secondary(t, app.auto_days_mask == ALL_DAYS_MASK))
                    .on_press(Message::SetAutoDaysMask(ALL_DAYS_MASK)),
            ]
            .spacing(4);
            for (idx, lbl) in ["M", "T", "W", "T", "F", "S", "S"].iter().enumerate() {
                let on = (app.auto_days_mask & (1u8 << idx)) != 0;
                day_row = day_row.push(
                    button(text(*lbl).size(11).font(font))
                        .padding([5, 8])
                        .style(theme::btn_secondary(t, on))
                        .on_press(Message::ToggleAutoDay(idx as u8)),
                );
            }
            row![
                text("Start:").size(12).font(font),
                text_input("23:00", &app.auto_start_draft)
                    .style(theme::text_input_style(t))
                    .on_input(Message::SetAutoStartDraft)
                    .width(Length::Fixed(72.0)),
                text("End:").size(12).font(font),
                text_input("07:00", &app.auto_end_draft)
                    .style(theme::text_input_style(t))
                    .on_input(Message::SetAutoEndDraft)
                    .width(Length::Fixed(72.0)),
                day_row,
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into()
        }
        AutoTriggerDraft::AppRunning => row![
            text("Executable:").size(12).font(font),
            text_input("Cyberpunk2077.exe", &app.auto_exe_draft)
                .style(theme::text_input_style(t))
                .on_input(Message::SetAutoExeDraft)
                .width(Length::Fixed(240.0)),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into(),
        AutoTriggerDraft::Idle => row![
            text("Idle for").size(12).font(font),
            text_input("10", &app.auto_idle_draft)
                .style(theme::text_input_style(t))
                .on_input(Message::SetAutoIdleDraft)
                .width(Length::Fixed(64.0)),
            text(format!(
                "minutes (currently idle {}s)",
                app.snapshot.idle_secs
            ))
            .size(12)
            .font(font)
            .color(theme::rgb(p.ink_3)),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into(),
        AutoTriggerDraft::Locked => text("Triggers immediately whenever Windows is locked.")
            .size(12)
            .font(font)
            .color(theme::rgb(p.ink_3))
            .into(),
        AutoTriggerDraft::TempAbove => {
            let mut src_row = row![].spacing(4);
            for src in TempSource::ALL {
                let short = match src {
                    TempSource::Max => "Hotter",
                    TempSource::Cpu => "CPU",
                    TempSource::Gpu => "GPU",
                    TempSource::Liquid => "Coolant",
                };
                src_row = src_row.push(
                    button(text(short).size(11).font(font))
                        .padding([5, 9])
                        .style(theme::btn_secondary(t, app.auto_temp_source == src))
                        .on_press(Message::SetAutoTempSource(src)),
                );
            }
            let parsed_c = app.auto_temp_draft.trim().parse::<u8>().unwrap_or(80);
            let equiv = if unit == TempUnit::F {
                format!("°C ({})", unit.format_temp(parsed_c as f32))
            } else {
                "°C (3°C hysteresis)".to_string()
            };
            row![
                src_row,
                text("above").size(12).font(font),
                text_input("80", &app.auto_temp_draft)
                    .style(theme::text_input_style(t))
                    .on_input(Message::SetAutoTempDraft)
                    .width(Length::Fixed(64.0)),
                text(equiv).size(12).font(font).color(theme::rgb(p.ink_3)),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into()
        }
    };

    let act_btns = row![
        button(text("Apply preset").size(12).font(font))
            .padding([6, 11])
            .style(theme::btn_secondary(
                t,
                app.auto_action == AutoActionDraft::ApplyPreset
            ))
            .on_press(Message::SetAutoActionDraft(AutoActionDraft::ApplyPreset)),
        button(text("Turn lights off").size(12).font(font))
            .padding([6, 11])
            .style(theme::btn_secondary(
                t,
                app.auto_action == AutoActionDraft::LightsOff
            ))
            .on_press(Message::SetAutoActionDraft(AutoActionDraft::LightsOff)),
        button(text("Dim (%)").size(12).font(font))
            .padding([6, 11])
            .style(theme::btn_secondary(
                t,
                app.auto_action == AutoActionDraft::Dim
            ))
            .on_press(Message::SetAutoActionDraft(AutoActionDraft::Dim)),
        button(text("Fan curve").size(12).font(font))
            .padding([6, 11])
            .style(theme::btn_secondary(
                t,
                app.auto_action == AutoActionDraft::FanCurve
            ))
            .on_press(Message::SetAutoActionDraft(AutoActionDraft::FanCurve)),
    ]
    .spacing(6);

    let act_detail: Element<'_, Message> = match app.auto_action {
        AutoActionDraft::ApplyPreset => {
            let mut r = row![].spacing(5);
            for preset in app.snapshot.config.presets.iter().take(6) {
                r = r.push(
                    button(text(&preset.name).size(11.5).font(font))
                        .padding([5, 10])
                        .style(theme::btn_secondary(t, app.auto_preset_draft == preset.id))
                        .on_press(Message::SetAutoPresetDraft(preset.id.clone())),
                );
            }
            r.into()
        }
        AutoActionDraft::LightsOff => Space::new().width(0).into(),
        AutoActionDraft::Dim => container(labeled_slider(
            t,
            font,
            "Dim to",
            format!("{}%", app.auto_dim_draft),
            5,
            80,
            app.auto_dim_draft as u16,
            |v| Message::SetAutoDimDraft(v as u8),
        ))
        .width(Length::Fixed(240.0))
        .into(),
        AutoActionDraft::FanCurve => {
            let mut r = row![].spacing(5);
            for c in ["silent", "balanced", "performance"] {
                r = r.push(
                    button(text(cap_first(c)).size(11.5).font(font))
                        .padding([5, 10])
                        .style(theme::btn_secondary(t, app.auto_curve_draft == c))
                        .on_press(Message::SetAutoCurveDraft(c.to_string())),
                );
            }
            r.into()
        }
    };

    let builder = container(
        column![
            text("NEW AUTOMATION")
                .size(11)
                .font(theme::ui_semibold(t.design))
                .color(theme::rgb(p.ink_3)),
            row![text("When:").size(13).font(font), trig_btns]
                .spacing(10)
                .align_y(Alignment::Center),
            trig_detail,
            row![
                text("Then:").size(13).font(font),
                act_btns,
                Space::new().width(Length::Fill),
                button(
                    text("+ Add automation")
                        .size(12)
                        .font(theme::ui_semibold(t.design)),
                )
                .padding([8, 16])
                .style(theme::btn_primary(t))
                .on_press(Message::AddAutomation),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            act_detail,
        ]
        .spacing(11),
    )
    .padding(t.pad(16))
    .style(theme::panel_style(t));

    // 4 One-Click Smart Recipe Cards (2x2 balanced grid)
    let recipe_card = |badge: &'static str,
                       badge_col: u32,
                       title: &'static str,
                       desc: &'static str,
                       idea: StarterIdea| {
        button(
            column![
                row![
                    status_dot(badge_col, 7.0),
                    text(badge)
                        .size(10)
                        .font(theme::ui_semibold(t.design))
                        .color(theme::rgb(badge_col)),
                    Space::new().width(Length::Fill),
                    text("+ Add")
                        .size(10.5)
                        .font(theme::ui_semibold(t.design))
                        .color(theme::rgb(p.brass_hi)),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
                text(title)
                    .size(14.5)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(p.ink)),
                text(desc)
                    .size(11.5)
                    .font(font)
                    .color(theme::rgb(p.ink_3)),
            ]
            .spacing(5),
        )
        .padding(t.pad(14))
        .width(Length::Fill)
        .style(theme::card_button(t, false))
        .on_press(Message::AddStarterAutomation(idea))
    };

    let smart_recipes = container(
        column![
            row![
                text("SMART RECIPES (1-CLICK)")
                    .size(11)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(t.pink_rose)),
                Space::new().width(Length::Fill),
                text("Tap any card to add immediately")
                    .size(11)
                    .font(font)
                    .color(theme::rgb(p.ink_3)),
            ]
            .align_y(Alignment::Center),
            row![
                recipe_card(
                    "SCHEDULE",
                    t.pink_rose,
                    "Sunset dim at 23:00",
                    "Every evening 23:00–07:00, gently dim lights to 20%.",
                    StarterIdea::SunsetDim,
                ),
                recipe_card(
                    "APP TRIGGER",
                    p.brass_hi,
                    "Gaming boost",
                    "When Cyberpunk2077.exe launches, activate Gaming profile.",
                    StarterIdea::GamingBoost,
                ),
            ]
            .spacing(t.sp(12)),
            row![
                recipe_card(
                    "ACOUSTIC",
                    t.data_primary,
                    "Quiet when idle",
                    "After 15 idle minutes, switch case fans to Silent.",
                    StarterIdea::QuietWhenIdle,
                ),
                recipe_card(
                    "THERMAL",
                    p.warm,
                    "Thermal boost > 75°C",
                    "Whenever CPU or GPU exceeds 75°C, switch fans to Performance.",
                    StarterIdea::ThermalBoost,
                ),
            ]
            .spacing(t.sp(12)),
        ]
        .spacing(t.sp(10)),
    )
    .padding(t.pad(16))
    .style(theme::panel_style(t));

    column![top_bar, rules_col, builder, smart_recipes]
        .spacing(t.sp(14))
        .into()
}

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

pub fn view_devices<'a>(
    app: &'a AirglowApp,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
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

    let stat_card = |label: &'static str, value: String, sub: String, accent: u32| {
        container(
            column![
                row![
                    status_dot(accent, 7.0),
                    text(label)
                        .size(10.5)
                        .font(theme::ui_semibold(t.design))
                        .color(theme::rgb(p.ink_3)),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
                text(value)
                    .size(22)
                    .font(theme::display_semibold(t.design))
                    .color(theme::rgb(p.ink)),
                text(sub)
                    .size(11)
                    .font(font)
                    .color(theme::rgb(p.ink_3)),
            ]
            .spacing(4),
        )
        .padding(t.pad(13))
        .width(Length::Fill)
        .style(theme::panel_style(t))
    };

    let summary_strip = row![
        stat_card(
            "DIRECT USB HID",
            format!("{}", app.snapshot.hid_devices.len()),
            if app.snapshot.hid_devices.is_empty() {
                "Simulated rig mode".to_string()
            } else {
                "Active native USB streams".to_string()
            },
            p.ok,
        ),
        stat_card(
            "OPENRGB SDK",
            format!("{}", app.snapshot.openrgb.controller_count),
            if app.snapshot.openrgb.connected {
                "Connected via SDK".to_string()
            } else {
                "Standby / offline".to_string()
            },
            t.data_secondary,
        ),
        stat_card(
            "ACTIVE ZONES",
            format!("{enabled_zones} / {total_zones}"),
            "Custom 2D geometry".to_string(),
            p.brass_hi,
        ),
        stat_card(
            "ADDRESSABLE LEDS",
            format!("{total_leds}"),
            format!("{:.0} fps output stream", app.snapshot.fps),
            t.pink_rose,
        ),
    ]
    .spacing(t.sp(12));
    let mut hid_col = column![
        row![
            text("DIRECT USB HID CONTROLLERS")
                .size(11.5)
                .font(theme::ui_semibold(t.design))
                .color(theme::rgb(p.ink_3)),
            Space::new().width(Length::Fill),
            button(
                text("Rescan Hardware")
                    .size(12)
                    .font(theme::ui_semibold(t.design)),
            )
            .padding([7, 14])
            .style(theme::btn_primary(t))
            .on_press(Message::RescanHardware),
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(10);

    if app.snapshot.hid_devices.is_empty() {
        hid_col = hid_col.push(
            text("No supported NZXT or ASUS Aura USB controllers detected on this PC (running with live rig simulation).")
                .size(12.5)
                .font(font)
                .color(theme::rgb(p.ink_3)),
        );
    } else {
        for dev in &app.snapshot.hid_devices {
            hid_col = hid_col.push(
                container(
                    row![
                        status_dot(if dev.online { p.ok } else { p.warm }, 8.0),
                        column![
                            text(&dev.name)
                                .size(14)
                                .font(theme::ui_semibold(t.design)),
                            text(format!("{} · {}", dev.kind, dev.note))
                                .size(12)
                                .font(font)
                                .color(theme::rgb(p.ink_3)),
                        ]
                        .spacing(2),
                        Space::new().width(Length::Fill),
                        text(if dev.online { "Connected" } else { "Busy" })
                            .size(12)
                            .font(font)
                            .color(theme::rgb(if dev.online { p.ok } else { p.warm })),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                )
                .padding(12)
                .style(theme::well_style(t)),
            );
        }
    }

    let mut orgb_col = column![
        row![
            text("OPENRGB SDK")
                .size(11.5)
                .font(theme::ui_semibold(t.design))
                .color(theme::rgb(p.ink_3)),
            Space::new().width(Length::Fill),
            status_dot(
                if app.snapshot.openrgb.connected {
                    p.ok
                } else {
                    p.ink_3
                },
                8.0
            ),
        ]
        .align_y(Alignment::Center),
        text(&app.snapshot.openrgb.message)
            .size(13.5)
            .font(font)
            .color(theme::rgb(if app.snapshot.openrgb.connected {
                p.ok
            } else {
                p.ink_2
            })),
    ]
    .spacing(8);
    for ctrl in &app.snapshot.openrgb.controllers {
        orgb_col = orgb_col.push(
            text(format!("• [{}] {}", ctrl.index, ctrl.name))
                .size(12)
                .font(theme::mono_font())
                .color(theme::rgb(p.ink_3)),
        );
    }

    // Full per-zone setup editor
    let mut zone_rows = column![
        text("LIGHTING ZONES, GEOMETRY & COLOUR ORDER")
            .size(11.5)
            .font(theme::ui_semibold(t.design))
            .color(theme::rgb(p.ink_3)),
    ]
    .spacing(10);

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
                .label(format!(
                    "{} ({led_count} LEDs · {})",
                    z.name,
                    z.layout.shape_label()
                ))
                .style(theme::checkbox_style(t))
                .on_toggle(move |_| Message::ToggleZone(zid.clone())),
            Space::new().width(Length::Fill),
            button(
                row![
                    color_order_dots(z.color_order),
                    text(format!("{:?}", z.color_order).to_uppercase())
                        .size(11)
                        .font(theme::mono_font()),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding([5, 10])
            .style(theme::btn_secondary(t, false))
            .on_press(Message::CycleZoneColorOrder(zid_ord)),
            button(
                text(if flashing { "Blinking…" } else { "Identify" })
                    .size(11.5)
                    .font(font),
            )
            .padding([5, 11])
            .style(theme::btn_secondary(t, flashing))
            .on_press(Message::IdentifyZone(zid_id)),
            button(
                text(if expanded { "Hide setup" } else { "Setup" })
                    .size(11.5)
                    .font(font),
            )
            .padding([5, 11])
            .style(theme::btn_secondary(t, expanded))
            .on_press(Message::ToggleExpandZone(zid_exp)),
        ]
        .spacing(7)
        .align_y(Alignment::Center);

        let mut zone_col = column![header_row].spacing(10);
        if expanded {
            zone_col = zone_col.push(zone_setup_editor(z, t, font));
        }

        zone_rows = zone_rows.push(
            container(zone_col)
                .padding(12)
                .style(theme::well_style(t)),
        );
    }

    let mut page = column![
        text("Devices")
            .size(26)
            .font(theme::display_semibold(t.design)),
        summary_strip,
    ]
    .spacing(t.sp(14));
    if let Some(conflict_banner) = view_conflict_banner(app, t, font) {
        page = page.push(conflict_banner);
    }
    page = page
        .push(
            container(hid_col)
                .padding(t.pad(16))
                .width(Length::Fill)
                .style(theme::panel_style(t)),
        )
        .push(
            container(orgb_col)
                .padding(t.pad(16))
                .width(Length::Fill)
                .style(theme::panel_style(t)),
        )
        .push(
            container(zone_rows)
                .padding(t.pad(16))
                .width(Length::Fill)
                .style(theme::panel_style(t)),
        );

    page.into()
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
        text("Shape:").size(12).font(font),
        button(text("Fans").size(11.5).font(font))
            .padding([5, 10])
            .style(theme::btn_secondary(
                t,
                matches!(z.layout, ZoneLayout::Fans { .. })
            ))
            .on_press(Message::UpdateZone(to_fans)),
        button(text("Strip").size(11.5).font(font))
            .padding([5, 10])
            .style(theme::btn_secondary(
                t,
                matches!(z.layout, ZoneLayout::Strip { .. })
            ))
            .on_press(Message::UpdateZone(to_strip)),
        button(text("Matrix").size(11.5).font(font))
            .padding([5, 10])
            .style(theme::btn_secondary(
                t,
                matches!(z.layout, ZoneLayout::Matrix { .. })
            ))
            .on_press(Message::UpdateZone(to_matrix)),
    ]
    .spacing(6)
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

pub fn view_settings<'a>(
    app: &'a AirglowApp,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let settings = &app.snapshot.config.settings;
    let appr = &app.snapshot.config.appearance;

    // 1. Appearance Studio Hero Banner (Firefly Dusk + Bioluminescent Particles + Lantern Moth)
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
                                .size(26)
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
                        text("Handcraft your Airglow atelier — combine any of 13 designs with live custom accents, Saved Looks, Rig Styles, and GPU Power Mode.")
                            .size(12)
                            .font(font)
                            .color(theme::rgb(0xe2d4df)),
                    ]
                    .spacing(5)
                    .width(Length::Fill),
                    moth_img,
                    Space::new().width(Length::Fixed(14.0)),
                    button(
                        text("? Shortcuts")
                            .size(11.5)
                            .font(theme::ui_semibold(t.design)),
                    )
                    .padding([5, 11])
                    .style(theme::pill_action_btn(t, app.show_shortcuts))
                    .on_press(Message::ToggleShortcuts),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding([14, 22])
            .width(Length::Fill)
            .height(Length::Fill),
        ]
        .width(Length::Fill)
        .height(Length::Fixed(104.0)),
    )
    .width(Length::Fill)
    .clip(true)
    .style(theme::panel_style(t));

    // 2. Live Accent Override Swatches + Saved Looks Shelf
    let mut accent_chips = row![].spacing(8).align_y(Alignment::Center);
    accent_chips = accent_chips.push(
        button(
            row![
                status_dot(settings.design.palette().brass, 10.0),
                text("Default").size(11.5).font(font),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([6, 11])
        .style(theme::card_button(t, appr.accent_override.is_none()))
        .on_press(Message::SetAccentOverride(None)),
    );
    for (name, rgb) in ACCENT_SWATCHES {
        let selected = appr.accent_override == Some(*rgb);
        accent_chips = accent_chips.push(
            button(
                row![
                    color_dot(*rgb, 11.0),
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
            .padding([6, 11])
            .style(theme::card_button(t, selected))
            .on_press(Message::SetAccentOverride(Some(*rgb))),
        );
    }

    let mut looks_grid = column![].spacing(8);
    for chunk in app.snapshot.config.saved_looks.chunks(4) {
        let mut r = row![].spacing(8);
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
                color_dot(dot_rgb, 11.0),
                text(&look.name)
                    .size(12.5)
                    .font(theme::ui_semibold(t.design))
                    .width(Length::Fill),
            ]
            .spacing(7)
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
                    .size(11)
                    .font(font)
                    .color(theme::rgb(p.ink_3)),
                ]
                .spacing(4),
            )
            .padding([9, 11])
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
                            .padding([9, 8])
                            .style(theme::btn_secondary(t, false))
                            .on_press(Message::DeleteLook(look.id.clone())),
                    ]
                    .spacing(4)
                    .width(Length::Fill),
                );
            }
        }
        for _ in chunk.len()..4 {
            r = r.push(Space::new().width(Length::Fill));
        }
        looks_grid = looks_grid.push(r);
    }

    let custom_hex_preview = airglow_core::config::parse_hex_rgb(&app.accent_hex_draft)
        .or(appr.accent_override)
        .unwrap_or_else(|| theme::hex_to_rgb_arr(settings.design.palette().brass));

    let accent_hex_row = row![
        text("Custom #HEX:")
            .size(11.5)
            .font(font)
            .color(theme::rgb(p.ink_2)),
        color_dot(custom_hex_preview, 12.0),
        text_input("#F48FB1", &app.accent_hex_draft)
            .style(theme::text_input_style(t))
            .on_input(Message::AccentHexDraftChanged)
            .on_submit(Message::ApplyAccentHex)
            .width(Length::Fixed(104.0)),
        button(
            text("Apply")
                .size(11)
                .font(theme::ui_semibold(t.design)),
        )
        .padding([5, 10])
        .style(theme::btn_secondary(t, false))
        .on_press(Message::ApplyAccentHex),
    ]
    .spacing(7)
    .align_y(Alignment::Center);

    let accent_and_looks = container(
        column![
            row![
                text("LIVE ACCENT OVERRIDE")
                    .size(11.5)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(t.pink_rose)),
                Space::new().width(Length::Fill),
                accent_hex_row,
            ]
            .align_y(Alignment::Center),
            accent_chips,
            Space::new().height(4),
            row![
                text("SAVED LOOKS")
                    .size(11.5)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(p.ink_3)),
                Space::new().width(Length::Fill),
                text_input("Name this look…", &app.look_name_draft)
                    .style(theme::text_input_style(t))
                    .on_input(Message::LookNameDraftChanged)
                    .on_submit(Message::SaveLook)
                    .width(Length::Fixed(210.0)),
                button(
                    text("+ Save look")
                        .size(11.5)
                        .font(theme::ui_semibold(t.design)),
                )
                .padding([6, 12])
                .style(theme::btn_primary(t))
                .on_press(Message::SaveLook),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            looks_grid,
        ]
        .spacing(10),
    )
    .padding(t.pad(16))
    .width(Length::Fill)
    .style(theme::panel_style(t));

    // 3. Performance, GPU Power Mode & Rig Visuals
    let mut fps_seg = row![].spacing(2);
    for (fps, label) in [
        (15u8, "Eco (15 FPS)"),
        (30, "Balanced (30 FPS)"),
        (60, "Smooth (60 FPS)"),
    ] {
        fps_seg = fps_seg.push(
            button(text(label).size(11.5).font(font))
                .padding([5, 11])
                .style(theme::segmented_item_btn(t, appr.ui_fps_cap == fps))
                .on_press(Message::SetUiFpsCap(fps)),
        );
    }

    let mut rig_seg = row![].spacing(2);
    for style in RigStyle::ALL {
        rig_seg = rig_seg.push(
            button(text(style.label()).size(11.5).font(font))
                .padding([5, 11])
                .style(theme::segmented_item_btn(t, appr.rig_style == style))
                .on_press(Message::SetRigStyle(style)),
        );
    }

    let app_cpu = app.snapshot.sensors.app_cpu_pct;
    let app_mem = app.snapshot.sensors.app_mem_mb;
    let resource_pill = container(
        row![
            status_dot(t.data_primary, 6.5),
            text(format!(
                "App: {app_cpu:.1}% CPU · {app_mem:.0} MB · {} FPS",
                appr.ui_fps_cap
            ))
            .size(11)
            .font(theme::mono_font())
            .color(theme::rgb(t.data_primary)),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .padding([4, 10])
    .style(theme::chip_style(t, Some(t.data_primary)));

    let perf_card = container(
        column![
            row![
                text("PERFORMANCE, GPU POWER MODE & VISUAL ATMOSPHERE")
                    .size(11.5)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(p.ink_3)),
                Space::new().width(Length::Fill),
                resource_pill,
            ]
            .align_y(Alignment::Center),
            row![
                text("UI animation frame rate:").size(12.5).font(font),
                container(fps_seg)
                    .padding(3)
                    .style(theme::segmented_rail_style(t)),
                Space::new().width(14),
                text("Rig stage style:").size(12.5).font(font),
                container(rig_seg)
                    .padding(3)
                    .style(theme::segmented_rail_style(t)),
                Space::new().width(Length::Fill),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            row![
                row![
                    pill_toggle(t, appr.airflow_lines, Message::ToggleAirflowLines),
                    text("Airflow ribbons").size(12).font(font),
                ]
                .spacing(7)
                .align_y(Alignment::Center),
                row![
                    pill_toggle(t, appr.spinning_fans, Message::ToggleSpinningFans),
                    text("Spinning fans").size(12).font(font),
                ]
                .spacing(7)
                .align_y(Alignment::Center),
                row![
                    pill_toggle(t, appr.ambient_bloom, Message::ToggleAmbientBloom),
                    text("LED ambient bloom").size(12).font(font),
                ]
                .spacing(7)
                .align_y(Alignment::Center),
                row![
                    pill_toggle(t, appr.firefly_particles, Message::ToggleFireflyParticles),
                    text("Firefly bokeh").size(12).font(font),
                ]
                .spacing(7)
                .align_y(Alignment::Center),
                row![
                    pill_toggle(t, appr.compact_density, Message::ToggleCompactDensity),
                    text("Compact density").size(12).font(font),
                ]
                .spacing(7)
                .align_y(Alignment::Center),
                Space::new().width(Length::Fill),
            ]
            .spacing(18)
            .align_y(Alignment::Center),
        ]
        .spacing(11),
    )
    .padding(t.pad(16))
    .width(Length::Fill)
    .style(theme::panel_style(t));

    // 4. 13-Design Visual Gallery using DesignPreviewCanvas miniature Airglow windows
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
            .height(Length::Fixed(98.0));

            let mut title_row = row![
                text(d.label())
                    .size(14.5)
                    .font(theme::display_semibold(t.design))
                    .color(theme::rgb(if selected { p.brass_hi } else { p.ink })),
                Space::new().width(Length::Fill),
            ]
            .align_y(Alignment::Center);

            if selected {
                title_row = title_row.push(
                    container(
                        text("IN USE")
                            .size(10)
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
                            .size(11.5)
                            .font(font)
                            .color(theme::rgb(p.ink_3)),
                    ]
                    .spacing(8),
                )
                .padding(11)
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

    // 5. General & System preferences
    let temp_seg = container(
        row![
            button(text("Celsius (°C)").size(12).font(font))
                .padding([5, 12])
                .style(theme::segmented_item_btn(t, settings.temp_unit == TempUnit::C))
                .on_press(Message::SetTempUnit(TempUnit::C)),
            button(text("Fahrenheit (°F)").size(12).font(font))
                .padding([5, 12])
                .style(theme::segmented_item_btn(t, settings.temp_unit == TempUnit::F))
                .on_press(Message::SetTempUnit(TempUnit::F)),
        ]
        .spacing(2),
    )
    .padding(3)
    .style(theme::segmented_rail_style(t));

    let general = container(
        column![
            text("GENERAL, TEMPERATURE UNIT & WINDOWS BEHAVIOUR")
                .size(11.5)
                .font(theme::ui_semibold(t.design))
                .color(theme::rgb(p.ink_3)),
            row![text("Temperature unit:").size(13).font(font), temp_seg,]
                .spacing(12)
                .align_y(Alignment::Center),
            checkbox(settings.start_with_windows)
                .label("Start Airglow with Windows (background tray startup)")
                .style(theme::checkbox_style(t))
                .on_toggle(Message::ToggleStartWithWindows),
            checkbox(settings.lights_off_on_sleep)
                .label("Turn lights off on sleep and restore on wake")
                .style(theme::checkbox_style(t))
                .on_toggle(Message::ToggleLightsOffSleep),
            checkbox(settings.lights_off_on_lock)
                .label("Turn lights off while Windows is locked")
                .style(theme::checkbox_style(t))
                .on_toggle(Message::ToggleLightsOffLock),
            checkbox(settings.demo_mode)
                .label("Demo telemetry fallback when LibreHardwareMonitor is offline")
                .style(theme::checkbox_style(t))
                .on_toggle(Message::ToggleDemoMode),
        ]
        .spacing(11),
    )
    .padding(t.pad(16))
    .width(Length::Fill)
    .style(theme::panel_style(t));

    // 6. Sensors, OpenRGB & Electricity tariff
    let lhm_auth_badge = if app.snapshot.lhm_auth_configured {
        "Auth configured (lhm-auth.json)"
    } else {
        "No auth header"
    };

    let integrations = container(
        column![
            text("SENSORS, OPENRGB & ELECTRICITY TARIFF")
                .size(11.5)
                .font(theme::ui_semibold(t.design))
                .color(theme::rgb(p.ink_3)),
            row![
                text("LibreHardwareMonitor URL:").size(13).font(font),
                text_input("http://127.0.0.1:8085", &app.lhm_url_draft)
                    .style(theme::text_input_style(t))
                    .on_input(Message::SetLhmUrl)
                    .width(Length::Fixed(220.0)),
                text(lhm_auth_badge)
                    .size(11.5)
                    .font(font)
                    .color(theme::rgb(if app.snapshot.lhm_auth_configured {
                        p.ok
                    } else {
                        p.ink_3
                    })),
                Space::new().width(Length::Fill),
                text("Tariff (€/kWh):").size(13).font(font),
                text_input("0.3797", &app.tariff_draft)
                    .style(theme::text_input_style(t))
                    .on_input(Message::SetTariffDraft)
                    .width(Length::Fixed(86.0)),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            row![
                checkbox(settings.openrgb_enabled)
                    .label("OpenRGB SDK")
                    .style(theme::checkbox_style(t))
                    .on_toggle(Message::ToggleOpenRgb),
                text("Host:").size(12).font(font),
                text_input("127.0.0.1", &app.openrgb_host_draft)
                    .style(theme::text_input_style(t))
                    .on_input(Message::SetOpenRgbHost)
                    .width(Length::Fixed(130.0)),
                text("Port:").size(12).font(font),
                text_input("6742", &app.openrgb_port_draft)
                    .style(theme::text_input_style(t))
                    .on_input(Message::SetOpenRgbPort)
                    .width(Length::Fixed(70.0)),
                text(&app.snapshot.openrgb.message)
                    .size(11.5)
                    .font(font)
                    .color(theme::rgb(if app.snapshot.openrgb.connected {
                        p.ok
                    } else {
                        p.ink_3
                    })),
                Space::new().width(Length::Fill),
                button(
                    text("Rescan Hardware")
                        .size(12)
                        .font(theme::ui_semibold(t.design)),
                )
                .padding([7, 13])
                .style(theme::btn_primary(t))
                .on_press(Message::RescanHardware),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ]
        .spacing(11),
    )
    .padding(t.pad(16))
    .width(Length::Fill)
    .style(theme::panel_style(t));

    // 7. Migration, Diagnostics & About
    let tools_about = container(
        column![
            text("MIGRATION, DIAGNOSTICS & ABOUT")
                .size(11.5)
                .font(theme::ui_semibold(t.design))
                .color(theme::rgb(p.ink_3)),
            row![
                button(
                    text("Import settings from Python Airglow")
                        .size(12)
                        .font(theme::ui_semibold(t.design)),
                )
                .padding([8, 14])
                .style(theme::btn_primary(t))
                .on_press(Message::ImportPythonSettings),
                button(text("Save diagnostics report").size(12).font(font))
                    .padding([8, 14])
                    .style(theme::btn_secondary(t, false))
                    .on_press(Message::ExportDiagnostics),
                Space::new().width(Length::Fill),
                column![
                    text(format!("Airglow (Rust) v{}", env!("CARGO_PKG_VERSION")))
                        .size(13.5)
                        .font(theme::display_semibold(t.design)),
                    text("Native Iced GPU UI · Pure-Rust hardware core · Zero telemetry")
                        .size(11.5)
                        .font(font)
                        .color(theme::rgb(p.ink_3)),
                ]
                .spacing(2)
                .align_x(Alignment::End),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        ]
        .spacing(10),
    )
    .padding(t.pad(16))
    .width(Length::Fill)
    .style(theme::panel_style(t));

    column![
        hero_banner,
        accent_and_looks,
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
        general,
        integrations,
        tools_about,
    ]
    .spacing(t.sp(14))
    .into()
}
