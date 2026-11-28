//! Shared UI instrument helpers (pill toggle, segmented rail, hero readouts, dusk masthead),
//! the responsive 4-column Bento Overview Tile Workshop + Theater Rig Mode,
//! Compact Mini-Mode, and Fans view.

use airglow_core::config::{
    OverviewTile, OverviewTileKind, OverviewWorkspace, TempUnit, TileStyle,
};
use airglow_core::effects::{BlendMode, EffectId, HARMONIES, LayerKind};
use airglow_core::sensors;
use iced::widget::{
    Space, button, checkbox, column, container, image, row, scrollable, slider, stack, text,
    text_input,
};
use iced::{Alignment, ContentFit, Element, Length, Padding};

use crate::bento_grid::{BentoGrid, BentoSpec};
use crate::canvas_ambient::{AudioVisualizerCanvas, FireflyParticlesCanvas, UiIcon, icon_canvas};
use crate::canvas_colorwheel::{ColorWheelCanvas, ColorWheelMessage};
use crate::canvas_stage::CaseStageCanvas;
use crate::canvas_widgets::{
    BarMeterCanvas, EffectThumbCanvas, MiniFanCurveCanvas, PowerGraphCanvas, RadialGaugeCanvas,
    SparklineCanvas,
};
use crate::theme::{self, DesignTokens};
use crate::views_palette;
use crate::{AddTileTab, AirglowApp, Message};

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

fn sleep_pill_label(remaining_s: Option<u32>) -> String {
    match remaining_s {
        Some(rem) => format!("☾ {:02}:{:02}", rem / 60, rem % 60),
        None => "☾ Sleep".to_string(),
    }
}

fn view_workspace_bar<'a>(
    app: &'a AirglowApp,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let cfg = &app.snapshot.config;
    let active_id = &cfg.active_workspace_id;

    let mut pills = row![
        text("Workspaces")
            .size(12)
            .font(theme::ui_semibold(t.design))
            .color(theme::rgb(p.ink_3)),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    for ws in &cfg.overview_workspaces {
        let is_active = ws.id == *active_id;
        pills = pills.push(
            button(
                text(truncate_str(&ws.name, 20))
                    .size(12)
                    .font(if is_active {
                        theme::ui_semibold(t.design)
                    } else {
                        font
                    }),
            )
            .padding([5, 12])
            .style(theme::masthead_pill_btn(t, is_active))
            .on_press(Message::SwitchWorkspace(ws.id.clone())),
        );
    }

    pills = pills.push(
        button(
            text("+ New workspace")
                .size(11.5)
                .font(theme::ui_semibold(t.design)),
        )
        .padding([5, 11])
        .style(theme::masthead_pill_btn(t, app.workspace_create_open))
        .on_press(Message::ToggleWorkspaceCreate),
    );

    let mut right_tools = row![
        button(text("Rename").size(11.5).font(font))
            .padding([4, 10])
            .style(theme::btn_ghost(t, app.workspace_rename_open))
            .on_press(Message::ToggleWorkspaceRename),
        button(text("Reset layout").size(11.5).font(font))
            .padding([4, 10])
            .style(theme::btn_ghost(t, false))
            .on_press(Message::ResetActiveWorkspace),
    ]
    .spacing(5)
    .align_y(Alignment::Center);

    if cfg.overview_workspaces.len() > 1 {
        right_tools = right_tools.push(
            button(text("Delete").size(11.5).font(font))
                .padding([4, 10])
                .style(theme::btn_danger(t))
                .on_press(Message::DeleteWorkspace(active_id.clone())),
        );
    }

    let mut bar_col = column![
        row![pills, Space::new().width(Length::Fill), right_tools]
            .spacing(8)
            .align_y(Alignment::Center),
    ]
    .spacing(8);

    if app.workspace_create_open {
        let templates = OverviewWorkspace::starter_templates();
        let mut tpl_row = row![
            text("Template:")
                .size(11.5)
                .font(font)
                .color(theme::rgb(p.ink_3)),
            button(text("Default").size(11.5).font(font))
                .padding([4, 10])
                .style(theme::masthead_pill_btn(
                    t,
                    app.workspace_template_draft.is_none()
                ))
                .on_press(Message::WorkspaceTemplateSelected(None)),
        ]
        .spacing(5)
        .align_y(Alignment::Center);

        for tpl in &templates {
            let selected = app.workspace_template_draft.as_deref() == Some(tpl.id.as_str());
            tpl_row = tpl_row.push(
                button(text(tpl.name.clone()).size(11.5).font(font))
                    .padding([4, 10])
                    .style(theme::masthead_pill_btn(t, selected))
                    .on_press(Message::WorkspaceTemplateSelected(Some(tpl.id.clone()))),
            );
        }

        bar_col = bar_col.push(
            row![
                text_input("New workspace name…", &app.workspace_name_draft)
                    .style(theme::text_input_style(t))
                    .on_input(Message::WorkspaceNameDraftChanged)
                    .on_submit(Message::CreateWorkspace)
                    .width(Length::Fixed(190.0)),
                tpl_row,
                Space::new().width(Length::Fill),
                button(
                    text("Create")
                        .size(11.5)
                        .font(theme::ui_semibold(t.design)),
                )
                .padding([5, 13])
                .style(theme::btn_primary(t))
                .on_press(Message::CreateWorkspace),
                button(text("Cancel").size(11.5).font(font))
                    .padding([5, 10])
                    .style(theme::btn_ghost(t, false))
                    .on_press(Message::ToggleWorkspaceCreate),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
    }

    if app.workspace_rename_open {
        bar_col = bar_col.push(
            row![
                text("Rename workspace:")
                    .size(11.5)
                    .font(font)
                    .color(theme::rgb(p.ink_2)),
                text_input("Workspace name…", &app.workspace_rename_draft)
                    .style(theme::text_input_style(t))
                    .on_input(Message::WorkspaceRenameDraftChanged)
                    .on_submit(Message::ConfirmWorkspaceRename)
                    .width(Length::Fixed(220.0)),
                button(
                    text("Save")
                        .size(11.5)
                        .font(theme::ui_semibold(t.design)),
                )
                .padding([5, 12])
                .style(theme::btn_primary(t))
                .on_press(Message::ConfirmWorkspaceRename),
                button(text("Cancel").size(11.5).font(font))
                    .padding([5, 10])
                    .style(theme::btn_ghost(t, false))
                    .on_press(Message::ToggleWorkspaceRename),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
    }

    container(bar_col)
        .padding([9, 14])
        .width(Length::Fill)
        .style(theme::bento_tile_style(t, false))
        .into()
}

/// Single clean, borderless floating Overview masthead directly on the deep plum ground.
fn view_overview_masthead<'a>(
    app: &'a AirglowApp,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let s = &app.snapshot.sensors;
    let ws_name = app.snapshot.config.active_workspace().name.as_str();
    let active_zones = app.snapshot.frames.iter().filter(|f| f.enabled).count();
    let zones_label = if active_zones == 1 {
        "1 zone online".to_string()
    } else {
        format!("{active_zones} zones online")
    };

    let manage_lbl = if app.workspaces_bar_open && !app.edit_tiles_mode {
        "Hide workspaces ▴"
    } else {
        "Workspaces ▾"
    };

    let title_stack = column![
        text(ws_name)
            .size(36)
            .font(theme::ui_bold(t.design))
            .color(theme::rgb(p.ink)),
        row![
            status_dot(
                if s.lhm_connected || active_zones > 0 {
                    p.ok
                } else {
                    p.warm
                },
                7.0
            ),
            text(zones_label)
                .size(13)
                .font(font)
                .color(theme::rgb(p.ink_2)),
            text("·")
                .size(13)
                .font(font)
                .color(theme::rgba(p.ink_3, 0.55)),
            button(
                text(manage_lbl)
                    .size(13)
                    .font(theme::ui_semibold(t.design)),
            )
            .padding([1, 4])
            .style(theme::btn_frameless(t))
            .on_press(Message::ToggleWorkspacesBar),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(2);

    let left_cluster: Element<'a, Message> = if t.is_firefly() {
        row![
            image(&app.moth_art)
                .content_fit(ContentFit::Contain)
                .width(Length::Fixed(56.0))
                .height(Length::Fixed(56.0)),
            title_stack,
        ]
        .spacing(14)
        .align_y(Alignment::Center)
        .into()
    } else {
        title_stack.into()
    };

    let ctl_label = match app.snapshot.sleep_timer_remaining_s {
        Some(rem) => format!("Control center · {:02}:{:02}", rem / 60, rem % 60),
        None => "Control center".to_string(),
    };

    let right_pills: Element<'a, Message> = if app.edit_tiles_mode {
        row![
            button(
                row![
                    icon_canvas(UiIcon::Plus, theme::rgb(p.brass_hi), 13.5),
                    text(if app.add_tile_open {
                        "Add tile ✓"
                    } else {
                        "Add tile"
                    })
                    .size(12.5)
                    .font(theme::ui_semibold(t.design)),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding([8, 15])
            .style(theme::masthead_pill_btn(t, app.add_tile_open))
            .on_press(Message::ToggleAddTileCatalog),
            button(
                row![
                    icon_canvas(UiIcon::Reset, theme::rgb(p.ink_2), 13.5),
                    text("Reset layout").size(12.5).font(font),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding([8, 15])
            .style(theme::masthead_pill_btn(t, false))
            .on_press(Message::ResetActiveWorkspace),
            button(
                row![
                    icon_canvas(UiIcon::Sliders, theme::rgb(p.ink_2), 13.5),
                    text(ctl_label).size(12.5).font(font),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding([8, 15])
            .style(theme::masthead_pill_btn(t, app.customize_open))
            .on_press(Message::ToggleCustomizeDrawer),
            button(
                row![
                    icon_canvas(UiIcon::Check, theme::rgb(t.primary_ink), 13.5),
                    text("Done")
                        .size(12.5)
                        .font(theme::ui_semibold(t.design)),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding([8, 18])
            .style(theme::btn_primary(t))
            .on_press(Message::ToggleEditTilesMode),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
    } else {
        row![
            button(
                row![
                    icon_canvas(
                        UiIcon::Sliders,
                        theme::rgb(if app.customize_open { p.brass_hi } else { p.ink_2 }),
                        14.0,
                    ),
                    text(ctl_label)
                        .size(12.5)
                        .font(theme::ui_semibold(t.design)),
                ]
                .spacing(7)
                .align_y(Alignment::Center),
            )
            .padding([8, 16])
            .style(theme::masthead_pill_btn(t, app.customize_open))
            .on_press(Message::ToggleCustomizeDrawer),
            button(
                row![
                    icon_canvas(UiIcon::Customize, theme::rgb(p.brass_hi), 14.0),
                    text("Customize")
                        .size(12.5)
                        .font(theme::ui_semibold(t.design)),
                ]
                .spacing(7)
                .align_y(Alignment::Center),
            )
            .padding([8, 16])
            .style(theme::masthead_pill_btn(t, false))
            .on_press(Message::ToggleEditTilesMode),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
    };

    container(
        row![left_cluster, Space::new().width(Length::Fill), right_pills]
            .spacing(12)
            .align_y(Alignment::Center),
    )
    .padding([2, 2])
    .width(Length::Fill)
    .into()
}

/// Interactive `+ Add Tile` Catalog Drawer (Controls, 12 Built-in Sensors, Live 222+ LHM Sensors).
fn view_add_tile_catalog<'a>(
    app: &'a AirglowApp,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let mono = theme::mono_font();
    let lhm_count = app.snapshot.sensors.readings.len();

    let tab_rail = container(
        row![
            button(text("Controls (6)").size(11.5).font(font))
                .padding([5, 11])
                .style(theme::segmented_item_btn(
                    t,
                    app.add_tile_tab == AddTileTab::Controls
                ))
                .on_press(Message::SetAddTileTab(AddTileTab::Controls)),
            button(text("Sensors (12)").size(11.5).font(font))
                .padding([5, 11])
                .style(theme::segmented_item_btn(
                    t,
                    app.add_tile_tab == AddTileTab::Sensors
                ))
                .on_press(Message::SetAddTileTab(AddTileTab::Sensors)),
            button(
                text(format!("Live Sensors ({lhm_count} LHM)"))
                    .size(11.5)
                    .font(font),
            )
            .padding([5, 11])
            .style(theme::segmented_item_btn(
                t,
                app.add_tile_tab == AddTileTab::LiveLhm
            ))
            .on_press(Message::SetAddTileTab(AddTileTab::LiveLhm)),
        ]
        .spacing(2),
    )
    .padding(2)
    .style(theme::segmented_rail_style(t));

    let header = row![
        column![
            text("Add tile to workspace")
                .size(16)
                .font(theme::display_semibold(t.design))
                .color(theme::rgb(p.ink)),
            text("Pin controls, built-in instruments, or any live LibreHardwareMonitor sensor onto Overview.")
                .size(11.5)
                .font(font)
                .color(theme::rgb(p.ink_3)),
        ]
        .spacing(2),
        Space::new().width(Length::Fill),
        tab_rail,
        Space::new().width(8),
        button(
            text("Done")
                .size(11.5)
                .font(theme::ui_semibold(t.design)),
        )
        .padding([5, 13])
        .style(theme::btn_primary(t))
        .on_press(Message::ToggleAddTileCatalog),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let body: Element<'a, Message> = match app.add_tile_tab {
        AddTileTab::Controls => {
            let items = OverviewTileKind::builtin_controls();
            let mut grid = column![].spacing(8);
            for chunk in items.chunks(3) {
                let mut r = row![].spacing(8);
                for kind in chunk {
                    let span_lbl = format!("{}×{}", kind.default_span(), kind.default_rows());
                    r = r.push(
                        container(
                            column![
                                row![
                                    text(kind.label().to_string())
                                        .size(13)
                                        .font(theme::ui_semibold(t.design))
                                        .color(theme::rgb(p.ink)),
                                    Space::new().width(Length::Fill),
                                    container(
                                        text(span_lbl)
                                            .size(10)
                                            .font(mono)
                                            .color(theme::rgb(p.brass_hi)),
                                    )
                                    .padding([2, 6])
                                    .style(theme::chip_style(t, Some(p.brass))),
                                ]
                                .align_y(Alignment::Center),
                                text(kind.description())
                                    .size(11)
                                    .font(font)
                                    .color(theme::rgb(p.ink_3)),
                                row![
                                    Space::new().width(Length::Fill),
                                    button(
                                        text("+ Add tile")
                                            .size(11)
                                            .font(theme::ui_semibold(t.design)),
                                    )
                                    .padding([4, 11])
                                    .style(theme::btn_primary(t))
                                    .on_press(Message::AddOverviewTile(kind.clone())),
                                ],
                            ]
                            .spacing(6),
                        )
                        .padding(11)
                        .width(Length::Fill)
                        .style(theme::card_style(t)),
                    );
                }
                for _ in chunk.len()..3 {
                    r = r.push(Space::new().width(Length::Fill));
                }
                grid = grid.push(r);
            }
            grid.into()
        }
        AddTileTab::Sensors => {
            let items = OverviewTileKind::builtin_sensors();
            let mut grid = column![].spacing(8);
            for chunk in items.chunks(4) {
                let mut r = row![].spacing(8);
                for kind in chunk {
                    let live_preview = sensor_quick_preview(app, kind);
                    r = r.push(
                        container(
                            column![
                                row![
                                    text(kind.label().to_string())
                                        .size(12.5)
                                        .font(theme::ui_semibold(t.design))
                                        .color(theme::rgb(p.ink)),
                                    Space::new().width(Length::Fill),
                                    text(live_preview)
                                        .size(11.5)
                                        .font(mono)
                                        .color(theme::rgb(t.data_primary)),
                                ]
                                .align_y(Alignment::Center),
                                text(kind.description())
                                    .size(10.5)
                                    .font(font)
                                    .color(theme::rgb(p.ink_3)),
                                row![
                                    Space::new().width(Length::Fill),
                                    button(
                                        text("+ Pin")
                                            .size(10.5)
                                            .font(theme::ui_semibold(t.design)),
                                    )
                                    .padding([4, 10])
                                    .style(theme::btn_secondary(t, false))
                                    .on_press(Message::AddOverviewTile(kind.clone())),
                                ],
                            ]
                            .spacing(5),
                        )
                        .padding(10)
                        .width(Length::Fill)
                        .style(theme::card_style(t)),
                    );
                }
                for _ in chunk.len()..4 {
                    r = r.push(Space::new().width(Length::Fill));
                }
                grid = grid.push(r);
            }
            grid.into()
        }
        AddTileTab::LiveLhm => {
            let q = app.sensor_search_query.trim().to_ascii_lowercase();
            let matches: Vec<&sensors::SensorReading> = app
                .snapshot
                .sensors
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

            let total_matches = matches.len();
            let search_bar = row![
                text_input(
                    "Search live LHM sensors (e.g. Vcore, Clock, Fan, NVMe, Coolant, Voltage)…",
                    &app.sensor_search_query
                )
                .style(theme::text_input_style(t))
                .on_input(Message::SensorSearchChanged)
                .width(Length::Fill),
                container(
                    text(format!("{total_matches} match(es)"))
                        .size(11)
                        .font(mono)
                        .color(theme::rgb(t.data_primary)),
                )
                .padding([5, 10])
                .style(theme::chip_style(t, Some(t.data_primary))),
            ]
            .spacing(8)
            .align_y(Alignment::Center);

            let mut list = column![].spacing(6);
            if matches.is_empty() {
                list = list.push(
                    container(
                        text(if lhm_count == 0 {
                            "No live LHM readings yet — start LibreHardwareMonitor Remote Web Server to browse all 200+ sensors."
                        } else {
                            "No live sensors match your search query."
                        })
                        .size(12)
                        .font(font)
                        .color(theme::rgb(p.ink_3)),
                    )
                    .padding([16, 12]),
                );
            } else {
                for chunk in matches.iter().take(48).collect::<Vec<_>>().chunks(2) {
                    let mut r = row![].spacing(8);
                    for reading in chunk {
                        let val_str = format_custom_reading(reading.value, &reading.unit);
                        let kind = OverviewTileKind::CustomSensor {
                            sensor_id: reading.id.clone(),
                            label: reading.name.clone(),
                            unit: reading.unit.clone(),
                        };
                        r = r.push(
                            container(
                                row![
                                    column![
                                        text(truncate_str(&reading.name, 28))
                                            .size(12)
                                            .font(theme::ui_semibold(t.design))
                                            .color(theme::rgb(p.ink)),
                                        text(truncate_str(
                                            &format!("{} · {}", reading.hardware, reading.sensor_type),
                                            38
                                        ))
                                        .size(10.5)
                                        .font(font)
                                        .color(theme::rgb(p.ink_3)),
                                    ]
                                    .spacing(2)
                                    .width(Length::Fill),
                                    text(val_str)
                                        .size(12)
                                        .font(mono)
                                        .color(theme::rgb(p.brass_hi)),
                                    button(
                                        text("+ Pin")
                                            .size(10.5)
                                            .font(theme::ui_semibold(t.design)),
                                    )
                                    .padding([4, 9])
                                    .style(theme::btn_primary(t))
                                    .on_press(Message::AddOverviewTile(kind)),
                                ]
                                .spacing(8)
                                .align_y(Alignment::Center),
                            )
                            .padding([7, 10])
                            .width(Length::Fill)
                            .style(theme::card_style(t)),
                        );
                    }
                    if chunk.len() == 1 {
                        r = r.push(Space::new().width(Length::Fill));
                    }
                    list = list.push(r);
                }
            }

            column![
                search_bar,
                scrollable(list).height(Length::Fixed(210.0)),
            ]
            .spacing(8)
            .into()
        }
    };

    container(column![header, body].spacing(10))
        .padding(t.pad(14))
        .width(Length::Fill)
        .style(theme::drawer_panel_style(t))
        .into()
}

/// Computes fluid single-row Bento height so a 4-row asymmetric Bento grid fills 900p, 1080p,
/// and 1440p (1.25× HiDPI) viewports cleanly with zero empty bottom void and zero scrollbar.
pub fn compute_bento_row_height(window_h: f32) -> f32 {
    ((window_h - 198.0) / 4.0).clamp(150.0, 290.0)
}

/// Compact floating top-right pill cluster shown inside each tile during `✦ Customize` mode.
fn tile_customize_overlay<'a>(
    tile: &OverviewTile,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let mono = theme::mono_font();
    let id = tile.id.clone();
    let rows_val = tile.rows.max(1);

    let mut pills = row![
        button(
            text(format!("{}×{}", tile.span, rows_val))
                .size(10)
                .font(mono),
        )
        .padding([2, 7])
        .style(theme::masthead_pill_btn(t, false))
        .on_press(Message::CycleOverviewTileSpan(id.clone())),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    if tile.kind.is_sensor() {
        pills = pills.push(
            button(text(tile.style.label()).size(10).font(font))
                .padding([2, 7])
                .style(theme::masthead_pill_btn(t, tile.style != TileStyle::Auto))
                .on_press(Message::CycleOverviewTileStyle(id.clone())),
        );
    }

    pills = pills.push(
        button(text("✕").size(10).font(font))
            .padding([2, 7])
            .style(theme::btn_danger(t))
            .on_press(Message::RemoveOverviewTile(id)),
    );

    container(row![Space::new().width(Length::Fill), pills])
        .width(Length::Fill)
        .into()
}

fn wrap_tile<'a>(
    app: &AirglowApp,
    tile: &OverviewTile,
    inner: Element<'a, Message>,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let content: Element<'a, Message> = if app.edit_tiles_mode {
        stack![inner, tile_customize_overlay(tile, t, font)]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        inner
    };

    let card = container(content)
        .padding([t.pad(14), t.pad(16)])
        .width(Length::Fill)
        .height(Length::Fill)
        .clip(true);

    if app.edit_tiles_mode {
        card.style(theme::editing_tile_style(t)).into()
    } else {
        card.style(theme::rich_instrument_card_style(t, None, false))
            .into()
    }
}

fn bento_hero_number<'a>(
    num: &str,
    unit: &str,
    num_size: f32,
    unit_size: f32,
    num_col: u32,
    unit_col: u32,
    t: DesignTokens,
) -> Element<'a, Message> {
    let val_font = theme::ui_font(t.design);
    let mut r = row![
        text(num.to_string())
            .size(num_size)
            .font(val_font)
            .color(theme::rgb(num_col)),
    ]
    .spacing(5)
    .align_y(Alignment::End);

    if !unit.is_empty() {
        let bottom_lift = ((num_size - unit_size) * 0.22).round().max(2.0);
        r = r.push(
            container(
                text(unit.to_string())
                    .size(unit_size)
                    .font(val_font)
                    .color(theme::rgb(unit_col)),
            )
            .padding(Padding {
                top: 0.0,
                right: 0.0,
                bottom: bottom_lift,
                left: 0.0,
            }),
        );
    }
    r.into()
}

fn view_overview_tile<'a>(
    app: &'a AirglowApp,
    tile: &'a OverviewTile,
    span: u8,
    tile_h: f32,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    match &tile.kind {
        OverviewTileKind::Lighting => render_lighting_tile(app, tile, span, tile_h, t, font),
        OverviewTileKind::Profiles => render_profiles_tile(app, tile, span, tile_h, t, font),
        OverviewTileKind::Fans => render_fans_tile(app, tile, span, tile_h, t, font),
        OverviewTileKind::Hardware => render_hardware_tile(app, tile, span, tile_h, t, font),
        OverviewTileKind::DeskDisplay => render_desk_display_tile(app, tile, span, tile_h, t, font),
        OverviewTileKind::Lookbook => render_lookbook_tile(app, tile, span, tile_h, t, font),
        OverviewTileKind::PowerDraw => render_power_draw_tile(app, tile, span, tile_h, t, font),
        _ => render_sensor_tile(app, tile, span, tile_h, t, font),
    }
}

fn render_lighting_tile<'a>(
    app: &'a AirglowApp,
    tile: &'a OverviewTile,
    span: u8,
    _tile_h: f32,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let s = &app.snapshot.sensors;
    let eff = &app.snapshot.effective_scene;
    let appr = &app.snapshot.config.appearance;
    let unit = app.snapshot.config.settings.temp_unit;

    let mut header = row![
        text("Lighting canvas")
            .size(15)
            .font(theme::ui_semibold(t.design))
            .color(theme::rgb(p.ink)),
        Space::new().width(Length::Fill),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    if !app.edit_tiles_mode {
        header = header
            .push(
                button(
                    row![
                        icon_canvas(
                            UiIcon::Sparkle,
                            theme::rgb(if appr.overview_stage_animated {
                                if p.is_light { p.brass } else { p.brass_hi }
                            } else {
                                p.ink_3
                            }),
                            12.0,
                        ),
                        text(if appr.overview_stage_animated {
                            "On"
                        } else {
                            "Off"
                        })
                        .size(11.5)
                        .font(if appr.overview_stage_animated {
                            theme::ui_semibold(t.design)
                        } else {
                            font
                        }),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding([4, 10])
                .style(theme::masthead_pill_btn(t, appr.overview_stage_animated))
                .on_press(Message::ToggleOverviewStageAnimation),
            )
            .push(
                button(
                    row![
                        icon_canvas(UiIcon::Expand, theme::rgb(p.ink_2), 12.0),
                        text("Theater").size(11.5).font(font),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding([4, 10])
                .style(theme::masthead_pill_btn(t, false))
                .on_press(Message::ToggleTheaterRig),
            )
            .push(
                button(
                    row![
                        icon_canvas(UiIcon::Lighting, theme::rgb(p.brass_hi), 12.0),
                        text("Open Lighting ↗")
                            .size(11.5)
                            .font(theme::ui_semibold(t.design)),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding([4, 11])
                .style(theme::masthead_pill_btn(t, false))
                .on_press(Message::SelectTab(crate::Tab::Lighting)),
            );
    }

    let active_effect_title = if eff.effect == EffectId::Studio {
        eff.studio_stack.name.clone()
    } else {
        eff.effect.label().to_string()
    };

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
        anim_t: if appr.overview_stage_animated {
            app.anim_t
        } else {
            0.0
        },
        fan_duty: app.snapshot.fan_duty,
        cpu_temp: s.cpu_temp,
        gpu_temp: s.gpu_temp,
        ambient_rgb: app.snapshot.average_rgb,
        eyebrow: None,
        animate_lighting: appr.overview_stage_animated,
    })
    .width(Length::Fill)
    .height(Length::Fill);
    let stage_el = Element::from(stage).map(Message::Stage);

    let stage_well = container(stage_el)
        .width(Length::Fill)
        .height(Length::Fill)
        .clip(true)
        .style(theme::bento_stage_well_style(t));

    let profile_subtitle = if let Some(rule_id) = &app.snapshot.automation_status.light_winner {
        format!("Automation · {rule_id}")
    } else if let Some(pid) = &app.snapshot.config.active_preset
        && let Some(pr) = app.snapshot.config.presets.iter().find(|pr| &pr.id == pid)
    {
        format!("{} profile", pr.name)
    } else {
        "Custom lighting".to_string()
    };

    let slider_w = if span >= 3 {
        190.0
    } else if span >= 2 {
        148.0
    } else {
        92.0
    };
    let brightness = app.snapshot.config.scene.brightness;

    let brightness_stack = column![
        row![
            text("Brightness")
                .size(11.5)
                .font(font)
                .color(theme::rgb(p.ink_2)),
            Space::new().width(Length::Fill),
            text(format!("{brightness}%"))
                .size(11.5)
                .font(theme::ui_semibold(t.design))
                .color(theme::rgb(p.ink)),
        ]
        .align_y(Alignment::Center),
        slider(0.0..=100.0, brightness as f32, |v| {
            Message::SetBrightness(v.round() as u8)
        })
        .style(theme::slider_style(t))
        .width(Length::Fixed(slider_w)),
    ]
    .spacing(3)
    .width(Length::Fixed(slider_w));

    let bottom_bar = row![
        column![
            text(truncate_str(&active_effect_title, 24))
                .size(14)
                .font(theme::ui_semibold(t.design))
                .color(theme::rgb(p.ink)),
            text(truncate_str(&profile_subtitle, 30))
                .size(12)
                .font(font)
                .color(theme::rgb(p.ink_3)),
        ]
        .spacing(1)
        .width(Length::Fill),
        brightness_stack,
        pill_toggle(t, eff.on, Message::TogglePower),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    let inner = column![header, stage_well, bottom_bar]
        .spacing(10)
        .height(Length::Fill);

    wrap_tile(app, tile, inner.into(), t, font)
}

fn profile_preview_colors(preset: &airglow_core::config::Preset) -> ([u8; 3], [u8; 3]) {
    if preset.effect == EffectId::Studio
        && let Some(stack) = &preset.studio_stack
        && let Some(first) = stack.layers.first()
    {
        let c1 = first.color;
        let c2 = stack
            .layers
            .get(1)
            .map(|l| l.color)
            .unwrap_or(first.secondary);
        return (c1, c2);
    }

    let primary = preset_dot_color(preset.effect, preset.params.color);
    let secondary = match preset.effect {
        EffectId::Rainbow | EffectId::Cycle => [64, 210, 255],
        EffectId::Temperature | EffectId::Heatmap => [255, 118, 86],
        EffectId::Aurora => [196, 132, 255],
        EffectId::FireflySwarm => [244, 143, 177],
        EffectId::MeteorShower => [255, 195, 139],
        EffectId::Spectrum | EffectId::Pulse => [165, 222, 206],
        EffectId::Studio => [244, 143, 177],
        _ => {
            if preset.params.colors.len() >= 2 {
                preset.params.colors[1]
            } else if preset.params.secondary != [0, 0, 0]
                && preset.params.secondary != preset.params.color
            {
                preset.params.secondary
            } else {
                [
                    primary[0].saturating_sub(28),
                    primary[1].saturating_add(32),
                    primary[2].saturating_add(58),
                ]
            }
        }
    };
    (primary, secondary)
}

fn render_profiles_tile<'a>(
    app: &'a AirglowApp,
    tile: &'a OverviewTile,
    span: u8,
    _tile_h: f32,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let max_cards: usize = match span {
        0..=1 => 2,
        2 => 5,
        _ => 6,
    };

    let mut header = row![
        text("Profiles")
            .size(14.5)
            .font(theme::ui_semibold(t.design))
            .color(theme::rgb(p.ink)),
        Space::new().width(Length::Fill),
    ]
    .align_y(Alignment::Center);

    if !app.edit_tiles_mode {
        header = header.push(
            button(
                row![
                    icon_canvas(UiIcon::Sliders, theme::rgb(p.brass_hi), 12.0),
                    text("Edit profiles")
                        .size(11.5)
                        .font(theme::ui_semibold(t.design)),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .padding([4, 10])
            .style(theme::masthead_pill_btn(t, false))
            .on_press(Message::SelectTab(crate::Tab::Lighting)),
        );
    }

    let mut cards_row = row![].spacing(8).height(Length::Fill);
    for preset in app.snapshot.config.presets.iter().take(max_cards) {
        let active = app.snapshot.config.active_preset.as_deref() == Some(&preset.id);
        let (c1, c2) = profile_preview_colors(preset);

        let thumb = EffectThumbCanvas {
            effect: preset.effect,
            color: c1,
            secondary: c2,
            palette: p,
            accent_override: t.accent_override,
            anim_t: app.anim_t,
            selected: active,
        }
        .with_custom_colors(c1, c2);

        let swatch = container(
            iced::widget::canvas(thumb)
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .clip(true);

        let effect_sub = if active {
            format!("{} · On", truncate_str(preset.effect.label(), 10))
        } else {
            truncate_str(preset.effect.label(), 14)
        };

        let card_inner = column![
            swatch,
            column![
                text(truncate_str(&preset.name, 14))
                    .size(12.5)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(if active { p.brass_hi } else { p.ink })),
                text(effect_sub)
                    .size(11)
                    .font(font)
                    .color(theme::rgb(p.ink_3)),
            ]
            .spacing(1),
        ]
        .spacing(7)
        .width(Length::Fill)
        .height(Length::Fill);

        cards_row = cards_row.push(
            button(card_inner)
                .padding([8, 8])
                .width(Length::Fill)
                .height(Length::Fill)
                .style(theme::bento_profile_card_btn(t, active))
                .on_press(Message::ApplyPreset(preset.id.clone())),
        );
    }

    let inner = column![header, cards_row]
        .spacing(10)
        .height(Length::Fill);

    wrap_tile(app, tile, inner.into(), t, font)
}

fn render_power_draw_tile<'a>(
    app: &'a AirglowApp,
    tile: &'a OverviewTile,
    span: u8,
    tile_h: f32,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    if tile.style != TileStyle::Auto && tile.style != TileStyle::Graph {
        return render_sensor_tile(app, tile, span, tile_h, t, font);
    }

    let p = t.p;
    let s = &app.snapshot.sensors;
    let cpu_w_str = s
        .cpu_power_w
        .map(|v| format!("{v:.0}"))
        .unwrap_or_else(|| "—".to_string());
    let gpu_w_str = s
        .gpu_power_w
        .map(|v| format!("{v:.0}"))
        .unwrap_or_else(|| "—".to_string());

    let cpu_col = if t.is_firefly() {
        0xa5dece
    } else {
        p.brass_hi
    };
    let gpu_col = if t.is_firefly() {
        0xd1b9f0
    } else {
        p.ink_2
    };

    let dashed_gpu_swatch = row![
        container(Space::new().width(5).height(2.5)).style(move |_| {
            iced::widget::container::Style {
                background: Some(iced::Background::Color(theme::rgb(gpu_col))),
                border: iced::Border {
                    radius: iced::border::Radius::from(999.0),
                    ..Default::default()
                },
                ..Default::default()
            }
        }),
        container(Space::new().width(5).height(2.5)).style(move |_| {
            iced::widget::container::Style {
                background: Some(iced::Background::Color(theme::rgb(gpu_col))),
                border: iced::Border {
                    radius: iced::border::Radius::from(999.0),
                    ..Default::default()
                },
                ..Default::default()
            }
        }),
    ]
    .spacing(3)
    .align_y(Alignment::Center);

    let legend_row = row![
        row![
            dash_swatch(cpu_col),
            text("CPU power")
                .size(12)
                .font(font)
                .color(theme::rgb(p.ink_3)),
            text(cpu_w_str)
                .size(20)
                .font(theme::ui_semibold(t.design))
                .color(theme::rgb(p.ink)),
            text("W").size(12).font(font).color(theme::rgb(p.ink_3)),
        ]
        .spacing(5)
        .align_y(Alignment::Center),
        row![
            dashed_gpu_swatch,
            text("GPU power")
                .size(12)
                .font(font)
                .color(theme::rgb(p.ink_3)),
            text(gpu_w_str)
                .size(20)
                .font(theme::ui_semibold(t.design))
                .color(theme::rgb(p.ink)),
            text("W").size(12).font(font).color(theme::rgb(p.ink_3)),
        ]
        .spacing(5)
        .align_y(Alignment::Center),
    ]
    .spacing(16)
    .align_y(Alignment::Center);

    let chart = iced::widget::canvas(PowerGraphCanvas {
        cpu_w: &app.snapshot.cpu_power_history,
        gpu_w: &app.snapshot.gpu_power_history,
        palette: p,
        accent_override: t.accent_override,
        anim_t: app.anim_t,
        show_halo: false,
    })
    .width(Length::Fill)
    .height(Length::Fill);

    let inner = column![
        column![
            text("CPU + GPU power")
                .size(14)
                .font(theme::ui_semibold(t.design))
                .color(theme::rgb(p.ink)),
            legend_row,
        ]
        .spacing(4),
        chart,
    ]
    .spacing(6)
    .height(Length::Fill);

    wrap_tile(app, tile, inner.into(), t, font)
}

fn render_fans_tile<'a>(
    app: &'a AirglowApp,
    tile: &'a OverviewTile,
    span: u8,
    tile_h: f32,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let mono = theme::mono_font();
    let full_perf_label = span >= 2 || app.window_size.width >= 1220.0;
    let mut curve_seg = row![].spacing(2);
    for name in ["silent", "balanced", "performance"] {
        let active = app.snapshot.effective_curve == name;
        let label = match name {
            "silent" => "Silent",
            "balanced" => "Balanced",
            _ if full_perf_label => "Performance",
            _ => "Perf",
        };
        curve_seg = curve_seg.push(
            button(container(text(label).size(11).font(font)).center_x(Length::Fill))
                .padding([4, 4])
                .width(Length::Fill)
                .style(theme::segmented_pill_style(t, active))
                .on_press(Message::SelectFanCurve(name.to_string())),
        );
    }

    let rpm_badge = if !app.snapshot.fan_rpms.is_empty() {
        let sum: u32 = app.snapshot.fan_rpms.iter().map(|(_, r)| *r as u32).sum();
        let avg = sum / app.snapshot.fan_rpms.len() as u32;
        format!("{avg} rpm · {} fans", app.snapshot.fan_rpms.len())
    } else if !app.snapshot.lhm_channels.is_empty() {
        let rpms: Vec<u16> = app
            .snapshot
            .lhm_channels
            .iter()
            .filter_map(|c| c.rpm)
            .collect();
        if rpms.is_empty() {
            format!("{} channels", app.snapshot.lhm_channels.len())
        } else {
            let avg = rpms.iter().map(|&r| r as u32).sum::<u32>() / rpms.len() as u32;
            format!("{avg} rpm · {} fans", rpms.len())
        }
    } else {
        "Quiet curve".to_string()
    };

    let mut head = row![
        text("Case fans")
            .size(14.5)
            .font(theme::ui_semibold(t.design))
            .color(theme::rgb(p.ink)),
        Space::new().width(Length::Fill),
    ]
    .align_y(Alignment::Center);

    if !app.edit_tiles_mode {
        head = head.push(
            container(
                text(cap_first(&app.snapshot.effective_curve))
                    .size(10.5)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(p.ok)),
            )
            .padding([2, 8])
            .style(theme::chip_style(t, Some(p.ok))),
        );
    }

    let num_sz = if tile_h >= 200.0 { 38.0 } else { 32.0 };
    let unit_sz = if tile_h >= 200.0 { 16.0 } else { 14.0 };

    let stat_row = row![
        bento_hero_number(
            &app.snapshot.fan_duty.to_string(),
            "%",
            num_sz,
            unit_sz,
            p.ink,
            p.ink_3,
            t,
        ),
        Space::new().width(Length::Fill),
        text(rpm_badge)
            .size(11.5)
            .font(mono)
            .color(theme::rgb(p.ink_2)),
    ]
    .spacing(6)
    .align_y(Alignment::End);

    let curve_pts: Vec<(f32, f32)> = app
        .snapshot
        .config
        .fan_curves
        .get(&app.snapshot.effective_curve)
        .map(|pts| pts.iter().map(|pt| (pt.temp as f32, pt.duty as f32)).collect())
        .unwrap_or_else(|| vec![(30.0, 25.0), (55.0, 45.0), (75.0, 75.0), (90.0, 100.0)]);

    let mini_curve = iced::widget::canvas(MiniFanCurveCanvas {
        points: curve_pts,
        current_temp: app.snapshot.fan_temp,
        current_duty: app.snapshot.fan_duty as f32,
        phase: app.anim_t,
        spinning: app.snapshot.config.appearance.spinning_fans,
        palette: p,
        accent_override: t.accent_override,
        show_halo: false,
    })
    .width(Length::Fill)
    .height(Length::Fill);

    let inner = column![
        head,
        stat_row,
        mini_curve,
        container(curve_seg)
            .padding(2)
            .width(Length::Fill)
            .style(theme::segmented_track_style(t)),
    ]
    .spacing(4)
    .height(Length::Fill);

    wrap_tile(app, tile, inner.into(), t, font)
}

fn render_hardware_tile<'a>(
    app: &'a AirglowApp,
    tile: &'a OverviewTile,
    _span: u8,
    _tile_h: f32,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let s = &app.snapshot.sensors;

    let mut head = row![
        text("Hardware & telemetry")
            .size(14)
            .font(theme::ui_semibold(t.design))
            .color(theme::rgb(p.ink)),
        Space::new().width(Length::Fill),
    ]
    .align_y(Alignment::Center);

    if !app.edit_tiles_mode {
        head = head.push(
            row![
                status_dot(if s.lhm_connected { p.ok } else { p.warm }, 6.5),
                text(if s.lhm_connected {
                    "LHM Live"
                } else {
                    "Demo fallback"
                })
                .size(11.5)
                .font(font)
                .color(theme::rgb(if s.lhm_connected { p.ok } else { p.warm })),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        );
    }

    let inner = column![
        head,
        Space::new().height(Length::Fill),
        row![
            text(format!("{} USB HID", app.snapshot.hid_devices.len()))
                .size(22)
                .font(theme::ui_semibold(t.design)),
            Space::new().width(14),
            text(format!(
                "{} OpenRGB",
                app.snapshot.openrgb.controller_count
            ))
            .size(22)
            .font(theme::ui_semibold(t.design))
            .color(theme::rgb(t.data_secondary)),
            Space::new().width(14),
            text(format!(
                "{} zones",
                app.snapshot
                    .config
                    .zones
                    .iter()
                    .filter(|z| z.enabled)
                    .count()
            ))
            .size(22)
            .font(theme::ui_semibold(t.design))
            .color(theme::rgb(p.brass_hi)),
        ]
        .align_y(Alignment::Center),
        Space::new().height(Length::Fill),
        row![
            text(truncate_str(
                &format!(
                    "{} · {} sensors tracked",
                    app.snapshot.openrgb.message,
                    s.readings.len()
                ),
                48
            ))
            .size(12)
            .font(font)
            .color(theme::rgb(p.ink_3))
            .width(Length::Fill),
            button(text("Devices ↗").size(12).font(theme::ui_semibold(t.design)))
                .padding([2, 4])
                .style(theme::btn_frameless(t))
                .on_press(Message::SelectTab(crate::Tab::Devices)),
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(5)
    .height(Length::Fill);

    wrap_tile(app, tile, inner.into(), t, font)
}

fn render_desk_display_tile<'a>(
    app: &'a AirglowApp,
    tile: &'a OverviewTile,
    _span: u8,
    _tile_h: f32,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let mono = theme::mono_font();
    let fm = app.focus_timer_secs / 60;
    let fs = app.focus_timer_secs % 60;

    let mut head = row![
        text("Desk Display")
            .size(14)
            .font(theme::ui_semibold(t.design))
            .color(theme::rgb(p.ink)),
        Space::new().width(Length::Fill),
    ]
    .align_y(Alignment::Center);

    if !app.edit_tiles_mode {
        head = head.push(
            button(text("Mini-Mode ↗").size(12).font(theme::ui_semibold(t.design)))
                .padding([2, 4])
                .style(theme::btn_frameless(t))
                .on_press(Message::ToggleCompactMode),
        );
    }

    let inner = column![
        head,
        Space::new().height(Length::Fill),
        row![
            column![
                text(&app.clock_hhmm)
                    .size(38)
                    .font(theme::ui_font(t.design))
                    .color(theme::rgb(p.ink)),
                text(&app.clock_date)
                    .size(12)
                    .font(font)
                    .color(theme::rgb(p.ink_3)),
            ]
            .spacing(2),
            Space::new().width(Length::Fill),
            container(
                column![
                    row![
                        status_dot(
                            if app.focus_timer_running {
                                p.brass_hi
                            } else {
                                p.ink_3
                            },
                            6.5
                        ),
                        text("Focus")
                            .size(11.5)
                            .font(font)
                            .color(theme::rgb(p.ink_3)),
                        Space::new().width(6),
                        text(format!("{fm:02}:{fs:02}"))
                            .size(16)
                            .font(mono)
                            .color(theme::rgb(if app.focus_timer_running {
                                p.brass_hi
                            } else {
                                p.ink
                            })),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                    row![
                        button(
                            text(if app.focus_timer_running {
                                "Pause"
                            } else {
                                "Start"
                            })
                            .size(11)
                            .font(theme::ui_semibold(t.design)),
                        )
                        .padding([3, 10])
                        .style(theme::btn_primary(t))
                        .on_press(Message::ToggleFocusTimer),
                        button(text("15m").size(10.5).font(mono))
                            .padding([3, 7])
                            .style(theme::masthead_pill_btn(t, false))
                            .on_press(Message::SetFocusTimerMins(15)),
                        button(text("25m").size(10.5).font(mono))
                            .padding([3, 7])
                            .style(theme::masthead_pill_btn(t, false))
                            .on_press(Message::SetFocusTimerMins(25)),
                        button(text("Reset").size(10.5).font(font))
                            .padding([3, 7])
                            .style(theme::btn_ghost(t, false))
                            .on_press(Message::ResetFocusTimer),
                    ]
                    .spacing(4),
                ]
                .spacing(6),
            )
            .padding([8, 10])
            .style(theme::bento_stage_well_style(t)),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        Space::new().height(Length::Fill),
    ]
    .spacing(5)
    .height(Length::Fill);

    wrap_tile(app, tile, inner.into(), t, font)
}

fn render_lookbook_tile<'a>(
    app: &'a AirglowApp,
    tile: &'a OverviewTile,
    _span: u8,
    _tile_h: f32,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let active_eff = app.snapshot.effective_scene.effect;

    let mut harmony_row = row![].spacing(6);
    for (idx, h) in HARMONIES.iter().take(4).enumerate() {
        harmony_row = harmony_row.push(
            button(
                row![
                    color_dot(h.colors[0], 7.5),
                    color_dot(h.colors[1], 7.5),
                    text(truncate_str(h.name, 11)).size(11.5).font(font),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .padding([5, 9])
            .width(Length::Fill)
            .style(theme::bento_profile_card_btn(t, false))
            .on_press(Message::ApplyHarmony(idx)),
        );
    }

    let mut fx_row = row![].spacing(6);
    for eff in [
        EffectId::FireflySwarm,
        EffectId::Aurora,
        EffectId::MeteorShower,
        EffectId::Gradient,
        EffectId::Breathing,
    ] {
        let on = active_eff == eff;
        fx_row = fx_row.push(
            button(
                text(truncate_str(eff.label(), 11))
                    .size(11.5)
                    .font(if on { theme::ui_semibold(t.design) } else { font }),
            )
            .padding([5, 9])
            .width(Length::Fill)
            .style(theme::masthead_pill_btn(t, on))
            .on_press(Message::SelectEffect(eff)),
        );
    }

    let mut head = row![
        text("Lighting Lookbook")
            .size(14)
            .font(theme::ui_semibold(t.design))
            .color(theme::rgb(p.ink)),
        Space::new().width(Length::Fill),
    ]
    .align_y(Alignment::Center);

    if !app.edit_tiles_mode {
        head = head.push(
            button(text("Studio ↗").size(12).font(theme::ui_semibold(t.design)))
                .padding([2, 4])
                .style(theme::btn_frameless(t))
                .on_press(Message::SelectTab(crate::Tab::Studio)),
        );
    }

    let inner = column![
        head,
        Space::new().height(Length::Fill),
        harmony_row,
        fx_row,
        Space::new().height(Length::Fill),
    ]
    .spacing(7)
    .height(Length::Fill);

    wrap_tile(app, tile, inner.into(), t, font)
}

struct SensorTileData<'a> {
    title: String,
    lead_sub: String,
    trail_sub: String,
    right_meta: Option<String>,
    num: String,
    unit: String,
    fraction: f32,
    samples: &'a [f32],
    min_v: f32,
    max_v: f32,
    accent: u32,
    value_col: u32,
    default_auto_style: TileStyle,
}

fn format_custom_reading(val: f32, unit: &str) -> String {
    let abs = val.abs();
    let num = if abs >= 100.0 {
        format!("{val:.0}")
    } else if abs >= 10.0 {
        format!("{val:.1}")
    } else {
        format!("{val:.2}")
    };
    if unit.is_empty() {
        num
    } else {
        format!("{num} {unit}")
    }
}

fn sensor_quick_preview(app: &AirglowApp, kind: &OverviewTileKind) -> String {
    let s = &app.snapshot.sensors;
    let unit = app.snapshot.config.settings.temp_unit;
    match kind {
        OverviewTileKind::CpuTemp => sensors::format_temp(s.cpu_temp, unit),
        OverviewTileKind::GpuTemp => sensors::format_temp(s.gpu_temp, unit),
        OverviewTileKind::GpuHotspot => sensors::format_temp(s.gpu_hotspot, unit),
        OverviewTileKind::LiquidTemp => sensors::format_temp(s.liquid_temp, unit),
        OverviewTileKind::CpuLoad => format!("{:.0}%", s.cpu_load),
        OverviewTileKind::GpuLoad => format!("{:.0}%", s.gpu_load),
        OverviewTileKind::RamUsage => format!("{:.0}%", s.ram_load),
        OverviewTileKind::DriveTemp => sensors::format_temp(s.ssd_temp, unit),
        OverviewTileKind::PowerDraw => s
            .total_power_w
            .map(|w| format!("{w:.0} W"))
            .unwrap_or_else(|| "— W".to_string()),
        OverviewTileKind::CostEfficiency => {
            let cost_hr = (s.total_power_w.unwrap_or(0.0) / 1000.0)
                * app.snapshot.config.settings.energy_price_eur_kwh;
            format!("€{cost_hr:.3}/hr")
        }
        OverviewTileKind::NetDown => sensors::format_throughput(s.net_down_bps),
        OverviewTileKind::NetUp => sensors::format_throughput(s.net_up_bps),
        _ => "—".to_string(),
    }
}

fn temp_range_parts(samples: &[f32], unit: TempUnit) -> (String, String) {
    if samples.is_empty() {
        return ("Collecting…".to_string(), String::new());
    }
    let min = samples.iter().copied().fold(f32::INFINITY, f32::min);
    let max = samples.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let sfx = unit.suffix();
    (
        format!("{:.0}{sfx}–{:.0}{sfx}", unit.convert(min), unit.convert(max)),
        " over 10 min".to_string(),
    )
}

fn throughput_range_parts(samples: &[f32]) -> (String, String) {
    if samples.is_empty() {
        return ("Network Rx".to_string(), String::new());
    }
    let min = samples.iter().copied().fold(f32::INFINITY, f32::min);
    let max = samples.iter().copied().fold(0.0_f32, f32::max);
    let fmt_rate = |bps: f32| -> String {
        let (n, u) = split_throughput(Some(bps));
        format!("{n} {u}")
    };
    (
        format!("{}–{}", fmt_rate(min), fmt_rate(max)),
        " over 10 min".to_string(),
    )
}

fn extract_sensor_data<'a>(
    app: &'a AirglowApp,
    kind: &'a OverviewTileKind,
    t: DesignTokens,
) -> SensorTileData<'a> {
    let p = t.p;
    let s = &app.snapshot.sensors;
    let unit = app.snapshot.config.settings.temp_unit;

    match kind {
        OverviewTileKind::CpuTemp => {
            let (num, u) = split_temp(s.cpu_temp, unit);
            let (lead, trail) = temp_range_parts(&app.snapshot.cpu_temp_history, unit);
            SensorTileData {
                title: "Processor temperature".to_string(),
                lead_sub: lead,
                trail_sub: trail,
                right_meta: None,
                num,
                unit: u.to_string(),
                fraction: s
                    .cpu_temp
                    .map(|v| ((v - 25.0) / 70.0).clamp(0.0, 1.0))
                    .unwrap_or(0.0),
                samples: &app.snapshot.cpu_temp_history,
                min_v: 25.0,
                max_v: 95.0,
                accent: t.data_primary,
                value_col: thermal_temp_color(s.cpu_temp, t, p.ink),
                default_auto_style: TileStyle::Number,
            }
        }
        OverviewTileKind::GpuTemp => {
            let (num, u) = split_temp(s.gpu_temp, unit);
            let (lead, trail) = temp_range_parts(&app.snapshot.gpu_temp_history, unit);
            SensorTileData {
                title: "Graphics temperature".to_string(),
                lead_sub: lead,
                trail_sub: trail,
                right_meta: None,
                num,
                unit: u.to_string(),
                fraction: s
                    .gpu_temp
                    .map(|v| ((v - 25.0) / 70.0).clamp(0.0, 1.0))
                    .unwrap_or(0.0),
                samples: &app.snapshot.gpu_temp_history,
                min_v: 25.0,
                max_v: 95.0,
                accent: if t.is_firefly() {
                    t.data_secondary
                } else {
                    t.data_primary
                },
                value_col: thermal_temp_color(s.gpu_temp, t, p.ink),
                default_auto_style: TileStyle::Number,
            }
        }
        OverviewTileKind::GpuHotspot => {
            let (num, u) = split_temp(s.gpu_hotspot, unit);
            SensorTileData {
                title: "Graphics hotspot".to_string(),
                lead_sub: String::new(),
                trail_sub: short_hw_name(&s.gpu_name),
                right_meta: None,
                num,
                unit: u.to_string(),
                fraction: s
                    .gpu_hotspot
                    .map(|v| ((v - 30.0) / 75.0).clamp(0.0, 1.0))
                    .unwrap_or(0.0),
                samples: &app.snapshot.gpu_temp_history,
                min_v: 30.0,
                max_v: 105.0,
                accent: p.warm,
                value_col: thermal_temp_color(s.gpu_hotspot, t, p.ink),
                default_auto_style: TileStyle::Gauge,
            }
        }
        OverviewTileKind::LiquidTemp => {
            let (num, u) = split_temp(s.liquid_temp, unit);
            SensorTileData {
                title: "Liquid coolant".to_string(),
                lead_sub: String::new(),
                trail_sub: "20°C to 60°C".to_string(),
                right_meta: Some("AIO loop".to_string()),
                num,
                unit: u.to_string(),
                fraction: s
                    .liquid_temp
                    .map(|v| ((v - 20.0) / 40.0).clamp(0.0, 1.0))
                    .unwrap_or(0.0),
                samples: &app.snapshot.cpu_temp_history,
                min_v: 20.0,
                max_v: 60.0,
                accent: t.data_primary,
                value_col: p.ink,
                default_auto_style: TileStyle::Bar,
            }
        }
        OverviewTileKind::CpuLoad => {
            let w_str = s
                .cpu_power_w
                .map(|w| format!("{w:.0} W"))
                .unwrap_or_else(|| "— W".to_string());
            let t_str = sensors::format_temp(s.cpu_temp, unit);
            SensorTileData {
                title: "Processor load".to_string(),
                lead_sub: String::new(),
                trail_sub: short_hw_name(&s.cpu_name),
                right_meta: Some(format!("{w_str} · {t_str}")),
                num: format!("{:.0}", s.cpu_load),
                unit: "%".to_string(),
                fraction: (s.cpu_load / 100.0).clamp(0.0, 1.0),
                samples: &app.snapshot.cpu_temp_history,
                min_v: 0.0,
                max_v: 100.0,
                accent: if t.is_firefly() {
                    t.data_primary
                } else {
                    p.brass_hi
                },
                value_col: p.ink,
                default_auto_style: TileStyle::Gauge,
            }
        }
        OverviewTileKind::GpuLoad => {
            let w_str = s
                .gpu_power_w
                .map(|w| format!("{w:.0} W"))
                .unwrap_or_else(|| "— W".to_string());
            let t_str = sensors::format_temp(s.gpu_temp, unit);
            SensorTileData {
                title: "Graphics load".to_string(),
                lead_sub: String::new(),
                trail_sub: short_hw_name(&s.gpu_name),
                right_meta: Some(format!("{w_str} · {t_str}")),
                num: format!("{:.0}", s.gpu_load),
                unit: "%".to_string(),
                fraction: (s.gpu_load / 100.0).clamp(0.0, 1.0),
                samples: &app.snapshot.gpu_temp_history,
                min_v: 0.0,
                max_v: 100.0,
                accent: if t.is_firefly() {
                    t.data_secondary
                } else {
                    p.brass_hi
                },
                value_col: p.ink,
                default_auto_style: TileStyle::Gauge,
            }
        }
        OverviewTileKind::RamUsage => {
            let right_lbl = match (s.ram_used_gb, s.ram_total_gb) {
                (Some(u), Some(tot)) => format!("{u:.1} / {tot:.1} GB"),
                _ => format!("peak {:.0}%", s.ram_load),
            };
            let (used_lbl, avail_lbl) = match (s.ram_used_gb, s.ram_total_gb) {
                (Some(u), Some(tot)) => (
                    format!("Used {u:.1} GB"),
                    format!("Available {:.1} GB", (tot - u).max(0.0)),
                ),
                _ => ("Active memory".to_string(), "0% to 100%".to_string()),
            };
            SensorTileData {
                title: "Memory in use".to_string(),
                lead_sub: used_lbl,
                trail_sub: avail_lbl,
                right_meta: Some(right_lbl),
                num: format!("{:.0}", s.ram_load),
                unit: "%".to_string(),
                fraction: (s.ram_load / 100.0).clamp(0.0, 1.0),
                samples: &[],
                min_v: 0.0,
                max_v: 100.0,
                accent: t.pink_rose,
                value_col: p.ink,
                default_auto_style: TileStyle::Bar,
            }
        }
        OverviewTileKind::DriveTemp => {
            let (num, u) = split_temp(s.ssd_temp, unit);
            let (lead, trail) = temp_range_parts(&app.snapshot.ssd_temp_history, unit);
            SensorTileData {
                title: "Drive temperature".to_string(),
                lead_sub: lead,
                trail_sub: trail,
                right_meta: None,
                num,
                unit: u.to_string(),
                fraction: s
                    .ssd_temp
                    .map(|v| ((v - 20.0) / 55.0).clamp(0.0, 1.0))
                    .unwrap_or(0.0),
                samples: &app.snapshot.ssd_temp_history,
                min_v: 20.0,
                max_v: 75.0,
                accent: t.data_primary,
                value_col: thermal_temp_color(s.ssd_temp, t, p.ink),
                default_auto_style: TileStyle::Number,
            }
        }
        OverviewTileKind::PowerDraw => {
            let total_w = s.total_power_w.unwrap_or(0.0);
            let cpu_w = s
                .cpu_power_w
                .map(|v| format!("{v:.0} W"))
                .unwrap_or_else(|| "—".to_string());
            let gpu_w = s
                .gpu_power_w
                .map(|v| format!("{v:.0} W"))
                .unwrap_or_else(|| "—".to_string());
            SensorTileData {
                title: "CPU + GPU power".to_string(),
                lead_sub: format!("CPU {cpu_w}"),
                trail_sub: format!(" · GPU {gpu_w}"),
                right_meta: Some(format!("peak {:.0} W", app.snapshot.power_peak_w.max(total_w))),
                num: s
                    .total_power_w
                    .map(|v| format!("{v:.0}"))
                    .unwrap_or_else(|| "—".to_string()),
                unit: "W".to_string(),
                fraction: (total_w / 450.0).clamp(0.0, 1.0),
                samples: &app.snapshot.cpu_power_history,
                min_v: 0.0,
                max_v: 350.0,
                accent: p.brass_hi,
                value_col: p.ink,
                default_auto_style: TileStyle::Graph,
            }
        }
        OverviewTileKind::CostEfficiency => {
            let total_w = s.total_power_w.unwrap_or(0.0);
            let tariff = app.snapshot.config.settings.energy_price_eur_kwh;
            let cost_hr = (total_w / 1000.0) * tariff;
            let cost_mo = cost_hr * 8.0 * 30.0;
            SensorTileData {
                title: "Cost & efficiency".to_string(),
                lead_sub: format!("€{cost_mo:.1}/mo"),
                trail_sub: format!(" at €{tariff:.2}/kWh"),
                right_meta: Some(format!("{total_w:.0} W")),
                num: format!("€{cost_hr:.3}"),
                unit: "/hr".to_string(),
                fraction: (cost_hr / 0.30).clamp(0.0, 1.0),
                samples: &app.snapshot.cost_history,
                min_v: 0.0,
                max_v: 0.35,
                accent: p.brass,
                value_col: p.ink,
                default_auto_style: TileStyle::Number,
            }
        }
        OverviewTileKind::NetDown => {
            let (num, u) = split_throughput(s.net_down_bps);
            let (lead, trail) = throughput_range_parts(&app.snapshot.net_down_history);
            SensorTileData {
                title: "Download".to_string(),
                lead_sub: lead,
                trail_sub: trail,
                right_meta: None,
                num,
                unit: u.to_string(),
                fraction: (s.net_down_bps.unwrap_or(0.0) / (50.0 * 1024.0 * 1024.0))
                    .clamp(0.0, 1.0),
                samples: &app.snapshot.net_down_history,
                min_v: 0.0,
                max_v: 1024.0 * 1024.0,
                accent: p.ok,
                value_col: p.ink,
                default_auto_style: TileStyle::Number,
            }
        }
        OverviewTileKind::NetUp => {
            let (num, u) = split_throughput(s.net_up_bps);
            SensorTileData {
                title: "Upload".to_string(),
                lead_sub: String::new(),
                trail_sub: "Network Tx".to_string(),
                right_meta: None,
                num,
                unit: u.to_string(),
                fraction: (s.net_up_bps.unwrap_or(0.0) / (25.0 * 1024.0 * 1024.0))
                    .clamp(0.0, 1.0),
                samples: &[],
                min_v: 0.0,
                max_v: 1024.0 * 1024.0,
                accent: t.data_secondary,
                value_col: p.ink,
                default_auto_style: TileStyle::Bar,
            }
        }
        OverviewTileKind::CustomSensor {
            sensor_id,
            label,
            unit: unit_str,
        } => {
            let reading = s.readings.iter().find(|r| r.id == *sensor_id);
            let (num, sub, frac) = if let Some(r) = reading {
                let abs = r.value.abs();
                let n = if abs >= 100.0 {
                    format!("{:.0}", r.value)
                } else if abs >= 10.0 {
                    format!("{:.1}", r.value)
                } else {
                    format!("{:.2}", r.value)
                };
                let f = match r.unit.as_str() {
                    "%" => (r.value / 100.0).clamp(0.0, 1.0),
                    "°C" => ((r.value - 20.0) / 80.0).clamp(0.0, 1.0),
                    "RPM" => (r.value / 2500.0).clamp(0.0, 1.0),
                    "W" => (r.value / 350.0).clamp(0.0, 1.0),
                    _ => 0.5,
                };
                (n, truncate_str(&r.hardware, 26), f)
            } else {
                ("—".to_string(), "Waiting for sensor…".to_string(), 0.0)
            };
            SensorTileData {
                title: if label.trim().is_empty() {
                    sensor_id.clone()
                } else {
                    label.clone()
                },
                lead_sub: String::new(),
                trail_sub: sub,
                right_meta: Some("LHM".to_string()),
                num,
                unit: unit_str.clone(),
                fraction: frac,
                samples: &[],
                min_v: 0.0,
                max_v: 100.0,
                accent: p.brass_hi,
                value_col: p.ink,
                default_auto_style: TileStyle::Bar,
            }
        }
        _ => SensorTileData {
            title: kind.label().to_string(),
            lead_sub: String::new(),
            trail_sub: String::new(),
            right_meta: None,
            num: "—".to_string(),
            unit: String::new(),
            fraction: 0.0,
            samples: &[],
            min_v: 0.0,
            max_v: 100.0,
            accent: p.brass_hi,
            value_col: p.ink,
            default_auto_style: TileStyle::Number,
        },
    }
}

fn render_sensor_subtitle<'a>(
    lead: &str,
    trail: &str,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    if lead.is_empty() {
        text(truncate_str(trail, 30))
            .size(12.5)
            .font(font)
            .color(theme::rgb(p.ink_3))
            .into()
    } else {
        row![
            text(lead.to_string())
                .size(12.5)
                .font(theme::ui_semibold(t.design))
                .color(theme::rgb(p.ink_2)),
            text(trail.to_string())
                .size(12.5)
                .font(font)
                .color(theme::rgb(p.ink_3)),
        ]
        .spacing(0)
        .align_y(Alignment::Center)
        .into()
    }
}

fn render_sensor_tile<'a>(
    app: &'a AirglowApp,
    tile: &'a OverviewTile,
    span: u8,
    tile_h: f32,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let mono = theme::mono_font();
    let d = extract_sensor_data(app, &tile.kind, t);
    let accent = tile
        .accent
        .map(theme::rgb_arr_to_hex)
        .unwrap_or(d.accent);
    let effective_style = match tile.style {
        TileStyle::Auto => d.default_auto_style,
        other => other,
    };
    let spacious = tile_h >= 200.0;

    let title_el = text(truncate_str(&d.title, if span >= 2 { 36 } else { 24 }))
        .size(14.5)
        .font(theme::ui_semibold(t.design))
        .color(theme::rgb(p.ink));

    let body: Element<'a, Message> = match effective_style {
        TileStyle::Gauge => {
            let gauge_sz = (tile_h - 30.0).clamp(118.0, 155.0);
            let gauge = iced::widget::canvas(RadialGaugeCanvas {
                percent: d.fraction * 100.0,
                accent,
                palette: p,
                accent_override: t.accent_override,
                show_halo: false,
            })
            .width(Length::Fixed(gauge_sz))
            .height(Length::Fixed(gauge_sz));

            let num_sz = if spacious { 54.0 } else { 44.0 };
            let unit_sz = if spacious { 20.0 } else { 17.0 };

            let mut left_col = column![
                title_el,
                bento_hero_number(&d.num, &d.unit, num_sz, unit_sz, d.value_col, p.ink_3, t),
            ]
            .spacing(3)
            .width(Length::FillPortion(6))
            .height(Length::Fill);

            if !d.trail_sub.is_empty() {
                left_col = left_col.push(
                    container(
                        text(truncate_str(&d.trail_sub, 20))
                            .size(10.5)
                            .font(font)
                            .color(theme::rgb(p.ink_2)),
                    )
                    .padding([3, 8])
                    .style(theme::chip_style(t, None)),
                );
            }

            left_col = left_col.push(Space::new().height(Length::Fill));

            if let Some(sec) = &d.right_meta {
                left_col = left_col.push(
                    container(
                        text(sec.clone())
                            .size(11)
                            .font(mono)
                            .color(theme::rgb(accent)),
                    )
                    .padding([3, 8])
                    .style(theme::chip_style(t, Some(accent))),
                );
            } else if !d.lead_sub.is_empty() {
                left_col = left_col.push(render_sensor_subtitle(&d.lead_sub, "", t, font));
            }

            let right_col = container(gauge)
                .width(Length::FillPortion(6))
                .height(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center);

            row![left_col, right_col]
                .spacing(6)
                .height(Length::Fill)
                .align_y(Alignment::Center)
                .into()
        }
        TileStyle::Bar => {
            let num_sz = if spacious { 50.0 } else { 40.0 };
            let unit_sz = if spacious { 19.0 } else { 16.0 };

            let mut top_row = row![
                column![
                    title_el,
                    bento_hero_number(&d.num, &d.unit, num_sz, unit_sz, d.value_col, p.ink_3, t),
                ]
                .spacing(2),
                Space::new().width(Length::Fill),
            ]
            .align_y(Alignment::Start);

            if !app.edit_tiles_mode
                && let Some(rm) = &d.right_meta
            {
                top_row = top_row.push(
                    container(
                        text(rm.clone())
                            .size(11)
                            .font(mono)
                            .color(theme::rgb(accent)),
                    )
                    .padding([3, 9])
                    .style(theme::chip_style(t, Some(accent))),
                );
            }

            let bar = iced::widget::canvas(BarMeterCanvas {
                fraction: d.fraction,
                accent,
                palette: p,
                accent_override: t.accent_override,
                anim_t: app.anim_t,
                show_halo: false,
            })
            .width(Length::Fill)
            .height(Length::Fill);

            let footer_left = if d.lead_sub.is_empty() {
                String::new()
            } else {
                d.lead_sub.clone()
            };
            let footer_right = d.trail_sub.clone();
            let footer_row = row![
                text(truncate_str(&footer_left, 18))
                    .size(11.5)
                    .font(font)
                    .color(theme::rgb(p.ink_2)),
                Space::new().width(Length::Fill),
                text(truncate_str(&footer_right, 20))
                    .size(11.5)
                    .font(mono)
                    .color(theme::rgb(p.ink_3)),
            ]
            .align_y(Alignment::Center);

            column![top_row, bar, footer_row]
                .spacing(6)
                .height(Length::Fill)
                .into()
        }
        TileStyle::Graph => {
            let chart = iced::widget::canvas(SparklineCanvas {
                samples: d.samples,
                min_val: d.min_v,
                max_val: d.max_v,
                color: accent,
                palette: p,
                accent_override: t.accent_override,
                anim_t: app.anim_t,
                show_halo: false,
            })
            .width(Length::Fill)
            .height(Length::Fill);

            column![
                row![
                    title_el,
                    Space::new().width(Length::Fill),
                    if !app.edit_tiles_mode {
                        Element::from(
                            text(format!("{} {}", d.num, d.unit))
                                .size(15)
                                .font(theme::ui_semibold(t.design))
                                .color(theme::rgb(d.value_col)),
                        )
                    } else {
                        Space::new().width(0).into()
                    },
                ]
                .align_y(Alignment::Center),
                chart,
                render_sensor_subtitle(&d.lead_sub, &d.trail_sub, t, font),
            ]
            .spacing(5)
            .height(Length::Fill)
            .into()
        }
        TileStyle::Number | TileStyle::Auto => {
            let num_sz = if spacious { 52.0 } else { 42.0 };
            let unit_sz = if spacious { 20.0 } else { 17.0 };

            let badge_txt = if !d.lead_sub.is_empty() {
                Some(d.lead_sub.clone())
            } else {
                d.right_meta.clone()
            };

            let mut head_row = row![title_el, Space::new().width(Length::Fill)]
                .spacing(6)
                .align_y(Alignment::Center);

            if !app.edit_tiles_mode
                && let Some(badge) = badge_txt
            {
                head_row = head_row.push(
                    container(
                        text(truncate_str(&badge, 18))
                            .size(10.5)
                            .font(mono)
                            .color(theme::rgb(accent)),
                    )
                    .padding([2, 8])
                    .style(theme::chip_style(t, Some(accent))),
                );
            }

            let top_stack = column![
                head_row,
                bento_hero_number(&d.num, &d.unit, num_sz, unit_sz, d.value_col, p.ink_3, t),
            ]
            .spacing(3);

            if d.samples.len() >= 2 {
                let spark = iced::widget::canvas(SparklineCanvas {
                    samples: d.samples,
                    min_val: d.min_v,
                    max_val: d.max_v,
                    color: accent,
                    palette: p,
                    accent_override: t.accent_override,
                    anim_t: app.anim_t,
                    show_halo: false,
                })
                .width(Length::Fill)
                .height(Length::Fill);

                column![top_stack, spark]
                    .spacing(4)
                    .height(Length::Fill)
                    .into()
            } else {
                column![
                    top_stack,
                    render_sensor_subtitle(&d.lead_sub, &d.trail_sub, t, font),
                    Space::new().height(Length::Fill),
                ]
                .spacing(4)
                .height(Length::Fill)
                .into()
            }
        }
    };

    wrap_tile(app, tile, body, t, font)
}

pub fn view_overview<'a>(
    app: &'a AirglowApp,
    t: DesignTokens,
    font: iced::Font,
) -> Element<'a, Message> {
    let p = t.p;
    let grid_gap = 14.0_f32;
    let cols: usize = if app.window_size.width >= 960.0 { 4 } else { 2 };
    let row_h = compute_bento_row_height(app.window_size.height);

    let mut page = column![].spacing(14);
    if let Some(conflict_banner) = view_conflict_banner(app, t, font) {
        page = page.push(conflict_banner);
    }
    page = page.push(view_overview_masthead(app, t, font));

    if app.workspaces_bar_open || app.edit_tiles_mode {
        page = page.push(view_workspace_bar(app, t, font));
    }
    if app.add_tile_open {
        page = page.push(view_add_tile_catalog(app, t, font));
    }
    if app.customize_open {
        page = page.push(views_palette::view_customize_drawer(app, t, font));
    }

    let ws = app.snapshot.config.active_workspace();
    if ws.tiles.is_empty() {
        page = page.push(
            container(
                column![
                    text("This workspace has no tiles yet.")
                        .size(16)
                        .font(theme::ui_semibold(t.design)),
                    text("Click \"✦ Customize\" and \"+ Add tile\" above to pin Lighting, Profiles, or live sensors.")
                        .size(12.5)
                        .font(font)
                        .color(theme::rgb(p.ink_3)),
                    button(
                        text("+ Open Tile Catalog")
                            .size(12.5)
                            .font(theme::ui_semibold(t.design)),
                    )
                    .padding([7, 16])
                    .style(theme::btn_primary(t))
                    .on_press(Message::ToggleAddTileCatalog),
                ]
                .spacing(10)
                .align_x(Alignment::Center),
            )
            .padding(36)
            .width(Length::Fill)
            .center_x(Length::Fill)
            .style(theme::bento_tile_style(t, false)),
        );
    } else {
        let mut children = Vec::with_capacity(ws.tiles.len());
        let mut specs = Vec::with_capacity(ws.tiles.len());

        for tile in &ws.tiles {
            let eff_span = tile.span.clamp(1, cols as u8);
            let eff_rows = if tile.rows == 0 {
                if tile.tall { 2 } else { 1 }
            } else {
                tile.rows.clamp(1, 3)
            };
            let tile_h = eff_rows as f32 * row_h + (eff_rows.saturating_sub(1) as f32) * grid_gap;
            specs.push(BentoSpec::new(tile.id.clone(), eff_span, eff_rows));
            children.push(view_overview_tile(app, tile, eff_span, tile_h, t, font));
        }

        let tile_radius = if p.square_corners { 0.0 } else { 16.0 };
        let accent_col = theme::rgb(if p.is_light { p.brass } else { p.brass_hi });

        let bento = BentoGrid::new(
            children,
            specs,
            cols,
            row_h,
            grid_gap,
            app.edit_tiles_mode,
            accent_col,
            p.is_light,
            tile_radius,
            Message::ReorderOverviewTile,
            Message::ResizeOverviewTile,
        );
        page = page.push(bento);
    }

    page.into()
}

/// Cinematic full-viewport Theater Rig Mode (`Shift+F11` or button on Living Rig tile).
pub fn view_theater_rig<'a>(
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

    let active_effect_title = if eff.effect == EffectId::Studio {
        eff.studio_stack.name.clone()
    } else {
        eff.effect.label().to_string()
    };

    let hud_pill = |label: &'static str, val: String, col: u32| {
        container(
            row![
                status_dot(col, 7.0),
                text(label).size(11).font(font).color(theme::rgb(p.ink_3)),
                text(val)
                    .size(16)
                    .font(mono)
                    .color(theme::rgb(col)),
            ]
            .spacing(7)
            .align_y(Alignment::Center),
        )
        .padding([7, 14])
        .style(theme::ambient_pill_style(t))
    };

    let power_str = s
        .total_power_w
        .map(|w| format!("{w:.0} W"))
        .unwrap_or_else(|| "— W".to_string());

    let top_hud = row![
        row![
            image(&app.moth_art)
                .content_fit(ContentFit::Contain)
                .width(Length::Fixed(34.0))
                .height(Length::Fixed(32.0)),
            column![
                text("THEATER RIG")
                    .size(10)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(p.brass_hi)),
                text(active_effect_title)
                    .size(18)
                    .font(theme::display_semibold(t.design))
                    .color(theme::rgb(p.ink)),
            ]
            .spacing(1),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        Space::new().width(Length::Fill),
        hud_pill(
            "CPU",
            sensors::format_temp(s.cpu_temp, unit),
            thermal_temp_color(s.cpu_temp, t, t.data_primary),
        ),
        hud_pill(
            "GPU",
            sensors::format_temp(s.gpu_temp, unit),
            thermal_temp_color(s.gpu_temp, t, t.data_secondary),
        ),
        hud_pill("POWER", power_str, p.brass_hi),
        hud_pill("FANS", format!("{}%", app.snapshot.fan_duty), p.ok),
        Space::new().width(Length::Fill),
        button(
            text(if app.is_fullscreen {
                "Exit F11"
            } else {
                "⛶ Fullscreen"
            })
            .size(11.5)
            .font(font),
        )
        .padding([6, 12])
        .style(theme::btn_secondary(t, app.is_fullscreen))
        .on_press(Message::ToggleFullscreen),
        button(
            text("✕ Exit Theater")
                .size(11.5)
                .font(theme::ui_semibold(t.design)),
        )
        .padding([6, 14])
        .style(theme::btn_primary(t))
        .on_press(Message::ToggleTheaterRig),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

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
        eyebrow: Some("AIRGLOW THEATER"),
        animate_lighting: true,
    })
    .width(Length::Fill)
    .height(Length::Fill);
    let stage_el = Element::from(stage).map(Message::Stage);

    let stage_frame = container(stage_el)
        .width(Length::Fill)
        .height(Length::Fill)
        .clip(true)
        .style(theme::hero_panel_style(t));

    // Bottom floating dock: Brightness + Power + Profiles + Fan curves
    let mut profile_row = row![].spacing(6).align_y(Alignment::Center);
    for preset in app.snapshot.config.presets.iter().take(5) {
        let active = app.snapshot.config.active_preset.as_deref() == Some(&preset.id);
        let dot_col = preset_dot_color(preset.effect, preset.params.color);
        profile_row = profile_row.push(
            button(
                row![
                    color_dot(dot_col, 8.0),
                    text(truncate_str(&preset.name, 14))
                        .size(11.5)
                        .font(if active {
                            theme::ui_semibold(t.design)
                        } else {
                            font
                        }),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding([6, 11])
            .style(theme::card_button(t, active))
            .on_press(Message::ApplyPreset(preset.id.clone())),
        );
    }

    let mut curve_seg = row![].spacing(2);
    for (id, lbl) in [
        ("silent", "Silent"),
        ("balanced", "Balanced"),
        ("performance", "Perf"),
    ] {
        let active = app.snapshot.effective_curve == id;
        curve_seg = curve_seg.push(
            button(text(lbl).size(11).font(font))
                .padding([5, 10])
                .style(theme::segmented_item_btn(t, active))
                .on_press(Message::SelectFanCurve(id.to_string())),
        );
    }

    let bottom_dock = container(
        row![
            pill_toggle(t, eff.on, Message::TogglePower),
            text("Brightness").size(11.5).font(font).color(theme::rgb(p.ink_3)),
            slider(
                0.0..=100.0,
                app.snapshot.config.scene.brightness as f32,
                |v| Message::SetBrightness(v.round() as u8)
            )
            .style(theme::slider_style(t))
            .width(Length::Fixed(160.0)),
            text(format!("{}%", app.snapshot.config.scene.brightness))
                .size(12)
                .font(mono)
                .color(theme::rgb(p.ink_2))
                .width(Length::Fixed(36.0)),
            Space::new().width(Length::Fill),
            profile_row,
            Space::new().width(Length::Fill),
            container(curve_seg)
                .padding(2)
                .style(theme::segmented_rail_style(t)),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding([10, 16])
    .width(Length::Fill)
    .style(theme::panel_style(t));

    container(
        column![top_hud, stage_frame, bottom_dock]
            .spacing(14)
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .padding([16, 22])
    .width(Length::Fill)
    .height(Length::Fill)
    .style(theme::window_ground_style(t))
    .into()
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
        animate_lighting: true,
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

/// Full-height widescreen Studio Layer Mode workbench (`Length::Fill` width & height,
/// 3 fluid columns: Layers & Saved Effects | Full-Height Live Rig + 24-Band EQ + Harmonies | Inspector).
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
                .padding([5, 10])
                .style(theme::segmented_pill_style(t, active))
                .on_press(Message::SetStudioSpeedMult(mult)),
        );
    }

    let undo_btn = if can_undo {
        button(
            row![
                icon_canvas(UiIcon::Reset, theme::rgb(p.ink_2), 12.5),
                text("Undo").size(12).font(font),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
        )
        .padding([6, 12])
        .style(theme::btn_secondary(t, false))
        .on_press(Message::StudioUndo)
    } else {
        button(
            row![
                icon_canvas(UiIcon::Reset, theme::rgb(p.ink_3), 12.5),
                text("Undo").size(12).font(font).color(theme::rgb(p.ink_3)),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
        )
        .padding([6, 12])
        .style(theme::btn_secondary(t, false))
    };

    let redo_btn = if can_redo {
        button(text("Redo").size(12).font(font))
            .padding([6, 12])
            .style(theme::btn_secondary(t, false))
            .on_press(Message::StudioRedo)
    } else {
        button(text("Redo").size(12).font(font).color(theme::rgb(p.ink_3)))
            .padding([6, 12])
            .style(theme::btn_secondary(t, false))
    };

    let top_bar = container(
        row![
            row![
                icon_canvas(UiIcon::Studio, theme::rgb(p.brass_hi), 18.0),
                text(format!("Studio · {}", stack_cfg.name))
                    .size(19)
                    .font(theme::display_semibold(t.design))
                    .color(theme::rgb(p.ink)),
            ]
            .spacing(9)
            .align_y(Alignment::Center),
            Space::new().width(8),
            button(
                row![
                    icon_canvas(UiIcon::Sparkle, theme::rgb(p.brass_hi), 13.0),
                    text("Surprise me")
                        .size(12)
                        .font(theme::ui_semibold(t.design)),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding([6, 13])
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
                .padding(2)
                .style(theme::segmented_track_style(t)),
            button(
                row![
                    icon_canvas(
                        if is_live {
                            UiIcon::Check
                        } else {
                            UiIcon::Lighting
                        },
                        theme::rgb(t.primary_ink),
                        13.5,
                    ),
                    text(if is_live {
                        "Live on lights"
                    } else {
                        "Use on my lights"
                    })
                    .size(12)
                    .font(theme::ui_semibold(t.design)),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding([7, 15])
            .style(theme::btn_primary(t))
            .on_press(Message::SelectEffect(EffectId::Studio)),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([11, 16])
    .width(Length::Fill)
    .style(theme::rich_instrument_card_style(t, Some(p.brass), false));

    // Left column: Layer stack + Add Layer + Saved Studio Library (fills 100% height)
    let mut layer_cards = column![].spacing(6);

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
                    .padding([8, 9])
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
                    .style(theme::btn_danger(t))
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
                    container(text(format!("+ {}", kind.label())).size(10.5).font(font))
                        .center_x(Length::Fill),
                )
                .padding([6, 6])
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

    let layers_card = container(
        column![
            row![
                text("LAYERS (TOP DRAWS OVER BOTTOM)")
                    .size(11)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(p.ink_3)),
                Space::new().width(Length::Fill),
                container(
                    text(format!("{} active", stack_cfg.layers.len()))
                        .size(10.5)
                        .font(theme::mono_font())
                        .color(theme::rgb(p.brass_hi)),
                )
                .padding([2, 8])
                .style(theme::chip_style(t, Some(p.brass))),
            ]
            .align_y(Alignment::Center),
            scrollable(layer_cards).height(Length::Fill),
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
        .spacing(6)
        .height(Length::Fill),
    )
    .padding(t.pad(14))
    .width(Length::Fill)
    .height(Length::FillPortion(6))
    .style(theme::rich_instrument_card_style(t, Some(t.data_secondary), false));

    let mut saved_list = column![].spacing(6);
    for saved in &app.snapshot.config.studio_effects {
        let active = app.snapshot.config.scene.active_studio_effect.as_deref()
            == Some(saved.id.as_str());
        let dot_rgb = saved
            .stack
            .layers
            .first()
            .map(|l| l.color)
            .unwrap_or([165, 222, 206]);
        saved_list = saved_list.push(
            row![
                button(
                    row![
                        color_dot(dot_rgb, 9.0),
                        text(&saved.name).size(12).font(if active {
                            theme::ui_semibold(t.design)
                        } else {
                            font
                        }),
                        Space::new().width(Length::Fill),
                        text(format!("{}L", saved.stack.layers.len()))
                            .size(10.5)
                            .font(theme::mono_font())
                            .color(theme::rgb(p.ink_3)),
                    ]
                    .spacing(7)
                    .align_y(Alignment::Center),
                )
                .padding([7, 10])
                .width(Length::Fill)
                .style(theme::card_button(t, active))
                .on_press(Message::LoadStudioEffect(saved.id.clone())),
                button(text("Dup").size(10.5).font(font))
                    .padding([7, 9])
                    .style(theme::btn_secondary(t, false))
                    .on_press(Message::DuplicateStudioEffect(saved.id.clone())),
                button(text("×").size(10.5).font(font))
                    .padding([7, 9])
                    .style(theme::btn_danger(t))
                    .on_press(Message::DeleteStudioEffect(saved.id.clone())),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
        );
    }

    let library_card = container(
        column![
            text("SAVED STUDIO EFFECTS")
                .size(11)
                .font(theme::ui_semibold(t.design))
                .color(theme::rgb(p.ink_3)),
            row![
                text_input("Effect name", &app.studio_save_name)
                    .style(theme::text_input_style(t))
                    .on_input(Message::StudioSaveNameChanged)
                    .width(Length::Fill),
                button(
                    row![
                        icon_canvas(UiIcon::Plus, theme::rgb(t.primary_ink), 12.0),
                        text("Save")
                            .size(11.5)
                            .font(theme::ui_semibold(t.design)),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding([7, 13])
                .style(theme::btn_primary(t))
                .on_press(Message::SaveStudioEffect),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
            scrollable(saved_list).height(Length::Fill),
        ]
        .spacing(8)
        .height(Length::Fill),
    )
    .padding(t.pad(14))
    .width(Length::Fill)
    .height(Length::FillPortion(4))
    .style(theme::rich_instrument_card_style(t, Some(t.pink_rose), false));

    let left_panel = column![layers_card, library_card]
        .spacing(t.sp(12))
        .width(Length::FillPortion(4))
        .height(Length::Fill);

    // Center: Full-Height Real-Rig Live Studio Preview + 24-Band EQ + Colour Harmonies
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
        eyebrow: Some("STUDIO LIVE RIG"),
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
    .height(Length::Fixed(58.0));

    let mut studio_harmonies = column![].spacing(6);
    for (row_idx, chunk) in HARMONIES.chunks(2).enumerate() {
        let mut r = row![].spacing(6);
        for (col_idx, h) in chunk.iter().enumerate() {
            let idx = row_idx * 2 + col_idx;
            r = r.push(
                button(
                    row![
                        color_dot(h.colors[0], 8.0),
                        color_dot(h.colors[1], 8.0),
                        color_dot(h.colors[2], 8.0),
                        text(h.name).size(11.5).font(font),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding([6, 10])
                .width(Length::Fill)
                .style(theme::card_button(t, false))
                .on_press(Message::ApplyHarmony(idx)),
            );
        }
        studio_harmonies = studio_harmonies.push(r);
    }

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
                container(
                    text(if is_live {
                        "Live on rig"
                    } else if app.snapshot.studio_test_beat {
                        "Test beat active"
                    } else {
                        "Real-rig preview"
                    })
                    .size(11)
                    .font(theme::ui_semibold(t.design))
                    .color(theme::rgb(if is_live || app.snapshot.studio_test_beat {
                        t.pink_rose
                    } else {
                        t.data_primary
                    })),
                )
                .padding([3, 9])
                .style(theme::chip_style(
                    t,
                    Some(if is_live || app.snapshot.studio_test_beat {
                        t.pink_rose
                    } else {
                        t.data_primary
                    }),
                )),
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
            studio_harmonies,
        ]
        .spacing(9)
        .height(Length::Fill),
    )
    .padding(t.pad(15))
    .width(Length::FillPortion(5))
    .height(Length::Fill)
    .style(theme::rich_instrument_card_style(t, Some(t.data_primary), false));

    // Right: Layer inspector (scrollable inside full-height container)
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
            .width(Length::Fixed(122.0))
            .height(Length::Fixed(122.0)),
        )
        .map(|msg| match msg {
            ColorWheelMessage::PickRgb(rgb) => Message::StudioColorPicked(rgb),
        });

        let mut swatches_primary_a = row![].spacing(6);
        for col in CURATED_SWATCHES.iter().take(6) {
            let mut next = layer.clone();
            next.color = *col;
            swatches_primary_a = swatches_primary_a.push(
                button(Space::new().width(16).height(16))
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
                button(Space::new().width(16).height(16))
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
                button(Space::new().width(16).height(16))
                    .padding(0)
                    .width(Length::Fixed(20.0))
                    .height(Length::Fixed(20.0))
                    .style(theme::swatch_btn(*col, layer.secondary == *col, t))
                    .on_press(Message::UpdateSelectedLayer(next)),
            );
        }

        let mut rev_next = layer.clone();
        rev_next.reverse = !rev_next.reverse;

        scrollable(
            column![
                row![
                    text(format!("LAYER · {}", layer.kind.label().to_uppercase()))
                        .size(12)
                        .font(theme::ui_semibold(t.design))
                        .color(theme::rgb(p.brass_hi)),
                    Space::new().width(Length::Fill),
                    color_dot(layer.color, 10.0),
                ]
                .align_y(Alignment::Center),
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
                            color_dot(layer.color, 13.0),
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
            .spacing(10),
        )
        .height(Length::Fill)
        .into()
    } else {
        container(
            text("Add a layer on the left or press Surprise me to start composing.")
                .size(13)
                .font(font)
                .color(theme::rgb(p.ink_3)),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    };

    let right_panel = container(inspector_body)
        .padding(t.pad(15))
        .width(Length::FillPortion(4))
        .height(Length::Fill)
        .style(theme::rich_instrument_card_style(t, Some(p.brass), false));

    container(
        column![
            top_bar,
            row![left_panel, center_panel, right_panel]
                .spacing(t.sp(14))
                .height(Length::Fill),
        ]
        .spacing(t.sp(12))
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .padding([16, 26])
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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
        show_halo: true,
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
        (format!("{:.1}", v / (1024.0 * 1024.0)), "MiB/s")
    } else if v >= 1024.0 {
        let kib = v / 1024.0;
        if kib >= 100.0 {
            (format!("{kib:.0}"), "KiB/s")
        } else {
            (format!("{kib:.1}"), "KiB/s")
        }
    } else {
        (format!("{v:.0}"), "B/s")
    }
}

#[allow(dead_code)]
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

#[cfg(test)]
mod tests {
    use super::compute_bento_row_height;
    use crate::bento_grid::{BentoSpec, pack_bento_2d};
    use airglow_core::config::OverviewWorkspace;

    #[test]
    fn default_workspace_packs_into_exact_4x4_bento() {
        let ws = OverviewWorkspace::default_workspace();
        let specs: Vec<BentoSpec> = ws
            .tiles
            .iter()
            .map(|t| BentoSpec::new(t.id.clone(), t.span, t.rows.max(1)))
            .collect();
        let packed = pack_bento_2d(&specs, 4, 1384.0, 178.0, 14.0);
        assert_eq!(packed.total_rows, 4);

        let packed_2col = pack_bento_2d(&specs, 2, 720.0, 168.0, 14.0);
        assert_eq!(packed_2col.total_rows, 8);
    }

    #[test]
    fn bento_row_heights_scale_smoothly_across_resolutions() {
        let h_900 = compute_bento_row_height(900.0);
        let h_1080 = compute_bento_row_height(1040.0);
        let h_1152 = compute_bento_row_height(1152.0);
        let h_1440 = compute_bento_row_height(1370.0);
        assert!(h_900 >= 150.0 && h_900 <= 290.0);
        assert!(h_1080 > h_900);
        assert!(h_1152 > h_1080);
        assert!(h_1440 > h_1152 && h_1440 <= 290.0);
    }
}
