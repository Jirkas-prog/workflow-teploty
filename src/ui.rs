use crate::{
    lifecycle::{self, Preferences, Tray, TrayAction},
    model::{Curve, NamedProfile, Point, Settings},
    native,
    worker::{Command, State, Worker},
};
use eframe::egui::{self, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{path::PathBuf, time::Duration};

#[derive(Clone, Copy)]
struct Palette {
    ink: Color32,
    muted: Color32,
    green: Color32,
    gold: Color32,
    border: Color32,
    bg: Color32,
    card: Color32,
    red: Color32,
    chart: Color32,
    selection: Color32,
}
impl Palette {
    fn new(dark: bool) -> Self {
        let rgb = Color32::from_rgb;
        if dark {
            Self {
                ink: rgb(230, 238, 244),
                muted: rgb(162, 181, 194),
                green: rgb(99, 207, 181),
                gold: rgb(226, 184, 93),
                border: rgb(57, 77, 89),
                bg: rgb(20, 29, 36),
                card: rgb(29, 41, 50),
                red: rgb(245, 133, 124),
                chart: rgb(24, 35, 42),
                selection: rgb(41, 77, 69),
            }
        } else {
            Self {
                ink: rgb(28, 43, 57),
                muted: rgb(91, 107, 120),
                green: rgb(22, 110, 98),
                gold: rgb(185, 135, 42),
                border: rgb(213, 222, 228),
                bg: rgb(245, 248, 250),
                card: Color32::WHITE,
                red: rgb(174, 58, 50),
                chart: rgb(248, 251, 250),
                selection: rgb(221, 240, 234),
            }
        }
    }
    fn ui(ui: &egui::Ui) -> Self {
        Self::new(ui.visuals().dark_mode)
    }
}
fn install_style(ctx: &egui::Context, dark: bool) {
    let mut fonts = egui::FontDefinitions::default();
    if let Ok(data) = std::fs::read(r"C:\Windows\Fonts\segoeui.ttf") {
        fonts
            .font_data
            .insert("segoe".into(), egui::FontData::from_owned(data).into());
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "segoe".into());
    }
    ctx.set_fonts(fonts);
    apply_theme(ctx, dark);
}
fn apply_theme(ctx: &egui::Context, dark: bool) {
    let colors = Palette::new(dark);
    ctx.set_theme(if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    let mut style = (*ctx.style()).clone();
    style.visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    style.visuals.override_text_color = Some(colors.ink);
    style.visuals.panel_fill = colors.bg;
    style.visuals.window_fill = colors.card;
    style.visuals.selection.bg_fill = colors.selection;
    style.visuals.selection.stroke = Stroke::new(1_f32, colors.green);
    style.visuals.widgets.inactive.bg_fill = colors.card;
    style.visuals.widgets.inactive.weak_bg_fill = colors.card;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1_f32, colors.border);
    style.visuals.widgets.hovered.bg_fill = colors.selection;
    style.visuals.widgets.active.bg_fill = colors.selection;
    style.spacing.item_spacing = Vec2::new(8., 6.);
    style.spacing.button_padding = Vec2::new(10., 5.);
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(14.));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(14.));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, FontId::proportional(23.));
    ctx.set_style(style);
}
pub fn icon() -> egui::IconData {
    let mut rgba = Vec::new();
    for y in 0i32..32 {
        for x in 0i32..32 {
            let mut c = [245, 248, 250, 255];
            if (x - 16).pow(2) + (y - 23).pow(2) <= 49
                || ((12..=20).contains(&x) && (4..=24).contains(&y))
            {
                c = [22, 110, 98, 255];
            }
            if (14..=18).contains(&x) && (7..=22).contains(&y) {
                c = [241, 238, 218, 255];
            }
            if (x - 16).pow(2) + (y - 23).pow(2) <= 16
                || ((15..=17).contains(&x) && (13..=23).contains(&y))
            {
                c = [211, 181, 76, 255];
            }
            rgba.extend(c);
        }
    }
    egui::IconData {
        rgba,
        width: 32,
        height: 32,
    }
}
pub struct App {
    settings: Settings,
    worker: Worker,
    directory: PathBuf,
    demo: bool,
    selected: [Option<usize>; 2],
    tab: usize,
    notice: String,
    dirty: bool,
    unlock_confirmation: bool,
    preferences: Preferences,
    tray: Option<Tray>,
    _listener: native::InstanceListener,
    _instance: native::Instance,
    quitting: bool,
    hide_first_frame: bool,
    startup_enabled: bool,
    startup_result: Option<std::sync::mpsc::Receiver<anyhow::Result<bool>>>,
    window: isize,
    profiles: Vec<NamedProfile>,
    profile_name: String,
    profiles_readable: bool,
    history_series: [bool; 3],
}
impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        settings: Settings,
        directory: PathBuf,
        demo: bool,
        background: bool,
        mut notice: String,
        instance: native::Instance,
    ) -> Self {
        let preferences = Preferences::load(&directory);
        crate::i18n::set(preferences.language);
        install_style(&cc.egui_ctx, preferences.dark);
        let worker = Worker::new(
            settings.clone(),
            directory.clone(),
            demo,
            preferences.start_control && notice.is_empty(),
        );
        let window = cc
            .window_handle()
            .ok()
            .and_then(|h| match h.as_raw() {
                RawWindowHandle::Win32(w) => Some(w.hwnd.get()),
                _ => None,
            })
            .unwrap_or(0);
        let tray = match Tray::new(&cc.egui_ctx, window) {
            Ok(tray) => Some(tray),
            Err(e) => {
                notice = crate::tf!(
                    "Ikonu u hodin nelze vytvořit: {e}. Okno zůstane otevřené.",
                    "Cannot create tray icon: {e}. The window will stay open."
                );
                None
            }
        };
        let listener = instance.listen(cc.egui_ctx.clone(), window);
        let startup_result =
            (!demo).then(|| lifecycle::startup_async(None, directory.clone(), cc.egui_ctx.clone()));
        let profile_path = directory.join("profily.json");
        let profiles_result = if profile_path.exists() {
            std::fs::read(&profile_path)
                .map_err(anyhow::Error::from)
                .and_then(|bytes| Ok(serde_json::from_slice::<Vec<NamedProfile>>(&bytes)?))
                .and_then(|profiles| {
                    crate::model::validate_profiles(&profiles).map_err(anyhow::Error::msg)?;
                    Ok(profiles)
                })
        } else {
            Ok(Vec::new())
        };
        let profiles_readable = profiles_result.is_ok();
        let profiles = profiles_result.unwrap_or_else(|e| {
            notice = crate::tf!(
                "Profily nelze načíst; původní soubor zůstal zachován. {e}",
                "Cannot load profiles; the original file was preserved. {e}"
            );
            Vec::new()
        });
        Self {
            settings,
            worker,
            directory,
            demo,
            selected: [None, None],
            tab: 0,
            notice,
            dirty: false,
            unlock_confirmation: false,
            preferences,
            tray,
            _listener: listener,
            _instance: instance,
            quitting: false,
            hide_first_frame: background,
            startup_enabled: false,
            startup_result,
            window,
            profiles,
            profile_name: String::new(),
            profiles_readable,
            history_series: [true, true, false],
        }
    }
    fn save_preferences(&mut self) {
        if let Err(e) = self.preferences.save(&self.directory) {
            self.notice = crate::tf!(
                "Volby aplikace nelze uložit: {e}",
                "Cannot save application preferences: {e}"
            );
        }
    }
    fn export(&mut self, state: &State) {
        let path = self.directory.join(format!(
            "mereni-{}.csv",
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        ));
        let mut csv = String::from(crate::tr!(
            "\u{feff}Čas;CPU °C;Baterie °C;GPU °C;CPU %;Odběr W;Limit %;Na nabíječce\r\n",
            "\u{feff}Time;CPU °C;Battery °C;GPU °C;CPU %;Power W;Limit %;AC connected\r\n"
        ));
        let n = |v: Option<f32>| {
            v.map(|v| format!("{v:.1}").replace('.', ","))
                .unwrap_or_default()
        };
        for point in &state.history {
            let s = &point.sample;
            csv.push_str(&format!(
                "{};{};{};{};{};{};{:.0};{}\r\n",
                s.timestamp,
                n(s.cpu_temperature),
                n(s.battery_temperature),
                n(s.gpu_temperature),
                n(s.cpu_usage),
                n(s.discharge_watts),
                point.limit,
                if s.on_ac {
                    crate::tr!("ano", "yes")
                } else {
                    crate::tr!("ne", "no")
                }
            ));
        }
        self.notice = match std::fs::write(&path, csv) {
            Ok(()) => crate::tf!(
                "Měření uloženo: {}",
                "Measurements saved: {}",
                path.display()
            ),
            Err(e) => crate::tf!("Export selhal: {e}", "Export failed: {e}"),
        };
    }
}
fn button(ui: &mut egui::Ui, text: &str, primary: bool) -> egui::Response {
    let colors = Palette::ui(ui);
    ui.add(
        egui::Button::new(RichText::new(text).color(if primary {
            Color32::WHITE
        } else {
            colors.ink
        }))
        .fill(if primary {
            Color32::from_rgb(22, 110, 98)
        } else {
            colors.card
        })
        .min_size(Vec2::new(110., 36.)),
    )
}
fn value(temp: Option<f32>, unit: &str) -> String {
    temp.map(|t| format!("{t:.1} {unit}"))
        .unwrap_or_else(|| "—".into())
}
fn metric(ui: &mut egui::Ui, label: &str, value: &str, detail: &str, color: Color32) {
    let colors = Palette::ui(ui);
    egui::Frame::new()
        .fill(colors.card)
        .stroke(Stroke::new(1_f32, colors.border))
        .corner_radius(8)
        .inner_margin(14)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new(label).size(12.).color(colors.muted));
            ui.label(RichText::new(value).size(27.).strong().color(color));
            ui.label(RichText::new(detail).size(11.).color(colors.muted));
        });
}
impl App {
    fn conditions(&mut self, ui: &mut egui::Ui, state: &State) {
        let colors = Palette::ui(ui);
        ui.strong(crate::tr!("Kdy regulovat", "When to regulate"));
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(
                &mut self.settings.closed_lid_only,
                crate::tr!(
                    "Regulovat pouze při zavřeném víku",
                    "Regulate only with the lid closed"
                ),
            );
            ui.colored_label(colors.green, state.sample.lid_state.label());
        });
        ui.label(RichText::new(crate::tr!("Otevření víka obnoví původní výkon a vypne teplotní uspávání CPU i baterie.", "Opening the lid restores original performance and disables CPU and battery thermal sleep.")).size(12.).color(colors.muted));
        ui.checkbox(
            &mut self.settings.battery_only,
            crate::tr!(
                "Regulovat pouze při provozu na baterii",
                "Regulate only when running on battery"
            ),
        );
        if self.settings.battery_only {
            ui.label(
                RichText::new(crate::tr!(
                    "Připojení nabíječky pozastaví regulaci i teplotní uspávání.",
                    "Connecting AC power pauses regulation and thermal sleep."
                ))
                .size(12.)
                .color(colors.muted),
            );
        }
    }

    fn overview(&mut self, ui: &mut egui::Ui, state: &State) {
        let colors = Palette::ui(ui);
        ui.columns(4, |columns| {
            metric(
                &mut columns[0],
                "CPU",
                &value(state.sample.cpu_temperature, "°C"),
                &crate::tf!(
                    "Vytížení {}",
                    "Usage {}",
                    value(state.sample.cpu_usage, "%")
                ),
                colors.green,
            );
            metric(
                &mut columns[1],
                crate::tr!("BATERIE", "BATTERY"),
                &value(state.sample.battery_temperature, "°C"),
                if state.sample.battery_temperature.is_some() {
                    crate::tr!("Teplota baterie", "Battery temperature")
                } else {
                    crate::tr!("Čidlo není dostupné", "Sensor unavailable")
                },
                colors.gold,
            );
            metric(
                &mut columns[2],
                crate::tr!("ODBĚR", "POWER DRAW"),
                &value(state.sample.discharge_watts, "W"),
                if state.sample.on_ac {
                    crate::tr!("Na nabíječce", "AC connected")
                } else {
                    crate::tr!("Provoz na baterii", "Running on battery")
                },
                colors.ink,
            );
            metric(
                &mut columns[3],
                crate::tr!("VÝKON CPU", "CPU LIMIT"),
                &format!("{:.0} %", state.limit),
                if state.enabled {
                    crate::tr!("Z původního nastavení", "Of original settings")
                } else {
                    crate::tr!("Bez regulace", "Regulation off")
                },
                colors.green,
            );
        });
        ui.add_space(8.);
        card(ui, |ui| self.conditions(ui, state));
        ui.add_space(8.);
        ui.columns(2, |columns| {
            card(&mut columns[0], |ui| {
                ui.strong(crate::tr!("Aktivní pravidla", "Applied rules"));
                let applied = &state.applied_settings;
                for (name, curve) in [
                    ("CPU", &applied.cpu),
                    (crate::tr!("Baterie", "Battery"), &applied.battery),
                ] {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(name).strong());
                        ui.label(if curve.enabled {
                            crate::tr!("Křivka zapnutá", "Curve enabled")
                        } else {
                            crate::tr!("Křivka vypnutá", "Curve disabled")
                        });
                    });
                    if curve.sleep.enabled {
                        ui.label(crate::tf!(
                            "Uspat při {:.0} °C",
                            "Sleep at {:.0} °C",
                            curve.sleep.temperature
                        ));
                    }
                }
                if ui
                    .button(crate::tr!(
                        "Upravit profily a křivky",
                        "Edit profiles and curves"
                    ))
                    .clicked()
                {
                    self.tab = 2;
                }
            });
            card(&mut columns[1], |ui| {
                ui.strong(crate::tr!("Stav měření", "Sensor status"));
                ui.label(&state.sample.cpu_source);
                ui.label(&state.sample.battery_source);
                ui.label(&state.sample.lid_source);
                if self.demo {
                    ui.colored_label(
                        colors.gold,
                        crate::tr!(
                            "Ukázková data. Výkon ani spánek se nemění.",
                            "Demo data. No performance or sleep changes."
                        ),
                    );
                }
            });
        });
    }

    fn history(&mut self, ui: &mut egui::Ui, state: &State) {
        let colors = Palette::ui(ui);
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.history_series[0], "CPU");
            ui.checkbox(
                &mut self.history_series[1],
                crate::tr!("Baterie", "Battery"),
            );
            ui.checkbox(&mut self.history_series[2], "GPU");
            ui.separator();
            if ui
                .button(crate::tr!("Exportovat CSV", "Export CSV"))
                .clicked()
            {
                self.export(state);
            }
            ui.label(crate::tf!(
                "{} / 900 měření",
                "{} / 900 readings",
                state.history.len()
            ));
        });
        let height = (ui.available_height() - 160.).clamp(200., 420.);
        history_chart(ui, state, height, self.history_series);
        ui.label(RichText::new(crate::tr!("Body: minimum / maximum · červeně: překročení meze uspání · svisle: změna limitu nebo víka. Najetím zobrazíš přesné hodnoty.", "Points: minimum / maximum · red: sleep threshold crossing · vertical marks: limit or lid changes. Hover for exact readings.")).size(12.).color(colors.muted));
        egui::CollapsingHeader::new(crate::tr!(
            "Události a zdroje měření",
            "Events and sensor sources"
        ))
        .default_open(false)
        .show(ui, |ui| {
            ui.label(&state.sample.cpu_source);
            ui.label(&state.sample.battery_source);
            for line in &state.events {
                ui.label(RichText::new(line).size(12.));
            }
        });
    }

    fn profiles(&mut self, ui: &mut egui::Ui, state: &State) {
        let mut load = None;
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("saved-profile")
                .selected_text(crate::tr!("Načíst uložený profil", "Load saved profile"))
                .width(190.)
                .show_ui(ui, |ui| {
                    for (index, profile) in self.profiles.iter().enumerate() {
                        if ui.selectable_label(false, &profile.name).clicked() {
                            load = Some(index);
                        }
                    }
                    if self.profiles.is_empty() {
                        ui.label(crate::tr!(
                            "Zatím žádné uložené profily",
                            "No saved profiles yet"
                        ));
                    }
                });
            ui.add(
                egui::TextEdit::singleline(&mut self.profile_name)
                    .hint_text(crate::tr!("Název profilu", "Profile name"))
                    .desired_width(150.)
                    .char_limit(60),
            );
            if ui
                .add_enabled(
                    self.profiles_readable
                        && !self.profile_name.trim().is_empty()
                        && self.settings.validate().is_ok(),
                    egui::Button::new(crate::tr!("Uložit profil", "Save profile")),
                )
                .clicked()
            {
                let mut profiles = self.profiles.clone();
                let profile = NamedProfile {
                    name: self.profile_name.trim().into(),
                    settings: self.settings.clone(),
                };
                if let Some(existing) = profiles.iter_mut().find(|p| p.name == profile.name) {
                    *existing = profile;
                } else {
                    profiles.push(profile);
                }
                let result = crate::model::validate_profiles(&profiles)
                    .map_err(anyhow::Error::msg)
                    .and_then(|()| {
                        native::write_json(&self.directory.join("profily.json"), &profiles)
                    });
                match result {
                    Ok(()) => {
                        self.profiles = profiles;
                        self.notice = crate::tr!(
                            "Profil uložen. Aktivní pravidla změní až Uložit a použít.",
                            "Profile saved. Save and apply changes the active rules."
                        )
                        .into();
                    }
                    Err(e) => self.notice = e.to_string(),
                }
            }
        });
        if let Some(index) = load {
            let profile = &self.profiles[index];
            let mut settings = profile.settings.clone();
            if settings.resume_without_password && !self.settings.resume_without_password {
                settings.resume_without_password = false;
                self.unlock_confirmation = true;
            }
            self.settings = settings;
            self.profile_name = profile.name.clone();
            self.selected = [None, None];
            self.notice = crate::tr!(
                "Profil načten k úpravám. Změny aktivuje Uložit a použít.",
                "Profile loaded for editing. Save and apply activates the changes."
            )
            .into();
        }
        ui.label(RichText::new(crate::tr!("Teplota → povolený výkon CPU. Přísnější křivka má přednost; 100 % zachová původní nastavení.", "Temperature → allowed CPU performance. The stricter curve wins; 100% preserves original settings.")).size(12.).color(Palette::ui(ui).muted));
        ui.add_space(4.);
        ui.columns(2, |columns| {
            curve_card(
                &mut columns[0],
                crate::tr!("Procesor", "CPU"),
                &mut self.settings.cpu,
                state.sample.cpu_temperature,
                &mut self.selected[0],
                false,
            );
            curve_card(
                &mut columns[1],
                crate::tr!("Baterie", "Battery"),
                &mut self.settings.battery,
                state.sample.battery_temperature,
                &mut self.selected[1],
                true,
            );
        });
    }

    fn settings_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, state: &State) {
        let colors = Palette::ui(ui);
        card(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.strong(crate::tr!("Vzhled", "Appearance"));
                let old = self.preferences.dark;
                ui.selectable_value(
                    &mut self.preferences.dark,
                    false,
                    crate::tr!("Světlý", "Light"),
                );
                ui.selectable_value(
                    &mut self.preferences.dark,
                    true,
                    crate::tr!("Tmavý", "Dark"),
                );
                if old != self.preferences.dark {
                    apply_theme(ctx, self.preferences.dark);
                    self.save_preferences();
                }
                ui.separator();
                ui.label(crate::tr!("Jazyk", "Language"));
                let old = self.preferences.language;
                ui.selectable_value(
                    &mut self.preferences.language,
                    crate::i18n::Language::Cs,
                    "Čeština",
                );
                ui.selectable_value(
                    &mut self.preferences.language,
                    crate::i18n::Language::En,
                    "English",
                );
                if old != self.preferences.language {
                    crate::i18n::set(self.preferences.language);
                    if let Some(tray) = &self.tray {
                        tray.refresh_language();
                    }
                    self.worker.send(Command::LanguageChanged);
                    self.save_preferences();
                }
            });
        });
        ui.add_space(4.);
        card(ui, |ui| {
            ui.strong(crate::tr!(
                "Spouštění a běh na pozadí",
                "Startup and background operation"
            ));
            let mut startup = self.startup_enabled;
            if ui
                .add_enabled(
                    !self.demo && self.startup_result.is_none(),
                    egui::Checkbox::new(
                        &mut startup,
                        crate::tr!(
                            "Spouštět aplikaci po přihlášení k Windows",
                            "Launch the application at Windows sign-in"
                        ),
                    ),
                )
                .changed()
            {
                self.startup_result = Some(lifecycle::startup_async(
                    Some(startup),
                    self.directory.clone(),
                    ctx.clone(),
                ));
            }
            if ui
                .checkbox(
                    &mut self.preferences.start_control,
                    crate::tr!(
                        "Po spuštění aplikace automaticky zapnout regulaci",
                        "Automatically start regulation when the application launches"
                    ),
                )
                .changed()
            {
                self.save_preferences();
            }
            ui.label(RichText::new(crate::tr!("Jinak se pouze měří teploty. Zavření okna ponechá aplikaci u hodin; Ukončit obnoví výkon a aplikaci vypne.", "Otherwise, temperatures are only monitored. Closing the window keeps the app in the tray; Quit restores performance and exits.")).size(12.).color(colors.muted));
            if self.demo {
                ui.colored_label(
                    colors.gold,
                    crate::tr!(
                        "Ukázkový režim: start s Windows a regulace jsou nedostupné.",
                        "Demo mode: Windows startup and regulation are unavailable."
                    ),
                );
            }
        });
        ui.add_space(4.);
        card(ui, |ui| {
            ui.strong(crate::tr!(
                "Měření a návrat výkonu",
                "Sampling and performance recovery"
            ));
            ui.horizontal_wrapped(|ui| {
                ui.label(crate::tr!("Měřit každých", "Sample every"));
                ui.add(
                    egui::DragValue::new(&mut self.settings.sample_seconds)
                        .range(2..=10)
                        .suffix(" s"),
                );
                ui.separator();
                ui.label(crate::tr!(
                    "Po ochlazení zvýšit výkon za",
                    "Increase performance after cooling for"
                ));
                ui.add(
                    egui::DragValue::new(&mut self.settings.recovery_seconds)
                        .range(5..=120)
                        .suffix(" s"),
                );
            });
            let mut unlock = self.settings.resume_without_password;
            if ui
                .checkbox(
                    &mut unlock,
                    crate::tr!(
                        "Po automatickém probuzení nevyžadovat heslo",
                        "Do not require a password after automatic wake"
                    ),
                )
                .changed()
            {
                if unlock {
                    self.unlock_confirmation = true;
                } else {
                    self.settings.resume_without_password = false;
                }
            }
            ui.label(RichText::new(crate::tr!("Spánek pozastaví výpočty i přenosy. Po probuzení práce pokračuje, přenos může potřebovat obnovit spojení.", "Sleep pauses computations and transfers. Work resumes after wake; a transfer may need to reconnect.")).size(12.).color(colors.muted));
        });
        ui.add_space(4.);
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(crate::tr!("Spustit jako správce", "Run as administrator"))
                .clicked()
            {
                if state.enabled || self.settings != state.applied_settings {
                    self.notice = crate::tr!(
                        "Nejdřív ulož změny a zastav regulaci.",
                        "Save changes and stop regulation first."
                    )
                    .into();
                } else {
                    match native::elevate(&self.directory, self.demo) {
                        Ok(()) => {
                            self.quitting = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        Err(e) => self.notice = e.to_string(),
                    }
                }
            }
            if ui
                .button(crate::tr!("Složka nastavení", "Settings folder"))
                .clicked()
                && let Err(e) = native::open_path(&self.directory)
            {
                self.notice = e.to_string();
            }
            if state.needs_recovery
                && !state.enabled
                && ui
                    .button(crate::tr!(
                        "Obnovit nastavení napájení",
                        "Recover power settings"
                    ))
                    .clicked()
            {
                self.worker.send(Command::Recover);
            }
        });
    }
}

fn card<R>(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let colors = Palette::ui(ui);
    egui::Frame::new()
        .fill(colors.card)
        .stroke(Stroke::new(1_f32, colors.border))
        .corner_radius(7)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            body(ui)
        })
        .inner
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_secs(1));
        let colors = Palette::new(self.preferences.dark);
        if let Some(result) = self
            .startup_result
            .as_ref()
            .and_then(|rx| rx.try_recv().ok())
        {
            self.startup_result = None;
            match result {
                Ok(enabled) => self.startup_enabled = enabled,
                Err(e) => self.notice = crate::tf!("Automatický start: {e:#}", "Startup: {e:#}"),
            }
        }
        let tray_events: Vec<_> = self
            .tray
            .as_ref()
            .map(|t| t.events.try_iter().collect())
            .unwrap_or_default();
        for action in tray_events {
            match action {
                TrayAction::Show => lifecycle::show_window(ctx, self.window),
                TrayAction::Pause => self.worker.send(Command::Disable),
                TrayAction::Quit => {
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        if self.hide_first_frame {
            self.hide_first_frame = false;
            if self.tray.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting && self.tray.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        let state = self.worker.snapshot();
        let pages = [
            crate::tr!("Přehled", "Overview"),
            crate::tr!("Teploty v čase", "Temperature history"),
            crate::tr!("Profily a křivky", "Profiles and curves"),
            crate::tr!("Nastavení", "Settings"),
        ];
        egui::SidePanel::left("navigation")
            .exact_width(190.)
            .resizable(false)
            .frame(egui::Frame::new().fill(colors.card).inner_margin(14))
            .show(ctx, |ui| {
                ui.add_space(6.);
                ui.label(
                    RichText::new("CoolTakt")
                        .size(24.)
                        .strong()
                        .color(colors.green),
                );
                ui.label(
                    RichText::new(crate::tr!("Výkon podle teploty", "Thermal control"))
                        .size(12.)
                        .color(colors.muted),
                );
                ui.add_space(20.);
                for (index, title) in pages.iter().enumerate() {
                    if ui
                        .add_sized(
                            [ui.available_width(), 36.],
                            egui::Button::new(*title).selected(self.tab == index),
                        )
                        .clicked()
                    {
                        self.tab = index;
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.label(
                        RichText::new("v0.1.0 · Rust / Windows")
                            .size(10.)
                            .color(colors.muted),
                    );
                    if ui
                        .button(crate::tr!("Ukončit aplikaci", "Quit application"))
                        .clicked()
                    {
                        self.quitting = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if self.tray.is_some()
                        && ui
                            .button(crate::tr!("Skrýt k hodinám", "Hide to tray"))
                            .clicked()
                    {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                    }
                    ui.add_space(8.);
                    ui.colored_label(
                        if state.enabled {
                            colors.green
                        } else {
                            colors.muted
                        },
                        if self.demo {
                            crate::tr!("UKÁZKOVÝ REŽIM", "DEMO MODE")
                        } else if state.enabled {
                            crate::tr!("REGULACE ZAPNUTÁ", "REGULATION ON")
                        } else {
                            crate::tr!("POUZE SLEDOVÁNÍ", "MONITORING ONLY")
                        },
                    );
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(colors.bg).inner_margin(18))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(RichText::new(pages[self.tab]).size(25.));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(state.sample.lid_state.label())
                                .size(12.)
                                .color(colors.muted),
                        );
                    });
                });
                ui.add_space(8.);
                if let Some((source, seconds)) = &state.countdown {
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(
                            colors.red,
                            crate::tf!(
                                "{source}: uspání za {seconds} s",
                                "{source}: sleep in {seconds} s"
                            ),
                        );
                        if ui
                            .button(crate::tr!("Zrušit uspání", "Cancel sleep"))
                            .clicked()
                        {
                            self.worker.send(Command::CancelSleep);
                        }
                    });
                }
                let footer_height = if self.notice.is_empty() && self.settings.validate().is_ok() {
                    66.
                } else {
                    90.
                };
                let page_height = (ui.available_height() - footer_height).max(100.);
                ui.allocate_ui_with_layout(
                    Vec2::new(ui.available_width(), page_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt(("page", self.tab))
                            .auto_shrink([false, false])
                            .show(ui, |ui| match self.tab {
                                0 => self.overview(ui, &state),
                                1 => self.history(ui, &state),
                                2 => self.profiles(ui, &state),
                                _ => self.settings_page(ui, ctx, &state),
                            });
                    },
                );
                ui.separator();
                self.dirty = self.settings != state.applied_settings;
                let valid = self.settings.validate();
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            valid.is_ok(),
                            egui::Button::new(crate::tr!("Uložit a použít", "Save and apply")),
                        )
                        .clicked()
                    {
                        self.worker.send(Command::Apply(self.settings.clone()));
                        self.notice.clear();
                    }
                    let can_start = !self.demo
                        && !self.dirty
                        && !state.needs_recovery
                        && state
                            .sampled_at
                            .is_some_and(|t| t.elapsed() < Duration::from_secs(10))
                        && crate::model::requested_limit(&self.settings, &state.sample).is_ok()
                        && valid.is_ok();
                    if state.enabled {
                        if button(ui, crate::tr!("Zastavit regulaci", "Stop regulation"), true)
                            .clicked()
                        {
                            self.worker.send(Command::Disable);
                        }
                    } else if ui
                        .add_enabled(
                            can_start,
                            egui::Button::new(crate::tr!("Zapnout regulaci", "Start regulation")),
                        )
                        .clicked()
                    {
                        self.worker.send(Command::Enable);
                    }
                    if self.dirty {
                        ui.colored_label(
                            colors.gold,
                            crate::tr!("Neuložené změny", "Unsaved changes"),
                        );
                    }
                });
                ui.add(
                    egui::Label::new(RichText::new(&state.status).size(11.).color(colors.muted))
                        .truncate(),
                )
                .on_hover_text(&state.status);
                if let Err(error) = valid {
                    ui.add(egui::Label::new(RichText::new(error).color(colors.red)).truncate());
                } else if !self.notice.is_empty() {
                    ui.add(
                        egui::Label::new(RichText::new(&self.notice).size(12.).color(colors.muted))
                            .truncate(),
                    )
                    .on_hover_text(&self.notice);
                }
            });
        if self.unlock_confirmation {
            egui::Window::new(crate::tr!("Probuzení bez hesla", "Resume without a password")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO).default_width(440.).show(ctx, |ui| {
                ui.label(crate::tr!("Po automatickém probuzení může mít k otevřené relaci přístup kdokoliv u notebooku. Heslo se neukládá; dočasně se změní požadavek Windows na heslo při probuzení.", "After automatic wake, anyone at the computer may access the open session. No password is stored; the Windows wake password requirement is temporarily changed."));
                ui.label(crate::tr!("Firemní zásady, ruční zamčení nebo Windows Hello mohou návrat bez hesla znemožnit. Práce může pokračovat i při zamčené obrazovce.", "Organization policies, manual locking or Windows Hello may prevent password-free resume. Work can continue while the screen is locked."));
                ui.horizontal_wrapped(|ui| {
                    if ui.button(crate::tr!("Ponechat heslo", "Keep password required")).clicked() { self.unlock_confirmation = false; }
                    if ui.button(crate::tr!("Povolit bez hesla", "Allow password-free resume")).clicked() { self.settings.resume_without_password = true; self.unlock_confirmation = false; }
                });
            });
        }
    }
}

fn curve_card(
    ui: &mut egui::Ui,
    title: &str,
    curve: &mut Curve,
    current: Option<f32>,
    selected: &mut Option<usize>,
    battery: bool,
) -> bool {
    let before = curve.clone();
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(title).size(20.).strong());
            ui.label(value(current, "°C"));
        });
        ui.add_enabled(
            !battery || current.is_some() || curve.enabled,
            egui::Checkbox::new(
                &mut curve.enabled,
                crate::tr!(
                    "Omezovat výkon podle této teploty",
                    "Limit performance using this temperature"
                ),
            ),
        );
        curve_graph(ui, curve, current, selected, battery);
        ui.label(
            RichText::new(crate::tr!(
                "Přetáhni bod · dvojklik přidá bod",
                "Drag a point · double-click to add one"
            ))
            .size(11.)
            .color(Palette::ui(ui).muted),
        );
        egui::CollapsingHeader::new(crate::tf!(
            "Přesné hodnoty · {} bodů",
            "Exact values · {} points",
            curve.points.len()
        ))
        .id_salt(("points", battery))
        .show(ui, |ui| {
            let mut remove = None;
            let can_remove = curve.points.len() > 2;
            egui::Grid::new(("point-grid", battery))
                .striped(true)
                .show(ui, |ui| {
                    ui.strong(crate::tr!("Teplota", "Temperature"));
                    ui.strong(crate::tr!("Výkon CPU", "CPU limit"));
                    ui.end_row();
                    for (index, point) in curve.points.iter_mut().enumerate() {
                        ui.add(
                            egui::DragValue::new(&mut point.temperature)
                                .speed(0.5)
                                .range(if battery { 0.0..=80.0 } else { 10.0..=110.0 })
                                .suffix(" °C"),
                        );
                        ui.add(
                            egui::DragValue::new(&mut point.limit)
                                .speed(1.)
                                .range(5.0..=100.0)
                                .suffix(" %"),
                        );
                        if ui
                            .add_enabled(can_remove, egui::Button::new("−"))
                            .on_hover_text(crate::tr!("Odebrat bod", "Remove point"))
                            .clicked()
                        {
                            remove = Some(index);
                        }
                        ui.end_row();
                    }
                });
            if let Some(index) = remove {
                curve.points.remove(index);
                *selected = None;
            }
        });
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled(
                !battery || current.is_some() || curve.sleep.enabled,
                egui::Checkbox::new(
                    &mut curve.sleep.enabled,
                    crate::tr!("Uspat při", "Sleep at"),
                ),
            );
            ui.add_enabled(
                curve.sleep.enabled,
                egui::DragValue::new(&mut curve.sleep.temperature)
                    .range(if battery { 0.0..=80.0 } else { 10.0..=110.0 })
                    .suffix(" °C"),
            );
        });
        ui.add_enabled_ui(curve.sleep.enabled, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(
                    &mut curve.sleep.wake_automatically,
                    crate::tr!("Automaticky probudit po", "Automatically wake after"),
                );
                ui.add_enabled(
                    curve.sleep.wake_automatically,
                    egui::DragValue::new(&mut curve.sleep.minutes)
                        .range(1..=120)
                        .suffix(" min"),
                );
            });
        });
        if !curve.sleep.wake_automatically {
            ui.label(
                RichText::new(crate::tr!("Probuzení pouze ručně.", "Manual wake only."))
                    .size(11.)
                    .color(Palette::ui(ui).muted),
            );
        }
        if battery && current.is_none() {
            ui.label(
                RichText::new(crate::tr!(
                    "Čidlo není dostupné. Křivku lze připravit; pravidla zatím nelze zapnout.",
                    "Sensor unavailable. You can edit the curve; its rules cannot be enabled yet."
                ))
                .size(12.)
                .color(Palette::ui(ui).gold),
            );
        }
    });
    before != *curve
}

fn curve_graph(
    ui: &mut egui::Ui,
    curve: &mut Curve,
    current: Option<f32>,
    selected: &mut Option<usize>,
    battery: bool,
) -> bool {
    let colors = Palette::ui(ui);
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 160.),
        Sense::click_and_drag(),
    );
    let chart = Rect::from_min_max(
        rect.min + Vec2::new(34., 18.),
        rect.max - Vec2::new(12., 26.),
    );
    let low = if battery { 0. } else { 10. };
    let high = if battery { 80. } else { 110. };
    let min = curve
        .points
        .first()
        .map_or(low, |p| (p.temperature - 5.).max(low));
    let max = curve
        .points
        .last()
        .map_or(high, |p| (p.temperature + 5.).max(min + 10.).min(high));
    let to_screen = |t: f32, l: f32| {
        Pos2::new(
            egui::remap_clamp(t, min..=max, chart.x_range()),
            egui::remap_clamp(l, 0.0..=100.0, chart.bottom()..=chart.top()),
        )
    };
    let mut changed = false;
    if response.drag_started()
        && let Some(pos) = response.interact_pointer_pos()
    {
        *selected = curve
            .points
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                to_screen(a.temperature, a.limit)
                    .distance_sq(pos)
                    .total_cmp(&to_screen(b.temperature, b.limit).distance_sq(pos))
            })
            .filter(|(_, p)| to_screen(p.temperature, p.limit).distance(pos) < 20.)
            .map(|(i, _)| i);
    }
    if response.dragged()
        && let (Some(i), Some(pos)) = (*selected, response.interact_pointer_pos())
        && i < curve.points.len()
    {
        let left = if i == 0 {
            low
        } else {
            curve.points[i - 1].temperature + 0.5
        };
        let right = curve
            .points
            .get(i + 1)
            .map_or(high, |p| p.temperature - 0.5);
        let bottom = curve.points.get(i + 1).map_or(5., |p| p.limit);
        let top = if i == 0 {
            100.
        } else {
            curve.points[i - 1].limit
        };
        if left <= right && bottom <= top {
            curve.points[i].temperature = (egui::remap_clamp(pos.x, chart.x_range(), min..=max)
                * 2.)
                .round()
                .clamp(left * 2., right * 2.)
                / 2.;
            curve.points[i].limit =
                egui::remap_clamp(pos.y, chart.bottom()..=chart.top(), 0.0..=100.0)
                    .round()
                    .clamp(bottom, top);
            changed = true;
        }
    }
    if response.double_clicked()
        && curve.points.len() < 12
        && let Some(pos) = response.interact_pointer_pos()
        && chart.contains(pos)
    {
        let t = (egui::remap_clamp(pos.x, chart.x_range(), min..=max) * 2.).round() / 2.;
        if curve
            .points
            .iter()
            .all(|p| (p.temperature - t).abs() >= 0.5)
        {
            let index = curve.points.partition_point(|p| p.temperature < t);
            let lower = curve.points.get(index).map_or(5., |p| p.limit);
            let upper = if index == 0 {
                100.
            } else {
                curve.points[index - 1].limit
            };
            if lower <= upper {
                let limit = egui::remap_clamp(pos.y, chart.bottom()..=chart.top(), 0.0..=100.0)
                    .round()
                    .clamp(lower, upper);
                curve.points.insert(
                    index,
                    Point {
                        temperature: t,
                        limit,
                    },
                );
                *selected = Some(index);
                changed = true;
            }
        }
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(chart, 4, colors.chart);
    for l in [0., 25., 50., 75., 100.] {
        let y = to_screen(min, l).y;
        painter.line_segment(
            [Pos2::new(chart.left(), y), Pos2::new(chart.right(), y)],
            Stroke::new(1_f32, colors.border),
        );
        painter.text(
            Pos2::new(chart.left() - 6., y),
            egui::Align2::RIGHT_CENTER,
            format!("{l:.0}"),
            FontId::proportional(10.),
            colors.muted,
        );
    }
    for i in 0..=4 {
        let t = min + (max - min) * i as f32 / 4.;
        let x = to_screen(t, 0.).x;
        painter.text(
            Pos2::new(x, chart.bottom() + 12.),
            egui::Align2::CENTER_CENTER,
            format!("{t:.0}°"),
            FontId::proportional(11.),
            colors.muted,
        );
    }
    painter.text(
        rect.min,
        egui::Align2::LEFT_TOP,
        crate::tr!("výkon %", "performance %"),
        FontId::proportional(10.),
        colors.muted,
    );
    let color = if battery { colors.gold } else { colors.green };
    for pair in curve.points.windows(2) {
        painter.line_segment(
            [
                to_screen(pair[0].temperature, pair[0].limit),
                to_screen(pair[1].temperature, pair[1].limit),
            ],
            Stroke::new(2.5_f32, color),
        );
    }
    if let Some(t) = current {
        let pos = to_screen(t, curve.limit_at(t));
        painter.line_segment(
            [
                Pos2::new(pos.x, chart.top()),
                Pos2::new(pos.x, chart.bottom()),
            ],
            Stroke::new(1_f32, color.gamma_multiply(0.5)),
        );
        painter.circle_filled(pos, 5., colors.ink);
    }
    if curve.sleep.enabled && (min..=max).contains(&curve.sleep.temperature) {
        let x = to_screen(curve.sleep.temperature, 0.).x;
        painter.line_segment(
            [Pos2::new(x, chart.top()), Pos2::new(x, chart.bottom())],
            Stroke::new(1_f32, colors.red),
        );
    }
    for (i, p) in curve.points.iter().enumerate() {
        let pos = to_screen(p.temperature, p.limit);
        painter.circle_filled(
            pos,
            if *selected == Some(i) { 6.5 } else { 5. },
            colors.card,
        );
        painter.circle_stroke(
            pos,
            if *selected == Some(i) { 6.5 } else { 5. },
            Stroke::new(2_f32, color),
        );
    }
    changed
}
fn history_chart(ui: &mut egui::Ui, state: &State, height: f32, visible: [bool; 3]) {
    let colors = Palette::ui(ui);
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let chart = Rect::from_min_max(
        rect.min + Vec2::new(40., 24.),
        rect.max - Vec2::new(16., 30.),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(chart, 5, colors.card);
    let times: Vec<_> = state
        .history
        .iter()
        .map(|point| {
            chrono::DateTime::parse_from_rfc3339(&point.sample.timestamp)
                .ok()
                .map(|time| time.timestamp_millis())
        })
        .collect();
    let Some(start) = times.iter().flatten().next().copied() else {
        painter.text(
            chart.center(),
            egui::Align2::CENTER_CENTER,
            crate::tr!("Čekám na první měření…", "Waiting for the first reading…"),
            FontId::proportional(14.),
            colors.muted,
        );
        return;
    };
    let end = times
        .iter()
        .flatten()
        .last()
        .copied()
        .unwrap_or(start)
        .max(start + 1000);
    let duration = (end - start) as f32;
    let to_pos = |time: i64, temperature: f32| {
        Pos2::new(
            egui::remap((time - start) as f32, 0.0..=duration, chart.x_range()),
            egui::remap_clamp(temperature, 0.0..=110.0, chart.bottom()..=chart.top()),
        )
    };
    for temperature in [0., 20., 40., 60., 80., 100.] {
        let y = to_pos(start, temperature).y;
        painter.line_segment(
            [Pos2::new(chart.left(), y), Pos2::new(chart.right(), y)],
            Stroke::new(1_f32, colors.border),
        );
        painter.text(
            Pos2::new(chart.left() - 7., y),
            egui::Align2::RIGHT_CENTER,
            format!("{temperature:.0}"),
            FontId::proportional(11.),
            colors.muted,
        );
    }
    painter.text(
        rect.min,
        egui::Align2::LEFT_TOP,
        "°C",
        FontId::proportional(11.),
        colors.muted,
    );
    for tick in 0..=4 {
        let time = start + ((end - start) as f64 * f64::from(tick) / 4.) as i64;
        let label = chrono::DateTime::from_timestamp_millis(time)
            .map(|t| {
                t.with_timezone(&chrono::Local)
                    .format("%H:%M:%S")
                    .to_string()
            })
            .unwrap_or_default();
        painter.text(
            Pos2::new(to_pos(time, 0.).x, chart.bottom() + 15.),
            if tick == 0 {
                egui::Align2::LEFT_CENTER
            } else if tick == 4 {
                egui::Align2::RIGHT_CENTER
            } else {
                egui::Align2::CENTER_CENTER
            },
            label,
            FontId::proportional(11.),
            colors.muted,
        );
    }
    let sample_value = |point: &crate::worker::HistoryPoint, series| match series {
        0 => point.sample.cpu_temperature,
        1 => point.sample.battery_temperature,
        _ => point.sample.gpu_temperature,
    };
    for (series, show) in visible.into_iter().enumerate() {
        if !show {
            continue;
        }
        let color = [colors.green, colors.gold, colors.ink][series];
        let mut previous: Option<(Pos2, i64, f32)> = None;
        let mut min: Option<(Pos2, f32)> = None;
        let mut max: Option<(Pos2, f32)> = None;
        for (index, point) in state.history.iter().enumerate() {
            let Some((time, temperature)) = times[index]
                .zip(sample_value(point, series))
                .filter(|(_, t)| t.is_finite())
            else {
                previous = None;
                continue;
            };
            let pos = to_pos(time, temperature);
            if let Some((prev, prev_time, _)) = previous
                && (0..=30_000).contains(&(time - prev_time))
            {
                painter.line_segment([prev, pos], Stroke::new(2_f32, color));
            }
            let threshold = match series {
                0 => point.cpu_sleep_threshold,
                1 => point.battery_sleep_threshold,
                _ => None,
            };
            if let Some(threshold) = threshold
                && temperature >= threshold
                && previous.is_none_or(|(_, _, t)| t < threshold)
            {
                painter.circle_filled(pos, 5., colors.red);
                painter.circle_stroke(pos, 7., Stroke::new(1_f32, colors.red));
            }
            if min.is_none_or(|(_, t)| temperature < t) {
                min = Some((pos, temperature));
            }
            if max.is_none_or(|(_, t)| temperature > t) {
                max = Some((pos, temperature));
            }
            previous = Some((pos, time, temperature));
        }
        for (extreme, label) in [(min, "min"), (max, "max")] {
            if label == "min" && min.map(|p| p.1) == max.map(|p| p.1) {
                continue;
            }
            if let Some((pos, temperature)) = extreme {
                painter.circle_filled(pos, 4., color);
                let label_y = if label == "max" {
                    pos.y - 8.
                } else {
                    pos.y + 8.
                };
                let align = if label == "max" {
                    egui::Align2::CENTER_BOTTOM
                } else {
                    egui::Align2::CENTER_TOP
                };
                let name = ["CPU", crate::tr!("baterie", "battery"), "GPU"][series];
                painter.text(
                    Pos2::new(
                        pos.x.clamp(chart.left() + 65., chart.right() - 65.),
                        label_y.clamp(chart.top() + 16., chart.bottom() - 16.),
                    ),
                    align,
                    format!("{name} {label} {temperature:.1}°"),
                    FontId::proportional(11.),
                    color,
                );
            }
        }
    }
    for (index, time) in times.iter().enumerate().skip(1) {
        let prev = &state.history[index - 1];
        let point = &state.history[index];
        if ((point.limit - prev.limit).abs() >= 0.5
            || point.sample.lid_state != prev.sample.lid_state)
            && let Some(time) = *time
        {
            let x = to_pos(time, 0.).x;
            painter.line_segment(
                [
                    Pos2::new(x, chart.bottom() - 12.),
                    Pos2::new(x, chart.bottom()),
                ],
                Stroke::new(2_f32, colors.muted),
            );
        }
    }
    if let Some(pointer) = response.hover_pos().filter(|p| chart.contains(*p)) {
        let closest = times
            .iter()
            .enumerate()
            .filter_map(|(index, time)| time.map(|time| (index, time)))
            .min_by(|(_, a), (_, b)| {
                (to_pos(*a, 0.).x - pointer.x)
                    .abs()
                    .total_cmp(&(to_pos(*b, 0.).x - pointer.x).abs())
            });
        if let Some((index, time)) = closest {
            let point = &state.history[index];
            let x = to_pos(time, 0.).x;
            painter.line_segment(
                [Pos2::new(x, chart.top()), Pos2::new(x, chart.bottom())],
                Stroke::new(1_f32, colors.border),
            );
            response.on_hover_ui(|ui| {
                let time = chrono::DateTime::from_timestamp_millis(time)
                    .unwrap()
                    .with_timezone(&chrono::Local);
                ui.strong(time.format("%H:%M:%S").to_string());
                ui.label(format!(
                    "CPU {} · {} {} · GPU {}",
                    value(point.sample.cpu_temperature, "°C"),
                    crate::tr!("Baterie", "Battery"),
                    value(point.sample.battery_temperature, "°C"),
                    value(point.sample.gpu_temperature, "°C")
                ));
                ui.label(crate::tf!(
                    "Výkon CPU {:.0} % · {}",
                    "CPU limit {:.0}% · {}",
                    point.limit,
                    point.sample.lid_state.label()
                ));
            });
        }
    }
}
