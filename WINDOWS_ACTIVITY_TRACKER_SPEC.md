# Windows Activity Tracker - Specifica tecnica e piano di implementazione

## Obiettivo
Realizzare un'applicazione desktop per Windows scritta in Rust, nativa, a basso consumo di risorse, senza usare framework web come Electron, Tauri o simili.

L'app deve:
- monitorare l'utilizzo totale della RAM in percentuale, mostrando solo il numero da 0 a 100 senza il simbolo `%`
- mostrare una finestra principale con il dettaglio RAM per singolo processo
- permettere ricerca testuale case-insensitive sui processi attivi
- permettere ordinamento per nome processo e per RAM occupata, sia ASC che DESC
- mostrare un'icona nella taskbar / system tray con il valore della RAM totale occupata in percentuale, senza simbolo
- nascondere la finestra principale quando l'app viene ridotta a icona o chiusa, mantenendo attivo solo il tray icon
- offrire una sezione Impostazioni con:
  - avvio automatico con Windows seguendo le best practice di startup su Windows
  - avvio in background che mostra solo l'icona nel tray
  - intervallo di aggiornamento della RAM totale in secondi
- aprire la finestra principale con doppio click sull'icona tray
- aprire un menu contestuale con click destro sull'icona tray per aprire la finestra principale o uscire definitivamente

## Vincoli non funzionali
1. Usare la minor quantità possibile di RAM e CPU.
2. Integrare la presenza dell'app nel tray di Windows.
3. Non usare tecnologie web.
4. Usare Rust.
5. Preferire una UX semplice e robusta invece di animazioni o rendering complessi.

## Scelta tecnica consigliata
Per rispettare i vincoli, la soluzione più adatta è una app Rust nativa basata su API Windows dirette.

### Stack consigliato
- Linguaggio: Rust stabile
- GUI: Win32 API native tramite crate `windows` oppure `windows-sys`
- Tray icon: `Shell_NotifyIconW`
- Finestra principale: Win32 window class + controlli standard Windows
- Lista processi: `ListView` in modalità virtuale oppure controllo equivalente nativo
- Persistenza impostazioni: file locale in `%AppData%` o registry per poche preferenze
- Autostart: supporto primario tramite Startup Task se l'app sarà pacchettizzata, altrimenti chiave `Run` nel registry per installazione classica
- Enumerazione processi: API Windows native
- Lettura memoria totale: `GlobalMemoryStatusEx`
- Lettura memoria per processo: `GetProcessMemoryInfo` o API equivalenti native

### Perché questa scelta
- Nessun runtime web
- Minimo overhead grafico
- Integrazione naturale con tray, finestre e menu Windows
- Controllo fine sul ciclo di vita del processo e sulla quantità di polling

## Architettura generale
L'app va separata in 5 blocchi principali.

### 1. Core system monitor
Responsabile di:
- leggere RAM totale usata
- enumerare i processi attivi
- leggere memoria per singolo processo
- calcolare le percentuali e formattare i dati per la UI
- mantenere il polling al minimo indispensabile

### 2. UI principale
Responsabile di:
- mostrare la tabella dei processi
- mostrare search box
- mostrare ordinamento per nome o RAM
- mostrare impostazioni basilari
- reagire agli eventi di refresh del monitor

### 3. Tray controller
Responsabile di:
- creare l'icona tray
- aggiornare il testo/label con la RAM totale
- gestire doppio click e menu contestuale
- mantenere la presenza dell'app quando la finestra è nascosta

### 4. Settings store
Responsabile di:
- salvare e caricare le preferenze
- gestire avvio automatico
- gestire background mode
- gestire intervallo di refresh

### 5. Lifecycle manager
Responsabile di:
- decidere quando mostrare/nascondere la finestra
- intercettare close/minimize e trasformarli in hide-to-tray
- uscire solo su comando esplicito dal menu tray

## Modello dati minimo
Definire strutture dati semplici e stabili.

### ProcessInfo
Campi suggeriti:
- pid
- nome processo
- memoria usata in byte
- memoria usata in MB o MiB per visualizzazione
- percentuale opzionale rispetto al totale RAM, se utile alla UI

### AppSettings
Campi suggeriti:
- start_on_windows: bool
- start_in_background: bool
- refresh_interval_seconds: u64
- window_start_hidden: bool
- show_tray_tooltip: bool

### SortState
Campi suggeriti:
- sort_column: Name | Memory
- sort_direction: Asc | Desc

## Windows API da usare
Le API devono restare native e leggere.

### Finestra e messaggi
- `RegisterClassExW`
- `CreateWindowExW`
- `ShowWindow`
- `UpdateWindow`
- message loop classico con `GetMessageW` / `TranslateMessage` / `DispatchMessageW`

### Tray icon
- `Shell_NotifyIconW`
- `NOTIFYICONDATAW`
- messaggi custom per click singolo, doppio click e menu contestuale

### Menu contestuale tray
- `CreatePopupMenu`
- `AppendMenuW`
- `TrackPopupMenu`

### Gestione memoria e processi
- `GlobalMemoryStatusEx` per RAM totale
- `CreateToolhelp32Snapshot` + `Process32FirstW` / `Process32NextW` per enumerazione processi, se basta
- `OpenProcess` + `GetProcessMemoryInfo` per memoria per processo
- chiusura immediata degli handle dopo l'uso

### Autostart
Due strategie possibili:
- App installata classica: scrivere una voce in `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
- App pacchettizzata: valutare `StartupTask` e integrazione con packaging Windows, se il progetto evolverà verso MSIX

Per una prima versione semplice e robusta, usare `Run` nel registry ma incapsulare la logica dietro un modulo dedicato, così sarà facile sostituirla con Startup Task in futuro.

## Comportamento richiesto

### All'avvio
1. Caricare le impostazioni.
2. Creare la finestra principale ma non mostrarla se `start_in_background` è attivo.
3. Creare il tray icon.
4. Avviare il primo refresh dati.
5. Attivare il timer di polling con l'intervallo configurato.

### Alla chiusura della finestra
1. Intercettare il close event.
2. Impedire la terminazione del processo.
3. Nascondere la finestra.
4. Lasciare vivo il tray icon.

### Alla minimizzazione
1. Intercettare minimize.
2. Nascondere la finestra.
3. Ridurre la presenza visiva al solo tray icon.

### Doppio click sul tray icon
1. Riportare la finestra principale in primo piano.
2. Ripristinare la finestra se era minimizzata o nascosta.
3. Portare il focus sulla search box o sulla lista, secondo UX scelta.

### Click destro sul tray icon
1. Mostrare menu contestuale.
2. Offrire almeno due azioni: apri finestra, esci.
3. Selezionare esci solo dopo conferma implicita dall'azione dell'utente.

## Interfaccia principale
La finestra principale dovrebbe essere composta da 3 aree.

### Header
- valore RAM totale usata in grande evidenza
- eventuale indicatore di refresh in corso, molto leggero
- pulsante impostazioni

### Area filtri
- search box per filtrare i processi in case-insensitive match
- controlli di sorting
- opzionale: refresh manuale

### Tabella processi
Colonne suggerite:
- nome processo
- pid
- RAM usata
- percentuale sul totale, se utile
- eventuale percorso eseguibile se recuperabile senza impatto eccessivo

Per minimizzare le risorse, usare una tabella virtualizzata o comunque controlli nativi efficienti, evitando di ricreare tutte le righe ad ogni refresh se i dati non cambiano.

## Ricerca e ordinamento

### Ricerca
- normalizzare sia query che nome processo in lowercase
- usare match case-insensitive tipo contains
- applicare il filtro solo lato UI, senza rifare l'enumerazione se non necessario

### Ordinamento
- ordinamento stabile
- per nome: confronto case-insensitive
- per RAM: confronto numerico
- supportare toggle ASC/DESC
- preservare l'ordine consistente quando i valori sono uguali

## Strategia di refresh a basso consumo
Questa è una parte critica.

### Principi
- evitare polling troppo frequente
- usare timer Windows nativi, non loop attivi
- aggiornare la UI solo quando ci sono dati nuovi
- separare refresh totale RAM e refresh lista processi se serve in futuro

### Raccomandazione iniziale
- refresh RAM totale: ogni N secondi configurabili
- refresh processi: stesso intervallo nella prima versione
- se il numero di processi è alto, valutare caching e diff incrementali

### Ottimizzazioni
- riusare buffer dove possibile
- chiudere subito gli handle dei processi
- evitare conversioni stringa ripetute
- evitare logging verboso in produzione

## Persistenza impostazioni
Le impostazioni devono essere memorizzate localmente in modo semplice e affidabile.

### Opzione consigliata
- file JSON o TOML in `%AppData%\ActivityTracker\settings.json`

### Motivi
- facile da debuggare
- non richiede amministratore
- isolato per utente
- semplice migrazione futura

### Alternative
- registry per impostazioni minime
- file binario solo se serve un formato più compatto, ma non è necessario

## Autostart best practice su Windows
Per la prima versione, evitare scorciatoie nel Startup folder come soluzione primaria.

### Approccio consigliato
- usare una preferenza esplicita dell'utente in Impostazioni
- su abilitazione, registrare l'app in `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
- su disabilitazione, rimuovere la voce
- scrivere solo in ambito utente corrente
- non richiedere privilegi elevati
- mantenere il comportamento reversibile e trasparente

### Nota importante
Se il progetto in futuro sarà distribuito come app MSIX, conviene migrare il meccanismo a Startup Task per integrarsi meglio con l'ecosistema Windows moderno. Il codice deve quindi isolare la logica in un modulo dedicato, non sparsa nella UI.

## Struttura consigliata del progetto
Una possibile organizzazione del codice:

- `src/main.rs`
- `src/app.rs`
- `src/ui/`
- `src/ui/window.rs`
- `src/ui/tray.rs`
- `src/ui/menu.rs`
- `src/monitor/`
- `src/monitor/memory.rs`
- `src/monitor/processes.rs`
- `src/settings/`
- `src/settings/store.rs`
- `src/settings/autostart.rs`
- `src/model.rs`
- `src/runtime.rs`
- `src/error.rs`

### Obiettivo di questa separazione
- tenere separate responsabilità UI, monitor e persistenza
- ridurre l'accoppiamento
- rendere testabili le funzioni di parsing, sorting e filtering
- semplificare il lavoro di un LLM che genera il codice

## Librerie Rust consigliate
Usare il minimo indispensabile.

### Possibili crate utili
- `windows` o `windows-sys` per le API Win32
- `serde` e `serde_json` per settings
- `tracing` o logging minimo solo in debug
- eventualmente `thiserror` per errori applicativi

### Evitare
- framework GUI pesanti
- motori web embedded
- dipendenze che portano runtime significativi se non necessarie

## Dettaglio dei passi di implementazione

### Fase 1 - bootstrap progetto
1. Creare un progetto Cargo Rust binario.
2. Configurare il target Windows x86_64.
3. Aggiungere dipendenze minime per Win32 API e serializzazione.
4. Verificare build base su Windows.

### Fase 2 - runtime app e ciclo di vita
1. Implementare il punto di ingresso dell'app.
2. Creare una struttura `AppState` centralizzata.
3. Creare il message loop Windows nativo.
4. Gestire la finestra principale e il tray icon.
5. Intercettare close/minimize per nascondere la finestra.

### Fase 3 - monitor memoria totale
1. Implementare lettura RAM totale con `GlobalMemoryStatusEx`.
2. Convertire i byte in percentuale 0-100.
3. Aggiornare il testo del tray icon con il valore numerico.
4. Aggiornare il valore nella UI principale.

### Fase 4 - monitor processi
1. Enumerare i processi attivi con API native.
2. Aprire ogni processo con permessi minimi necessari.
3. Leggere la memoria per processo.
4. Comporre una lista ordinata di `ProcessInfo`.
5. Chiudere tutti gli handle dopo l'uso.

### Fase 5 - UI lista processi
1. Creare la finestra principale con controlli nativi.
2. Inserire search box e controlli sorting.
3. Visualizzare la lista processi in una tabella efficiente.
4. Aggiornare la vista solo quando cambiano i dati o il filtro.
5. Evitare ridisegni completi inutili.

### Fase 6 - search e sort
1. Implementare filtro case-insensitive sui nomi processo.
2. Implementare ordinamento per nome e RAM.
3. Permettere toggle ASC/DESC.
4. Garantire comportamento stabile e prevedibile.

### Fase 7 - tray behavior
1. Creare l'icona tray all'avvio.
2. Mostrare il valore RAM nel tray, compatibilmente con le limitazioni del testo disponibile.
3. Gestire doppio click per aprire la finestra principale.
4. Gestire click destro con menu contestuale.
5. Implementare uscita definitiva dal menu tray.

### Fase 8 - settings
1. Salvare e caricare le impostazioni da file locale.
2. Implementare toggle avvio automatico.
3. Implementare toggle avvio in background.
4. Implementare intervallo refresh configurabile in secondi.
5. Validare i valori per evitare impostazioni non sensate.

### Fase 9 - performance e robustness
1. Misurare l'impatto di CPU e memoria.
2. Ridurre allocazioni nella scansione processi.
3. Usare timer e non busy loop.
4. Gestire errori di permesso o processi terminati in modo silenzioso e stabile.
5. Evitare crash su processi protetti o non accessibili.

### Fase 10 - testing
1. Testare parsing, filtering e sorting con unit test puri.
2. Testare la serializzazione delle impostazioni.
3. Testare i comportamenti UI principali manualmente su Windows.
4. Verificare tray icon, doppio click, menu contestuale e hide-to-tray.
5. Verificare che l'app non lasci handle aperti.

## Error handling
L'app deve degradare bene in caso di errori.

### Casi da gestire
- processo terminato tra enumerazione e lettura memoria
- accesso negato ad alcuni processi
- tray icon non creato correttamente
- file impostazioni mancante o corrotto
- chiave registry non scrivibile per policy utente

### Comportamento desiderato
- non crashare
- ignorare i processi non leggibili
- mantenere la UI aggiornata per i processi restanti
- notificare l'errore solo se serve davvero all'utente

## Distribuzione futura
Se l'app sarà distribuita a utenti finali, considerare in ordine:
1. build release ottimizzata
2. firma del binario
3. installer classico oppure MSIX
4. integrazione corretta con startup e disinstallazione
5. eventuale telemetria disattivata di default

## Scelte da fissare prima di scrivere codice
Prima di implementare, un LLM o uno sviluppatore deve decidere esplicitamente:
- usare `windows` o `windows-sys`
- usare `ListView` virtuale o altra UI nativa equivalente
- rappresentare la memoria per processo come working set, private bytes o entrambi
- gestire l'autostart tramite `Run` o predisporre subito supporto MSIX
- mostrare solo il numero percentuale nel tray oppure anche in tooltip

## Raccomandazione finale
Per ottenere un tool davvero leggero e ben integrato con Windows, la versione iniziale dovrebbe essere:
- Rust stabile
- Win32 API native
- tray icon con `Shell_NotifyIconW`
- lista processi con controlli nativi Windows
- settings su file locale
- autostart per utente in `HKCU\...\Run`
- refresh configurabile e basato su timer

Questa combinazione massimizza compatibilità, minimizza consumo di risorse e rispetta tutti i vincoli richiesti.
