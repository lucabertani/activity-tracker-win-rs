# Activity Tracker (Win32 + Rust)

Monitor total RAM usage, average CPU core temperature and per-core thermal
throttling (HWiNFO-style) from the system tray.

> **Don't want to compile?** Grab the prebuilt executable from the
> [latest release](https://github.com/lucabertani/activity-tracker-win-rs/releases/latest)
> (`activity-tracker-win-rs.exe`, self-contained, no VC++ redistributable needed).
> The driver for real temperatures is bundled in the release as well — see
> [below](#cpu-temperature-and-thermal-throttling-driver).

## Requirements

- Windows 10/11 x64
- [Rust](https://rustup.rs) with the MSVC toolchain (`rustup default stable-x86_64-pc-windows-msvc`)
- Visual Studio Build Tools (C++), or Visual Studio with "Desktop development with C++"

## Build

```bash
cargo build --release
```

Resulting executable: `target\release\activity-tracker-win-rs.exe`
(self-contained, no VC++ redistributable needed).

## Run

1. Double-click `target\release\activity-tracker-win-rs.exe`.
2. The main window opens. If you don't see it: look for the icon in the
   tray (the `^` arrow next to the clock) and double-click it.
3. From a terminal: `target\release\activity-tracker-win-rs.exe`
   (add `--background` to start hidden in the tray).

## CPU temperature and thermal throttling (driver)

The driver (bundled in this repo under [`driver/`](driver/) and in the
[latest release](https://github.com/lucabertani/activity-tracker-win-rs/releases/latest))
enables real per-core MSR reads. **Install it once** (admin/UAC required):

1. Open PowerShell as administrator.
2. Run (from the project folder or the release folder):
   ```powershell
   powershell -ExecutionPolicy Bypass -File .\driver\install-driver.ps1
   ```
3. Verify the state is RUNNING:
   ```powershell
   sc.exe query WinRing0_1_2_0
   ```
4. The driver loads by itself at every Windows boot (`start= system`):
   **no admin needed afterwards**, neither at boot nor for the app.

To uninstall the driver (again from an admin PowerShell):

```powershell
powershell -ExecutionPolicy Bypass -File .\driver\install-driver.ps1 -Uninstall
```

See [`driver/README.md`](driver/README.md) for details.

Without the driver, the app falls back to a frequency+load heuristic and
cannot read temperatures (the "CPU avg" label shows `n/a`). With the driver,
it reads the real per-core MSR flag (`IA32_THERM_STATUS`) exactly like HWiNFO
— a single throttled core is enough to show the red warning — and reports the
average core temperature from the same register (no extra reads).

## Tests and diagnostics

```bash
cargo test                          # unit/integration tests
cargo run --example msr_probe       # per-core temperatures and throttle flags (requires the driver)
cargo run --example cpu_probe       # per-core frequencies/usage + throttle verdict
```

## Notes

- Close the app before rebuilding: Windows locks the running exe
  (`cargo build` fails with "Access is denied").
- Autostart: tick "Start automatically with Windows" and "Start in
  background" in the app window. A manual double-click always shows the
  window; only the autostart launch starts hidden.
- Security: the WinRing0 driver lets any local user read/write CPU MSRs
  (like HWiNFO/CrystalDiskInfo). Fine on a personal single-user machine;
  see `driver/README.md`.