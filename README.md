# Activity Tracker (Win32 + Rust)

Monitor RAM and per-core CPU thermal throttling (HWiNFO-style) from the tray.

## Requisiti

- Windows 10/11 x64
- [Rust](https://rustup.rs) con toolchain MSVC (`rustup default stable-x86_64-pc-windows-msvc`)
- Visual Studio Build Tools (C++), oppure Visual Studio con "Desktop development with C++"

## Compilare

```bash
cargo build --release
```

Eseguibile generato: `target\release\activity-tracker-win-rs.exe` (autonomo, non serve il redistribuibile VC++).

## Avviare

1. Doppio click su `target\release\activity-tracker-win-rs.exe`.
2. La finestra si apre. Se non la vedi: cerca l'icona nella tray (freccia `^` accanto all'orologio) e fai doppio click su di essa.
3. Da terminale: `target\release\activity-tracker-win-rs.exe` (aggiungi `--background` per avviare nascosto nella tray).

## Rilevamento thermal throttling (opzionale, consigliato)

Senza driver, l'app usa un'euristica frequenza+carico. Con il driver legge il flag MSR reale (`IA32_THERM_STATUS`) come HWiNFO — basta 1 core in throttle per mostrare la scritta rossa.

Installa il driver **una sola volta** (serve admin, UAC):

1. Apri PowerShell come amministratore.
2. Esegui:
   ```powershell
   cd <cartella-del-progetto>
   powershell -ExecutionPolicy Bypass -File .\driver\install-driver.ps1
   ```
3. Verifica che lo stato sia RUNNING:
   ```powershell
   sc.exe query WinRing0_1_2_0
   ```
4. Il driver si carica da solo a ogni avvio di Windows (`start= system`): **nessun admin richiesto in seguito**, né all'avvio né per l'app.

Per disinstallare il driver (sempre da PowerShell admin):

```powershell
powershell -ExecutionPolicy Bypass -File .\driver\install-driver.ps1 -Uninstall
```

## Test e diagnostica

```bash
cargo test                          # test di unità/integrazione
cargo run --example msr_probe       # temperature e flag throttling per-core (richiede il driver)
cargo run --example cpu_probe       # frequenze e utilizzo per-core + verdetto throttling
```

## Note

- Chiudi l'app prima di ricompilare: Windows blocca l'exe in esecuzione (`cargo build` fallisce con "Access is denied").
- Avvio automatico: spunta "Avvio automatico con Windows" e "Avvio in background" nella finestra dell'app. Il doppio click manuale mostra sempre la finestra; solo l'avvio da autostart parte nascosto.
- Sicurezza: il driver WinRing0 permette a qualsiasi utente locale di leggere/scrivere MSR (come HWiNFO/CrystalDiskInfo). OK su macchina personale a utente singolo; vedi `driver/README.md`.
