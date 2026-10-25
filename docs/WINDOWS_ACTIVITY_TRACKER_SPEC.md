# Windows Activity Tracker - Technical specification and implementation plan

## Goal
Build a native Windows desktop application written in Rust, with low resource
consumption and without using web frameworks like Electron, Tauri or similar.

The app must:
- monitor total RAM usage as a percentage, showing only the number from 0 to 100 without the `%` symbol
- show a main window with per-process RAM details
- allow case-insensitive textual search over active processes
- allow sorting by process name and by RAM used, both ASC and DESC
- show an icon in the taskbar / system tray with the total RAM usage percentage, without the symbol
- hide the main window when the app is minimized or closed, keeping only the tray icon alive
- offer a Settings section with:
  - automatic startup with Windows, following Windows startup best practices
  - background startup that shows only the tray icon
  - total RAM refresh interval in seconds
- open the main window on double-click of the tray icon
- open a context menu on right-click of the tray icon, to open the main window or quit for good

## Non-functional constraints
1. Use the smallest possible amount of RAM and CPU.
2. Integrate the app's presence into the Windows tray.
3. Do not use web technologies.
4. Use Rust.
5. Prefer a simple and robust UX over animations or complex rendering.

## Recommended technical choice
To respect the constraints, the best fit is a native Rust app based on direct
Windows APIs.

### Recommended stack
- Language: stable Rust
- GUI: native Win32 API via the `windows` or `windows-sys` crate
- Tray icon: `Shell_NotifyIconW`
- Main window: Win32 window class + standard Windows controls
- Process list: `ListView` in virtual mode, or an equivalent native control
- Settings persistence: a local file in `%AppData%` or the registry, for a few preferences
- Autostart: primary support via Startup Task if the app will be packaged, otherwise the `Run` registry key for a classic install
- Process enumeration: native Windows APIs
- Total memory read: `GlobalMemoryStatusEx`
- Per-process memory read: `GetProcessMemoryInfo` or equivalent native APIs

### Why this choice
- No web runtime
- Minimal graphic overhead
- Natural integration with Windows tray, windows and menus
- Fine control over the process lifecycle and the amount of polling

## General architecture
The app is split into 5 main blocks.

### 1. Core system monitor
Responsible for:
- reading total RAM used
- enumerating active processes
- reading memory per single process
- computing percentages and formatting data for the UI
- keeping polling at the bare minimum

### 2. Main UI
Responsible for:
- showing the process table
- showing the search box
- showing sorting by name or RAM
- showing basic settings
- reacting to monitor refresh events

### 3. Tray controller
Responsible for:
- creating the tray icon
- updating the text/label with the total RAM
- handling double-click and the context menu
- keeping the app alive while the window is hidden

### 4. Settings store
Responsible for:
- saving and loading preferences
- managing automatic startup
- managing background mode
- managing the refresh interval

### 5. Lifecycle manager
Responsible for:
- deciding when to show/hide the window
- intercepting close/minimize and turning them into hide-to-tray
- quitting only on an explicit command from the tray menu

## Minimal data model
Define simple and stable data structures.

### ProcessInfo
Suggested fields:
- pid
- process name
- memory used in bytes
- memory used in MB or MiB for display
- optional percentage relative to total RAM, if useful for the UI

### AppSettings
Suggested fields:
- start_on_windows: bool
- start_in_background: bool
- refresh_interval_seconds: u64
- window_start_hidden: bool
- show_tray_tooltip: bool

### SortState
Suggested fields:
- sort_column: Name | Memory
- sort_direction: Asc | Desc

## Windows APIs to use
The APIs must stay native and light.

### Window and messages
- `RegisterClassExW`
- `CreateWindowExW`
- `ShowWindow`
- `UpdateWindow`
- classic message loop with `GetMessageW` / `TranslateMessage` / `DispatchMessageW`

### Tray icon
- `Shell_NotifyIconW`
- `NOTIFYICONDATAW`
- custom messages for single click, double click and context menu

### Tray context menu
- `CreatePopupMenu`
- `AppendMenuW`
- `TrackPopupMenu`

### Memory and process management
- `GlobalMemoryStatusEx` for total RAM
- `CreateToolhelp32Snapshot` + `Process32FirstW` / `Process32NextW` for process enumeration, if that is enough
- `OpenProcess` + `GetProcessMemoryInfo` for per-process memory
- closing handles immediately after use

### Autostart
Two possible strategies:
- Classic installed app: write an entry in `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
- Packaged app: evaluate `StartupTask` and integration with Windows packaging, if the project evolves toward MSIX

For a simple and robust first version, use the registry `Run` key but encapsulate the logic behind a dedicated module, so it is easy to replace it with Startup Task in the future.

## Required behavior

### At startup
1. Load the settings.
2. Create the main window but do not show it if `start_in_background` is active.
3. Create the tray icon.
4. Run the first data refresh.
5. Start the polling timer with the configured interval.

### On window close
1. Intercept the close event.
2. Prevent process termination.
3. Hide the window.
4. Keep the tray icon alive.

### On minimization
1. Intercept minimize.
2. Hide the window.
3. Reduce the visual presence to the tray icon only.

### Double-click on the tray icon
1. Bring the main window to the foreground.
2. Restore the window if it was minimized or hidden.
3. Move focus to the search box or the list, according to the chosen UX.

### Right-click on the tray icon
1. Show the context menu.
2. Offer at least two actions: open window, quit.
3. Quit only after the implicit confirmation from the user's action.

## Main interface
The main window should be composed of 3 areas.

### Header
- total RAM used value in large, prominent display
- optional in-progress refresh indicator, very light
- settings button

### Filters area
- search box to filter processes with case-insensitive match
- sorting controls
- optional: manual refresh

### Process table
Suggested columns:
- process name
- pid
- RAM used
- percentage of the total, if useful
- optional executable path if retrievable without excessive impact

To minimize resources, use a virtualized table or, in any case, efficient
native controls, avoiding recreating all the rows on every refresh when the
data did not change.

## Search and sorting

### Search
- normalize both query and process name to lowercase
- use case-insensitive contains-style matching
- apply the filter only on the UI side, without re-running the enumeration if not necessary

### Sorting
- stable sorting
- by name: case-insensitive comparison
- by RAM: numeric comparison
- support ASC/DESC toggle
- preserve a consistent order when values are equal

## Low-consumption refresh strategy
This is a critical part.

### Principles
- avoid overly frequent polling
- use native Windows timers, not active loops
- update the UI only when there is new data
- separate total RAM refresh and process list refresh if needed in the future

### Initial recommendation
- total RAM refresh: every N configurable seconds
- process refresh: same interval in the first version
- if the process count is high, evaluate caching and incremental diffs

### Optimizations
- reuse buffers where possible
- close process handles immediately
- avoid repeated string conversions
- avoid verbose logging in production

## Settings persistence
Settings must be stored locally in a simple and reliable way.

### Recommended option
- a JSON or TOML file in `%AppData%\ActivityTracker\settings.json`

### Reasons
- easy to debug
- does not require administrator
- isolated per user
- simple future migration

### Alternatives
- registry for minimal settings
- binary file only if a more compact format is needed, but it is not necessary

## Windows autostart best practices
For the first version, avoid Startup folder shortcuts as the primary solution.

### Recommended approach
- use an explicit user preference in Settings
- on enable, register the app in `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
- on disable, remove the entry
- write only in the current user scope
- do not require elevated privileges
- keep the behavior reversible and transparent

### Important note
If the project will be distributed in the future as an MSIX app, it is worth
migrating the mechanism to Startup Task to integrate better with the modern
Windows ecosystem. The code must therefore isolate the logic in a dedicated
module, not scattered in the UI.

## Suggested project structure
A possible code organization:

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

### Goal of this separation
- keep UI, monitor and persistence responsibilities separate
- reduce coupling
- make parsing, sorting and filtering functions testable
- simplify the work of an LLM generating the code

## Recommended Rust libraries
Use the minimum necessary.

### Possibly useful crates
- `windows` or `windows-sys` for the Win32 APIs
- `serde` and `serde_json` for settings
- `tracing` or minimal logging, only in debug
- optionally `thiserror` for application errors

### Avoid
- heavy GUI frameworks
- embedded web engines
- dependencies that bring significant runtimes when not needed

## Implementation step details

### Phase 1 - project bootstrap
1. Create a Cargo Rust binary project.
2. Configure the Windows x86_64 target.
3. Add minimal dependencies for Win32 APIs and serialization.
4. Verify a base build on Windows.

### Phase 2 - app runtime and lifecycle
1. Implement the app entry point.
2. Create a centralized `AppState` structure.
3. Create the native Windows message loop.
4. Handle the main window and the tray icon.
5. Intercept close/minimize to hide the window.

### Phase 3 - total memory monitor
1. Implement total RAM reading with `GlobalMemoryStatusEx`.
2. Convert bytes to a 0-100 percentage.
3. Update the tray icon text with the numeric value.
4. Update the value in the main UI.

### Phase 4 - process monitor
1. Enumerate active processes with native APIs.
2. Open each process with the minimum required permissions.
3. Read per-process memory.
4. Compose a sorted list of `ProcessInfo`.
5. Close all handles after use.

### Phase 5 - process list UI
1. Create the main window with native controls.
2. Insert the search box and sorting controls.
3. Display the process list in an efficient table.
4. Update the view only when the data or the filter change.
5. Avoid useless full redraws.

### Phase 6 - search and sort
1. Implement case-insensitive filtering on process names.
2. Implement sorting by name and RAM.
3. Allow ASC/DESC toggle.
4. Guarantee stable and predictable behavior.

### Phase 7 - tray behavior
1. Create the tray icon at startup.
2. Show the RAM value in the tray, within the available text limits.
3. Handle double-click to open the main window.
4. Handle right-click with the context menu.
5. Implement the final quit from the tray menu.

### Phase 8 - settings
1. Save and load settings from a local file.
2. Implement the automatic startup toggle.
3. Implement the background startup toggle.
4. Implement a configurable refresh interval in seconds.
5. Validate values to avoid nonsensical settings.

### Phase 9 - performance and robustness
1. Measure the CPU and memory impact.
2. Reduce allocations in the process scan.
3. Use timers, not busy loops.
4. Handle permission errors or terminated processes silently and stably.
5. Avoid crashes on protected or inaccessible processes.

### Phase 10 - testing
1. Test parsing, filtering and sorting with pure unit tests.
2. Test settings serialization.
3. Test the main UI behaviors manually on Windows.
4. Verify tray icon, double click, context menu and hide-to-tray.
5. Verify that the app leaves no open handles.

## Error handling
The app must degrade gracefully on errors.

### Cases to handle
- process terminated between enumeration and memory read
- access denied to some processes
- tray icon not created correctly
- missing or corrupted settings file
- registry key not writable due to user policy

### Desired behavior
- do not crash
- ignore unreadable processes
- keep the UI updated for the remaining processes
- notify the error only if it really matters to the user

## Future distribution
If the app will be distributed to end users, consider in order:
1. optimized release build
2. binary signing
3. classic installer or MSIX
4. correct integration with startup and uninstall
5. optional telemetry, disabled by default

## Choices to fix before writing code
Before implementing, an LLM or a developer must explicitly decide:
- use `windows` or `windows-sys`
- use a virtual `ListView` or another equivalent native UI
- represent per-process memory as working set, private bytes, or both
- handle autostart via `Run` or immediately prepare MSIX support
- show only the percentage number in the tray, or also a tooltip

## Final recommendation
To get a tool that is truly light and well integrated with Windows, the initial
version should be:
- stable Rust
- native Win32 APIs
- tray icon with `Shell_NotifyIconW`
- process list with native Windows controls
- settings on a local file
- per-user autostart in `HKCU\...\Run`
- configurable, timer-based refresh

This combination maximizes compatibility, minimizes resource consumption and
respects all the required constraints.
