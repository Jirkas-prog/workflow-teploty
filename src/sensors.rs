use crate::{model::Sample, native};
use std::collections::HashMap;
use wmi::{Variant, WMIConnection};

fn number(v: &Variant) -> Option<f32> {
    match v {
        Variant::UI1(v) => Some(*v as f32),
        Variant::UI2(v) => Some(*v as f32),
        Variant::UI4(v) => Some(*v as f32),
        Variant::UI8(v) => Some(*v as f32),
        Variant::I2(v) => Some(*v as f32),
        Variant::I4(v) => Some(*v as f32),
        Variant::I8(v) => Some(*v as f32),
        Variant::R4(v) => Some(*v),
        Variant::R8(v) => Some(*v as f32),
        _ => None,
    }
}
fn string(v: &Variant) -> Option<String> {
    if let Variant::String(v) = v {
        Some(v.clone())
    } else {
        None
    }
}
fn valid(v: Option<f32>, min: f32, max: f32) -> Option<f32> {
    v.filter(|v| v.is_finite() && (min..=max).contains(v))
}

pub struct Sensors {
    lid: Option<native::LidMonitor>,
    lid_source: String,
    wmi: Option<WMIConnection>,
    lenovo_path: Option<String>,
    cpu_error: String,
    battery: Option<native::BatteryDevice>,
    battery_error: String,
    fallback: Option<WMIConnection>,
    fallback_name: String,
    previous_times: Option<(u64, u64)>,
}
impl Sensors {
    pub fn new() -> Self {
        let (lid, lid_source) = match native::LidMonitor::new() {
            Ok(monitor) => (
                Some(monitor),
                crate::tr!("Windows · spínač víka", "Windows · lid switch").into(),
            ),
            Err(e) => (None, e.to_string()),
        };
        let mut cpu_error = String::new();
        let wmi = match WMIConnection::with_namespace_path("ROOT\\WMI") {
            Ok(c) => Some(c),
            Err(e) => {
                cpu_error = e.to_string();
                None
            }
        };
        let lenovo_path = wmi.as_ref().and_then(|w| {
            let result =
                w.raw_query::<HashMap<String, Variant>>("SELECT __PATH FROM LENOVO_GAMEZONE_DATA");
            match result {
                Ok(rows) => rows.first().and_then(|r| r.get("__PATH")).and_then(string),
                Err(e) => {
                    cpu_error = e.to_string();
                    None
                }
            }
        });
        let mut fallback = None;
        let mut fallback_name = String::new();
        if lenovo_path.is_none() {
            for namespace in ["ROOT\\LibreHardwareMonitor", "ROOT\\OpenHardwareMonitor"] {
                if let Ok(connection) = WMIConnection::with_namespace_path(namespace) {
                    fallback = Some(connection);
                    fallback_name = namespace.into();
                    break;
                }
            }
        }
        let (battery, battery_error) = match native::BatteryDevice::open() {
            Ok(d) => (Some(d), String::new()),
            Err(e) => (None, e.to_string()),
        };
        Self {
            lid,
            lid_source,
            wmi,
            lenovo_path,
            cpu_error,
            battery,
            battery_error,
            fallback,
            fallback_name,
            previous_times: native::cpu_times(),
        }
    }
    pub fn lid_state(&self) -> crate::model::LidState {
        self.lid
            .as_ref()
            .map_or(crate::model::LidState::Unknown, native::LidMonitor::state)
    }
    fn lenovo(&self, method: &str) -> Option<f32> {
        let result = self
            .wmi
            .as_ref()?
            .exec_method(self.lenovo_path.as_ref()?, method, None)
            .ok()??;
        if matches!(
            result.get_property("ReturnValue").ok(),
            Some(Variant::Bool(false))
        ) {
            return None;
        }
        number(&result.get_property("Data").ok()?)
    }
    pub fn read(&mut self) -> Sample {
        let (on_ac, battery_percent) = native::battery_status();
        let mut sample = Sample {
            timestamp: chrono::Local::now().to_rfc3339(),
            on_ac,
            battery_percent,
            lid_state: self.lid_state(),
            lid_source: self.lid_source.clone(),
            ..Default::default()
        };
        sample.cpu_temperature = valid(self.lenovo("GetCPUTemp"), 10., 115.);
        sample.gpu_temperature = valid(self.lenovo("GetGPUTemp"), 10., 115.);
        if sample.cpu_temperature.is_some() {
            sample.cpu_source =
                crate::tr!("Lenovo · čidlo procesoru", "Lenovo · CPU sensor").into();
        } else if let Some(connection) = &self.fallback {
            if let Ok(rows) = connection.raw_query::<HashMap<String, Variant>>(
                "SELECT Name, Identifier, Value FROM Sensor WHERE SensorType='Temperature'",
            ) {
                sample.cpu_temperature = rows
                    .iter()
                    .filter(|r| {
                        let name = r
                            .get("Name")
                            .and_then(string)
                            .unwrap_or_default()
                            .to_lowercase();
                        let id = r
                            .get("Identifier")
                            .and_then(string)
                            .unwrap_or_default()
                            .to_lowercase();
                        name.contains("cpu") || id.contains("cpu")
                    })
                    .filter_map(|r| valid(r.get("Value").and_then(number), 10., 115.))
                    .max_by(f32::total_cmp);
            }
            sample.cpu_source = self.fallback_name.clone();
        } else {
            sample.cpu_source = crate::tf!(
                "Čidlo CPU není dostupné. Zkus Spustit jako správce. {}",
                "CPU sensor unavailable. Try Run as administrator. {}",
                self.cpu_error
            );
        }
        if let Some(connection) = &self.wmi {
            if let Ok(rows) = connection
                .raw_query::<HashMap<String, Variant>>("SELECT Temperature FROM BatteryTemperature")
            {
                sample.battery_temperature = rows
                    .iter()
                    .filter_map(|r| r.get("Temperature").and_then(number))
                    .map(|v| v / 10. - 273.15)
                    .filter(|v| (-20.0..=100.).contains(v))
                    .max_by(f32::total_cmp);
                if sample.battery_temperature.is_some() {
                    sample.battery_source = crate::tr!(
                        "Windows · čidlo baterie (WMI)",
                        "Windows · battery sensor (WMI)"
                    )
                    .into();
                }
            }
            if let Ok(rows) = connection.raw_query::<HashMap<String, Variant>>(
                "SELECT DischargeRate, Discharging FROM BatteryStatus",
            ) && !on_ac
            {
                sample.discharge_watts = rows
                    .first()
                    .and_then(|r| r.get("DischargeRate"))
                    .and_then(number)
                    .filter(|v| (0.0..=400_000.).contains(v))
                    .map(|v| v / 1000.);
            }
        }
        if sample.battery_temperature.is_none() {
            if let Some(device) = &self.battery {
                match device.temperature() {
                    Ok(t) => {
                        sample.battery_temperature = Some(t);
                        sample.battery_source = crate::tr!(
                            "Windows · přímé čidlo baterie",
                            "Windows · native battery sensor"
                        )
                        .into();
                    }
                    Err(e) => self.battery_error = e.to_string(),
                }
            }
            if sample.battery_temperature.is_none() {
                sample.battery_source = crate::tf!(
                    "Teplota baterie není poskytována. {}",
                    "Battery temperature is not available. {}",
                    self.battery_error
                );
            }
        }
        if let Some((idle, total)) = native::cpu_times() {
            if let Some((old_idle, old_total)) = self.previous_times {
                let delta = total.saturating_sub(old_total);
                if delta > 0 {
                    sample.cpu_usage = Some(
                        (100. * (1. - idle.saturating_sub(old_idle) as f64 / delta as f64))
                            .clamp(0., 100.) as f32,
                    );
                }
            }
            self.previous_times = Some((idle, total));
        }
        sample
    }
}
