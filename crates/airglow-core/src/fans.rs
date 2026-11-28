//! Fan curve math: linear interpolation, source temperature selection, acoustic hysteresis,
//! failsafe, and ramp limiting.

use crate::config::{CurvePoint, SAFETY_TEMP, TempSource};
use crate::sensors::SensorSnapshot;

/// Acoustic hysteresis filter for smooth, whisper-quiet fan control.
///
/// - Heating up or reaching [`SAFETY_TEMP`] (≥ 90 °C) responds immediately with zero lag.
/// - Cooling down holds the latched effective temperature until the measured temperature
///   drops at least `hysteresis_c` °C below the latched value, preventing annoying fan hunting.
#[derive(Debug, Clone, Default)]
pub struct FanHysteresis {
    effective_temp: Option<f32>,
}

impl FanHysteresis {
    pub fn new() -> Self {
        Self {
            effective_temp: None,
        }
    }

    pub fn update(&mut self, raw_temp: Option<f32>, hysteresis_c: f32) -> Option<f32> {
        let Some(temp) = raw_temp else {
            self.effective_temp = None;
            return None;
        };
        let hyst = hysteresis_c.clamp(0.0, 15.0);
        if temp >= SAFETY_TEMP || hyst <= 0.0 {
            self.effective_temp = Some(temp);
            return Some(temp);
        }
        match self.effective_temp {
            None => {
                self.effective_temp = Some(temp);
                Some(temp)
            }
            Some(prev) => {
                if temp >= prev || prev - temp >= hyst {
                    self.effective_temp = Some(temp);
                    Some(temp)
                } else {
                    Some(prev)
                }
            }
        }
    }

    pub fn current(&self) -> Option<f32> {
        self.effective_temp
    }

    pub fn reset(&mut self) {
        self.effective_temp = None;
    }
}

pub fn curve_duty(points: &[CurvePoint], temp: f32) -> f32 {
    if points.is_empty() {
        return 50.0;
    }
    let mut sorted = points.to_vec();
    sorted.sort_by_key(|p| p.temp);
    if temp <= sorted[0].temp as f32 {
        return sorted[0].duty as f32;
    }
    for win in sorted.windows(2) {
        let (a, b) = (win[0], win[1]);
        if temp <= b.temp as f32 {
            let span = (b.temp as f32 - a.temp as f32).max(1.0);
            let t = (temp - a.temp as f32) / span;
            return a.duty as f32 + (b.duty as f32 - a.duty as f32) * t;
        }
    }
    sorted.last().map(|p| p.duty as f32).unwrap_or(100.0)
}

pub fn source_temp(sensors: &SensorSnapshot, source: TempSource) -> Option<f32> {
    match source {
        TempSource::Cpu => sensors.cpu_temp,
        TempSource::Gpu => sensors.gpu_temp,
        TempSource::Liquid => sensors.liquid_temp.or_else(|| max_known(sensors)),
        TempSource::Max => max_known(sensors),
    }
}

fn max_known(s: &SensorSnapshot) -> Option<f32> {
    [s.cpu_temp, s.gpu_temp, s.gpu_hotspot]
        .into_iter()
        .flatten()
        .reduce(f32::max)
}

pub fn target_duty(points: &[CurvePoint], temp: Option<f32>, failsafe: u8) -> f32 {
    match temp {
        None => failsafe as f32,
        Some(t) if t >= SAFETY_TEMP => 100.0,
        Some(t) => curve_duty(points, t),
    }
}

pub fn step_toward(current: Option<u8>, target: f32, max_step: u8) -> u8 {
    let target_rounded = target.round().clamp(0.0, 100.0) as u8;
    match current {
        None => target_rounded,
        Some(_) if target >= 100.0 => 100,
        Some(cur) => {
            let delta = (target_rounded as i16 - cur as i16)
                .clamp(-(max_step as i16), max_step as i16);
            (cur as i16 + delta).clamp(0, 100) as u8
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn balanced() -> Vec<CurvePoint> {
        vec![
            CurvePoint { temp: 40, duty: 30 },
            CurvePoint { temp: 60, duty: 50 },
            CurvePoint { temp: 80, duty: 90 },
        ]
    }

    #[test]
    fn interpolates_clamps_and_overrides_at_safety_temp() {
        let pts = balanced();
        assert_eq!(curve_duty(&pts, 30.0), 30.0);
        assert!((curve_duty(&pts, 50.0) - 40.0).abs() < 0.01);
        assert_eq!(target_duty(&pts, None, 35), 35.0);
        assert_eq!(target_duty(&pts, Some(90.0), 35), 100.0);
    }

    #[test]
    fn ramp_limiting_jumps_immediately_to_100() {
        assert_eq!(step_toward(Some(30), 60.0, 5), 35);
        assert_eq!(step_toward(Some(30), 100.0, 5), 100);
        assert_eq!(step_toward(Some(50), 25.0, 5), 45);
    }

    #[test]
    fn acoustic_hysteresis_holds_on_small_cooldown_and_ramps_instantly() {
        let mut hyst = FanHysteresis::new();
        // Initial latch
        assert_eq!(hyst.update(Some(70.0), 2.0), Some(70.0));
        // Small cool-down within 2°C deadband stays latched at 70°C
        assert_eq!(hyst.update(Some(69.0), 2.0), Some(70.0));
        assert_eq!(hyst.update(Some(68.2), 2.0), Some(70.0));
        // Heating up responds immediately
        assert_eq!(hyst.update(Some(72.5), 2.0), Some(72.5));
        // Drop >= 2.0°C releases to the new cooler temperature
        assert_eq!(hyst.update(Some(70.4), 2.0), Some(70.4));
        // Safety override >= 90°C responds immediately even if hysteresis is large
        assert_eq!(hyst.update(Some(90.0), 8.0), Some(90.0));
        // Zero hysteresis passes through directly
        assert_eq!(hyst.update(Some(65.0), 0.0), Some(65.0));
        // None clears state
        assert_eq!(hyst.update(None, 2.0), None);
        assert_eq!(hyst.current(), None);
    }
}
