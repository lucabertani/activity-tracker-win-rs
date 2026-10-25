# Piano: migrazione della GUI a GPUI + revisione del thermal throttling

Stato: **approvato** il 2026-10-25, in corso sul branch `gpui-migration`.

## Decisioni e avanzamento

Decisioni prese:

1. Parte B approvata per intero (B1–B9).
2. Settings come **pannello nella finestra principale**.
3. **Titlebar custom**.
4. Exe più grande accettato.
5. La GUI Win32 attuale resta come **fallback**: feature Cargo `gui-win32`
   e flag di avvio `--legacy-ui`; fallback automatico se GPUI non parte.
6. Codice diviso in **più crate** (logica separata dalle GUI), vedi A.4.

Avanzamento:

- [x] Parte B — thermal throttling (commit `fix: rework thermal throttling
      detection`).
- [x] Fase 0 — spike: `gpui-kit` 0.8.0 compila su MSVC con CRT statico.
      Hello world release: exe 18,8 MB, finestra in ~0,9 s, CPU a riposo
      ~10 ms ogni 3 s, **working set ~170 MB** (renderer DirectX + font).
      Conseguenza: quando va nel tray la finestra GPUI viene *chiusa*
      (non solo nascosta) e ricreata all'apertura.
- [x] Fase 1 — workspace a crate, GUI legacy su `tracker-core`.
- [x] Fase 2 — `Sampler` (thread di campionamento con pausa dell'elenco
      processi a finestra chiusa), `TrayHost` (tray su thread dedicato,
      ri-aggiunge le icone se Explorer riparte), `TrayIcons` condiviso.
- [x] Fase 3 — finestra principale GPUI: titlebar custom, card
      RAM/CPU/throttling, ricerca, `DataTable` virtualizzata con sort
      dall'header (pulsanti di sort rimossi), gruppi espandibili, selezione
      stabile per identità, Terminate (pulsante + `Del`), chiusura nel tray
      e riapertura dal tray, Exit.
- [x] Fase 4 — pannello impostazioni sovrapposto a destra, applicazione
      immediata e salvataggio, anteprime icone identiche al tray.
- [x] Fase 5 — tema chiaro/scuro che segue Windows, Segoe UI Variable,
      shortcut (`Ctrl+F`, `Del`, `Enter`, `Ctrl+W`).
- [x] Fase 6 — README; fallback automatico e `--legacy-ui`.

Verifiche fatte (smoke test automatizzati con click/tastiera simulati e
screenshot, `APPDATA` temporaneo per non toccare le impostazioni reali):
avvio, espansione gruppo, ricerca, sort per nome, pannello impostazioni,
salvataggio su file, chiusura nel tray, riapertura dal tray (una sola
finestra), Exit dal menu del tray.

Note emerse:

- In questa versione il componente virtualizzato è `DataTable` (non
  `Table`); il click sull'header cicla Default → Desc → Asc; "Default"
  torna all'ordinamento dell'app (RAM decrescente).
- `gpui-pre` incorpora già un manifest (asInvoker + DPI per-monitor +
  Common Controls v6): `build.rs` aggiunge il nostro solo senza GPUI.
- Chiudere la finestra GPUI libera solo una parte della memoria: il device
  DirectX resta allocato a livello di processo.

Misure release (Core Ultra 9 185H, refresh 4 s, senza driver):

| | GPUI, finestra aperta | GPUI, nel tray | Legacy Win32 |
|---|---|---|---|
| exe | 30,3 MB | | pochi MB con la build solo legacy (non rimisurato) |
| apertura finestra | 0,32 s | | |
| working set | 123 MB | 120 MB | 16 MB |
| CPU (20 s) | 469 ms (~2,3% di un core) | 78 ms (~0,4%) | 109 ms (~0,5%) |

A finestra aperta e senza tick la GUI GPUI consuma ~190 ms ogni 20 s; il
resto è il ridisegno della tabella a ogni aggiornamento. Possibile
ottimizzazione futura: ridisegnare solo se le righe visibili cambiano.

Limite noto: con GPUI compilato l'exe usa il manifest DPI-aware di gpui,
quindi anche la GUI legacy (`--legacy-ui`) diventa DPI-aware; il suo layout
a pixel fissi appare più piccolo con scaling > 100%.


Il documento ha due parti indipendenti:

- **Parte A**: migrazione della GUI Win32 (controlli standard) a GPUI.
- **Parte B**: verifica del thermal throttling, con i problemi trovati e le
  correzioni proposte.

Le due parti si possono approvare separatamente. Consiglio di fare **prima la
Parte B** (è piccola, tocca solo `src/monitor/` ed è indipendente dalla GUI) e
poi la Parte A, così la nuova GUI nasce già con il modello dati giusto
(stato del throttling "sconosciuto", motivo del throttling, ecc.).

---

## Parte A — Migrazione a GPUI

### A.1 Obiettivi e vincoli

- GUI nativa compilata, niente webview (no Tauri/Electron/WebView2).
- Semplice, pulita, moderna, veloce (avvio rapido, refresh fluido, CPU idle
  trascurabile quando la finestra è nascosta).
- **Parità funzionale completa** con l'attuale versione (checklist in A.3).
- Rimozione dei pulsanti "Sort Name" / "Sort RAM": l'ordinamento si fa
  cliccando l'intestazione di colonna, che la tabella supporta nativamente.

### A.2 Scelta tecnologica

| Crate | Versione (crates.io, ott 2026) | Ruolo |
|---|---|---|
| `gpui` | 0.2.2 (Zed) | framework: rendering GPU (DirectX 11 su Windows), layout, input, finestre |
| `gpui-component` | 0.8.0 (Longbridge, repo `longbridge/gpui-kit`) | componenti pronti: `Table` virtualizzata con sort per colonna e colonne ridimensionabili, `Input`, `Button`, `Switch`/`Checkbox`, `Tabs`, `ColorPicker`, `Dropdown`, `Notification`, temi chiaro/scuro |

Perché `gpui-component` e non GPUI "nudo": GPUI non ha widget (solo
primitive `div`, testo, liste virtuali). Scrivere a mano una tabella
virtualizzata con header cliccabili, selezione, scroll e un campo di testo con
cursore/selezione/IME sarebbe la parte più lunga del lavoro. `gpui-component`
fornisce `TableDelegate` con:

- `columns_count` / `rows_count` / `column` / `render_td` (obbligatori);
- `perform_sort(col_ix, ColumnSort, …)`: hook chiamato al click sull'header
  → **sostituisce i pulsanti di sort**;
- `render_tr` (per il doppio click su riga gruppo → espandi/comprimi);
- `context_menu(row_ix, menu, …)`: menu contestuale per riga (ci mettiamo
  "Termina processo" / "Termina gruppo");
- virtual scrolling (centinaia di migliaia di righe: i nostri ~300 processi
  sono banali).

**Rischio da chiudere nella Fase 0**: l'ecosistema GPUI fuori da Zed si muove
in fretta. `gpui-component` 0.8 dipende da un GPUI "pre" pinnato
(`gpui-pre =0.3.9` + `gpui-base`), non dal `gpui` 0.2.2 di crates.io, e il
progetto si sta riorganizzando sotto il crate ombrello `gpui-kit`. La Fase 0
fissa una combinazione che compila su Windows/MSVC e la pinna in
`Cargo.toml` con versioni esatte (`=x.y.z`).

Alternative scartate (per memoria): `egui` (immediate mode, ottimo ma look
meno "app"), `iced` (valido, tabella da costruire a mano), `slint` (DSL
separato, licenza GPL/commerciale per desktop), WinUI 3 via `windows-rs`
(pesante da usare da Rust).

### A.3 Checklist di parità funzionale

Tutto ciò che c'è oggi deve esserci anche dopo (usata come test di
accettazione della Fase 6):

**Finestra principale**
- [ ] Header: "Total RAM used: N%", "CPU avg: N °C" / "n/a", indicatore
      rosso di thermal throttling.
- [ ] Pulsante "Settings".
- [ ] Campo di ricerca processi (filtro live mentre si scrive).
- [ ] Ordinamento per Nome / RAM, asc/desc (ora: click sull'header; default
      RAM desc come oggi).
- [ ] Tabella colonne Process / PID / RAM MiB / RAM %.
- [ ] Righe raggruppate per nome processo: riga gruppo `► nome (Nx)` con
      totale; doppio click espande/comprime; le righe figlie mostrano i
      singoli PID; stato espanso conservato tra un refresh e l'altro.
- [ ] Riga finale "System & shared" (memoria di nessun processo).
- [ ] Processi che negano l'apertura sono elencati (commit `cad19d2`).
- [ ] La selezione e la posizione di scroll **non saltano** al refresh
      periodico (oggi c'è codice dedicato: `LVSICF_NOSCROLL`).
- [ ] "Terminate process": su riga singola termina il PID, su riga gruppo
      tutti i PID del gruppo; mai il processo dell'app stessa; riga System
      non terminabile; errori mostrati all'utente.
- [ ] Refresh periodico (intervallo da settings, 1–60 s).
- [ ] Minimizza → nasconde nel tray; chiudi (X) → nasconde nel tray, l'app
      resta attiva.

**Tray**
- [ ] Icona RAM (percentuale) e icona CPU (temperatura), disegnate con GDI e
      antialias come oggi (`src/ui/tray.rs`), colori/font da settings.
- [ ] Sfondo rosso durante il throttling (se abilitato; sulla CPU, o sulla
      RAM se la CPU è spenta).
- [ ] Icona fallback "AT" quando entrambe sono disabilitate.
- [ ] Tooltip aggiornati.
- [ ] Doppio click → mostra finestra in primo piano; tasto destro → menu
      "Open window" / "Exit".

**Settings** (oggi finestra a tab separata, `settings_window.rs`)
- [ ] Tab General: avvio con Windows, avvio in background, icone rosse in
      throttling, intervallo di refresh, Apply.
- [ ] Tab Tray icons: abilita RAM/CPU, anteprime live delle icone, color
      picker sfondo/testo per ciascuna, dimensione font (auto, 14–20),
      peso font (100–550), "Restore defaults".
- [ ] Persistenza `settings.json` (stesso formato: compatibilità con i file
      esistenti), autostart via Task Scheduler.

**Avvio**
- [ ] Relaunch elevato (UAC) se PawnIO nega l'accesso; rifiuto UAC → l'app
      parte senza temperature.
- [ ] `--background` / `window_start_hidden` → parte nascosta.
- [ ] Panic hook e errori di avvio in MessageBox (restano Win32: devono
      funzionare anche se GPUI non parte).
- [ ] Exe singolo self-contained, CRT statico (`.cargo/config.toml`), icona
      e manifest via `build.rs` invariati.

### A.4 Architettura target

Workspace Cargo: il pacchetto radice è il binario, le librerie stanno in
`crates/`. Le frecce indicano "dipende da".

```
activity-tracker-win-rs (bin, root)   main.rs: panic hook, elevazione,
   │                                   scelta GUI (feature + --legacy-ui)
   ├── tracker-gui-gpui   (feature gui-gpui)   nuova GUI
   ├── tracker-gui-win32  (feature gui-win32)  GUI legacy / fallback
   │        │
   │        └──► tracker-platform   Windows: monitor (MSR, sampler, processi,
   │                 │               memoria), settings store, autostart,
   │                 │               elevazione, startup, tray (GDI +
   │                 │               Shell_NotifyIcon)
   │                 └──► tracker-core   modello, righe, ProcessList (ricerca,
   │                                     sort, gruppi, selezione per identità,
   │                                     PID da terminare): niente OS, niente GUI
```

Regole:

- `tracker-core` non dipende da Windows né da una GUI: si testa ovunque.
- Le GUI non leggono il sistema direttamente: passano da
  `tracker-platform` e da `tracker-core::process_list::ProcessList`.
  Cambiare GUI = scrivere un nuovo crate `tracker-gui-*` (~1 file di
  rendering + adattatori).
- Il binario sceglie la GUI: `--legacy-ui` forza quella Win32; se GPUI
  fallisce l'avvio si passa automaticamente alla legacy. Con
  `--no-default-features --features gui-win32` si compila solo la legacy
  (exe piccolo).
- `--no-elevate` (sviluppo/smoke test): non chiede l'UAC.

Punti chiave del design:

1. **Separare la logica dalla GUI prima di cambiare GUI** (Fase 1). Oggi
   `AppState` contiene `HBRUSH`/`HICON`/`HWND` e la logica (sort, gruppi,
   terminate) vive dentro `wnd_proc`. Estraendola in `core/` la si testa con
   unit test e la nuova GUI diventa un sottile strato di rendering.
2. **Campionamento fuori dal thread UI.** Oggi `refresh_ui_data` gira nel
   `WM_TIMER` e blocca la UI durante snapshot processi + letture MSR. Con
   GPUI: un thread `sampler` produce uno `Snapshot { ram, cpu_temp,
   throttle, processes }` ogni N secondi; la GUI lo riceve via canale
   (`cx.spawn` + `async_channel`/`smol`) e fa solo `cx.notify()`.
   Quando la finestra è nascosta il sampler continua (serve al tray) ma la
   GUI non ridisegna nulla → CPU idle minima.
3. **Il tray resta Win32.** GPUI non ha API per la system tray. Il codice
   GDI attuale funziona bene e va solo spostato: un thread dedicato con una
   finestra *message-only* (`HWND_MESSAGE`) riceve `WM_TRAYICON`, invia
   eventi alla GUI (`ShowWindow`, `Exit`) e riceve gli aggiornamenti icona
   (percentuale, temperatura, rosso, colori) dal sampler. Alternativa da
   valutare nello spike: crate `tray-icon` (ma dovremmo comunque passargli
   le icone disegnate con GDI → poco guadagno).
4. **Nascondi invece di chiudere.** `window.on_window_should_close(|…| {
   hide(); false })` + `ShowWindow(SW_HIDE)` sull'HWND ottenuto da
   `raw-window-handle` (GPUI implementa `HasWindowHandle`). Minimize → hide
   via subclass/`WM_SIZE` se GPUI non espone l'evento (da verificare nello
   spike). L'app non deve uscire quando l'ultima finestra è nascosta.
5. **Settings**: pannello a scomparsa a destra nella finestra principale
   (deciso). Le anteprime delle icone tray si ottengono
   esportando il bitmap che il renderer GDI già produce come RGBA →
   `gpui::ImageSource` (anteprima pixel-identica al tray vero).
6. **Tabella**: sort tramite `perform_sort` (Nome, RAM MiB, RAM %; PID
   non ordinabile o ordinabile, a scelta); i gruppi restano "blocchi"
   ordinati per totale, figli ordinati dentro il gruppo, riga System
   sempre in fondo (logica già in `build_display_rows`). Selezione per
   *identità* (PID o nome gruppo), non per indice, così il refresh non la
   sposta. Numeri allineati a destra con font tabulare.
7. **Terminate**: pulsante nella toolbar (abilitato solo con selezione
   valida) + voce nel menu contestuale della riga + tasto `Del`. Conferma
   per i gruppi (dialog `gpui-component`). Errori come `Notification`
   toast invece di MessageBox.

### A.5 Proposta grafica

Stile "Fluent-like" sobrio, coerente con Windows 11:

- Tema chiaro/scuro che segue quello di sistema
  (`HKCU\...\Personalize\AppsUseLightTheme`), font Segoe UI Variable.
- **Header a "card"**: tre riquadri compatti — RAM (percentuale grande +
  barra sottile + "12.3 / 32 GB"), CPU (temperatura grande + colore che
  scala verde→ambra→rosso), stato throttling (badge "OK" / "THROTTLING" /
  "n/a" con motivo nel tooltip).
- Toolbar: campo ricerca con icona lente e `Ctrl+F`, pulsante Terminate,
  icona ingranaggio per i settings.
- Tabella a righe alternate leggere, niente gridlines, header sticky con
  freccia di ordinamento, chevron animato per i gruppi, barra RAM% inline
  nella colonna percentuale.
- Titlebar custom GPUI (deciso): titolo, pulsanti minimizza/chiudi che
  mandano nel tray, area di trascinamento.

### A.6 Fasi

Ogni fase termina con: `cargo build`, `cargo test`, `cargo clippy -- -D
warnings`, `cargo fmt --all -- --check` puliti (regola di `AGENTS.md`) e un
commit.

| Fase | Contenuto | Esito atteso |
|---|---|---|
| **0. Spike** (timebox) | Branch `gpui-spike`. Scegliere e pinnare la combinazione `gpui`/`gpui-component` che compila su MSVC con CRT statico. Prototipo: finestra con `Table` di 300 righe finte ordinabili, `Input`, hide-on-close, tray Win32 su thread separato che riapre la finestra, HWND da raw-window-handle. Misurare dimensione exe, tempo di avvio, RAM e CPU idle. | Go/No-Go documentato in questo file. Se No-Go: ripiego su `iced` o `egui` con lo stesso piano. |
| **1. Estrazione core** | Creare `core/` (stato, sort, ricerca, gruppi, terminate) e `platform/` (tray, elevation) dal codice attuale, **senza cambiare la GUI Win32**. Unit test su sort/ricerca/gruppi/selezione per identità. | GUI vecchia identica, logica testata e indipendente da Win32. |
| **2. Sampler** | Thread di campionamento + `Snapshot` + canale; la GUI Win32 attuale lo consuma (verifica che il modello funzioni prima di cambiare GUI). | UI non più bloccata durante il refresh. |
| **3. Finestra principale GPUI** | `gui/` con header, ricerca, tabella (sort da header, gruppi, System row, selezione stabile), terminate, hide/show, tray collegato. Feature flag Cargo `gui-gpui` per poter ancora compilare la vecchia GUI. | Uso quotidiano possibile con la nuova GUI. |
| **4. Settings GPUI** | Pannello settings con tutti i controlli, anteprime icone, color picker, restore defaults, autostart. | Parità completa. |
| **5. Rifinitura** | Tema chiaro/scuro, card header, toast, shortcut (`Ctrl+F`, `Del`, `Esc` per nascondere), titlebar custom (opzionale), DPI/multi-monitor. | UI finale. |
| **6. Pulizia e rilascio** | Rimuovere la GUI Win32 e il feature flag, file `tray_old_*`, aggiornare README/screenshot, verificare workflow `.github/workflows/build-windows.yml`, checklist A.3 completa a mano. | Release. |

### A.7 Delega a sub-agenti (con mia supervisione)

Le fasi sono sequenziali (ognuna usa il risultato della precedente), quindi
non le parallelizzo alla cieca; delego i sotto-task ben delimitati e reviewo
ogni diff prima del commit.

| Sotto-task | Agente | Note |
|---|---|---|
| Ricerca API esatte `gpui`/`gpui-component` per la versione scelta (Table, Input, finestre, hide, raw handle) | Explore/general-purpose, in parallelo, sola lettura | input per lo spike |
| Spike Fase 0 | io direttamente | decisione critica, non delegata |
| Fase 1: estrazione `core/` + test | general-purpose in worktree isolato | io reviewo il diff e verifico che la GUI vecchia sia invariata |
| Fase 1: spostamento `platform/` (tray, elevation) | general-purpose, worktree separato (file disgiunti dal precedente) | in parallelo al precedente |
| Fase 3: `process_table.rs` (TableDelegate) | general-purpose | contratto: riceve `&[DisplayRow]`, emette azioni |
| Fase 3: header/card + toolbar | general-purpose, in parallelo | file disgiunti |
| Fase 4: settings view | general-purpose | dopo Fase 3 |
| Review finale di ogni fase | `/code-review` + io | clippy/fmt/test obbligatori |

Ogni agente riceve: obiettivo, file che può toccare, contratto di
interfaccia, criteri di accettazione (test + clippy + fmt). Integrazione e
merge li faccio io.

### A.8 Rischi

- **Instabilità API GPUI** (nessuna 1.0, breaking change frequenti) →
  versioni pinnate esatte, aggiornamenti solo deliberati.
- **Requisiti GPU**: GPUI su Windows usa DirectX 11; su VM/RDP senza GPU
  potrebbe usare WARP (software) o non partire → verificare nello spike;
  il panic hook MessageBox resta come rete di sicurezza.
- **Dimensione exe / tempo di build**: GPUI porta molte dipendenze
  (l'exe attuale è di pochi MB; con GPUI stimo 15–30 MB, prima build lenta).
  Da misurare nella Fase 0 e accettare consapevolmente.
- **Hide-to-tray e minimize**: comportamento da verificare con GPUI (punto
  4 di A.4); se serve, subclassing dell'HWND.
- **Licenze**: GPUI Apache-2.0, gpui-component Apache-2.0 → compatibili.

---

## Parte B — Verifica del thermal throttling

### B.1 Come funziona oggi

`CpuThrottleMonitor::refresh()` (`src/monitor/cpu.rs`), chiamato a ogni tick
del timer (default 3 s):

1. **Percorso driver** (PawnIO + modulo IntelMSR, app elevata): per ogni
   processore logico fissa l'affinità del thread, legge
   `IA32_THERM_STATUS` (0x19C) e `IA32_TEMPERATURE_TARGET` (0x1A2);
   throttling = bit 0 attivo su almeno un core in quell'istante.
2. **Fallback senza driver** (o UAC rifiutato): euristica frequenza+carico
   con `sysinfo` — core al ≥60% di uso e frequenza ≤70% del massimo visto →
   throttling dopo 2 tick consecutivi.

### B.2 Problemi trovati

Macchina di test: Intel Core Ultra 9 185H (Meteor Lake, 6P+8E+2LPE, 22
thread), PawnIO 2.2.0 installato. La sessione di verifica **non era
elevata**, quindi il percorso MSR non l'ho potuto eseguire dal vivo (vedi
B.4); il fallback sì.

#### 1. Il fallback senza driver non può mai rilevare nulla — VERIFICATO

`sysinfo` su Windows legge la frequenza da `CallNtPowerInformation`
(`CurrentMhz`), che su questa macchina è **statica**. Test fatto
(`cargo run --example cpu_probe` con 22 processi di stress al 100%):

- sysinfo: tutti i core fermi a **2300 MHz** (P) / **1800 MHz** (E), sia a
  riposo sia sotto carico;
- contatore PDH `\Processor Information(_Total)\% Processor Performance`
  nello stesso momento: **178%** del nominale (≈ 4.1 GHz reali).

Con frequenze costanti il rapporto non scende mai sotto 0.70 → l'euristica
restituisce sempre `false`. In pratica: **senza driver/elevazione la "T"
non si accende mai**, e l'utente non ha modo di saperlo (vede "nessun
throttling", non "non lo so").

Anche se la frequenza fosse corretta, l'euristica ha un difetto di fondo:
il riferimento è il massimo mai visto (un picco turbo single-core, es. 5.1
GHz), e sotto carico all-core la frequenza scende legittimamente per limiti
di potenza (PL1/PL2) ben sotto il 70% → falsi positivi che non sono
throttling *termico*.

#### 2. Campionamento istantaneo: episodi brevi persi

Il bit 0 è lo stato *in quel momento*. Viene letto una volta ogni
`refresh_interval_seconds` (3 s di default, fino a 60): un throttling di
qualche centinaio di ms tra due letture non viene mai visto. Il bit "log"
(bit 1, sticky) risolverebbe, ma va azzerato scrivendo l'MSR, e il modulo
IntelMSR **non permette scritture su 0x19C** (whitelist scritture: solo
0x150, 0x1A4, 0x601, 0x607, 0x608, 0x610 — verificato sul sorgente
`IntelMSR.p`).

#### 3. Si guarda solo il throttling termico del singolo core

Su un portatile (questa macchina è un Yoga Pro) il rallentamento che si
percepisce spesso **non** è il bit 0 del core, ma:

- **PROCHOT esterno** (bit 2 di 0x19C): l'EC/VRM della scheda madre frena
  la CPU (es. temperatura dello chassis o della batteria);
- **limite di potenza** (bit 10 di 0x19C, "Power Limitation Status");
- **throttling a livello package** (`IA32_PACKAGE_THERM_STATUS` 0x1B1,
  bit 0).

HWiNFO li mostra come voci separate. Il modulo IntelMSR consente la lettura
di 0x1B1, 0x198 (`PERF_STATUS`), 0xE7/0xE8 (`MPERF`/`APERF`) e 0x613
(`PKG_PERF_STATUS`), quindi si possono aggiungere senza nuovi driver.

#### 4. Lettura della temperatura accettata anche se non valida

`temperature_celsius()` (`src/monitor/msr.rs`) scarta la lettura solo se il
bit 31 "Reading Valid" è spento **e** il readout è 0. Se il bit 31 è spento
ma il readout è diverso da 0, il valore (non valido per specifica Intel)
entra nella media. LibreHardwareMonitor richiede il bit 31. Il test
`decodes_readout_as_offset_below_tjmax` usa proprio valori con bit 31
spento (`0x0037_0001`) e quindi codifica questo comportamento.

#### 5. Media pesata male sui CPU ibridi con Hyper-Threading

La media è fatta per *processore logico*: sul 185H i 6 P-core con HT
contano doppio (12 voci su 22), quindi la media pende verso i P-core, che
sono i più caldi. Va fatta per *core fisico*
(`GetLogicalProcessorInformationEx(RelationProcessorCore)`).

#### 6. Letture ridondanti

`IA32_TEMPERATURE_TARGET` (TjMax) è costante ma viene riletto su ogni core a
ogni tick: raddoppia le chiamate al driver (44 IOCTL + 22 cambi di affinità
per tick sul 185H). Va letto una volta in `open()`.

#### 7. Errori silenziosi

- Se il driver si apre ma tutte le letture falliscono, `refresh()` torna
  `false` ("non sta throttlando") invece di "sconosciuto".
- `1usize << core` va in overflow con più di 64 processori logici (processor
  groups): non riguarda questa macchina, ma va protetto.
- Nel percorso MSR `sysinfo` non viene mai aggiornato, quindi
  `CpuThrottleMonitor::system()` restituisce dati vuoti
  (`examples/cpu_probe.rs` lo usa).

#### 8. UI: indicatore che lampeggia e non spiega

La "T" rossa appare/scompare a ogni tick senza memoria, e non dice né
*quale* motivo né *quanto* spesso. Senza driver mostra vuoto come se tutto
fosse a posto.

### B.3 Correzioni proposte

| # | Correzione | File |
|---|---|---|
| B1 | Stato a tre valori: `ThrottleState::{Unknown, No, Yes(reasons)}`. Senza driver → `Unknown` (UI: "n/a", tooltip "serve PawnIO + avvio elevato"). Rimuovere l'euristica basata su `sysinfo` (non funziona su Windows). | `monitor/cpu.rs`, `model.rs`, UI |
| B2 | Thread campionatore MSR a ~250 ms che fa *latch* di qualsiasi bit attivo fino al tick UI successivo (o "hold" minimo di 5 s per evitare lampeggi). Costo stimato: <1% di un core; da misurare. | `monitor/msr.rs`, nuovo `monitor/thermal_sampler.rs` |
| B3 | Motivi separati: core termico (0x19C b0), PROCHOT (b2), power limit (b10), package termico (0x1B1 b0). Tooltip/badge con il motivo, es. "Throttling: PROCHOT (EC)". Opzionale: temperatura package da 0x1B1. | `monitor/msr.rs` |
| B4 | Opzionale, con driver: frequenza effettiva per core da `APERF/MPERF` × clock nominale (`PLATFORM_INFO` 0xCE) — vera, a differenza di sysinfo — da mostrare nel tooltip/header. | `monitor/msr.rs` |
| B5 | Richiedere il bit 31 per la temperatura; aggiornare i test. | `monitor/msr.rs` |
| B6 | Media per core fisico (topologia letta una volta in `open()`); TjMax letto una volta per core in `open()`. | `monitor/msr.rs` |
| B7 | Letture tutte fallite → `Unknown`; guardia per >64 processori logici (saltarli o gestire i gruppi). | `monitor/msr.rs` |
| B8 | Statistiche: percentuale di campioni in throttling nell'ultimo minuto ("throttled 35% dell'ultimo minuto") — utile perché i bursts sono brevi. | `monitor/`, UI |
| B9 | Aggiornare `msr_probe` per stampare per core: bit 0/1/2/10, 0x1B1, frequenza effettiva; rimuovere/riscrivere `cpu_probe`. | `examples/` |

B1, B2, B5, B6, B7 sono le correzioni che consiglio di fare comunque; B3,
B4, B8 sono miglioramenti (B3 è quello con più valore pratico su un
portatile).

### B.4 Come verificheremo

Il percorso MSR richiede privilegi di amministratore, quindi serve il tuo
aiuto:

1. Da un terminale **elevato**: `cargo run --example msr_probe` (dopo B9)
   a riposo, poi con `stress/stress.py` su tutti i core (o `mandelbrot.py`)
   per alcuni minuti, laptop sulla scrivania e poi "soffocato".
2. In parallelo HWiNFO (Sensors → "Core Thermal Throttling", "Package/Ring
   Thermal Throttling", "IA Limit Reasons"): i motivi riportati devono
   coincidere.
3. Senza elevazione: l'app deve mostrare "n/a" per throttling e
   temperatura, non "OK".

---
