//! Native Windows system tray integration powered by Tauri's `tray-icon` + `muda`.
//! Exposes Open, Compact Mini-Mode, Lights On, Overview Static Canvas toggle, Brightness presets,
//! Profiles/Effects/Fan Curves submenus with active checkmarks, Quit, live-tinted icon, and °C/°F tooltip.

use airglow_core::effects::EffectId;
use muda::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::brand;

const BRIGHTNESS_LEVELS: [u8; 5] = [100, 75, 50, 25, 10];

#[derive(Debug, Clone)]
pub enum TrayAction {
    OpenWindow,
    ToggleCompactMode,
    ToggleLights,
    ToggleOverviewStageAnimation,
    SetBrightness(u8),
    ApplyPreset(String),
    SelectEffect(EffectId),
    SelectFanCurve(String),
    Quit,
}

#[derive(Debug, Clone)]
pub struct TrayMenuState<'a> {
    pub lights_on: bool,
    pub brightness: u8,
    pub active_preset: Option<&'a str>,
    pub active_effect: EffectId,
    pub active_curve: &'a str,
    pub overview_stage_animated: bool,
}

impl Default for TrayMenuState<'static> {
    fn default() -> Self {
        Self {
            lights_on: true,
            brightness: 100,
            active_preset: None,
            active_effect: EffectId::Gradient,
            active_curve: "balanced",
            overview_stage_animated: true,
        }
    }
}

fn nearest_brightness_preset(b: u8) -> u8 {
    BRIGHTNESS_LEVELS
        .iter()
        .copied()
        .min_by_key(|&lvl| (lvl as i16 - b as i16).abs())
        .unwrap_or(100)
}

struct MenuBindings {
    open_id: MenuId,
    compact_id: MenuId,
    lights_id: MenuId,
    lights_item: CheckMenuItem,
    overview_static_id: MenuId,
    overview_static_item: CheckMenuItem,
    quit_id: MenuId,
    brightness_items: Vec<(MenuId, u8, CheckMenuItem)>,
    preset_items: Vec<(MenuId, String, CheckMenuItem)>,
    effect_items: Vec<(MenuId, EffectId, CheckMenuItem)>,
    curve_items: Vec<(MenuId, String, CheckMenuItem)>,
}

pub struct TrayController {
    tray: TrayIcon,
    bindings: MenuBindings,
    presets_snapshot: Vec<(String, String)>,
    last_tint_key: (u8, u8, u8),
    last_tooltip: String,
    last_lights_on: bool,
    last_overview_static: bool,
    last_brightness_preset: u8,
    last_preset: Option<String>,
    last_effect: EffectId,
    last_curve: String,
}

fn build_menu(presets: &[(String, String)], state: &TrayMenuState<'_>) -> (Menu, MenuBindings) {
    let menu = Menu::new();
    let open_item = MenuItem::new("Open Airglow", true, None);
    let compact_item = MenuItem::new("Compact Mini-Mode", true, None);
    let lights_item = CheckMenuItem::new("Lights On", true, state.lights_on, None);
    let overview_static = !state.overview_stage_animated;
    let overview_static_item =
        CheckMenuItem::new("Overview Static Canvas (Low GPU)", true, overview_static, None);
    let quit_item = MenuItem::new("Quit Airglow", true, None);

    let open_id = open_item.id().clone();
    let compact_id = compact_item.id().clone();
    let lights_id = lights_item.id().clone();
    let overview_static_id = overview_static_item.id().clone();
    let quit_id = quit_item.id().clone();

    let _ = menu.append(&open_item);
    let _ = menu.append(&compact_item);
    let _ = menu.append(&lights_item);
    let _ = menu.append(&overview_static_item);
    let _ = menu.append(&PredefinedMenuItem::separator());

    // Brightness submenu
    let bright_sub = Submenu::new("Brightness", true);
    let active_b = nearest_brightness_preset(state.brightness);
    let mut brightness_items = Vec::with_capacity(BRIGHTNESS_LEVELS.len());
    for lvl in BRIGHTNESS_LEVELS {
        let item = CheckMenuItem::new(format!("{lvl}%"), true, lvl == active_b, None);
        brightness_items.push((item.id().clone(), lvl, item.clone()));
        let _ = bright_sub.append(&item);
    }
    let _ = menu.append(&bright_sub);

    // Profiles submenu
    let profiles_sub = Submenu::new("Profiles", true);
    let mut preset_items = Vec::with_capacity(presets.len());
    for (pid, name) in presets {
        let checked = state.active_preset == Some(pid.as_str());
        let item = CheckMenuItem::new(name, true, checked, None);
        preset_items.push((item.id().clone(), pid.clone(), item.clone()));
        let _ = profiles_sub.append(&item);
    }
    let _ = menu.append(&profiles_sub);

    // Built-in Effects submenu
    let effects_sub = Submenu::new("Effects", true);
    let mut effect_items = Vec::with_capacity(EffectId::ALL.len());
    for eff in EffectId::ALL {
        let item = CheckMenuItem::new(eff.label(), true, state.active_effect == eff, None);
        effect_items.push((item.id().clone(), eff, item.clone()));
        let _ = effects_sub.append(&item);
    }
    let _ = menu.append(&effects_sub);

    // Fan Curves submenu
    let curves_sub = Submenu::new("Fan Curves", true);
    let mut curve_items = Vec::with_capacity(3);
    for (id, label) in [
        ("silent", "Silent"),
        ("balanced", "Balanced"),
        ("performance", "Performance"),
    ] {
        let item = CheckMenuItem::new(label, true, state.active_curve == id, None);
        curve_items.push((item.id().clone(), id.to_string(), item.clone()));
        let _ = curves_sub.append(&item);
    }
    let _ = menu.append(&curves_sub);

    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&quit_item);

    (
        menu,
        MenuBindings {
            open_id,
            compact_id,
            lights_id,
            lights_item,
            overview_static_id,
            overview_static_item,
            quit_id,
            brightness_items,
            preset_items,
            effect_items,
            curve_items,
        },
    )
}

impl TrayController {
    pub fn new(presets: &[(String, String)], state: &TrayMenuState<'_>) -> Option<Self> {
        let (menu, bindings) = build_menu(presets, state);
        let rgba = brand::tray_mark_rgba(32, [0, 155, 222]);
        let icon = Icon::from_rgba(rgba, 32, 32).ok()?;
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Airglow")
            .with_icon(icon)
            .build()
            .ok()?;

        Some(Self {
            tray,
            bindings,
            presets_snapshot: presets.to_vec(),
            last_tint_key: (0, 0, 0),
            last_tooltip: "Airglow".to_string(),
            last_lights_on: state.lights_on,
            last_overview_static: !state.overview_stage_animated,
            last_brightness_preset: nearest_brightness_preset(state.brightness),
            last_preset: state.active_preset.map(str::to_string),
            last_effect: state.active_effect,
            last_curve: state.active_curve.to_string(),
        })
    }

    pub fn sync_state(&mut self, presets: &[(String, String)], state: &TrayMenuState<'_>) {
        let next_overview_static = !state.overview_stage_animated;
        let next_bright = nearest_brightness_preset(state.brightness);

        if self.presets_snapshot != presets {
            self.presets_snapshot = presets.to_vec();
            let (menu, bindings) = build_menu(presets, state);
            self.bindings = bindings;
            self.tray.set_menu(Some(Box::new(menu)));
            self.last_lights_on = state.lights_on;
            self.last_overview_static = next_overview_static;
            self.last_brightness_preset = next_bright;
            self.last_preset = state.active_preset.map(str::to_string);
            self.last_effect = state.active_effect;
            self.last_curve = state.active_curve.to_string();
            return;
        }

        if self.last_lights_on != state.lights_on {
            self.last_lights_on = state.lights_on;
            self.bindings.lights_item.set_checked(state.lights_on);
        }
        if self.last_overview_static != next_overview_static {
            self.last_overview_static = next_overview_static;
            self.bindings
                .overview_static_item
                .set_checked(next_overview_static);
        }
        if self.last_brightness_preset != next_bright {
            self.last_brightness_preset = next_bright;
            for (_, lvl, item) in &self.bindings.brightness_items {
                item.set_checked(*lvl == next_bright);
            }
        }
        if self.last_preset.as_deref() != state.active_preset {
            self.last_preset = state.active_preset.map(str::to_string);
            for (_, pid, item) in &self.bindings.preset_items {
                item.set_checked(self.last_preset.as_deref() == Some(pid.as_str()));
            }
        }
        if self.last_effect != state.active_effect {
            self.last_effect = state.active_effect;
            for (_, eff, item) in &self.bindings.effect_items {
                item.set_checked(*eff == state.active_effect);
            }
        }
        if self.last_curve != state.active_curve {
            self.last_curve = state.active_curve.to_string();
            for (_, cid, item) in &self.bindings.curve_items {
                item.set_checked(cid == state.active_curve);
            }
        }
    }

    pub fn poll_actions(&self) -> Vec<TrayAction> {
        let mut actions = Vec::new();
        while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
            if matches!(
                ev,
                TrayIconEvent::Click {
                    button: tray_icon::MouseButton::Left,
                    button_state: tray_icon::MouseButtonState::Up,
                    ..
                } | TrayIconEvent::DoubleClick { .. }
            ) {
                actions.push(TrayAction::OpenWindow);
            }
        }
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            if ev.id == self.bindings.open_id {
                actions.push(TrayAction::OpenWindow);
            } else if ev.id == self.bindings.compact_id {
                actions.push(TrayAction::ToggleCompactMode);
            } else if ev.id == self.bindings.lights_id {
                actions.push(TrayAction::ToggleLights);
            } else if ev.id == self.bindings.overview_static_id {
                actions.push(TrayAction::ToggleOverviewStageAnimation);
            } else if ev.id == self.bindings.quit_id {
                actions.push(TrayAction::Quit);
            } else if let Some((_, lvl, _)) = self
                .bindings
                .brightness_items
                .iter()
                .find(|(id, _, _)| *id == ev.id)
            {
                actions.push(TrayAction::SetBrightness(*lvl));
            } else if let Some((_, pid, _)) = self
                .bindings
                .preset_items
                .iter()
                .find(|(id, _, _)| *id == ev.id)
            {
                actions.push(TrayAction::ApplyPreset(pid.clone()));
            } else if let Some((_, eff, _)) = self
                .bindings
                .effect_items
                .iter()
                .find(|(id, _, _)| *id == ev.id)
            {
                actions.push(TrayAction::SelectEffect(*eff));
            } else if let Some((_, cid, _)) = self
                .bindings
                .curve_items
                .iter()
                .find(|(id, _, _)| *id == ev.id)
            {
                actions.push(TrayAction::SelectFanCurve(cid.clone()));
            }
        }
        actions
    }

    pub fn update_status(&mut self, avg_rgb: [u8; 3], lights_on: bool, tooltip: &str) {
        let target = if lights_on { avg_rgb } else { [80, 84, 90] };
        let key = (target[0] / 12, target[1] / 12, target[2] / 12);
        if key != self.last_tint_key {
            self.last_tint_key = key;
            let rgba = brand::tray_mark_rgba(32, target);
            if let Ok(icon) = Icon::from_rgba(rgba, 32, 32) {
                let _ = self.tray.set_icon(Some(icon));
            }
        }
        if tooltip != self.last_tooltip {
            self.last_tooltip = tooltip.to_string();
            let _ = self.tray.set_tooltip(Some(tooltip));
        }
    }
}
