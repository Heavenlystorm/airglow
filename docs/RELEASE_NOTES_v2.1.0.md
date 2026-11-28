# Airglow 2.1.0 — Auto-HiDPI Scaling, Tactile UI, Live 2D Bento Drag & Auto-Updater

Airglow 2.1.0 is a major visual, ergonomic, and functional upgrade on the native Rust + Iced (`wgpu`) architecture — delivering crisp HiDPI scaling, live pointer drag-and-drop dashboard customization, tactile hardware-milled controls, full-viewport widescreen layouts across all 7 tabs, and a native SHA-256 verified GitHub Auto-Updater.

## What's New in 2.1.0

### Auto-HiDPI Scaling, Tactile Chrome & Living Backdrop
- **Smart Auto-HiDPI Scaling**: Automatically scales crisp UI elements at `1.25×` on 1440p/ultrawide displays (`2048×1152` logical canvas at 2560×1440), `1.50×` on 4K displays, and `1.0×` at 1440×900 — plus manual **`Auto / 100% / 115% / 125% / 140%`** presets in the header & Settings and **`Ctrl +` / `Ctrl -` / `Ctrl 0`** zoom shortcuts.
- **Centered Frosted Navigation Dock**: Top header dock with geometric vector icons (`UiIcon`) on all 7 tabs, active profile badge, and quick tactile controls (`Ctrl+K`, pinned CPU/GPU readouts, Scale switcher, Fullscreen, Mini-Mode, and Power).
- **Milled-Hardware Tactile Controls & Living Backdrop**: Vertical `Linear` gradients, top-lit bevel highlights, physical press states, recessed segmented pill tracks, diagonal surface plates, and a cached full-window atmospheric **`WindowBackdropCanvas`**.

### Native 60 FPS 2D Bento Overview & Tile Workshop
- **Live Mouse Drag-and-Drop & Corner-Drag Resize**: Grab any Overview tile's header (or anywhere in **`✦ Customize`** mode) to float it with a live snap-slot guide and automatic 2D reflow, or drag bottom-right corner grips to resize tiles smoothly (`1×1` up to `4×3`).
- **Workspace Switcher & 3-Tab Tile Catalog**: Create, switch, rename, and reset workspaces (including starter templates), add Controls (incl. **Desk Display** & **Lighting Lookbook**), built-in Sensors, or pin any live LibreHardwareMonitor sensor onto Overview.
- **Rich Zero-Dead-Space Instruments**: 28-tick precision CPU/GPU tachometers, live **`MiniFanCurveCanvas`** inside Case Fans, 24-segment glowing RAM matrix, and full-bleed sparklines.

### Full-Viewport Widescreen Overhaul Across All 7 Tabs
- **Lighting Atelier**: Clickable Zone Filter pill bar, collision-free de-duplicated rig labels with leader lines, all 16 live-animated effects + 10 Colour Harmonies + Saved Studio Looks visible together, 6 visual Quick Profile swatch cards, and 164px jewel-halo HSV Color Wheel.
- **Studio, Fans, Automations, Devices & Settings**: Full-height 3-column Studio workbench, Fans Cooling Cockpit with thermal regime bands + live crosshair pill + 7 illuminated fan bars, tinted Smart Rules Studio + Live Engine Evaluator, 4-card Devices hardware deck + searchable 222-sensor explorer, and 2-pane Settings.
- **Fullscreen (`F11`) & Theater Rig Mode (`Shift+F11`)**: Full-screen bento responsiveness plus a dedicated full-viewport RGB Theater stage with floating telemetry HUD pills.

### Built-In GitHub Auto-Updater & Rig Backup
- **SHA-256 Verified Auto-Updater**: Checks `Heavenlystorm/airglow` releases (with **`Stable` | `Beta`** channel selection and optional automatic checks), verifies `SHA256SUMS.txt`, and supports **1-click Download & Restart-to-Update**.
- **Rig Backup Export / Import**: Export and restore complete rig configurations cleanly as JSON bundles.

## Downloads
- **`Airglow.exe`** — Standalone portable Windows x64 executable (double-click to run).
- **`Airglow-2.1.0-windows-x64-portable.zip`** — Portable archive with `Airglow.exe`, `airglow.ico`, `README.md`, and `LICENSE`.
- **`SHA256SUMS.txt`** — SHA-256 verification checksums.
