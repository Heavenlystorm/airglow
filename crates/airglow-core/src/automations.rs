//! State-based automation rules ("when <trigger>, <action>").

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::config::{Preset, TempSource};
use crate::fans;
use crate::sensors::SensorSnapshot;

pub const TEMP_HYSTERESIS_C: f32 = 3.0;
pub const ALL_DAYS_MASK: u8 = 0b0111_1111;

fn all_days_mask() -> u8 {
    ALL_DAYS_MASK
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Trigger {
    TimeWindow {
        start_hour: u8,
        start_min: u8,
        end_hour: u8,
        end_min: u8,
        /// Bitmask of active weekdays (bit 0 = Mon .. bit 6 = Sun). Default 0x7F (all 7 days).
        #[serde(default = "all_days_mask")]
        days_mask: u8,
    },
    AppRunning {
        exe: String,
    },
    Idle {
        minutes: u16,
    },
    Locked,
    TempAbove {
        source: TempSource,
        temp_c: u8,
    },
}

impl Trigger {
    pub fn describe(&self) -> String {
        match self {
            Self::TimeWindow {
                start_hour,
                start_min,
                end_hour,
                end_min,
                days_mask,
            } => {
                let days = describe_days_mask(*days_mask);
                if days.is_empty() {
                    format!(
                        "Between {:02}:{:02} and {:02}:{:02}",
                        start_hour, start_min, end_hour, end_min
                    )
                } else {
                    format!(
                        "Between {:02}:{:02} and {:02}:{:02} ({days})",
                        start_hour, start_min, end_hour, end_min
                    )
                }
            }
            Self::AppRunning { exe } => format!("When {exe} is running"),
            Self::Idle { minutes } => format!("When PC is idle for {minutes} min"),
            Self::Locked => "When PC is locked".to_string(),
            Self::TempAbove { source, temp_c } => {
                let src = match source {
                    TempSource::Max => "the hotter part",
                    TempSource::Cpu => "processor",
                    TempSource::Gpu => "graphics card",
                    TempSource::Liquid => "coolant",
                };
                format!("When {src} is above {temp_c}°C")
            }
        }
    }
}

pub fn describe_days_mask(mask: u8) -> String {
    let m = mask & ALL_DAYS_MASK;
    if m == ALL_DAYS_MASK || m == 0 {
        return String::new();
    }
    if m == 0b001_1111 {
        return "Mon–Fri".to_string();
    }
    if m == 0b110_0000 {
        return "Sat–Sun".to_string();
    }
    const NAMES: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    NAMES
        .iter()
        .enumerate()
        .filter(|(i, _)| (m & (1 << i)) != 0)
        .map(|(_, n)| *n)
        .collect::<Vec<_>>()
        .join(", ")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    ApplyPreset { preset: String },
    LightsOff,
    Dim { brightness: u8 },
    FanCurve { curve: String },
}

impl Action {
    pub fn describe(&self, presets: &[Preset]) -> String {
        match self {
            Self::ApplyPreset { preset } => {
                let name = presets
                    .iter()
                    .find(|p| &p.id == preset)
                    .map(|p| p.name.as_str())
                    .unwrap_or(preset.as_str());
                format!("use {name}")
            }
            Self::LightsOff => "turn the lights off".to_string(),
            Self::Dim { brightness } => format!("dim the lights to {brightness}%"),
            Self::FanCurve { curve } => {
                let mut ch = curve.chars();
                let cap = match ch.next() {
                    Some(first) => format!("{}{}", first.to_uppercase(), ch.as_str()),
                    None => String::new(),
                };
                format!("switch the fans to {cap}")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationRule {
    pub id: String,
    pub enabled: bool,
    pub trigger: Trigger,
    pub action: Action,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutomationStatus {
    pub paused: bool,
    pub active_ids: Vec<String>,
    pub light_winner: Option<String>,
    pub fan_winner: Option<String>,
}

pub struct AutomationContext<'a> {
    pub running_exes: &'a HashSet<String>,
    pub local_hm: (u8, u8),
    /// 0 = Monday .. 6 = Sunday.
    pub weekday_mon0: u8,
    pub idle_secs: u64,
    pub locked: bool,
    pub sensors: &'a SensorSnapshot,
}

pub fn default_rules() -> Vec<AutomationRule> {
    vec![
        AutomationRule {
            id: "night-lights-off".to_string(),
            enabled: false,
            trigger: Trigger::TimeWindow {
                start_hour: 23,
                start_min: 0,
                end_hour: 7,
                end_min: 0,
                days_mask: ALL_DAYS_MASK,
            },
            action: Action::LightsOff,
        },
        AutomationRule {
            id: "gaming-profile".to_string(),
            enabled: false,
            trigger: Trigger::AppRunning {
                exe: "Cyberpunk2077.exe".to_string(),
            },
            action: Action::ApplyPreset {
                preset: "gaming".to_string(),
            },
        },
        AutomationRule {
            id: "thermal-performance-fans".to_string(),
            enabled: false,
            trigger: Trigger::TempAbove {
                source: TempSource::Max,
                temp_c: 80,
            },
            action: Action::FanCurve {
                curve: "performance".to_string(),
            },
        },
    ]
}

/// Returns `(hour, minute, weekday_mon0)` where `weekday_mon0` is `0=Mon .. 6=Sun`.
#[cfg(windows)]
pub fn local_time_hm_wday() -> (u8, u8, u8) {
    #[repr(C)]
    #[derive(Default)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        milliseconds: u16,
    }
    unsafe extern "system" {
        fn GetLocalTime(lp_system_time: *mut SystemTime);
    }
    let mut st = SystemTime::default();
    unsafe { GetLocalTime(&mut st) };
    // Win32 day_of_week: 0=Sun..6=Sat -> convert to 0=Mon..6=Sun.
    let wday_mon0 = ((st.day_of_week + 6) % 7) as u8;
    (st.hour as u8, st.minute as u8, wday_mon0)
}

#[cfg(not(windows))]
pub fn local_time_hm_wday() -> (u8, u8, u8) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mins = (secs / 60) % (24 * 60);
    // 1970-01-01 was Thursday (wday_mon0 = 3).
    let days = secs / 86_400;
    let wday_mon0 = ((days + 3) % 7) as u8;
    ((mins / 60) as u8, (mins % 60) as u8, wday_mon0)
}

pub fn local_time_hm() -> (u8, u8) {
    let (h, m, _) = local_time_hm_wday();
    (h, m)
}

pub fn in_time_window(now_hm: (u8, u8), start_hm: (u8, u8), end_hm: (u8, u8)) -> bool {
    in_time_window_on_day(now_hm, 0, start_hm, end_hm, ALL_DAYS_MASK)
}

pub fn in_time_window_on_day(
    now_hm: (u8, u8),
    weekday_mon0: u8,
    start_hm: (u8, u8),
    end_hm: (u8, u8),
    days_mask: u8,
) -> bool {
    let mask = if days_mask == 0 { ALL_DAYS_MASK } else { days_mask };
    let now = now_hm.0 as u16 * 60 + now_hm.1 as u16;
    let start = start_hm.0 as u16 * 60 + start_hm.1 as u16;
    let end = end_hm.0 as u16 * 60 + end_hm.1 as u16;
    if start == end {
        return false;
    }
    let today_bit = 1u8 << (weekday_mon0 % 7);
    if start < end {
        now >= start && now < end && (mask & today_bit) != 0
    } else if now >= start {
        (mask & today_bit) != 0
    } else {
        let prev_day = (weekday_mon0 + 6) % 7;
        now < end && (mask & (1u8 << prev_day)) != 0
    }
}

pub fn is_rule_active(
    rule: &AutomationRule,
    ctx: &AutomationContext<'_>,
    was_active: bool,
) -> bool {
    match &rule.trigger {
        Trigger::TimeWindow {
            start_hour,
            start_min,
            end_hour,
            end_min,
            days_mask,
        } => in_time_window_on_day(
            ctx.local_hm,
            ctx.weekday_mon0,
            (*start_hour, *start_min),
            (*end_hour, *end_min),
            *days_mask,
        ),
        Trigger::AppRunning { exe } => ctx
            .running_exes
            .contains(&exe.trim().to_ascii_lowercase()),
        Trigger::Idle { minutes } => ctx.idle_secs >= (*minutes as u64) * 60,
        Trigger::Locked => ctx.locked,
        Trigger::TempAbove { source, temp_c } => {
            let limit = if was_active {
                *temp_c as f32 - TEMP_HYSTERESIS_C
            } else {
                *temp_c as f32
            };
            fans::source_temp(ctx.sensors, *source).is_some_and(|t| t >= limit)
        }
    }
}

pub fn evaluate_rules(
    rules: &[AutomationRule],
    paused: bool,
    ctx: &AutomationContext<'_>,
    prev_active: &HashSet<String>,
    presets: &[Preset],
) -> AutomationStatus {
    if paused {
        return AutomationStatus {
            paused: true,
            ..AutomationStatus::default()
        };
    }

    let mut active_ids = Vec::new();
    let mut light_winner = None;
    let mut fan_winner = None;

    for rule in rules {
        if !rule.enabled {
            continue;
        }
        let was_active = prev_active.contains(&rule.id);
        if !is_rule_active(rule, ctx, was_active) {
            continue;
        }
        active_ids.push(rule.id.clone());

        match &rule.action {
            Action::LightsOff | Action::Dim { .. } => {
                if light_winner.is_none() {
                    light_winner = Some(rule.id.clone());
                }
            }
            Action::ApplyPreset { preset } => {
                let found = presets.iter().find(|p| &p.id == preset);
                if light_winner.is_none() && found.is_some() {
                    light_winner = Some(rule.id.clone());
                }
                if fan_winner.is_none() && found.is_some_and(|p| !p.fan_curve.is_empty()) {
                    fan_winner = Some(rule.id.clone());
                }
            }
            Action::FanCurve { .. } => {
                if fan_winner.is_none() {
                    fan_winner = Some(rule.id.clone());
                }
            }
        }
    }

    AutomationStatus {
        paused: false,
        active_ids,
        light_winner,
        fan_winner,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;

    #[test]
    fn midnight_time_window_handles_wrap_and_weekdays() {
        assert!(in_time_window((23, 30), (23, 0), (7, 0)));
        assert!(in_time_window((2, 15), (23, 0), (7, 0)));
        assert!(!in_time_window((12, 0), (23, 0), (7, 0)));
        assert!(!in_time_window((7, 0), (23, 0), (7, 0)));

        // Friday-only window (bit 4) wraps into Saturday early morning (day 5)
        let fri_only = 1u8 << 4;
        assert!(in_time_window_on_day((23, 30), 4, (23, 0), (7, 0), fri_only));
        assert!(in_time_window_on_day((2, 0), 5, (23, 0), (7, 0), fri_only));
        assert!(!in_time_window_on_day((2, 0), 4, (23, 0), (7, 0), fri_only));
    }

    #[test]
    fn idle_and_locked_triggers_fire() {
        let exes = HashSet::new();
        let sensors = SensorSnapshot::default();
        let idle_rule = AutomationRule {
            id: "idle-quiet".to_string(),
            enabled: true,
            trigger: Trigger::Idle { minutes: 10 },
            action: Action::FanCurve {
                curve: "silent".to_string(),
            },
        };
        let lock_rule = AutomationRule {
            id: "lock-dark".to_string(),
            enabled: true,
            trigger: Trigger::Locked,
            action: Action::LightsOff,
        };
        let ctx_active = AutomationContext {
            running_exes: &exes,
            local_hm: (14, 0),
            weekday_mon0: 2,
            idle_secs: 605,
            locked: true,
            sensors: &sensors,
        };
        assert!(is_rule_active(&idle_rule, &ctx_active, false));
        assert!(is_rule_active(&lock_rule, &ctx_active, false));

        let ctx_inactive = AutomationContext {
            idle_secs: 120,
            locked: false,
            ..ctx_active
        };
        assert!(!is_rule_active(&idle_rule, &ctx_inactive, false));
        assert!(!is_rule_active(&lock_rule, &ctx_inactive, false));
    }

    #[test]
    fn temp_above_respects_three_degree_hysteresis() {
        let rule = AutomationRule {
            id: "hot".to_string(),
            enabled: true,
            trigger: Trigger::TempAbove {
                source: TempSource::Cpu,
                temp_c: 80,
            },
            action: Action::FanCurve {
                curve: "performance".to_string(),
            },
        };
        let exes = HashSet::new();
        let warm = SensorSnapshot {
            cpu_temp: Some(78.0),
            ..SensorSnapshot::default()
        };
        let ctx_warm = AutomationContext {
            running_exes: &exes,
            local_hm: (12, 0),
            weekday_mon0: 0,
            idle_secs: 0,
            locked: false,
            sensors: &warm,
        };
        assert!(!is_rule_active(&rule, &ctx_warm, false));
        assert!(is_rule_active(&rule, &ctx_warm, true));
        let cool = SensorSnapshot {
            cpu_temp: Some(76.5),
            ..SensorSnapshot::default()
        };
        let ctx_cool = AutomationContext {
            sensors: &cool,
            ..ctx_warm
        };
        assert!(!is_rule_active(&rule, &ctx_cool, true));
    }

    #[test]
    fn priority_resolves_lighting_and_fans_independently() {
        let cfg = AppConfig::default();
        let rules = vec![
            AutomationRule {
                id: "r-dim".to_string(),
                enabled: true,
                trigger: Trigger::TimeWindow {
                    start_hour: 22,
                    start_min: 0,
                    end_hour: 6,
                    end_min: 0,
                    days_mask: ALL_DAYS_MASK,
                },
                action: Action::Dim { brightness: 20 },
            },
            AutomationRule {
                id: "r-game".to_string(),
                enabled: true,
                trigger: Trigger::AppRunning {
                    exe: "Cyberpunk2077.exe".to_string(),
                },
                action: Action::ApplyPreset {
                    preset: "gaming".to_string(),
                },
            },
        ];
        let mut exes = HashSet::new();
        exes.insert("cyberpunk2077.exe".to_string());
        let sensors = SensorSnapshot::default();
        let ctx = AutomationContext {
            running_exes: &exes,
            local_hm: (23, 30),
            weekday_mon0: 1,
            idle_secs: 0,
            locked: false,
            sensors: &sensors,
        };
        let status = evaluate_rules(&rules, false, &ctx, &HashSet::new(), &cfg.presets);
        assert_eq!(status.light_winner.as_deref(), Some("r-dim"));
        assert_eq!(status.fan_winner.as_deref(), Some("r-game"));
    }
}
