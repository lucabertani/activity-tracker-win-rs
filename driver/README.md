# PawnIO kernel driver (for HWiNFO-style temperature + thermal throttling)

This folder holds the low-level access pieces used by the app to read the
per-core MSR `IA32_THERM_STATUS` (0x19C) — the register HWiNFO reads to flag
"thermal throttling" per core — plus the temperature readout.

## Why PawnIO (and no more WinRing0)

The app used to ship the classic OpenLibSys **WinRing0 1.2.0** driver. That
driver has a real, known vulnerability (**CVE-2020-14979**): its device object
is created with a NULL DACL, so *any* local process — even low-integrity ones —
can read/write arbitrary MSRs, I/O ports and physical memory through it (local
privilege escalation to SYSTEM). Microsoft Defender flags it as
`VulnerableDriver:WinNT/Winring0` and it is on Microsoft's vulnerable driver
blocklist. There is no "fixed newer WinRing0": the flaw is in its design, and
all rebuilds/forks share it. For that reason LibreHardwareMonitor dropped
WinRing0 too, and this project now does the same.

Current stack (both maintained by namazso):

- **PawnIO** — signed kernel driver that only executes signed, whitelisted
  modules. <https://pawnio.eu> (source: <https://github.com/namazso/PawnIO>)
- **IntelMSR module** (`pawnio/IntelMSR.bin`) — the official module from
  [namazso/PawnIO.Modules](https://github.com/namazso/PawnIO.Modules) 0.2.11,
  LGPL-2.1 (`pawnio/COPYING`). It exposes a *whitelist* of MSR reads/writes:
  the app uses only the read-only `IA32_THERM_STATUS` (0x19C) and
  `IA32_TEMPERATURE_TARGET` (0x1A2). No raw MSR/memory/port access exists.

Module provenance (verify before trusting the binary):

- release asset `release_0_2_11.zip`, sha256
  `43608cb89bc84247fef1368a139013f7d043e17db6d6c8dfc9b46bf0905a81f4`
  (matches the digest published by GitHub);
- `pawnio/IntelMSR.bin`, sha256
  `d6ed85d65ab17a22f813ef98207d6d537155ee2ded5976a21cb48413c9b92e5f`.

## Install (run once, as Administrator)

1. Download the **Official edition** installer from <https://pawnio.eu> and
   run it as administrator. (The "Unrestricted" edition disables module
   signature checking — do not use it.)
2. Done: the driver service loads automatically when the app opens the
   device. Verify with:

   ```powershell
   Get-ItemProperty HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\PawnIO
   ```

   or simply check the "CPU avg" label in the app.

## Access model: administrators only

The PawnIO device ACL grants access only to SYSTEM and the built-in
Administrators group (its INF sets `D:P(A;;GA;;;SY)(A;;GA;;;BA)`). That is the
secure design this project switched for. The app manifest is `asInvoker`, so
`cargo run` and double-clicks work normally; at startup, when the device is
present but denies access, the app relaunches itself elevated (UAC) and the
elevated copy takes over. Decline the prompt to run without CPU temperatures
(the label shows `n/a`).

Uninstall from "Apps and features" (PawnIO entry) or via the installer.

## How the app uses it

`src/monitor/msr.rs` opens `\\?\GLOBALROOT\Device\PawnIO`, loads
`pawnio/IntelMSR.bin` with `IOCTL_PIO_LOAD_BINARY`, then calls the module's
`ioctl_read_msr` per logical processor (with thread affinity) via
`IOCTL_PIO_EXECUTE_FN`. When PawnIO is missing (or the CPU is not Intel), the
app silently falls back to the frequency-based heuristic and shows `n/a` for
the temperature.

## Note on the old driver

The previous `WinRing0x64.sys` (sha256
`11bd2c9f9e2397c9a16e0990e4ed2cf0679498fe0fd418a3dfdac60b5c160ee5`, the
original OpenLibSys 1.2.0 build — not tampered with, but vulnerable) was
removed from the repository. If you installed it earlier, remove it with an
admin PowerShell:

```powershell
sc.exe stop WinRing0_1_2_0; sc.exe delete WinRing0_1_2_0
Remove-Item $env:SystemRoot\System32\drivers\WinRing0x64.sys -Force
```
