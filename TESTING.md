# Validation scope

This is an initial Windows x64 preview. Implemented features and automated checks do not by themselves establish hardware compatibility.

## Automated checks

Run on Windows with the MSVC Rust toolchain:

```powershell
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --release --all-targets --locked
cargo build --release --locked
```

The 14 tests cover curve interpolation and validation, missing sensors, stricter-rule selection, lid and AC gating, old-settings migration, independent CPU/battery sleep, optional wake precedence, delayed performance recovery, named-profile validation and persistence, language codes and persisted preferences, and application validation errors in both English and Czech.

Tests do not change the active power plan, register startup, request elevation, lock the session or put the computer to sleep.

## Desktop checks

The Windows desktop preview was checked with synthetic data at a 1200 × 740 client size:

- Overview with the enabled closed-lid condition visible below the live-value cards.
- Side-by-side CPU and battery curves, collapsed point tables and independent sleep controls.
- Time-based plots with extrema and threshold-crossing markers.
- Settings navigation, immediate English/Czech switching and light/dark appearance.
- Preferences written with the selected language and theme.
- Launching a second process returned exit code 0 and revealed the existing process instead of starting another controller.

Desktop checks use `--demo`; they do not validate physical power control or sleep.

## Read-only hardware observations

- Native Windows lid notifications returned a lid state.
- The tested Lenovo CPU provider required administrator access.
- The tested battery driver did not expose a temperature. An absent reading is displayed as unavailable; no temperature is fabricated.

These observations are specific to the tested machine. No device identifiers, diagnostics or local user settings are included in this repository.

## Remaining hardware validation

- Actual CPU limits and recovery on each supported processor/power plan, including external plan changes.
- Thermal sleep, timed wake and suppression of wake events on real firmware.
- Behavior under Modern Standby and organization power policies.
- Optional password-free resume and restoration of the original setting.
- Elevated startup task registration, removal and execution after a real sign-in.
- CPU and battery temperature providers on additional models.

Validate these with supervised workloads and adequate ventilation. The application cannot certify operation in an enclosed bag; CPU limiting also does not directly limit a dedicated GPU.
