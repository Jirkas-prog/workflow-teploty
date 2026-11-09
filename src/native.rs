//! Small, explicit Windows API boundary. No shell, external tools or drivers are started.
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::{
    ffi::c_void,
    mem::size_of,
    path::{Path, PathBuf},
    ptr::{null, null_mut},
    sync::atomic::{AtomicU8, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

type Handle = *mut c_void;
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Guid {
    a: u32,
    b: u16,
    c: u16,
    d: [u8; 8],
}
impl Guid {
    pub const fn new(v: u128) -> Self {
        Self {
            a: (v >> 96) as u32,
            b: (v >> 80) as u16,
            c: (v >> 64) as u16,
            d: (v as u64).to_be_bytes(),
        }
    }
}
const CPU: Guid = Guid::new(0x54533251_82be_4824_96c1_47b60b740d00);
const SLEEP: Guid = Guid::new(0x238c9fa8_0aad_41ed_83f4_97be242c8f20);
const NONE: Guid = Guid::new(0xfea3413e_7e05_4911_9a71_700331f1c294);
const MAX: Guid = Guid::new(0xbc5038f7_23e0_4960_96da_33abaf5935ec);
const MAX1: Guid = Guid::new(0xbc5038f7_23e0_4960_96da_33abaf5935ed);
const BOOST: Guid = Guid::new(0xbe337238_0d82_4146_a960_4f3749d470c7);
const WAKE: Guid = Guid::new(0xbd3b718a_0680_4d9d_8ab2_e1d2b4ac806d);
const LOCK: Guid = Guid::new(0x0e796bdb_100d_47d6_a2d5_f7d2daa51f51);

#[link(name = "powrprof")]
unsafe extern "system" {
    fn PowerSettingRegisterNotification(
        setting: *const Guid,
        flags: u32,
        recipient: Handle,
        registration: *mut Handle,
    ) -> u32;
    fn PowerSettingUnregisterNotification(registration: Handle) -> u32;
    fn PowerGetActiveScheme(root: Handle, scheme: *mut *mut Guid) -> u32;
    fn PowerSetActiveScheme(root: Handle, scheme: *const Guid) -> u32;
    fn PowerReadACValueIndex(
        root: Handle,
        scheme: *const Guid,
        sub: *const Guid,
        setting: *const Guid,
        value: *mut u32,
    ) -> u32;
    fn PowerReadDCValueIndex(
        root: Handle,
        scheme: *const Guid,
        sub: *const Guid,
        setting: *const Guid,
        value: *mut u32,
    ) -> u32;
    fn PowerWriteACValueIndex(
        root: Handle,
        scheme: *const Guid,
        sub: *const Guid,
        setting: *const Guid,
        value: u32,
    ) -> u32;
    fn PowerWriteDCValueIndex(
        root: Handle,
        scheme: *const Guid,
        sub: *const Guid,
        setting: *const Guid,
        value: u32,
    ) -> u32;
    fn SetSuspendState(hibernate: u8, force: u8, disable_wake: u8) -> u8;
}

const LID: Guid = Guid::new(0xba3e0f4d_b817_4094_a2d1_d56379e6a0f3);
// Static storage remains valid even if an OS callback overlaps application shutdown.
static LID_STATE: AtomicU8 = AtomicU8::new(255);
#[repr(C)]
struct PowerBroadcastHeader {
    setting: Guid,
    length: u32,
}
#[repr(C)]
struct PowerSubscription {
    callback: unsafe extern "system" fn(Handle, u32, Handle) -> u32,
    context: Handle,
}
unsafe extern "system" fn lid_changed(_context: Handle, kind: u32, data: Handle) -> u32 {
    if kind == 0x8013 && !data.is_null() {
        // Windows supplies POWERBROADCAST_SETTING; Data begins immediately after this header.
        let header = unsafe { std::ptr::read_unaligned(data.cast::<PowerBroadcastHeader>()) };
        if header.setting == LID && header.length == 4 {
            let value = unsafe {
                std::ptr::read_unaligned(
                    data.cast::<u8>()
                        .add(size_of::<PowerBroadcastHeader>())
                        .cast::<u32>(),
                )
            };
            LID_STATE.store(
                if value <= 1 { value as u8 } else { 255 },
                Ordering::Release,
            );
        }
    }
    0
}
pub struct LidMonitor {
    registration: Handle,
    _subscription: Box<PowerSubscription>,
}
impl LidMonitor {
    pub fn new() -> Result<Self> {
        LID_STATE.store(255, Ordering::Release);
        let mut subscription = Box::new(PowerSubscription {
            callback: lid_changed,
            context: null_mut(),
        });
        let mut registration = null_mut();
        check(
            unsafe {
                PowerSettingRegisterNotification(
                    &LID,
                    2,
                    (&mut *subscription as *mut PowerSubscription).cast(),
                    &mut registration,
                )
            },
            crate::tr!("Sledování víka", "Lid monitoring"),
        )?;
        Ok(Self {
            registration,
            _subscription: subscription,
        })
    }
    pub fn state(&self) -> crate::model::LidState {
        use crate::model::LidState;
        match LID_STATE.load(Ordering::Acquire) {
            0 => LidState::Closed,
            1 => LidState::Open,
            _ => LidState::Unknown,
        }
    }
}
impl Drop for LidMonitor {
    fn drop(&mut self) {
        unsafe {
            PowerSettingUnregisterNotification(self.registration);
        }
    }
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn LocalFree(memory: Handle) -> Handle;
    fn CloseHandle(handle: Handle) -> i32;
    fn GetLastError() -> u32;
    fn SetLastError(error: u32);
    fn CreateMutexW(attributes: Handle, owner: i32, name: *const u16) -> Handle;
    fn CreateEventW(attributes: Handle, manual: i32, initial: i32, name: *const u16) -> Handle;
    fn OpenEventW(access: u32, inherit: i32, name: *const u16) -> Handle;
    fn SetEvent(event: Handle) -> i32;
    fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
    fn CreateWaitableTimerW(attributes: Handle, manual: i32, name: *const u16) -> Handle;
    fn SetWaitableTimer(
        timer: Handle,
        due: *const i64,
        period: i32,
        callback: Handle,
        arg: Handle,
        resume: i32,
    ) -> i32;
    fn CancelWaitableTimer(timer: Handle) -> i32;
    fn GetCurrentProcess() -> Handle;
    fn SetThreadExecutionState(flags: u32) -> u32;
    fn GetSystemTimes(idle: *mut u64, kernel: *mut u64, user: *mut u64) -> i32;
    fn GetSystemPowerStatus(status: *mut SystemPowerStatus) -> i32;
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        security: Handle,
        creation: u32,
        flags: u32,
        template: Handle,
    ) -> Handle;
    fn DeviceIoControl(
        device: Handle,
        code: u32,
        input: *const c_void,
        input_len: u32,
        output: *mut c_void,
        output_len: u32,
        returned: *mut u32,
        overlapped: Handle,
    ) -> i32;
}
#[repr(C)]
struct Luid {
    low: u32,
    high: i32,
}
#[repr(C)]
struct TokenPrivileges {
    count: u32,
    luid: Luid,
    attributes: u32,
}
#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenProcessToken(process: Handle, access: u32, token: *mut Handle) -> i32;
    fn LookupPrivilegeValueW(system: *const u16, name: *const u16, luid: *mut Luid) -> i32;
    fn AdjustTokenPrivileges(
        token: Handle,
        disable: i32,
        new: *const TokenPrivileges,
        length: u32,
        previous: Handle,
        returned: Handle,
    ) -> i32;
}
#[link(name = "shell32")]
unsafe extern "system" {
    fn ShellExecuteW(
        window: Handle,
        verb: *const u16,
        file: *const u16,
        args: *const u16,
        directory: *const u16,
        show: i32,
    ) -> isize;
}
#[link(name = "user32")]
unsafe extern "system" {
    fn MessageBoxW(window: Handle, text: *const u16, caption: *const u16, flags: u32) -> i32;
    fn ShowWindowAsync(window: Handle, command: i32) -> i32;
    fn SetForegroundWindow(window: Handle) -> i32;
}

fn wide(s: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    s.as_ref().encode_wide().chain(Some(0)).collect()
}
fn error(context: &str) -> anyhow::Error {
    anyhow!("{context}: {}", std::io::Error::last_os_error())
}
fn check(code: u32, context: &str) -> Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(anyhow!(
            "{context}: {}",
            std::io::Error::from_raw_os_error(code as i32)
        ))
    }
}

pub struct OwnedHandle(Handle);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 as isize != -1 {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

pub struct Instance {
    _mutex: OwnedHandle,
    _show: OwnedHandle,
}
impl Instance {
    pub fn listen(&self, ctx: eframe::egui::Context, window: isize) -> InstanceListener {
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = stop.clone();
        // Open the handle on its owning thread rather than sending a raw handle across threads.
        let join = std::thread::spawn(move || {
            let handle = unsafe { OpenEventW(0x100000, 0, wide(SHOW_EVENT).as_ptr()) };
            if handle.is_null() {
                return;
            }
            let handle = OwnedHandle(handle);
            while !flag.load(Ordering::Acquire) {
                if unsafe { WaitForSingleObject(handle.0, 200) } == 0 {
                    crate::lifecycle::show_window(&ctx, window);
                }
            }
        });
        InstanceListener {
            stop,
            join: Some(join),
        }
    }
}
pub struct InstanceListener {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    join: Option<std::thread::JoinHandle<()>>,
}
impl Drop for InstanceListener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
const SHOW_EVENT: &str = "Local\\WorkFlow.Teploty.Show.v1";
pub fn show_existing() -> bool {
    let event = unsafe { OpenEventW(2, 0, wide(SHOW_EVENT).as_ptr()) };
    if event.is_null() {
        return false;
    }
    let event = OwnedHandle(event);
    unsafe { SetEvent(event.0) != 0 }
}
pub fn current_user_sid() -> Result<String> {
    use windows::{
        Win32::{
            Foundation::{HANDLE, HLOCAL},
            Security::{
                Authorization::ConvertSidToStringSidW, GetTokenInformation, TOKEN_USER, TokenUser,
            },
        },
        core::PWSTR,
    };
    let mut token = null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), 8, &mut token) } == 0 {
        return Err(error(crate::tr!("Identita uživatele", "User identity")));
    }
    let token = OwnedHandle(token);
    let mut needed = 0;
    unsafe {
        let _ = GetTokenInformation(HANDLE(token.0), TokenUser, None, 0, &mut needed);
    }
    if !(size_of::<TOKEN_USER>() as u32..=65536).contains(&needed) {
        crate::tbail!("Neplatná identita uživatele.", "Invalid user identity.");
    }
    let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
    unsafe {
        GetTokenInformation(
            HANDLE(token.0),
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            needed,
            &mut needed,
        )?;
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut text = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut text)?;
        let result = text.to_string();
        windows::Win32::Foundation::LocalFree(Some(HLOCAL(text.0.cast())));
        Ok(result?)
    }
}
pub fn single_instance() -> Result<Instance> {
    let h = unsafe { CreateMutexW(null_mut(), 0, wide("Local\\WorkFlow.Teploty.v1").as_ptr()) };
    if h.is_null() {
        return Err(error(crate::tr!("Zámek aplikace", "Application lock")));
    }
    let already = unsafe { GetLastError() } == 183;
    let h = OwnedHandle(h);
    if already {
        crate::tbail!(
            "CoolTakt už běží. Otevři jeho existující okno.",
            "CoolTakt is already running. Open its existing window."
        );
    }
    // Permit the same user's non-elevated launcher to signal an elevated instance.
    use windows::{
        Win32::{
            Foundation::HLOCAL,
            Security::{
                Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
                PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
            },
        },
        core::PCWSTR,
    };
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    let acl = format!(
        "D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;0x100002;;;{})S:(ML;;NW;;;ME)",
        current_user_sid()?
    );
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(wide(acl).as_ptr()),
            1,
            &mut descriptor,
            None,
        )?;
    }
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: false.into(),
    };
    let show = unsafe {
        CreateEventW(
            (&attributes as *const SECURITY_ATTRIBUTES)
                .cast_mut()
                .cast(),
            0,
            0,
            wide(SHOW_EVENT).as_ptr(),
        )
    };
    unsafe {
        windows::Win32::Foundation::LocalFree(Some(HLOCAL(descriptor.0)));
    }
    if show.is_null() {
        return Err(error(crate::tr!(
            "Komunikace s běžící aplikací",
            "Communication with the running application"
        )));
    }
    Ok(Instance {
        _mutex: h,
        _show: OwnedHandle(show),
    })
}
pub fn show_error(text: &str) {
    unsafe {
        MessageBoxW(
            null_mut(),
            wide(text).as_ptr(),
            wide("CoolTakt").as_ptr(),
            0x10,
        );
    }
}
pub fn reveal_window(window: isize) {
    if window != 0 {
        unsafe {
            ShowWindowAsync(window as Handle, 9);
            SetForegroundWindow(window as Handle);
        }
    }
}
pub fn open_path(path: &Path) -> Result<()> {
    let result = unsafe {
        ShellExecuteW(
            null_mut(),
            wide("open").as_ptr(),
            wide(path).as_ptr(),
            null(),
            null(),
            1,
        )
    };
    if result <= 32 {
        crate::tbail!(
            "Nelze otevřít cestu (Windows {result}).",
            "Cannot open path (Windows {result})."
        );
    }
    Ok(())
}
pub fn elevate(directory: &Path, demo: bool) -> Result<()> {
    let exe = std::env::current_exe()?;
    let parameters = format!(
        "--wait-for-instance --data-dir \"{}\"{}",
        directory.display(),
        if demo { " --demo" } else { "" }
    );
    let result = unsafe {
        ShellExecuteW(
            null_mut(),
            wide("runas").as_ptr(),
            wide(exe).as_ptr(),
            wide(parameters).as_ptr(),
            null(),
            1,
        )
    };
    if result <= 32 {
        crate::tbail!(
            "Spuštění jako správce nebylo potvrzeno (Windows {result}).",
            "Administrator launch was not confirmed (Windows {result})."
        );
    }
    Ok(())
}
pub fn keep_working(enable: bool) {
    unsafe {
        SetThreadExecutionState(0x80000000 | if enable { 1 } else { 0 });
    }
}
pub fn cpu_times() -> Option<(u64, u64)> {
    let (mut i, mut k, mut u) = (0, 0, 0);
    (unsafe { GetSystemTimes(&mut i, &mut k, &mut u) } != 0).then_some((i, k + u))
}
#[repr(C)]
#[derive(Default)]
struct SystemPowerStatus {
    ac: u8,
    flags: u8,
    percent: u8,
    reserved: u8,
    life: u32,
    full: u32,
}
pub fn battery_status() -> (bool, Option<u8>) {
    let mut p = SystemPowerStatus::default();
    if unsafe { GetSystemPowerStatus(&mut p) } == 0 {
        return (true, None);
    }
    (p.ac != 0, (p.percent <= 100).then_some(p.percent))
}
pub fn active_scheme() -> Result<Guid> {
    let mut ptr = null_mut();
    check(
        unsafe { PowerGetActiveScheme(null_mut(), &mut ptr) },
        crate::tr!("Aktivní profil", "Active power plan"),
    )?;
    if ptr.is_null() {
        crate::tbail!(
            "Windows nevrátily profil napájení.",
            "Windows did not return a power plan."
        );
    }
    let value = unsafe { *ptr };
    unsafe {
        LocalFree(ptr.cast());
    }
    Ok(value)
}
fn read_value(scheme: &Guid, sub: &Guid, setting: &Guid, ac: bool) -> Result<u32> {
    let mut value = 0;
    check(
        unsafe {
            if ac {
                PowerReadACValueIndex(null_mut(), scheme, sub, setting, &mut value)
            } else {
                PowerReadDCValueIndex(null_mut(), scheme, sub, setting, &mut value)
            }
        },
        crate::tr!("Čtení napájení", "Reading power settings"),
    )?;
    Ok(value)
}
fn write_value(scheme: &Guid, sub: &Guid, setting: &Guid, value: u32, ac: bool) -> Result<()> {
    check(
        unsafe {
            if ac {
                PowerWriteACValueIndex(null_mut(), scheme, sub, setting, value)
            } else {
                PowerWriteDCValueIndex(null_mut(), scheme, sub, setting, value)
            }
        },
        crate::tr!("Zápis napájení", "Writing power settings"),
    )
}

pub fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    workflow_teploty::persist::write_json(path, value)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SavedSetting {
    sub: Guid,
    key: Guid,
    ac: u32,
    dc: u32,
    last_ac: u32,
    last_dc: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Journal {
    scheme: Guid,
    settings: Vec<SavedSetting>,
}
pub struct PowerSession {
    journal: Journal,
    path: PathBuf,
    closed: bool,
    last_percent: Option<f32>,
}
impl PowerSession {
    pub fn begin(directory: &Path) -> Result<Self> {
        let path = directory.join("power-recovery.json");
        if path.exists() {
            crate::tbail!(
                "Nejdřív obnov předchozí nastavení tlačítkem Obnovit nastavení.",
                "First restore the previous settings using Restore settings."
            );
        }
        let scheme = active_scheme()?;
        let mut settings = Vec::new();
        for (sub, key, required) in [
            (CPU, MAX, true),
            (CPU, MAX1, false),
            (CPU, BOOST, true),
            (SLEEP, WAKE, false),
            (NONE, LOCK, false),
        ] {
            match (
                read_value(&scheme, &sub, &key, true),
                read_value(&scheme, &sub, &key, false),
            ) {
                (Ok(ac), Ok(dc)) => settings.push(SavedSetting {
                    sub,
                    key,
                    ac,
                    dc,
                    last_ac: ac,
                    last_dc: dc,
                }),
                _ if required => crate::tbail!(
                    "Profil nepodporuje řízení výkonu CPU.",
                    "This power plan does not support CPU performance control."
                ),
                _ => {}
            }
        }
        let journal = Journal { scheme, settings };
        write_json(&path, &journal)?;
        Ok(Self {
            journal,
            path,
            closed: false,
            last_percent: None,
        })
    }
    pub fn ensure_scheme(&self) -> Result<()> {
        if active_scheme()? != self.journal.scheme {
            crate::tbail!(
                "Změnil se napájecí profil. Řízení bylo zastaveno.",
                "The power plan changed. Regulation has stopped."
            );
        }
        Ok(())
    }
    fn set(&mut self, key: Guid, ac: u32, dc: u32) -> Result<()> {
        let Some(index) = self.journal.settings.iter().position(|x| x.key == key) else {
            crate::tbail!(
                "Požadované nastavení není podporováno Windows.",
                "This setting is not supported by Windows."
            );
        };
        let old = self.journal.settings[index].clone();
        if old.last_ac == ac && old.last_dc == dc {
            return Ok(());
        }
        // Persist intent before touching Windows: startup recovery can undo even a partial write.
        self.journal.settings[index].last_ac = ac;
        self.journal.settings[index].last_dc = dc;
        write_json(&self.path, &self.journal)?;
        write_value(&self.journal.scheme, &old.sub, &key, ac, true)?;
        write_value(&self.journal.scheme, &old.sub, &key, dc, false)?;
        Ok(())
    }
    fn activate(&self) -> Result<()> {
        self.ensure_scheme()?;
        check(
            unsafe { PowerSetActiveScheme(null_mut(), &self.journal.scheme) },
            crate::tr!("Použití limitu", "Applying performance limit"),
        )
    }
    pub fn apply(&mut self, percent: f32) -> Result<()> {
        self.ensure_scheme()?;
        if !percent.is_finite() || !(5.0..=100.0).contains(&percent) {
            crate::tbail!("Neplatný limit výkonu.", "Invalid performance limit.");
        }
        if self.last_percent.is_some_and(|p| (p - percent).abs() < 0.5) {
            return Ok(());
        }
        for key in [MAX, MAX1] {
            if let Some(base) = self.journal.settings.iter().find(|s| s.key == key).cloned() {
                let scale =
                    |v: u32| ((v as f32 * percent / 100.).round() as u32).clamp(1, v.max(1));
                self.set(key, scale(base.ac), scale(base.dc))?;
            }
        }
        let base = self
            .journal
            .settings
            .iter()
            .find(|s| s.key == BOOST)
            .unwrap()
            .clone();
        self.set(
            BOOST,
            if percent < 99.5 { 0 } else { base.ac },
            if percent < 99.5 { 0 } else { base.dc },
        )?;
        self.activate()?;
        self.last_percent = Some(percent);
        Ok(())
    }
    pub fn sleep(&mut self, minutes: Option<u32>, without_password: bool) -> Result<()> {
        self.ensure_scheme()?;
        enable_sleep_privilege()?;
        let timer = if let Some(m) = minutes {
            Some(WakeTimer::new(m)?)
        } else {
            None
        };
        let result = (|| {
            self.set(
                WAKE,
                u32::from(minutes.is_some()),
                u32::from(minutes.is_some()),
            )?;
            if without_password && minutes.is_some() {
                self.set(LOCK, 0, 0)?;
            }
            self.activate()?;
            keep_working(false);
            if unsafe { SetSuspendState(0, 0, u8::from(minutes.is_none())) } == 0 {
                return Err(error(crate::tr!("Uspání", "Sleep")));
            }
            Ok(())
        })();
        drop(timer);
        // Always restore wake/authentication settings immediately on return, including errors.
        let mut cleanup = Ok(());
        for key in [WAKE, LOCK] {
            if let Some(base) = self.journal.settings.iter().find(|s| s.key == key).cloned()
                && let Err(e) = self.set(key, base.ac, base.dc)
            {
                cleanup = Err(e);
            }
        }
        if active_scheme().ok() == Some(self.journal.scheme) {
            let _ = self.activate();
        }
        keep_working(true);
        result.and(cleanup)
    }
    pub fn restore(&mut self) -> Result<()> {
        let result = restore_journal(&self.journal, &self.path);
        if result.is_ok() {
            self.closed = true;
        }
        result
    }
    pub fn recover(directory: &Path) -> Result<()> {
        let path = directory.join("power-recovery.json");
        if !path.exists() {
            return Ok(());
        }
        let journal: Journal = serde_json::from_slice(&std::fs::read(&path)?)?;
        // Reject foreign or tampered recovery records rather than writing arbitrary power GUIDs.
        if journal.settings.iter().any(|s| {
            ![
                (CPU, MAX),
                (CPU, MAX1),
                (CPU, BOOST),
                (SLEEP, WAKE),
                (NONE, LOCK),
            ]
            .contains(&(s.sub, s.key))
        }) {
            crate::tbail!("Neplatný soubor obnovy.", "Invalid recovery file.");
        }
        restore_journal(&journal, &path)
    }
}
impl Drop for PowerSession {
    fn drop(&mut self) {
        if !self.closed {
            let _ = self.restore();
        }
    }
}
fn restore_journal(journal: &Journal, path: &Path) -> Result<()> {
    for entry in &journal.settings {
        for (ac, original, last) in [
            (true, entry.ac, entry.last_ac),
            (false, entry.dc, entry.last_dc),
        ] {
            let current = read_value(&journal.scheme, &entry.sub, &entry.key, ac)?;
            if current == last {
                write_value(&journal.scheme, &entry.sub, &entry.key, original, ac)?;
            }
            // An external edit is preserved, not overwritten by our old snapshot.
        }
    }
    if active_scheme().ok() == Some(journal.scheme) {
        check(
            unsafe { PowerSetActiveScheme(null_mut(), &journal.scheme) },
            crate::tr!("Obnova profilu", "Restoring power plan"),
        )?;
    }
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}
fn enable_sleep_privilege() -> Result<()> {
    let mut token = null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), 0x20 | 0x8, &mut token) } == 0 {
        return Err(error(crate::tr!("Oprávnění ke spánku", "Sleep privileges")));
    }
    let token = OwnedHandle(token);
    let mut p = TokenPrivileges {
        count: 1,
        luid: Luid { low: 0, high: 0 },
        attributes: 2,
    };
    if unsafe { LookupPrivilegeValueW(null(), wide("SeShutdownPrivilege").as_ptr(), &mut p.luid) }
        == 0
    {
        return Err(error(crate::tr!("Oprávnění ke spánku", "Sleep privileges")));
    }
    unsafe {
        SetLastError(0);
    }
    if unsafe { AdjustTokenPrivileges(token.0, 0, &p, 0, null_mut(), null_mut()) } == 0
        || unsafe { GetLastError() } != 0
    {
        return Err(error(crate::tr!("Povolení spánku", "Enabling sleep")));
    }
    Ok(())
}
struct WakeTimer(OwnedHandle);
impl WakeTimer {
    fn new(minutes: u32) -> Result<Self> {
        let handle = unsafe { CreateWaitableTimerW(null_mut(), 1, null()) };
        if handle.is_null() {
            return Err(error(crate::tr!("Časovač probuzení", "Wake timer")));
        }
        let result = Self(OwnedHandle(handle));
        // Absolute FILETIME: relative timers do not count time spent asleep on modern Windows.
        let seconds = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()
            + 11_644_473_600
            + u64::from(minutes) * 60;
        let due = (seconds * 10_000_000) as i64;
        unsafe {
            SetLastError(0);
        }
        if unsafe { SetWaitableTimer(handle, &due, 0, null_mut(), null_mut(), 1) } == 0 {
            return Err(error(crate::tr!(
                "Nastavení probuzení",
                "Setting wake timer"
            )));
        }
        if unsafe { GetLastError() } == 50 {
            crate::tbail!(
                "Firmware nepodporuje probuzení časovačem. Počítač nebyl uspán.",
                "The firmware does not support wake timers. The computer was not put to sleep."
            );
        }
        Ok(result)
    }
}
impl Drop for WakeTimer {
    fn drop(&mut self) {
        unsafe {
            CancelWaitableTimer(self.0.0);
        }
    }
}

#[repr(C)]
struct DeviceInterface {
    size: u32,
    class: Guid,
    flags: u32,
    reserved: usize,
}
#[repr(C)]
struct BatteryQuery {
    tag: u32,
    level: u32,
    rate: i32,
}
#[link(name = "setupapi")]
unsafe extern "system" {
    fn SetupDiGetClassDevsW(
        class: *const Guid,
        enumerator: *const u16,
        window: Handle,
        flags: u32,
    ) -> Handle;
    fn SetupDiEnumDeviceInterfaces(
        info: Handle,
        device: Handle,
        class: *const Guid,
        index: u32,
        data: *mut DeviceInterface,
    ) -> i32;
    fn SetupDiGetDeviceInterfaceDetailW(
        info: Handle,
        data: *const DeviceInterface,
        detail: Handle,
        size: u32,
        required: *mut u32,
        device: Handle,
    ) -> i32;
    fn SetupDiDestroyDeviceInfoList(info: Handle) -> i32;
}
pub struct BatteryDevice(OwnedHandle);
impl BatteryDevice {
    pub fn open() -> Result<Self> {
        const CLASS: Guid = Guid::new(0x72631e54_78a4_11d0_bcf7_00aa00b7b32a);
        let info = unsafe { SetupDiGetClassDevsW(&CLASS, null(), null_mut(), 0x12) };
        if info as isize == -1 {
            return Err(error(crate::tr!("Rozhraní baterie", "Battery interface")));
        }
        let result = (|| {
            let mut interface = DeviceInterface {
                size: size_of::<DeviceInterface>() as u32,
                class: CLASS,
                flags: 0,
                reserved: 0,
            };
            if unsafe { SetupDiEnumDeviceInterfaces(info, null_mut(), &CLASS, 0, &mut interface) }
                == 0
            {
                return Err(error(crate::tr!(
                    "Baterie nebyla nalezena",
                    "Battery not found"
                )));
            }
            let mut needed = 0;
            unsafe {
                SetupDiGetDeviceInterfaceDetailW(
                    info,
                    &interface,
                    null_mut(),
                    0,
                    &mut needed,
                    null_mut(),
                );
            }
            if !(8..=65536).contains(&needed) {
                crate::tbail!("Neplatné rozhraní baterie.", "Invalid battery interface.");
            }
            let mut buffer = vec![0u32; (needed as usize).div_ceil(4)];
            buffer[0] = if size_of::<usize>() == 8 { 8 } else { 6 };
            if unsafe {
                SetupDiGetDeviceInterfaceDetailW(
                    info,
                    &interface,
                    buffer.as_mut_ptr().cast(),
                    needed,
                    &mut needed,
                    null_mut(),
                )
            } == 0
            {
                return Err(error(crate::tr!("Cesta baterie", "Battery device path")));
            }
            let path = unsafe { buffer.as_ptr().cast::<u8>().add(4).cast::<u16>() };
            let h = unsafe { CreateFileW(path, 0x80000000, 3, null_mut(), 3, 0x80, null_mut()) };
            if h as isize == -1 {
                return Err(error(crate::tr!(
                    "Otevření čidla baterie",
                    "Opening battery sensor"
                )));
            }
            Ok(Self(OwnedHandle(h)))
        })();
        unsafe {
            SetupDiDestroyDeviceInfoList(info);
        }
        result
    }
    pub fn temperature(&self) -> Result<f32> {
        let timeout = 0u32;
        let mut tag = 0u32;
        let mut returned = 0u32;
        if unsafe {
            DeviceIoControl(
                self.0.0,
                0x294040,
                (&timeout as *const u32).cast(),
                4,
                (&mut tag as *mut u32).cast(),
                4,
                &mut returned,
                null_mut(),
            )
        } == 0
            || tag == 0
        {
            return Err(error(crate::tr!(
                "Identifikace baterie",
                "Battery identification"
            )));
        }
        let query = BatteryQuery {
            tag,
            level: 2,
            rate: 0,
        };
        let mut value = 0u32;
        if unsafe {
            DeviceIoControl(
                self.0.0,
                0x294044,
                (&query as *const BatteryQuery).cast(),
                size_of::<BatteryQuery>() as u32,
                (&mut value as *mut u32).cast(),
                4,
                &mut returned,
                null_mut(),
            )
        } == 0
        {
            return Err(error(crate::tr!(
                "Ovladač neposkytuje teplotu baterie",
                "The driver does not provide battery temperature"
            )));
        }
        let c = value as f32 / 10. - 273.15;
        if returned != 4 || !(-20.0..=100.).contains(&c) {
            crate::tbail!(
                "Ovladač vrátil neplatnou teplotu baterie.",
                "The driver returned an invalid battery temperature."
            );
        }
        Ok(c)
    }
}
