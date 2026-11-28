//! Native Windows system tray integration powered by Tauri's `tray-icon` + `muda`.
//! Exposes Open, Lights On/Off, Profiles submenu, Built-in Effects submenu, Fan Curves submenu,
//! Quit, dynamic live-tinted Airglow icon, and °C/°F telemetry tooltip.

use airglow_core::effects::EffectId;
use muda::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::brand;

#[derive(Debug, Clone)]
pub enum TrayAction {
    OpenWindow,
    ToggleCompactMode,
    ToggleLights,
    ApplyPreset(String),
    SelectEffect(EffectId),
    SelectFanCurve(String),
    Quit,
}

struct MenuBindings {
    open_id: MenuId,
    compact_id: MenuId,
    toggle_id: MenuId,
    quit_id: MenuId,
    preset_ids: Vec<(MenuId, String)>,
    effect_ids: Vec<(MenuId, EffectId)>,
    curve_ids: Vec<(MenuId, String)>,
}

pub struct TrayController {
    tray: TrayIcon,
    bindings: MenuBindings,
    presets_snapshot: Vec<(String, String)>,
    last_tint_key: (u8, u8, u8),
    last_tooltip: String,
}

fn build_menu(presets: &[(String, String)]) -> (Menu, MenuBindings) {
    let menu = Menu::new();
    let open_item = MenuItem::new("Open Airglow", true, None);
    let compact_item = MenuItem::new("Compact Mini-Mode", true, None);
    let toggle_item = MenuItem::new("Lights On / Off", true, None);
    let quit_item = MenuItem::new("Quit Airglow", true, None);

    let open_id = open_item.id().clone();
    let compact_id = compact_item.id().clone();
    let toggle_id = toggle_item.id().clone();
    let quit_id = quit_item.id().clone();

    let _ = menu.append(&open_item);
    let _ = menu.append(&compact_item);
    let _ = menu.append(&toggle_item);
    let _ = menu.append(&PredefinedMenuItem::separator());

    // Profiles submenu
    let profiles_sub = Submenu::new("Profiles", true);
    let mut preset_ids = Vec::new();
    for (pid, name) in presets {
        let item = MenuItem::new(name, true, None);
        preset_ids.push((item.id().clone(), pid.clone()));
        let _ = profiles_sub.append(&item);
    }
    let _ = menu.append(&profiles_sub);

    // Built-in Effects submenu
    let effects_sub = Submenu::new("Effects", true);
    let mut effect_ids = Vec::new();
    for eff in EffectId::ALL {
        let item = MenuItem::new(eff.label(), true, None);
        effect_ids.push((item.id().clone(), eff));
        let _ = effects_sub.append(&item);
    }
    let _ = menu.append(&effects_sub);

    // Fan Curves submenu
    let curves_sub = Submenu::new("Fan Curves", true);
    let mut curve_ids = Vec::new();
    for (id, label) in [
        ("silent", "Silent"),
        ("balanced", "Balanced"),
        ("performance", "Performance"),
    ] {
        let item = MenuItem::new(label, true, None);
        curve_ids.push((item.id().clone(), id.to_string()));
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
            toggle_id,
            quit_id,
            preset_ids,
            effect_ids,
            curve_ids,
        },
    )
}

impl TrayController {
    pub fn new(presets: &[(String, String)]) -> Option<Self> {
        let (menu, bindings) = build_menu(presets);
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
        })
    }

    pub fn sync_presets(&mut self, presets: &[(String, String)]) {
        if self.presets_snapshot == presets {
            return;
        }
        self.presets_snapshot = presets.to_vec();
        let (menu, bindings) = build_menu(presets);
        self.bindings = bindings;
        self.tray.set_menu(Some(Box::new(menu)));
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
            } else if ev.id == self.bindings.toggle_id {
                actions.push(TrayAction::ToggleLights);
            } else if ev.id == self.bindings.quit_id {
                actions.push(TrayAction::Quit);
            } else if let Some((_, pid)) = self
                .bindings
                .preset_ids
                .iter()
                .find(|(id, _)| *id == ev.id)
            {
                actions.push(TrayAction::ApplyPreset(pid.clone()));
            } else if let Some((_, eff)) = self
                .bindings
                .effect_ids
                .iter()
                .find(|(id, _)| *id == ev.id)
            {
                actions.push(TrayAction::SelectEffect(*eff));
            } else if let Some((_, cid)) = self
                .bindings
                .curve_ids
                .iter()
                .find(|(id, _)| *id == ev.id)
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
