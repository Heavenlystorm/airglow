# Airglow 2.0.0 — Native Rust & GPU Rewrite

Airglow 2.0 is a ground-up native Rust rewrite built with **Iced 0.14** (`wgpu`) and **Tauri's standalone OS tray crates** (`tray-icon` + `muda`) — zero WebView2, zero Python runtime, instant startup, and whisper-light background footprint.

## What's New in 2.0.0

### Speed & Efficiency
- **Standalone ~17 MB Windows x64 executable** — no installer bloat, no WebView2 processes, no console window.
- **~600–700 ms cold startup**, **~0.6–1.1% CPU** while animating 200+ LEDs at 30 FPS, and **~0.23% CPU** minimized to the system tray.
- **Dedicated non-blocking hardware thread** keeps UI animations and live **Studio real-rig previews** smooth at 30–34 FPS.

### Visual Craft & Customisation
- **13 Interface Designs** led by **Firefly** (velvet dusk-plum, bellflower meadow, glowing lantern moth), plus **Graphite**, **Paper** (daylight), **Midnight**, **Kiln**, **Spore**, **Amber CRT**, **ASCII**, **High Contrast**, and more.
- **Seamless Windows 11 Native Titlebar Tinting**: The Windows caption bar, text, and border dynamically match all 13 designs (dark & light).
- **Appearance Studio**: Live **Accent Overrides** (`#HEX` + curated swatches), **Saved Looks** (*Firefly Classic*, *Firefly Pink*, *Midnight Blueprint*, *Kiln Ember* + custom looks), **Rig Stage Styles** (*Living / Architectural / Minimal*), **GPU Power Mode** (`15 / 30 / 60 FPS`) with live app CPU/RAM readout, and **Overview Tile Customizer**.
- **Living PC Case Stage**: Spinning fans scaled to live RPM, toggleable **Thermal Airflow Ribbons**, AIO OLED CPU display, matrix keyboard lighting, hover zone X-Ray tooltips, and snap-to-grid **Arrange** mode.

### Lighting, Studio & Cooling
- **16 Live-Animated Lighting Effects** (including new **Aurora**, **Firefly swarm**, **Meteor shower**, and **Heat map**), **10 One-Click Colour Harmonies**, **350 ms smooth hardware RGB crossfading**, and an interactive jewel-halo **HSV Color Wheel** with Secondary colour & `⇄ Swap`.
- **Studio Multi-Layer Composer**: `Normal` / `Add` / `Screen` / `Multiply` layer blending, 30-step Undo/Redo (`Ctrl+Z` / `Ctrl+Y`), live 24-band audio EQ, and bespoke per-preset thumbnails.
- **Cooling & Telemetry**: Draggable fan curves with configurable **acoustic hysteresis (`0–8 °C`)**, LibreHardwareMonitor fan channel control & 200+ live sensors, interactive graph hover probes, and cooling session recorder.

### Everyday Quality of Life
- **Compact Companion Mini-Mode (`Ctrl+M`)**: Slim `430×404` desktop companion widget with live rig preview, brightness slider, instant profile pills, fan preset selector, Sleep timer, and live wattage footer.
- **Command Palette (`Ctrl+K`) & Keyboard Shortcuts (`?` / `Ctrl+/`)**: Control every tab, profile, effect, design, Saved Look, accent, fan mode, and Sleep timer straight from the keyboard.
- **Automatic Legacy Migration**: Existing Airglow 1.x settings in `%APPDATA%\Airglow` migrate cleanly into `%APPDATA%\Airglow-rs\config.json` on first run.

## Downloads
- **`Airglow.exe`** — Standalone portable Windows x64 executable (double-click to run).
- **`Airglow-2.0.0-windows-x64-portable.zip`** — Portable archive with `Airglow.exe`, `airglow.ico`, `README.md`, and `LICENSE`.
- **`SHA256SUMS.txt`** — SHA-256 verification checksums.
