# Changelog

## 0.2.1 — Lenovo temperature sensor fix

- Fix CPU/GPU temperature discovery: explicitly deserialize the WMI instance's `__PATH`, which generic WMI maps omit. Previously, temperature readings could remain unavailable even when running as administrator.
- Preserve method errors and report missing or invalid measurement results instead of silently discarding them.
- Add a real read-only Windows WMI regression test for system-property lookup. Update the power-recovery installer test to remain meaningful across version bumps.

## 0.2.0 — Signed GitHub updates

- Optional automatic update checks after launch and every six hours, with verified background downloads and installation at the next launch.
- Manual **Check for updates** and **Update and restart** controls in Settings, with English/Czech status and errors.
- Ed25519 release manifests, bounded HTTPS downloads, exact asset hashes, downgrade rejection and repeated verification before installation.
- Native Rust update helper waits for power restoration and process exit, preserves the previous regulation state, and restores/restarts the old executable if startup fails.
- Version display follows the package version; user settings and named profiles remain in place.
- Subprocess integration tests exercise actual Windows executable replacement and rollback without changing power settings.

## 0.1.0 — Initial preview

- Product name: **CoolTakt** — temperature-based performance control.

- Native Rust application for Windows x64 with English/Czech and light/dark interfaces.
- Editable CPU and battery temperature curves controlling CPU performance states.
- Prominent **When to regulate** section; closed-lid-only regulation and thermal sleep enabled by default.
- Independent thermal sleep thresholds with automatic wake optional and disabled by default.
- Tray operation, existing-instance activation, optional logon startup and automatic regulation.
- Power-setting recovery journal, sensor availability checks and measurement export.
- Clearer startup, lid, power-source and wake labels; demo mode has no system-control actions.
- Left sidebar with Overview, Temperature history, Profiles and curves, and Settings; compact pages with collapsible details.
- Named profiles and time-based temperature plots with extrema, threshold crossings and state-change markers.

See [TESTING.md](TESTING.md) for verified behavior and outstanding hardware validation.
