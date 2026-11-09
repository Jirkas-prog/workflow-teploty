use anyhow::{Context, Result};
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::mpsc,
};
use tray_icon::{
    MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};
use windows::{
    Win32::System::{
        Com::{
            CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
            CoUninitialize,
        },
        TaskScheduler::*,
        Variant::VARIANT,
    },
    core::{BSTR, Interface},
};

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub dark: bool,
    pub start_control: bool,
    pub language: crate::i18n::Language,
    pub auto_updates: bool,
}
impl Preferences {
    pub fn load(directory: &Path) -> Self {
        std::fs::read(directory.join("aplikace.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
    pub fn save(&self, directory: &Path) -> Result<()> {
        crate::native::write_json(&directory.join("aplikace.json"), self)
    }
}

#[derive(Clone, Copy)]
pub enum TrayAction {
    Show,
    Pause,
    Quit,
}
pub struct Tray {
    _icon: TrayIcon,
    show: MenuItem,
    pause: MenuItem,
    quit: MenuItem,
    pub events: mpsc::Receiver<TrayAction>,
}
impl Tray {
    pub fn new(ctx: &egui::Context, window: isize) -> Result<Self> {
        let menu = Menu::new();
        let show = MenuItem::new(crate::tr!("Otevřít CoolTakt", "Open CoolTakt"), true, None);
        let pause = MenuItem::new(
            crate::tr!(
                "Zastavit regulaci a obnovit výkon",
                "Stop regulation and restore performance"
            ),
            true,
            None,
        );
        let quit = MenuItem::new(
            crate::tr!("Ukončit aplikaci", "Quit application"),
            true,
            None,
        );
        menu.append_items(&[&show, &pause, &PredefinedMenuItem::separator(), &quit])?;
        let icon = crate::ui::icon();
        let tray = TrayIconBuilder::new()
            .with_tooltip(crate::tr!(
                "CoolTakt · otevřít kliknutím",
                "CoolTakt · click to open"
            ))
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_icon(tray_icon::Icon::from_rgba(
                icon.rgba,
                icon.width,
                icon.height,
            )?)
            .build()?;
        let (tx, events) = mpsc::channel();
        let menu_tx = tx.clone();
        let repaint = ctx.clone();
        let show_id = show.id().clone();
        let pause_id = pause.id().clone();
        let quit_id = quit.id().clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let action = if event.id == show_id {
                Some(TrayAction::Show)
            } else if event.id == pause_id {
                Some(TrayAction::Pause)
            } else if event.id == quit_id {
                Some(TrayAction::Quit)
            } else {
                None
            };
            if let Some(action) = action {
                // Ensure the hidden native window processes the queued action too.
                show_window(&repaint, window);
                let _ = menu_tx.send(action);
                repaint.request_repaint();
            }
        }));
        let repaint = ctx.clone();
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_window(&repaint, window);
                let _ = tx.send(TrayAction::Show);
                repaint.request_repaint();
            }
        }));
        Ok(Self {
            _icon: tray,
            show,
            pause,
            quit,
            events,
        })
    }
    pub fn refresh_language(&self) {
        self.show
            .set_text(crate::tr!("Otevřít CoolTakt", "Open CoolTakt"));
        self.pause.set_text(crate::tr!(
            "Zastavit regulaci a obnovit výkon",
            "Stop regulation and restore performance"
        ));
        self.quit
            .set_text(crate::tr!("Ukončit aplikaci", "Quit application"));
        let _ = self._icon.set_tooltip(Some(crate::tr!(
            "CoolTakt · otevřít kliknutím",
            "CoolTakt · click to open"
        )));
    }
}

// Task Scheduler is used directly through COM; no scripts, shell or stored password.
struct Com;
impl Drop for Com {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}
const DESCRIPTION: &str = "WorkFlow.Teploty.v1 - background startup at user logon";
fn startup_task(change: Option<bool>, directory: &Path) -> Result<bool> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
    }
    let _com = Com;
    unsafe {
        let service: ITaskService = CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)?;
        let empty = VARIANT::default();
        service.Connect(&empty, &empty, &empty, &empty)?;
        let user = format!(
            "{}\\{}",
            service.ConnectedDomain()?,
            service.ConnectedUser()?
        );
        let task_name = BSTR::from(format!(
            "WorkFlow.Teploty.v1.{}",
            user.replace(['\\', '/', ':'], "_")
        ));
        let folder = service.GetFolder(&BSTR::from("\\"))?;
        let existing = match folder.GetTask(&task_name) {
            Ok(task) => Some(task),
            Err(e) if e.code().0 as u32 == 0x80070002 => None,
            Err(e) => return Err(e.into()),
        };
        if let Some(task) = &existing {
            let mut description = BSTR::new();
            task.Definition()?
                .RegistrationInfo()?
                .Description(&mut description)?;
            if description != DESCRIPTION {
                crate::tbail!(
                    "Úloha stejného názvu patří jinému programu; nebyla změněna.",
                    "A task with this name belongs to another application; it was not changed."
                );
            }
        }
        let Some(enable) = change else {
            return Ok(existing.is_some_and(|t| t.Enabled().is_ok_and(|v| v.as_bool())));
        };
        if !enable {
            if existing.is_some() {
                folder.DeleteTask(&task_name, 0)?;
            }
            return Ok(false);
        }
        let task = service.NewTask(0)?;
        task.RegistrationInfo()?
            .SetDescription(&BSTR::from(DESCRIPTION))?;
        let principal = task.Principal()?;
        principal.SetUserId(&BSTR::from(&user))?;
        principal.SetLogonType(TASK_LOGON_INTERACTIVE_TOKEN)?;
        principal.SetRunLevel(TASK_RUNLEVEL_HIGHEST)?;
        let trigger: ILogonTrigger = task.Triggers()?.Create(TASK_TRIGGER_LOGON)?.cast()?;
        trigger.SetUserId(&BSTR::from(&user))?;
        trigger.SetEnabled(true.into())?;
        let settings = task.Settings()?;
        settings.SetEnabled(true.into())?;
        settings.SetStartWhenAvailable(true.into())?;
        settings.SetDisallowStartIfOnBatteries(false.into())?;
        settings.SetStopIfGoingOnBatteries(false.into())?;
        settings.SetExecutionTimeLimit(&BSTR::from("PT0S"))?;
        settings.SetMultipleInstances(TASK_INSTANCES_IGNORE_NEW)?;
        let action: IExecAction = task.Actions()?.Create(TASK_ACTION_EXEC)?.cast()?;
        let exe = std::env::current_exe()?;
        action.SetPath(&BSTR::from(exe.to_string_lossy().as_ref()))?;
        action.SetArguments(&BSTR::from(format!(
            "--background --data-dir \"{}\"",
            directory.display()
        )))?;
        if let Some(parent) = exe.parent() {
            action.SetWorkingDirectory(&BSTR::from(parent.to_string_lossy().as_ref()))?;
        }
        folder.RegisterTaskDefinition(&task_name, &task, TASK_CREATE_OR_UPDATE.0, &VARIANT::from(user.as_str()), &empty, TASK_LOGON_INTERACTIVE_TOKEN, &empty)
            .context(crate::tr!("Automatický start vyžaduje oprávnění správce. Použij Spustit jako správce a volbu zapni znovu", "Startup setup needs administrator privileges. Choose Run as administrator, then enable startup again"))?;
        Ok(true)
    }
}

pub fn show_window(ctx: &egui::Context, window: isize) {
    crate::native::reveal_window(window);
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    ctx.request_repaint();
}
pub fn startup_async(
    change: Option<bool>,
    directory: PathBuf,
    ctx: egui::Context,
) -> mpsc::Receiver<Result<bool>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(startup_task(change, &directory));
        ctx.request_repaint();
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        i18n::{self, Language},
        model::{Sample, Settings, requested_limit},
    };

    #[test]
    fn language_persists_and_validation_errors_follow_selection() {
        let dir = std::env::temp_dir().join(format!(
            "workflow-teploty-language-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let legacy: Preferences =
            serde_json::from_str(r#"{"dark":true,"start_control":false}"#).unwrap();
        assert_eq!(legacy.language, Language::Cs);
        assert!(legacy.dark);
        assert!(!legacy.auto_updates);
        for language in [Language::En, Language::Cs] {
            let preferences = Preferences {
                language,
                dark: true,
                start_control: false,
                auto_updates: true,
            };
            preferences.save(&dir).unwrap();
            let loaded = Preferences::load(&dir);
            assert_eq!(loaded.language, language);
            assert!(loaded.dark);
            assert!(loaded.auto_updates);
            i18n::set(loaded.language);
            let message = requested_limit(&Settings::default(), &Sample::default()).unwrap_err();
            assert!(message.contains(if language == Language::En {
                "not available"
            } else {
                "Chybí"
            }));
            let mut invalid = Settings::default();
            invalid.cpu.points.clear();
            assert_eq!(
                invalid.validate().unwrap_err(),
                if language == Language::En {
                    "A curve must contain 2 to 12 points."
                } else {
                    "Křivka musí mít 2 až 12 bodů."
                }
            );
        }
        std::fs::remove_file(dir.join("aplikace.json")).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
