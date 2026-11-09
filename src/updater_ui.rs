use eframe::egui;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};
use workflow_teploty::updates::{self, Progress, SignedManifest};

pub struct StartupUpdate {
    pub rules_valid: bool,
    pub resume_control: Option<bool>,
    pub health_plan: Option<PathBuf>,
}

pub struct UpdateManager {
    directory: PathBuf,
    pub pending: Option<SignedManifest>,
    pub error: String,
    pub checked: bool,
    progress: Arc<Mutex<Option<Progress>>>,
    check: Option<mpsc::Receiver<Result<Option<SignedManifest>, String>>>,
    pub install: Option<mpsc::Receiver<Result<PathBuf, String>>>,
    next_check: Instant,
}
impl UpdateManager {
    pub fn new(directory: PathBuf) -> Self {
        let result = updates::pending(&directory);
        let error = result
            .as_ref()
            .err()
            .map(|e| format!("{e:#}"))
            .unwrap_or_default();
        Self {
            directory,
            pending: result.ok().flatten(),
            error,
            checked: false,
            progress: Arc::new(Mutex::new(None)),
            check: None,
            install: None,
            next_check: Instant::now() + Duration::from_secs(30),
        }
    }
    pub fn busy(&self) -> bool {
        self.check.is_some() || self.install.is_some()
    }
    pub fn check(&mut self, manual: bool, ctx: &egui::Context) {
        if self.busy() {
            return;
        }
        self.error.clear();
        self.next_check = Instant::now() + Duration::from_secs(6 * 60 * 60);
        *self.progress.lock().unwrap() = Some(Progress::Checking);
        let (tx, rx) = mpsc::channel();
        self.check = Some(rx);
        let directory = self.directory.clone();
        let progress = self.progress.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result =
                updates::check_and_download(&directory, updates::CURRENT_VERSION, manual, |p| {
                    *progress.lock().unwrap() = Some(p);
                    ctx.request_repaint();
                })
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
    pub fn poll(&mut self, automatic: bool, demo: bool, ctx: &egui::Context) {
        if let Some(result) = self.check.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.check = None;
            *self.progress.lock().unwrap() = None;
            self.checked = true;
            match result {
                Ok(Some(signed)) => self.pending = Some(signed),
                Ok(None) => {}
                Err(error) => self.error = error,
            }
        }
        if automatic && !demo && !self.busy() && Instant::now() >= self.next_check {
            self.check(false, ctx);
        }
    }
    pub fn prepare(&mut self, resume: bool, ctx: &egui::Context) {
        let Some(signed) = self.pending.clone() else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        self.install = Some(rx);
        let directory = self.directory.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = updates::installer::prepare(&directory, signed, resume, false)
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
    pub fn status(&self) -> String {
        if self.install.is_some() {
            return crate::tr!("Připravuji bezpečný restart…", "Preparing a safe restart…").into();
        }
        if let Some(progress) = self.progress.lock().unwrap().as_ref() {
            return match progress {
                Progress::Checking => {
                    crate::tr!("Kontroluji vydání na GitHubu…", "Checking GitHub releases…").into()
                }
                Progress::Downloading {
                    version,
                    downloaded,
                    total,
                } => crate::tf!(
                    "Stahuji {version}: {:.1} / {:.1} MB",
                    "Downloading {version}: {:.1} / {:.1} MB",
                    *downloaded as f64 / 1_000_000.,
                    *total as f64 / 1_000_000.
                ),
            };
        }
        if !self.error.is_empty() {
            return crate::tf!("Aktualizace: {}", "Update: {}", self.error);
        }
        if let Some(signed) = &self.pending {
            return crate::tf!(
                "Verze {} je ověřená a připravená k instalaci.",
                "Version {} is verified and ready to install.",
                signed.manifest.version
            );
        }
        if self.checked {
            crate::tr!(
                "Používáš nejnovější dostupné vydání.",
                "You are using the latest available release."
            )
            .into()
        } else {
            crate::tr!(
                "Aktualizace zatím nebyly kontrolovány.",
                "Updates have not been checked yet."
            )
            .into()
        }
    }
}
