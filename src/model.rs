use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Point {
    pub temperature: f32,
    pub limit: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SleepRule {
    pub enabled: bool,
    pub temperature: f32,
    pub wake_automatically: bool,
    pub minutes: u32,
}
impl Default for SleepRule {
    fn default() -> Self {
        Self {
            enabled: false,
            temperature: 90.0,
            wake_automatically: false,
            minutes: 10,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Curve {
    pub enabled: bool,
    pub points: Vec<Point>,
    pub sleep: SleepRule,
}
impl Curve {
    pub fn limit_at(&self, t: f32) -> f32 {
        if !self.enabled || !t.is_finite() || self.points.is_empty() {
            return 100.0;
        }
        if t <= self.points[0].temperature {
            return self.points[0].limit;
        }
        for pair in self.points.windows(2) {
            if t <= pair[1].temperature {
                let ratio = (t - pair[0].temperature) / (pair[1].temperature - pair[0].temperature);
                return pair[0].limit + ratio * (pair[1].limit - pair[0].limit);
            }
        }
        self.points.last().unwrap().limit
    }
    pub fn validate(&self, battery: bool) -> Result<(), String> {
        if !(2..=12).contains(&self.points.len()) {
            return Err(crate::tr!(
                "Křivka musí mít 2 až 12 bodů.",
                "A curve must contain 2 to 12 points."
            )
            .into());
        }
        let (min, max) = if battery { (0.0, 80.0) } else { (10.0, 110.0) };
        for p in &self.points {
            if !p.temperature.is_finite()
                || !p.limit.is_finite()
                || !(min..=max).contains(&p.temperature)
                || !(5.0..=100.0).contains(&p.limit)
            {
                return Err(crate::tr!(
                    "Bod má neplatnou teplotu nebo limit (5–100 %).",
                    "A point has an invalid temperature or performance limit (5–100%)."
                )
                .into());
            }
        }
        for pair in self.points.windows(2) {
            if pair[1].temperature - pair[0].temperature < 0.5 {
                return Err(crate::tr!(
                    "Teploty bodů musí vzrůstat alespoň o 0,5 °C.",
                    "Point temperatures must increase by at least 0.5 °C."
                )
                .into());
            }
            if pair[1].limit > pair[0].limit {
                return Err(crate::tr!(
                    "S rostoucí teplotou se výkon nesmí zvyšovat.",
                    "Performance cannot increase as temperature rises."
                )
                .into());
            }
        }
        if !self.sleep.temperature.is_finite()
            || !(min..=max).contains(&self.sleep.temperature)
            || !(1..=120).contains(&self.sleep.minutes)
        {
            return Err(crate::tr!(
                "Neplatná mez uspání nebo doba probuzení (1–120 minut).",
                "Invalid sleep threshold or wake delay (1–120 minutes)."
            )
            .into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub cpu: Curve,
    pub battery: Curve,
    pub battery_only: bool,
    pub closed_lid_only: bool,
    pub resume_without_password: bool,
    pub recovery_seconds: u32,
    pub sample_seconds: u32,
}
impl Default for Settings {
    fn default() -> Self {
        let curve = |pairs: &[(f32, f32)], enabled, sleep_temp| Curve {
            enabled,
            points: pairs
                .iter()
                .map(|&(temperature, limit)| Point { temperature, limit })
                .collect(),
            sleep: SleepRule {
                temperature: sleep_temp,
                ..Default::default()
            },
        };
        Self {
            version: 1,
            cpu: curve(
                &[(40., 100.), (60., 90.), (72., 70.), (82., 45.), (90., 20.)],
                true,
                90.,
            ),
            battery: curve(
                &[(20., 100.), (32., 90.), (38., 70.), (42., 45.), (46., 20.)],
                false,
                45.,
            ),
            battery_only: true,
            closed_lid_only: true,
            resume_without_password: false,
            recovery_seconds: 10,
            sample_seconds: 2,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err(crate::tr!(
                "Nepodporovaná verze nastavení.",
                "Unsupported settings version."
            )
            .into());
        }
        self.cpu.validate(false)?;
        self.battery.validate(true)?;
        if !(2..=10).contains(&self.sample_seconds) || !(5..=120).contains(&self.recovery_seconds) {
            return Err(crate::tr!(
                "Neplatný interval měření nebo zpoždění uvolnění výkonu.",
                "Invalid sampling interval or performance recovery delay."
            )
            .into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct NamedProfile {
    pub name: String,
    pub settings: Settings,
}

pub fn validate_profiles(profiles: &[NamedProfile]) -> Result<(), String> {
    if profiles.len() > 32 {
        return Err(crate::tr!(
            "Lze uložit nejvýše 32 profilů.",
            "You can save up to 32 profiles."
        )
        .into());
    }
    for (index, profile) in profiles.iter().enumerate() {
        if profile.name.trim().is_empty()
            || profile.name.chars().count() > 60
            || profiles[..index].iter().any(|p| p.name == profile.name)
        {
            return Err(crate::tr!(
                "Název profilu musí být jedinečný a mít 1 až 60 znaků.",
                "Profile names must be unique and contain 1 to 60 characters."
            )
            .into());
        }
        profile.settings.validate()?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub enum LidState {
    Open,
    Closed,
    #[default]
    Unknown,
}
impl LidState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Open => crate::tr!("Víko otevřené", "Lid open"),
            Self::Closed => crate::tr!("Víko zavřené", "Lid closed"),
            Self::Unknown => crate::tr!("Stav víka neznámý", "Lid state unknown"),
        }
    }
}

/// A single gate for BOTH throttling and thermal sleep, including the final sleep check.
pub fn control_pause(settings: &Settings, lid: LidState, on_ac: bool) -> Option<&'static str> {
    if settings.closed_lid_only {
        match lid {
            LidState::Open => {
                return Some(crate::tr!(
                    "Víko otevřené · původní výkon, teplotní uspávání vypnuté.",
                    "Lid open · original performance restored, thermal sleep disabled."
                ));
            }
            LidState::Unknown => {
                return Some(crate::tr!(
                    "Stav víka neznámý · regulace a teplotní uspávání pozastavené.",
                    "Lid state unknown · regulation and thermal sleep paused."
                ));
            }
            LidState::Closed => {}
        }
    }
    if settings.battery_only && on_ac {
        return Some(crate::tr!(
            "Na nabíječce pouze sleduji (volba Jen na baterii).",
            "AC power connected · monitoring only (battery-only option)."
        ));
    }
    None
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Sample {
    pub timestamp: String,
    pub cpu_temperature: Option<f32>,
    pub battery_temperature: Option<f32>,
    pub gpu_temperature: Option<f32>,
    pub cpu_usage: Option<f32>,
    pub discharge_watts: Option<f32>,
    pub battery_percent: Option<u8>,
    pub on_ac: bool,
    pub lid_state: LidState,
    pub lid_source: String,
    pub cpu_source: String,
    pub battery_source: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SleepRequest {
    pub source: &'static str,
    pub wake_minutes: Option<u32>,
}
impl SleepRequest {
    pub fn retain_stricter(&mut self, newer: Self) {
        if newer.wake_minutes.unwrap_or(u32::MAX) > self.wake_minutes.unwrap_or(u32::MAX) {
            *self = newer;
        }
    }
}

pub fn requested_limit(s: &Settings, sample: &Sample) -> Result<f32, String> {
    let mut limit: f32 = 100.;
    for (curve, temp, name) in [
        (&s.cpu, sample.cpu_temperature, "CPU"),
        (
            &s.battery,
            sample.battery_temperature,
            crate::tr!("baterie", "battery"),
        ),
    ] {
        if curve.enabled || curve.sleep.enabled {
            let t = temp.filter(|t| t.is_finite()).ok_or_else(|| {
                crate::tf!(
                    "Chybí aktuální teplota {name}.",
                    "A current {name} temperature reading is not available."
                )
            })?;
            if curve.enabled {
                limit = limit.min(curve.limit_at(t));
            }
        }
    }
    Ok(limit.clamp(5., 100.))
}

pub fn sleep_request(s: &Settings, sample: &Sample) -> Option<SleepRequest> {
    let mut requests = Vec::new();
    for (curve, temp, source) in [
        (
            &s.cpu,
            sample.cpu_temperature,
            crate::tr!("procesor", "CPU"),
        ),
        (
            &s.battery,
            sample.battery_temperature,
            crate::tr!("baterie", "battery"),
        ),
    ] {
        if curve.sleep.enabled && temp.is_some_and(|t| t >= curve.sleep.temperature) {
            requests.push(SleepRequest {
                source,
                wake_minutes: curve
                    .sleep
                    .wake_automatically
                    .then_some(curve.sleep.minutes),
            });
        }
    }
    // A triggered rule without automatic wake always wins; otherwise the longer cooling interval wins.
    requests
        .into_iter()
        .max_by_key(|r| r.wake_minutes.unwrap_or(u32::MAX))
}

pub struct Regulator {
    pub applied: f32,
    cool_since: Option<Instant>,
}
impl Default for Regulator {
    fn default() -> Self {
        Self {
            applied: 100.,
            cool_since: None,
        }
    }
}
impl Regulator {
    pub fn step(&mut self, target: f32, now: Instant, recovery: Duration) -> f32 {
        if target < self.applied {
            self.applied = target;
            self.cool_since = None;
        } else if target > self.applied + 0.5 {
            let since = self.cool_since.get_or_insert(now);
            if now.duration_since(*since) >= recovery {
                self.applied = (self.applied + 3.).min(target);
            }
        } else {
            self.cool_since = None;
        }
        self.applied
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn named_profiles_preserve_rules_and_reject_invalid_or_duplicate_entries() {
        let mut settings = Settings::default();
        settings.cpu.sleep.enabled = true;
        settings.cpu.sleep.wake_automatically = true;
        settings.cpu.sleep.minutes = 23;
        let profile = NamedProfile {
            name: "Travel".into(),
            settings,
        };
        let profiles = vec![profile.clone()];
        validate_profiles(&profiles).unwrap();
        let restored: Vec<NamedProfile> =
            serde_json::from_str(&serde_json::to_string(&profiles).unwrap()).unwrap();
        assert_eq!(restored, profiles);
        assert!(restored[0].settings.closed_lid_only);
        assert!(validate_profiles(&[profile.clone(), profile.clone()]).is_err());
        let mut invalid = profile;
        invalid.settings.cpu.points.clear();
        assert!(validate_profiles(&[invalid]).is_err());
    }
    #[test]
    fn closed_lid_only_defaults_on_including_old_settings() {
        assert!(Settings::default().closed_lid_only);
        let mut old = serde_json::to_value(Settings::default()).unwrap();
        old.as_object_mut().unwrap().remove("closed_lid_only");
        let loaded: Settings = serde_json::from_value(old).unwrap();
        assert!(loaded.closed_lid_only);
    }
    #[test]
    fn lid_gate_pauses_both_actions_until_closed() {
        let mut settings = Settings::default();
        assert!(control_pause(&settings, LidState::Open, false).is_some());
        assert!(control_pause(&settings, LidState::Unknown, false).is_some());
        assert!(control_pause(&settings, LidState::Closed, false).is_none());
        assert!(control_pause(&settings, LidState::Closed, true).is_some());
        settings.battery_only = false;
        assert!(control_pause(&settings, LidState::Closed, true).is_none());
        assert!(control_pause(&settings, LidState::Open, true).is_some());
        settings.closed_lid_only = false;
        assert!(control_pause(&settings, LidState::Open, true).is_none());
        assert!(control_pause(&settings, LidState::Unknown, false).is_none());
    }
    #[test]
    fn curve_interpolates_and_clamps() {
        let c = Settings::default().cpu;
        assert_eq!(c.limit_at(0.), 100.);
        assert_eq!(c.limit_at(200.), 20.);
        assert!((c.limit_at(66.) - 80.).abs() < 0.01);
    }
    #[test]
    fn rejects_nan_and_uphill_curves() {
        let mut c = Settings::default().cpu;
        c.points[2].limit = 99.;
        assert!(c.validate(false).is_err());
        c.points[2].limit = f32::NAN;
        assert!(c.validate(false).is_err());
    }
    #[test]
    fn stronger_sensor_wins_and_missing_is_not_cold() {
        let mut s = Settings::default();
        s.battery.enabled = true;
        let mut sample = Sample {
            cpu_temperature: Some(40.),
            battery_temperature: Some(46.),
            ..Default::default()
        };
        assert_eq!(requested_limit(&s, &sample).unwrap(), 20.);
        sample.battery_temperature = None;
        assert!(requested_limit(&s, &sample).is_err());
    }
    #[test]
    fn no_automatic_wake_is_default_for_both_sensors() {
        let s = Settings::default();
        assert!(!s.cpu.sleep.wake_automatically);
        assert!(!s.battery.sleep.wake_automatically);
    }
    #[test]
    fn cpu_sleep_supports_optional_wake_and_roundtrips() {
        let mut settings = Settings::default();
        settings.cpu.enabled = false;
        settings.cpu.sleep.enabled = true;
        let sample = Sample {
            cpu_temperature: Some(95.),
            ..Default::default()
        };
        assert_eq!(
            sleep_request(&settings, &sample).unwrap().wake_minutes,
            None
        );
        settings.cpu.sleep.wake_automatically = true;
        settings.cpu.sleep.minutes = 17;
        let loaded: Settings =
            serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert_eq!(loaded, settings);
        assert_eq!(
            sleep_request(&loaded, &sample).unwrap().wake_minutes,
            Some(17)
        );
        assert_eq!(
            sleep_request(
                &loaded,
                &Sample {
                    cpu_temperature: Some(89.),
                    ..sample
                }
            ),
            None
        );
    }
    #[test]
    fn countdown_preserves_strictest_wake_rule() {
        let mut request = SleepRequest {
            source: crate::tr!("procesor", "CPU"),
            wake_minutes: Some(5),
        };
        request.retain_stricter(SleepRequest {
            source: crate::tr!("baterie", "battery"),
            wake_minutes: Some(20),
        });
        assert_eq!(request.wake_minutes, Some(20));
        request.retain_stricter(SleepRequest {
            source: crate::tr!("procesor", "CPU"),
            wake_minutes: None,
        });
        request.retain_stricter(SleepRequest {
            source: crate::tr!("baterie", "battery"),
            wake_minutes: Some(30),
        });
        assert_eq!(request.wake_minutes, None);
    }
    #[test]
    fn battery_sleep_works_without_battery_throttle() {
        let mut s = Settings::default();
        s.battery.sleep.enabled = true;
        let sample = Sample {
            battery_temperature: Some(46.),
            ..Default::default()
        };
        assert_eq!(
            sleep_request(&s, &sample),
            Some(SleepRequest {
                source: crate::tr!("baterie", "battery"),
                wake_minutes: None
            })
        );
    }
    #[test]
    fn no_wake_takes_precedence_over_timed_wake() {
        let mut s = Settings::default();
        s.cpu.sleep.enabled = true;
        s.cpu.sleep.wake_automatically = true;
        s.battery.sleep.enabled = true;
        let sample = Sample {
            cpu_temperature: Some(95.),
            battery_temperature: Some(46.),
            ..Default::default()
        };
        assert_eq!(sleep_request(&s, &sample).unwrap().wake_minutes, None);
    }
    #[test]
    fn fast_reduction_and_delayed_release() {
        let now = Instant::now();
        let mut r = Regulator::default();
        assert_eq!(r.step(30., now, Duration::from_secs(10)), 30.);
        assert_eq!(r.step(100., now, Duration::from_secs(10)), 30.);
        assert_eq!(
            r.step(100., now + Duration::from_secs(11), Duration::from_secs(10)),
            33.
        );
    }
}
