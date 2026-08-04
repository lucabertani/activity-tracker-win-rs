# WinRing0 kernel driver (for HWiNFO-style thermal throttling detection)

This folder contains the signed WinRing0 low-level access driver used by the
app to read the per-core MSR `IA32_THERM_STATUS` (0x19C) — the exact register
HWiNFO reads to flag "thermal throttling" per core.

## Files

- `WinRing0x64.sys` — the kernel driver (x64, signed).
- `install-driver.ps1` — installs/starts the driver as a kernel service
  (run as Administrator, once). `start= system` makes it load at every boot,
  so no admin is needed afterwards. Use `-Uninstall` to remove it.

## Origin and license

- The driver is the original **OpenLibSys WinRing0 1.2.0** binary, signed by
  its author Noriyuki Miyazaki (hiyohiyo, the CrystalDiskInfo author).
- It was extracted from the **LibreHardwareMonitor v0.9.2** release
  (`LibreHardwareMonitorLib.dll` → embedded resource `WinRing0x64.gz`),
  which bundles it under MPL-2.0; the driver itself is licensed under the
  OpenLibSys modified BSD license.
- Source: https://github.com/OpenLibSys/OpenLibSys (WinRing0 1.2.0)

## Security note — read this

This classic driver lets **any local user** read and write CPU MSRs (it was
never hardened). That is precisely what makes the non-admin detection
possible, but it also means any program running as your user could write MSRs
and compromise the machine. HWiNFO/CrystalDiskInfo-class tools ship the same
driver. Acceptable for a personal, single-user machine; do not deploy it on
a multi-user or untrusted machine. Remove it with `install-driver.ps1
-Uninstall` if you no longer want it.

## How the app uses it

`src/monitor/msr.rs` opens `\\.\WinRing0_1_2_0` and issues
`IOCTL_OLS_READ_MSR` (0x9C402084) on each logical processor (with thread
affinity), checking bit 0 of `IA32_THERM_STATUS`. When the driver is
missing, the app silently falls back to the frequency-based heuristic.
