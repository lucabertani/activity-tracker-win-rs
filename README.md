# Activity Tracker (Win32 + Rust)

Monitor total RAM usage, average CPU core temperature and per-core thermal
throttling (HWiNFO-style) from the system tray.

> **Don't want to compile?** Grab the prebuilt executable from the
> [latest release](https://github.com/lucabertani/activity-tracker-win-rs/releases/latest)
> (`activity-tracker-win-rs.exe`, self-contained, no VC++ redistributable needed).
> Real temperatures need the free [PawnIO](https://pawnio.eu) driver, installed
> separately — see
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

1. Double-click `target\release\activity-tracker-win-rs.exe`. If the
   temperature driver (PawnIO) is installed, the app restarts itself
   elevated: confirm the UAC dialog to get CPU temperatures, or decline to
   run without them.
2. The main window opens. If you don't see it: look for the icon in the
   tray (the `^` arrow next to the clock) and double-click it.
3. From a terminal: `target\release\activity-tracker-win-rs.exe`
   (add `--background` to start hidden in the tray).

## CPU temperature and thermal throttling (driver)

Real per-core MSR reads go through the **PawnIO** driver (signed and actively
maintained; the bundled module exposes only two read-only MSRs — see
[`driver/README.md`](driver/README.md) for why the old WinRing0 driver was
dropped). **Install it once** (admin/UAC required):

1. Download the **Official edition** installer from <https://pawnio.eu> and
   run it as administrator.
2. Verify it is installed:
   ```powershell
   Get-ItemProperty HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\PawnIO
   ```
3. The driver loads by itself when the app opens it. It is admin-only, so
   the app automatically restarts itself elevated (UAC) when it finds the
   driver — see below.

To uninstall, use "Apps and features" (PawnIO entry) or the installer.

See [`driver/README.md`](driver/README.md) for details.

Without the driver (or without elevation) temperature and throttling are
`n/a`: Windows reports fixed nominal clocks to user mode, so there is no
reliable fallback. With the driver, a background thread samples the thermal
MSRs of every physical core twice a second, like HWiNFO:

- **thermal throttling** (red warning): core thermal status
  (`IA32_THERM_STATUS` bit 0), package thermal status
  (`IA32_PACKAGE_THERM_STATUS` bit 0) or PROCHOT# (bit 2, often asserted by
  the laptop's embedded controller). A single core is enough, and a burst
  between two UI refreshes is kept visible for 5 seconds;
- **power limit** (bit 10): shown in the tooltip, not as throttling;
- average temperature over the physical cores (`TjMax` minus the digital
  readout, `TjMax` from `IA32_TEMPERATURE_TARGET`), package temperature and
  effective clock (APERF/MPERF).

## Tests and diagnostics

```bash
cargo test                          # unit/integration tests
cargo run --example msr_probe       # per-core status bits, temperatures, clocks (requires the driver + admin)
```

## Notes

- Close the app before rebuilding: Windows locks the running exe
  (`cargo build` fails with "Access is denied").
- Autostart: tick "Start automatically with Windows" and "Start in
  background" in the app window. It registers a Task Scheduler logon task
  ("Run with highest privileges"): the app starts elevated at every logon
  with no UAC prompt, so temperatures work right away. A manual double-click
  always shows the window; only the autostart launch starts hidden.
- Security: the old WinRing0 driver (CVE-2020-14979, any local process could
  read/write MSRs and memory) has been replaced by the signed PawnIO driver,
  which exposes only whitelisted read-only MSR access; see `driver/README.md`.