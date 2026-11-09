use super::*;
use std::{
    ffi::c_void,
    mem::size_of,
    os::windows::process::CommandExt,
    process::{Child, Command},
    ptr::{null, null_mut},
    thread,
    time::Instant,
};

type Handle = *mut c_void;
struct OwnedHandle(Handle);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[repr(C)]
struct ProcessEntry {
    size: u32,
    usage: u32,
    pid: u32,
    heap: usize,
    module: u32,
    threads: u32,
    parent: u32,
    priority: i32,
    flags: u32,
    name: [u16; 260],
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
    fn CloseHandle(handle: Handle) -> i32;
    fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
    fn QueryFullProcessImageNameW(
        handle: Handle,
        flags: u32,
        name: *mut u16,
        size: *mut u32,
    ) -> i32;
    fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> Handle;
    fn Process32FirstW(snapshot: Handle, entry: *mut ProcessEntry) -> i32;
    fn Process32NextW(snapshot: Handle, entry: *mut ProcessEntry) -> i32;
    fn ReplaceFileW(
        replaced: *const u16,
        replacement: *const u16,
        backup: *const u16,
        flags: u32,
        exclude: *mut c_void,
        reserved: *mut c_void,
    ) -> i32;
}
fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}
fn replace(target: &Path, replacement: &Path, backup: Option<&Path>) -> Result<()> {
    let backup = backup.map(wide);
    if unsafe {
        ReplaceFileW(
            wide(target).as_ptr(),
            wide(replacement).as_ptr(),
            backup.as_ref().map_or(null(), |b| b.as_ptr()),
            0,
            null_mut(),
            null_mut(),
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}
fn parent_process() -> Result<u32> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(2, 0) };
    ensure!(
        snapshot as isize != -1,
        "{}",
        std::io::Error::last_os_error()
    );
    let snapshot = OwnedHandle(snapshot);
    let mut entry: ProcessEntry = unsafe { std::mem::zeroed() };
    entry.size = size_of::<ProcessEntry>() as u32;
    let mut next = unsafe { Process32FirstW(snapshot.0, &mut entry) };
    while next != 0 {
        if entry.pid == std::process::id() {
            return Ok(entry.parent);
        }
        next = unsafe { Process32NextW(snapshot.0, &mut entry) };
    }
    anyhow::bail!(crate::tr!(
        "Nelze určit rodičovský proces aktualizace.",
        "Cannot identify the update's parent process."
    ))
}
fn process_image(handle: &OwnedHandle) -> Result<PathBuf> {
    let mut buffer = vec![0u16; 32768];
    let mut len = buffer.len() as u32;
    ensure!(
        unsafe { QueryFullProcessImageNameW(handle.0, 0, buffer.as_mut_ptr(), &mut len) } != 0,
        "{}",
        std::io::Error::last_os_error()
    );
    Ok(fs::canonicalize(String::from_utf16(
        &buffer[..len as usize],
    )?)?)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub signed: SignedManifest,
    pub directory: PathBuf,
    pub target: PathBuf,
    pub replacement: PathBuf,
    pub backup: PathBuf,
    pub original_sha256: String,
    pub parent_pid: u32,
    pub resume_control: bool,
    pub background: bool,
    pub language: crate::i18n::Language,
    pub nonce: String,
}

fn load_plan(path: &Path) -> Result<Plan> {
    let bytes = read_bounded(File::open(path)?, MAX_MANIFEST_SIZE * 4)?;
    let plan: Plan = serde_json::from_slice(&bytes)?;
    plan.signed.verify()?;
    ensure!(
        plan.nonce.len() == 32 && plan.nonce.bytes().all(|b| b.is_ascii_hexdigit()),
        crate::tr!(
            "Neplatný identifikátor aktualizace.",
            "Invalid update transaction identifier."
        )
    );
    let expected_dir = update_dir(&plan.directory).join(format!("apply-{}", plan.nonce));
    ensure!(
        fs::canonicalize(path)? == fs::canonicalize(expected_dir.join("plan.json"))?,
        crate::tr!(
            "Neplatné umístění plánu aktualizace.",
            "Invalid update plan location."
        )
    );
    contained_path(&update_dir(&plan.directory), path)?;
    let target_parent = plan.target.parent().context(crate::tr!(
        "Chybí cílová složka aktualizace.",
        "Update target has no parent directory."
    ))?;
    let name = plan
        .target
        .file_name()
        .context(crate::tr!(
            "Chybí cílový soubor aktualizace.",
            "Update target has no filename."
        ))?
        .to_string_lossy();
    ensure!(
        plan.replacement == target_parent.join(format!("{name}.new-{}", plan.nonce))
            && plan.backup == target_parent.join(format!("{name}.previous-{}", plan.nonce)),
        crate::tr!(
            "Neplatné cesty aktualizace.",
            "Invalid update transaction paths."
        )
    );
    Ok(plan)
}

fn app_command(target: &Path, plan: &Plan, health: Option<&Path>) -> Command {
    let mut command = Command::new(target);
    command.creation_flags(0x08000000);
    #[cfg(not(test))]
    {
        command
            .arg("--data-dir")
            .arg(&plan.directory)
            .arg("--skip-update-once");
        command.arg(if plan.resume_control {
            "--resume-control-after-update"
        } else {
            "--pause-control-after-update"
        });
        if plan.background {
            command.arg("--background");
        }
        if let Some(path) = health {
            command.arg("--update-health").arg(path);
        }
    }
    #[cfg(test)]
    {
        command
            .arg("--exact")
            .arg("updates::tests::process_entry")
            .arg("--nocapture");
        command.env(
            "COOLTAKT_TEST_ROLE",
            if health.is_some() {
                "child"
            } else {
                "rollback"
            },
        );
        command.env("COOLTAKT_TEST_DIRECTORY", &plan.directory);
        if let Some(path) = health {
            command.env("COOLTAKT_TEST_PLAN", path);
        }
    }
    command
}

/// Called on a background thread, only after thermal control has acknowledged
/// restoration. A successful return means the helper is ready, not installed yet.
pub fn prepare(
    directory: &Path,
    signed: SignedManifest,
    resume_control: bool,
    background: bool,
) -> Result<PathBuf> {
    ensure!(
        signed.newer_than(CURRENT_VERSION)?,
        crate::tr!(
            "Aktualizace není novější než tato verze.",
            "The update is not newer than this version."
        )
    );
    ensure!(
        !directory.join("power-recovery.json").exists(),
        crate::tr!(
            "Nejdřív obnov původní napájecí nastavení.",
            "Restore the original power settings first."
        )
    );
    let source = contained_path(&update_dir(directory), &payload_path(directory, &signed)?)?;
    signed.verify_file(&source)?;
    let target = fs::canonicalize(std::env::current_exe()?)?;
    let nonce = nonce()?;
    let working = update_dir(directory).join(format!("apply-{nonce}"));
    fs::create_dir(&working)?;
    let name = target.file_name().unwrap().to_string_lossy();
    let target_parent = target.parent().unwrap();
    let plan = Plan {
        signed,
        directory: fs::canonicalize(directory)?,
        replacement: target_parent.join(format!("{name}.new-{nonce}")),
        backup: target_parent.join(format!("{name}.previous-{nonce}")),
        original_sha256: hash_file(&target)?,
        target,
        parent_pid: std::process::id(),
        resume_control,
        background,
        language: if crate::i18n::english() {
            crate::i18n::Language::En
        } else {
            crate::i18n::Language::Cs
        },
        nonce,
    };
    let path = working.join("plan.json");
    let helper = working.join("helper.exe");
    let operation = (|| -> Result<()> {
        // create_new protects unrelated files from replacement.
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&plan.replacement)?;
        std::io::copy(&mut File::open(&source)?, &mut output)?;
        output.sync_all()?;
        drop(output);
        plan.signed.verify_file(&plan.replacement)?;
        fs::copy(&plan.target, &helper)?;
        ensure!(
            hash_file(&helper)? == plan.original_sha256,
            crate::tr!(
                "Kopie aktualizátoru nesouhlasí.",
                "Update helper copy does not match."
            )
        );
        crate::persist::write_json(&path, &plan)?;
        let pending_path = update_dir(directory).join("pending.json");
        if pending_path.exists() {
            fs::remove_file(pending_path)?;
        }
        let mut command = Command::new(&helper);
        command.creation_flags(0x08000000);
        #[cfg(not(test))]
        command.arg("--apply-update").arg(&path);
        #[cfg(test)]
        command
            .arg("--exact")
            .arg("updates::tests::process_entry")
            .arg("--nocapture")
            .env("COOLTAKT_TEST_ROLE", "helper")
            .env("COOLTAKT_TEST_PLAN", &path);
        let mut child = command.spawn()?;
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if working.join("ready").is_file() {
                // Consume only this pending update. A failed installation is never
                // automatically retried in a startup loop.
                return Ok(());
            }
            if child.try_wait()?.is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        let _ = child.kill();
        let _ = child.wait();
        anyhow::bail!(crate::tr!(
            "Aktualizátor se nepřipravil včas. Aplikace zůstává spuštěná.",
            "The updater did not become ready in time. The application remains running."
        ))
    })();
    if operation.is_err() {
        let _ = fs::remove_file(&plan.replacement);
    }
    operation?;
    Ok(path)
}

fn wait_health(child: &mut Child, working: &Path) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(if cfg!(test) { 10 } else { 45 });
    loop {
        ensure!(
            child.try_wait()?.is_none(),
            crate::tr!(
                "Nová verze předčasně skončila.",
                "The new version exited too early."
            )
        );
        if working.join("healthy").is_file() {
            return Ok(());
        }
        ensure!(
            Instant::now() < deadline,
            crate::tr!(
                "Nová verze nepotvrdila úspěšný start.",
                "The new version did not confirm a successful startup."
            )
        );
        thread::sleep(Duration::from_millis(100));
    }
}

pub fn apply(path: &Path) -> Result<()> {
    let plan = load_plan(path)?;
    crate::i18n::set(plan.language);
    let working = path.parent().unwrap();
    let mut parent_exited = false;
    let mut restarted = false;
    let operation = (|| -> Result<()> {
        ensure!(
            parent_process()? == plan.parent_pid,
            crate::tr!(
                "Aktualizaci musí spustit běžící aplikace.",
                "Update must be launched by its running application."
            )
        );
        let parent = unsafe { OpenProcess(0x0010_1000, 0, plan.parent_pid) };
        ensure!(!parent.is_null(), "{}", std::io::Error::last_os_error());
        let parent = OwnedHandle(parent);
        ensure!(
            process_image(&parent)? == plan.target,
            crate::tr!(
                "Cíl aktualizace neodpovídá rodičovskému procesu.",
                "Update target does not match its parent process."
            )
        );
        let own = fs::canonicalize(std::env::current_exe()?)?;
        contained_path(&update_dir(&plan.directory), &own)?;
        ensure!(
            hash_file(&own)? == plan.original_sha256
                && hash_file(&plan.target)? == plan.original_sha256,
            crate::tr!(
                "Aktualizátor a běžící aplikace se neshodují.",
                "Update helper and running application do not match."
            )
        );
        ensure!(
            plan.signed.newer_than(CURRENT_VERSION)?,
            crate::tr!(
                "Instalace starší verze byla zamítnuta.",
                "Update downgrade rejected."
            )
        );
        plan.signed.verify_file(&plan.replacement)?;
        ensure!(
            !plan.backup.exists(),
            crate::tr!(
                "Záloha aktualizace již existuje.",
                "Update backup path already exists."
            )
        );
        fs::write(working.join("ready"), b"ready")?;
        ensure!(
            unsafe { WaitForSingleObject(parent.0, 60_000) } == 0,
            crate::tr!(
                "Aplikace se neukončila včas. EXE nebylo změněno.",
                "The application did not exit in time. Its executable was not changed."
            )
        );
        drop(parent);
        parent_exited = true;
        // Recheck after waiting, immediately before the file replacement.
        ensure!(
            !plan.directory.join("power-recovery.json").exists(),
            crate::tr!(
                "Napájecí nastavení nebylo obnoveno. Aktualizace byla zrušena.",
                "Power settings were not restored. The update was cancelled."
            )
        );
        plan.signed.verify_file(&plan.replacement)?;
        ensure!(
            hash_file(&plan.target)? == plan.original_sha256,
            crate::tr!(
                "Aplikace se během čekání na aktualizaci změnila.",
                "Application changed while update was waiting."
            )
        );
        if let Err(error) = replace(&plan.target, &plan.replacement, Some(&plan.backup)) {
            // ReplaceFile may report a partially completed rename; preserve the
            // known original before attempting to restart it.
            if !plan.target.exists()
                && plan.backup.exists()
                && hash_file(&plan.backup)? == plan.original_sha256
            {
                fs::rename(&plan.backup, &plan.target)?;
            }
            if plan.target.exists() && hash_file(&plan.target)? == plan.original_sha256 {
                restarted = app_command(&plan.target, &plan, None).spawn().is_ok();
            }
            return Err(error);
        }
        let launch = (|| -> Result<()> {
            plan.signed.verify_file(&plan.target)?;
            let mut child = app_command(&plan.target, &plan, Some(path)).spawn()?;
            match wait_health(&mut child, working) {
                Ok(()) => Ok(()),
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    Err(error)
                }
            }
        })();
        if let Err(error) = launch {
            ensure!(
                hash_file(&plan.backup)? == plan.original_sha256,
                crate::tr!(
                    "Záloha původní aplikace nesouhlasí.",
                    "Original application backup does not match."
                )
            );
            replace(&plan.target, &plan.backup, None).context(crate::tr!(
                "Obnova předchozí verze selhala.",
                "Restoring the previous version failed."
            ))?;
            app_command(&plan.target, &plan, None).spawn()?;
            restarted = true;
            return Err(error.context(crate::tr!(
                "Byla obnovena předchozí verze.",
                "The previous version was restored."
            )));
        }
        // Only delete files created by this transaction, after a healthy start.
        let _ = fs::remove_file(&plan.backup);
        if let Ok(payload) = payload_path(&plan.directory, &plan.signed) {
            let _ = fs::remove_file(payload);
        }
        Ok(())
    })();
    if operation.is_err()
        && parent_exited
        && !restarted
        && hash_file(&plan.target).is_ok_and(|hash| hash == plan.original_sha256)
    {
        let _ = app_command(&plan.target, &plan, None).spawn();
    }
    let result = InstallResult {
        version: plan.signed.manifest.version.clone(),
        success: operation.is_ok(),
        detail: operation
            .as_ref()
            .err()
            .map(|e| format!("{e:#}"))
            .unwrap_or_default(),
    };
    let _ = crate::persist::write_json(
        &update_dir(&plan.directory).join("last-result.json"),
        &result,
    );
    let _ = fs::write(
        working.join("finished"),
        if operation.is_ok() { "ok" } else { "failed" },
    );
    operation
}

/// Called only after the new GUI has rendered its first frame. Thermal control
/// is not started until this acknowledgement succeeds.
pub fn acknowledge(path: &Path, directory: &Path) -> Result<()> {
    let plan = load_plan(path)?;
    ensure!(
        fs::canonicalize(directory)? == plan.directory,
        crate::tr!(
            "Datová složka aktualizace se neshoduje.",
            "Update data directory mismatch."
        )
    );
    ensure!(
        fs::canonicalize(std::env::current_exe()?)? == plan.target,
        crate::tr!(
            "Proces aktualizace se neshoduje.",
            "Update process mismatch."
        )
    );
    plan.signed.verify_file(&plan.target)?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path.parent().unwrap().join("healthy"))?;
    file.write_all(b"healthy")?;
    file.sync_all()?;
    Ok(())
}

/// Retired helpers cannot delete their own running EXE on Windows. Clean them
/// on a later launch, restricted to completed transaction directories only.
pub fn cleanup(directory: &Path) {
    let root = update_dir(directory);
    let Ok(entries) = fs::read_dir(&root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("apply-") || name.len() != 38 {
            continue;
        }
        let dir = entry.path();
        if !dir.join("finished").is_file() {
            continue;
        }
        let Ok(plan) = load_plan(&dir.join("plan.json")) else {
            continue;
        };
        if plan.nonce != name[6..] {
            continue;
        }
        if contained_path(&root, &dir).is_ok() {
            // Leave any unknown user files alone; never recursively delete.
            if let Err(e) = fs::remove_file(dir.join("helper.exe"))
                && e.kind() != std::io::ErrorKind::NotFound
            {
                continue;
            }
            for file in ["ready", "healthy", "plan.json", "finished"] {
                let _ = fs::remove_file(dir.join(file));
            }
            let _ = fs::remove_dir(dir);
        }
    }
}
