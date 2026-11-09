use crate::{
    model::{self, Regulator, Sample, Settings, SleepRequest},
    native::{self, PowerSession},
    sensors::Sensors,
};
use std::{
    collections::VecDeque,
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct HistoryPoint {
    pub sample: Sample,
    pub limit: f32,
    pub cpu_sleep_threshold: Option<f32>,
    pub battery_sleep_threshold: Option<f32>,
}
#[derive(Clone)]
pub struct State {
    pub applied_settings: Settings,
    pub sample: Sample,
    pub sampled_at: Option<Instant>,
    pub enabled: bool,
    pub limit: f32,
    pub status: String,
    pub countdown: Option<(String, u64)>,
    pub history: VecDeque<HistoryPoint>,
    pub events: VecDeque<String>,
    pub needs_recovery: bool,
    pub stop_epoch: u64,
}
impl Default for State {
    fn default() -> Self {
        Self {
            applied_settings: Settings::default(),
            sample: Sample::default(),
            sampled_at: None,
            enabled: false,
            limit: 100.,
            status: crate::tr!("Načítám čidla…", "Reading sensors…").into(),
            countdown: None,
            history: VecDeque::new(),
            events: VecDeque::new(),
            needs_recovery: false,
            stop_epoch: 0,
        }
    }
}
pub enum Command {
    LanguageChanged,
    Apply(Settings),
    Enable,
    Disable,
    CancelSleep,
    Recover,
    Shutdown,
}
pub struct Worker {
    pub state: Arc<Mutex<State>>,
    pub tx: mpsc::Sender<Command>,
    join: Option<thread::JoinHandle<()>>,
}
impl Worker {
    pub fn new(settings: Settings, directory: PathBuf, demo: bool, start_control: bool) -> Self {
        let state = Arc::new(Mutex::new(State {
            applied_settings: settings.clone(),
            ..State::default()
        }));
        let shared = state.clone();
        let (tx, rx) = mpsc::channel();
        let startup = (start_control && !demo).then(|| tx.clone());
        let join = thread::spawn(move || run(settings, directory, demo, shared, rx, startup));
        Self {
            state,
            tx,
            join: Some(join),
        }
    }
    pub fn snapshot(&self) -> State {
        self.state.lock().unwrap().clone()
    }
    pub fn send(&self, command: Command) {
        let _ = self.tx.send(command);
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Shutdown);
        if let Some(h) = self.join.take() {
            let _ = h.join();
        }
    }
}
fn event(shared: &Arc<Mutex<State>>, directory: &std::path::Path, text: impl Into<String>) {
    let line = format!(
        "{}  {}",
        chrono::Local::now().format("%H:%M:%S"),
        text.into()
    );
    let mut state = shared.lock().unwrap();
    state.status = line.clone();
    state.events.push_front(line.clone());
    state.events.truncate(100);
    drop(state);
    let path = directory.join("udalosti.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 2_000_000) {
        let _ = std::fs::rename(&path, directory.join("udalosti-predchozi.log"));
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{} {line}", chrono::Local::now().format("%Y-%m-%d"));
    }
}
fn stop(
    session: &mut Option<PowerSession>,
    shared: &Arc<Mutex<State>>,
    directory: &std::path::Path,
) {
    native::keep_working(false);
    if let Some(mut power) = session.take() {
        match power.restore() {
            Ok(()) => event(
                shared,
                directory,
                crate::tr!(
                    "Řízení zastaveno; původní nastavení obnoveno.",
                    "Regulation stopped; original settings restored."
                ),
            ),
            Err(e) => event(
                shared,
                directory,
                crate::tf!(
                    "Obnova vyžaduje pozornost: {e}",
                    "Recovery needs attention: {e}"
                ),
            ),
        }
    }
    let mut state = shared.lock().unwrap();
    state.enabled = false;
    state.limit = 100.;
    state.countdown = None;
    state.needs_recovery = directory.join("power-recovery.json").exists();
    state.stop_epoch = state.stop_epoch.wrapping_add(1);
}
fn run(
    mut settings: Settings,
    directory: PathBuf,
    demo: bool,
    shared: Arc<Mutex<State>>,
    rx: mpsc::Receiver<Command>,
    mut startup: Option<mpsc::Sender<Command>>,
) {
    let mut sensors = (!demo).then(Sensors::new);
    let mut session: Option<PowerSession> = None;
    let mut regulator = Regulator::default();
    let mut next_sample = Instant::now();
    let mut latest = Sample::default();
    let mut hot_since: Option<Instant> = None;
    let mut pending: Option<(SleepRequest, Instant)> = None;
    let mut sleep_grace = Instant::now();
    shared.lock().unwrap().needs_recovery = directory.join("power-recovery.json").exists();
    if demo {
        // A synthetic half-hour trace makes chart navigation useful immediately in demo mode.
        let now = chrono::Local::now();
        for index in 0..225 {
            let phase = index as f32 / 22.;
            let cpu = 64. + 28. * phase.sin();
            let battery = 35. + 10. * (phase * 0.7).sin();
            shared.lock().unwrap().history.push_back(HistoryPoint {
                sample: Sample {
                    timestamp: (now - chrono::Duration::seconds((225 - index) * 8)).to_rfc3339(),
                    cpu_temperature: Some(cpu),
                    battery_temperature: Some(battery),
                    gpu_temperature: Some(42. + 9. * (phase * 0.8).sin()),
                    lid_state: if index < 40 {
                        model::LidState::Open
                    } else {
                        model::LidState::Closed
                    },
                    ..Sample::default()
                },
                limit: if cpu >= 80. { 45. } else { 100. },
                cpu_sleep_threshold: Some(88.),
                battery_sleep_threshold: Some(43.),
            });
        }
    }
    event(
        &shared,
        &directory,
        if demo {
            crate::tr!(
                "Ukázková data; systémové akce jsou vypnuté.",
                "Demo data; system actions are disabled."
            )
        } else {
            crate::tr!(
                "Pouze sledování. Automatické řízení není zapnuté.",
                "Monitoring only. Regulation is off."
            )
        },
    );
    loop {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(Command::LanguageChanged) => {
                if session.is_none() {
                    shared.lock().unwrap().status = if demo {
                        crate::tr!(
                            "Ukázková data; systémové akce jsou vypnuté.",
                            "Demo data; system actions are disabled."
                        )
                    } else {
                        crate::tr!(
                            "Pouze sledování. Regulace není zapnutá.",
                            "Monitoring only. Regulation is off."
                        )
                    }
                    .into();
                }
                next_sample = Instant::now();
            }
            Ok(Command::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(Command::Disable) => {
                stop(&mut session, &shared, &directory);
                pending = None;
                hot_since = None;
            }
            Ok(Command::Recover) => {
                if session.is_none() {
                    match PowerSession::recover(&directory) {
                        Ok(()) => event(
                            &shared,
                            &directory,
                            crate::tr!(
                                "Předchozí nastavení bylo obnoveno.",
                                "Previous settings restored."
                            ),
                        ),
                        Err(e) => event(
                            &shared,
                            &directory,
                            crate::tf!("Obnova selhala: {e}", "Recovery failed: {e}"),
                        ),
                    };
                }
                shared.lock().unwrap().needs_recovery =
                    directory.join("power-recovery.json").exists();
            }
            Ok(Command::CancelSleep) => {
                pending = None;
                hot_since = None;
                sleep_grace = Instant::now() + Duration::from_secs(60);
                shared.lock().unwrap().countdown = None;
                event(
                    &shared,
                    &directory,
                    crate::tr!(
                        "Uspání zrušeno. Další automatické uspání nejdříve za minutu.",
                        "Sleep cancelled. Automatic sleep is paused for one minute."
                    ),
                );
            }
            Ok(Command::Apply(new)) => match new.validate() {
                Ok(()) => {
                    if session.is_some() && model::requested_limit(&new, &latest).is_err() {
                        event(
                            &shared,
                            &directory,
                            crate::tr!(
                                "Nové pravidlo vyžaduje nedostupné čidlo. Nejdřív zastav řízení.",
                                "The new rule needs an unavailable sensor. Stop regulation first."
                            ),
                        );
                    } else {
                        match native::write_json(&directory.join("nastaveni.json"), &new) {
                            Ok(()) => {
                                settings = new;
                                shared.lock().unwrap().applied_settings = settings.clone();
                                pending = None;
                                hot_since = None;
                                event(
                                    &shared,
                                    &directory,
                                    crate::tr!(
                                        "Křivky a pravidla uloženy a použity.",
                                        "Curves and rules saved and applied."
                                    ),
                                );
                            }
                            Err(e) => event(
                                &shared,
                                &directory,
                                crate::tf!("Uložení selhalo: {e}", "Save failed: {e}"),
                            ),
                        };
                    }
                }
                Err(e) => event(&shared, &directory, e),
            },
            Ok(Command::Enable) => {
                if !demo && session.is_none() {
                    let fresh = shared
                        .lock()
                        .unwrap()
                        .sampled_at
                        .is_some_and(|t| t.elapsed() < Duration::from_secs(10));
                    let check = settings
                        .validate()
                        .and_then(|_| model::requested_limit(&settings, &latest).map(|_| ()));
                    if !fresh {
                        event(
                            &shared,
                            &directory,
                            crate::tr!(
                                "Nejsou dostupná čerstvá měření. Řízení nebylo zapnuto.",
                                "Fresh readings are unavailable. Regulation was not started."
                            ),
                        );
                    } else if let Err(e) = check {
                        event(&shared, &directory, e);
                    } else if !settings.cpu.enabled
                        && !settings.battery.enabled
                        && !settings.cpu.sleep.enabled
                        && !settings.battery.sleep.enabled
                    {
                        event(
                            &shared,
                            &directory,
                            crate::tr!(
                                "Nejdřív zapni alespoň jednu křivku nebo teplotní mez.",
                                "Enable at least one curve or sleep temperature threshold first."
                            ),
                        );
                    } else {
                        match PowerSession::begin(&directory) {
                            Ok(power) => {
                                session = Some(power);
                                regulator = Regulator::default();
                                native::keep_working(true);
                                shared.lock().unwrap().enabled = true;
                                next_sample = Instant::now();
                                event(
                                    &shared,
                                    &directory,
                                    crate::tr!(
                                        "Automatické řízení zapnuto. Běžící aplikace pokračují.",
                                        "Automatic regulation started. Running applications continue."
                                    ),
                                );
                            }
                            Err(e) => event(
                                &shared,
                                &directory,
                                crate::tf!(
                                    "Řízení nelze zapnout: {e}",
                                    "Cannot start regulation: {e}"
                                ),
                            ),
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        // Lid events are checked independently of the slower temperature polling interval.
        latest.lid_state = sensors
            .as_ref()
            .map_or(model::LidState::Closed, Sensors::lid_state);
        if !demo {
            latest.on_ac = native::battery_status().0;
        }
        {
            let mut state = shared.lock().unwrap();
            state.sample.lid_state = latest.lid_state;
            state.sample.on_ac = latest.on_ac;
        }
        if let Some(reason) = model::control_pause(&settings, latest.lid_state, latest.on_ac) {
            pending = None;
            hot_since = None;
            regulator = Regulator::default();
            native::keep_working(false);
            if let Some(power) = session.as_mut() {
                match power.apply(100.) {
                    Ok(()) => {
                        let mut state = shared.lock().unwrap();
                        state.status = crate::tf!(
                            "Řízení připravené · {reason}",
                            "Regulation ready · {reason}"
                        );
                        state.limit = 100.;
                        state.countdown = None;
                    }
                    Err(e) => {
                        stop(&mut session, &shared, &directory);
                        event(&shared, &directory, e.to_string());
                    }
                }
            }
        } else if session.is_some() {
            native::keep_working(true);
        }
        if Instant::now() >= next_sample {
            let started = Instant::now();
            latest = if let Some(s) = sensors.as_mut() {
                s.read()
            } else {
                Sample {
                    timestamp: chrono::Local::now().to_rfc3339(),
                    cpu_temperature: Some(63.5),
                    battery_temperature: Some(34.2),
                    gpu_temperature: Some(42.),
                    cpu_usage: Some(12.),
                    discharge_watts: Some(14.8),
                    battery_percent: Some(78),
                    on_ac: false,
                    lid_state: model::LidState::Closed,
                    lid_source: crate::tr!("Ukázkové víko", "Demo lid").into(),
                    cpu_source: crate::tr!("Ukázkové čidlo", "Demo sensor").into(),
                    battery_source: crate::tr!("Ukázkové čidlo", "Demo sensor").into(),
                }
            };
            if let Some(sensors) = &sensors {
                // WMI can block; refresh the lid and power source after temperature reads.
                latest.lid_state = sensors.lid_state();
                latest.on_ac = native::battery_status().0;
            }
            if started.elapsed() > Duration::from_secs(10) {
                latest.cpu_temperature = None;
                latest.battery_temperature = None;
            }
            let now = Instant::now();
            next_sample = now + Duration::from_secs(u64::from(settings.sample_seconds));
            {
                let mut state = shared.lock().unwrap();
                state.sample = latest.clone();
                state.sampled_at = Some(started);
            }
            if let Some(tx) = startup.take()
                && !directory.join("power-recovery.json").exists()
            {
                let _ = tx.send(Command::Enable);
            }
            let mut failure = None;
            if let Some(power) = session.as_mut() {
                if let Err(e) = power.ensure_scheme() {
                    failure = Some(e.to_string());
                } else if let Some(reason) =
                    model::control_pause(&settings, latest.lid_state, latest.on_ac)
                {
                    if let Err(e) = power.apply(100.) {
                        failure = Some(e.to_string());
                    }
                    regulator = Regulator::default();
                    pending = None;
                    hot_since = None;
                    shared.lock().unwrap().status = crate::tf!(
                        "Řízení připravené · {reason}",
                        "Regulation ready · {reason}"
                    );
                } else {
                    match model::requested_limit(&settings, &latest) {
                        Ok(target) => {
                            let limit = regulator.step(
                                target,
                                now,
                                Duration::from_secs(u64::from(settings.recovery_seconds)),
                            );
                            if let Err(e) = power.apply(limit) {
                                failure = Some(e.to_string());
                            }
                            shared.lock().unwrap().status = crate::tf!(
                                "Řízení běží · limit křivky {:.0} % výchozího výkonu",
                                "Regulation on · limit {:.0}% of original performance",
                                limit
                            );
                            if now >= sleep_grace {
                                if let Some(request) = model::sleep_request(&settings, &latest) {
                                    let hot = hot_since.get_or_insert(now);
                                    if now.duration_since(*hot) >= Duration::from_secs(6)
                                        && pending.is_none()
                                    {
                                        event(
                                            &shared,
                                            &directory,
                                            crate::tf!(
                                                "Teplotní mez: {}. Uspání za 15 sekund.",
                                                "Temperature threshold: {}. Sleep in 15 seconds.",
                                                request.source
                                            ),
                                        );
                                        pending = Some((request, now + Duration::from_secs(15)));
                                    } else if let Some((old, _)) = pending.as_mut() {
                                        old.retain_stricter(request);
                                    }
                                } else {
                                    hot_since = None;
                                    pending = None;
                                }
                            }
                        }
                        Err(e) => {
                            // A lost sensor is never interpreted as cool. Keep work running at a conservative cap.
                            pending = None;
                            hot_since = None;
                            regulator.applied = 20.;
                            if let Err(error) = power.apply(20.) {
                                failure = Some(error.to_string());
                            }
                            shared.lock().unwrap().status = crate::tf!(
                                "{e} Do obnovení čidla je limit 20 %. Uspání je pozastaveno.",
                                "{e} Limit set to 20% until the sensor recovers. Sleep is paused."
                            );
                        }
                    }
                }
            }
            if let Some(e) = failure {
                event(&shared, &directory, e);
                stop(&mut session, &shared, &directory);
                pending = None;
            }
            let mut state = shared.lock().unwrap();
            state.limit = if session.is_some() {
                regulator.applied
            } else {
                100.
            };
            let limit = state.limit;
            state.history.push_back(HistoryPoint {
                sample: latest.clone(),
                limit,
                cpu_sleep_threshold: settings
                    .cpu
                    .sleep
                    .enabled
                    .then_some(settings.cpu.sleep.temperature),
                battery_sleep_threshold: settings
                    .battery
                    .sleep
                    .enabled
                    .then_some(settings.battery.sleep.temperature),
            });
            while state.history.len() > 900 {
                state.history.pop_front();
            }
        }
        if let Some((request, deadline)) = pending.clone() {
            let seconds = deadline.saturating_duration_since(Instant::now()).as_secs();
            shared.lock().unwrap().countdown = Some((request.source.into(), seconds));
            if Instant::now() >= deadline {
                // Recheck fresh telemetry at the last moment; UI stop/cancel commands are processed above.
                let fresh = shared
                    .lock()
                    .unwrap()
                    .sampled_at
                    .is_some_and(|t| t.elapsed() < Duration::from_secs(10));
                let lid = sensors
                    .as_ref()
                    .map_or(model::LidState::Closed, Sensors::lid_state);
                let on_ac = if demo {
                    latest.on_ac
                } else {
                    native::battery_status().0
                };
                if fresh
                    && model::control_pause(&settings, lid, on_ac).is_none()
                    && model::sleep_request(&settings, &latest).is_some()
                    && let Some(power) = session.as_mut()
                {
                    event(
                        &shared,
                        &directory,
                        match request.wake_minutes {
                            Some(m) => crate::tf!(
                                "Uspání; automatické probuzení za {m} min.",
                                "Going to sleep; automatic wake in {m} min."
                            ),
                            None => crate::tr!(
                                "Uspání bez naplánovaného probuzení.",
                                "Going to sleep without a scheduled wake."
                            )
                            .into(),
                        },
                    );
                    match power.sleep(request.wake_minutes, settings.resume_without_password) {
                        Ok(()) => event(
                            &shared,
                            &directory,
                            crate::tr!(
                                "Počítač byl probuzen; měření a práce pokračují.",
                                "Computer resumed; monitoring and work continue."
                            ),
                        ),
                        Err(e) => event(
                            &shared,
                            &directory,
                            crate::tf!("Uspání/probuzení: {e}", "Sleep/wake: {e}"),
                        ),
                    }
                }
                pending = None;
                hot_since = None;
                sleep_grace = Instant::now() + Duration::from_secs(60);
                next_sample = Instant::now();
            }
        } else {
            shared.lock().unwrap().countdown = None;
        }
    }
    stop(&mut session, &shared, &directory);
}
