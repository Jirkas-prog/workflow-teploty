#![windows_subsystem = "windows"]
mod lifecycle;
mod native;
mod sensors;
mod ui;
mod updater_ui;
mod worker;
use std::{path::PathBuf, time::Duration};
use workflow_teploty::{i18n, model};
pub use workflow_teploty::{tbail, terr, tf, tr};

fn run() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if let Some(index) = args.iter().position(|a| a == "--apply-update") {
        let path = args
            .get(index + 1)
            .ok_or_else(|| anyhow::anyhow!("Missing update plan"))?;
        return workflow_teploty::updates::installer::apply(std::path::Path::new(path));
    }
    if let Some(index) = args.iter().position(|a| a == "--diagnostics") {
        let path = args.get(index + 1).ok_or_else(|| {
            crate::terr!(
                "Chybí cesta pro diagnostiku.",
                "Missing diagnostics output path."
            )
        })?;
        let mut sensors = sensors::Sensors::new();
        let sample = sensors.read();
        native::write_json(std::path::Path::new(path), &sample)?;
        return Ok(());
    }
    let demo = args.iter().any(|a| a == "--demo");
    let background = args.iter().any(|a| a == "--background");
    let directory =
        if let Some(i) = args.iter().position(|a| a == "--data-dir") {
            PathBuf::from(args.get(i + 1).ok_or_else(|| {
                crate::terr!("Chybí složka nastavení.", "Missing settings directory.")
            })?)
        } else {
            PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or_else(|| {
                crate::terr!("Chybí LOCALAPPDATA.", "LOCALAPPDATA is not available.")
            })?)
            .join("WorkFlow")
            .join(if demo { "Teploty-demo" } else { "Teploty" })
        };
    std::fs::create_dir_all(&directory)?;
    let directory = std::path::absolute(directory)?;
    i18n::set(lifecycle::Preferences::load(&directory).language);
    let mut instance = native::single_instance();
    if args.iter().any(|a| a == "--wait-for-instance") {
        for _ in 0..30 {
            if instance.is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
            instance = native::single_instance();
        }
    }
    let instance = match instance {
        Ok(instance) => instance,
        Err(e) => {
            for _ in 0..10 {
                if native::show_existing() {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            return Err(e);
        }
    };
    let settings_path = directory.join("nastaveni.json");
    let mut notice = String::new();
    let settings = if settings_path.exists() {
        match std::fs::read(&settings_path)
            .map_err(anyhow::Error::from)
            .and_then(|b| Ok(serde_json::from_slice::<model::Settings>(&b)?))
            .and_then(|s| {
                s.validate().map_err(anyhow::Error::msg)?;
                Ok(s)
            }) {
            Ok(s) => s,
            Err(e) => {
                notice = crate::tf!(
                    "Nastavení nelze načíst; používám výchozí návrh. Soubor zůstal zachován. {e}",
                    "Cannot load settings; using a default draft. The original file was preserved. {e}"
                );
                model::Settings::default()
            }
        }
    } else {
        model::Settings::default()
    };
    let startup = updater_ui::StartupUpdate {
        rules_valid: notice.is_empty(),
        resume_control: if args.iter().any(|a| a == "--resume-control-after-update") {
            Some(true)
        } else if args.iter().any(|a| a == "--pause-control-after-update") {
            Some(false)
        } else {
            None
        },
        health_plan: args
            .iter()
            .position(|a| a == "--update-health")
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from),
    };
    if startup.health_plan.is_none() {
        workflow_teploty::updates::installer::cleanup(&directory);
        if !demo
            && lifecycle::Preferences::load(&directory).auto_updates
            && !args.iter().any(|a| a == "--skip-update-once")
            && !directory.join("power-recovery.json").exists()
        {
            let update = (|| -> anyhow::Result<bool> {
                let Some(signed) = workflow_teploty::updates::pending(&directory)? else {
                    return Ok(false);
                };
                if workflow_teploty::updates::last_result(&directory)
                    .is_some_and(|r| !r.success && r.version == signed.manifest.version)
                {
                    return Ok(false);
                }
                let resume = startup.rules_valid
                    && startup
                        .resume_control
                        .unwrap_or(lifecycle::Preferences::load(&directory).start_control);
                workflow_teploty::updates::installer::prepare(
                    &directory, signed, resume, background,
                )?;
                Ok(true)
            })();
            match update {
                Ok(true) => return Ok(()),
                Ok(false) => {}
                Err(error) => {
                    notice = crate::tf!(
                        "Automatická aktualizace se neprovedla: {error:#}",
                        "Automatic update was not applied: {error:#}"
                    )
                }
            }
        }
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1200., 740.])
            .with_min_inner_size([1000., 620.])
            .with_icon(ui::icon()),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            wgpu_setup: eframe::egui_wgpu::WgpuSetupCreateNew {
                power_preference: eframe::wgpu::PowerPreference::LowPower,
                instance_descriptor: eframe::wgpu::InstanceDescriptor {
                    backends: eframe::wgpu::Backends::DX12,
                    ..Default::default()
                },
                ..Default::default()
            }
            .into(),
            ..Default::default()
        },
        vsync: true,
        ..Default::default()
    };
    eframe::run_native(
        "CoolTakt",
        options,
        Box::new(move |cc| {
            Ok(Box::new(ui::App::new(
                cc, settings, directory, demo, background, notice, instance, startup,
            )))
        }),
    )
    .map_err(|e| crate::terr!("Okno aplikace: {e}", "Application window: {e}"))
}
fn main() {
    if let Err(e) = run() {
        if std::env::args().any(|a| a == "--apply-update") {
            std::process::exit(1);
        }
        native::show_error(&format!("{e:#}"));
    }
}
