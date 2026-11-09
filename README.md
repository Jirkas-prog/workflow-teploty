# CoolTakt

**Temperature-based CPU performance control for Windows laptops, written in Rust.**

CoolTakt is the product name. The repository, executable build target and existing data paths retain their original identifiers for compatibility.

Define separate CPU and battery temperature curves, keep regulation limited to a closed lid, and optionally let the computer sleep to cool down. The application starts in monitoring mode; enabling regulation is a separate action.

**Status: initial preview.** Windows x64 only. Physical sleep/wake, automatic sign-in behavior and startup registration still require hardware validation. See [TESTING.md](TESTING.md) for the verified scope.

## Controls that match how you work

The left sidebar separates **Overview**, **Temperature history**, **Profiles and curves**, and **Settings**. Overview keeps the lid condition and current readings visible. Curve point tables and detailed event logs are collapsible to preserve vertical space.

- **When to regulate:** “Regulate only with the lid closed” is enabled by default. Opening the lid restores original CPU performance and disables thermal sleep. An unknown lid state pauses both actions. A separate battery-only option pauses regulation on AC power.
- **Two temperature curves:** edit by dragging points, double-clicking the graph, or entering exact numbers. Higher temperatures cannot increase the permitted performance. The stricter enabled curve wins.
- **Independent sleep rules:** CPU and battery each have their own temperature threshold. Automatic wake after 1–120 minutes is optional and off by default for both.
- **Background operation:** a tray icon opens the window. Closing the window hides it; Quit restores power settings and exits. Launching the program again reveals the existing instance.
- **Startup:** optionally launch in the tray after Windows sign-in. Automatically starting regulation is a separate, initially disabled option.
- **English / Czech and Light / Dark:** changes take effect immediately and are saved for the next launch. Existing event log entries keep their original language; Windows error details follow the OS language.
- **History:** up to 900 readings on a real time axis, selectable CPU/battery/GPU series, minimum and maximum points, sleep-threshold crossings, and markers for CPU-limit or lid changes. Hover for exact readings; export CSV when needed.
- **Named profiles:** save and load curve/rule presets. Loading a profile edits a draft; **Save and apply** activates it. User-created profile names are preserved in their original language.
- **Signed GitHub updates:** optional background checks and downloads, with installation at the next launch. Manual checking and immediate restart are available in Settings.

## First use

1. Download **CoolTakt-Windows-x64.exe** from [Releases](https://github.com/Jirkas-prog/workflow-teploty/releases/latest), save it in a permanent writable folder, and run it. Initially it only reads sensors. Building from source is also supported.
2. Choose **Run as administrator** if the CPU sensor requires it. Available sensors depend on the firmware and installed drivers.
3. In **When to regulate**, select the lid and power conditions. Closed-lid regulation is enabled by default, including when loading older settings.
4. Edit the CPU and battery curves and, optionally, their sleep rules. Use **Save and apply**, then **Start regulation**.
5. **Stop regulation** restores the original power settings. **Quit** also stops the application. Closing the window keeps it running in the tray.

Windows' own lid-close action must be **Do nothing** if work is to continue with the lid closed. This application does not change that setting. It cannot guarantee safe temperatures in a closed bag or replace hardware thermal protection.

### What the performance percentage means

The curve limits CPU performance states relative to the power plan values captured when regulation starts. For example, a 50% curve limit applied to an original 85% maximum processor state produces approximately 43%. At 100%, the original limits are restored. CPU boost is disabled while limiting.

This is not a watt limit or a Task Manager utilization cap. The dedicated GPU is not directly throttled, so GPU-heavy work may still generate substantial heat.

### Sensor availability

CPU temperature is read through Lenovo WMI where supported, or an already-running LibreHardwareMonitor/OpenHardwareMonitor WMI provider. No extra driver or monitoring program is installed automatically.

Battery temperature is queried through WMI `BatteryTemperature` and the native Windows battery interface. Unsupported drivers produce an unavailable reading, never an estimate based on CPU temperature or charging state. Battery rules need a current battery sensor reading to start. The Windows interface defines the reported unit as tenths of a kelvin; the app converts it to Celsius. [Microsoft battery query documentation](https://learn.microsoft.com/en-us/windows/win32/power/battery-query-information-str).

## Sleep and resuming work

The temperature must remain above its threshold for six seconds. A cancellable 15-second countdown follows. Cooling below the threshold or opening the lid cancels the countdown when closed-lid mode is selected.

If both rules trigger, a rule without automatic wake takes precedence. Otherwise, the longer cooling interval is used. Timed wake uses a Windows wake timer and depends on firmware and Windows policies. With automatic wake disabled, the app schedules no timer and requests suppression of wake events. Manual wake remains available. [Microsoft SetSuspendState documentation](https://learn.microsoft.com/en-us/windows/win32/api/powrprof/nf-powrprof-setsuspendstate).

Sleep pauses computations and network transfers. Applications resume in the same session after wake, but interrupted transfers may need to reconnect. Sensors are refreshed after wake and automatic thermal sleep has a one-minute grace period.

**Do not require a password after automatic wake** is a separate option with an explicit confirmation. No credentials are stored. The Windows wake-password setting is changed temporarily and restored after resume. Organization policies, manual locking and Windows Hello may prevent password-free resume. This is not a stored-password automatic-login service; background work can continue while the screen is locked.

## Startup, recovery and stored data

Startup uses a Task Scheduler logon task for the current user. Set it up once from an elevated application if administrator access is required. It starts after sign-in, not before the Windows login screen. No password is saved. Disable the option to remove only this application's task. If you move the executable, configure startup again.

A recovery journal is written before changing power settings. Normal shutdown restores the original values. After a crash, **Restore settings** can recover them; an existing journal blocks automatic regulation. Changing the active power plan stops regulation without switching it back. Restoration preserves values changed by another application.

If an enabled sensor stops reporting, thermal sleep is paused and a conservative 20% CPU limit is applied until the sensor returns or regulation is stopped. Lid and power conditions still take precedence.

Data is stored under `%LOCALAPPDATA%\WorkFlow\Teploty`:

| File | Purpose |
|---|---|
| `nastaveni.json` | Curves and thermal rules |
| `aplikace.json` | Language, appearance, automatic regulation and update preferences |
| `profily.json` | User-created curve and rule profiles |
| `power-recovery.json` | Original power settings while regulation is active |
| `udalosti.log` | Local event history |
| `mereni-*.csv` | Explicitly exported measurements |
| `updates/` | Verified update payload, installation journal and last result |

No telemetry or cloud account is used.

## Automatic updates

In **Settings → GitHub updates**, enable **Automatically download and install updates**. This option is off by default. When enabled, CoolTakt checks after launch and every six hours, downloads a newer stable release in the background, and installs it the next time the application starts. Turning the option off also disables installation of an already downloaded update at launch.

**Check for updates** works independently of that preference. **Update and restart** installs an available download immediately. Save rule changes first; an active cooling countdown or unfinished power recovery blocks the restart. Regulation is stopped and its original power settings are restored before replacement. The app restarts with its previous enabled/paused state and preserves profiles, rules, language and appearance. Other applications are not restarted.

Each release includes an Ed25519-signed manifest with an exact version, official GitHub asset URL, size and SHA-256. CoolTakt verifies the signature against its embedded public key, bounds downloads, rejects older versions, and rechecks the executable before replacement. A temporary copy of the current app performs the Windows file replacement only after that app exits. If the new process fails its startup check, the previous executable is restored and restarted. A failed version is not retried automatically; a manual check can retry it. The last result appears in Settings.

The executable's folder must be writable at the app's current privilege level. This feature does not request elevation or store a GitHub token. Downloads contact GitHub and its asset hosts over HTTPS. Release manifests authenticate updates inside CoolTakt; they are not Windows Authenticode signatures. Offline or failed checks leave the existing app running.

See [RELEASING.md](RELEASING.md) for the signed Windows release procedure.

## Build and development

Requires Windows x64, the Rust MSVC toolchain, and Visual Studio C++ Build Tools with the Windows SDK. The interface uses egui with DirectX 12 and requests a low-power graphics adapter.

```powershell
cargo fmt --check
cargo clippy --all-targets --features release-tools --locked -- -D warnings
cargo test --release --all-targets --features release-tools --locked
cargo build --release --locked
```

Compilation is limited to two jobs. Application code and Windows integration are Rust; no PowerShell, Python or C# runtime helpers are shipped.

```powershell
.\target\release\workflow-teploty.exe --demo --data-dir .\qa\demo
.\target\release\workflow-teploty.exe --diagnostics .\qa\diagnostics.json
.\target\release\workflow-teploty.exe --background
```

Demo mode uses labelled synthetic readings, including a synthetic 30-minute chart, and disables system control and startup registration. Diagnostics only reads sensors. Automated tests do not put the computer to sleep. Test data, diagnostics, logs, local settings and build output are excluded from Git.
