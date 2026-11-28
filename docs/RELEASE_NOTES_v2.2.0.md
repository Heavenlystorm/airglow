# Airglow 2.2.0 — Low-GPU Caching, Tray Polish, Single-Instance & Quality-of-Life

Airglow 2.2.0 builds on the native Rust + Iced (`wgpu`) architecture with major GPU efficiency improvements, rock-solid system tray lifecycle behavior, cleaner Overview telemetry tiles, and workflow upgrades across Windows.

## What's New in 2.2.0

### Low-GPU Efficiency & Cleaner Overview Tiles
- **Overview Static Lighting Canvas Option**: Toggle the Overview **Lighting canvas** between **Live** and **Static (low GPU)** directly from the tile header, Control Center, Command Palette (`Ctrl+K`), or Settings -> Appearance. Static mode stops airflow/fan/bloom redraws, caches the LED stage layer, and throttles Overview refreshes to 4 Hz.
- **Halo-Free Overview Telemetry Tiles**: Removed concentric radial washes and neon stroke halos from Overview cards and charts (`SparklineCanvas`, `RingGaugeCanvas`, `PowerGraphCanvas`, `BarMeterCanvas`, `MiniFanCurveCanvas`) for a crisper, flatter instrument look.
- **Widget Geometry Caching (`canvas::Cache`)**: Added smart cache invalidation to `UiIconCanvas`, `RingGaugeCanvas`, `SparklineCanvas`, `BarMeterCanvas`, and `MiniFanCurveCanvas` so static vector paths are not re-tessellated every frame.
- **Tab-Aware & Focus-Aware Frame Throttling**: Automatically reduces UI tick rate to 4–5 Hz when Airglow is unfocused (e.g., while gaming) or on static tabs (`Automations`, `Devices`, `Settings`).

### System Tray Lifecycle, Single-Instance & Persistence
- **Persisted Minimize-to-Tray on Close & `--background` Startup**: `minimize_to_tray_on_close` now persists cleanly across restarts, tray creation retries automatically if the shell tray initializes late, clicking the tray icon un-minimizes & focuses the window, and `--background` (used by Windows autostart) launches silently into the tray.
- **Single-Instance Windows Guard**: Re-launching Airglow (e.g., from the Desktop shortcut) while minimized to the tray wakes and focuses the running instance instead of spawning a duplicate engine.
- **Richer Tray Context Menu**: Uses live checkmarks (`CheckMenuItem`) for **Lights On**, **Overview Static Canvas (Low GPU)**, **Profiles**, **Effects**, and **Fan Curves**, plus a **Brightness** preset submenu (`100%` / `75%` / `50%` / `25%` / `10%`).
- **Persisted Window Geometry & Active Tab**: Remembers window size, screen coordinates, and the last selected tab across restarts and tray close/reopen cycles.

### System-Wide Shortcuts & Native Backup Picker
- **Global Windows Hotkeys**: Added toggleable system-wide hotkeys (`Ctrl+Alt+Shift+L` toggle lights, `Ctrl+Alt+Shift+P` cycle profiles, `Ctrl+Alt+Shift+A` open/focus Airglow) that work even while minimized to the tray or in-game.
- **Native Windows File Picker for Rig Backups**: Added a **Browse…** button in Settings -> Backup & Diagnostics (`GetOpenFileNameW`) to select `.json` rig backups without typing paths manually.

## Downloads
- **`Airglow.exe`** — Standalone portable Windows x64 executable (double-click to run).
- **`Airglow-2.2.0-windows-x64-portable.zip`** — Portable archive with `Airglow.exe`, `airglow.ico`, `README.md`, and `LICENSE`.
- **`SHA256SUMS.txt`** — SHA-256 verification checksums.
