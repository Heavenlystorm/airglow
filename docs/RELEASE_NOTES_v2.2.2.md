# Airglow 2.2.2 — Clean Lighting Canvas Toggle, Smooth Tab-Out & Single-Instance Hardening

Airglow 2.2.2 refines the Overview lighting canvas toggle into a clean On/Off switch, keeps UI animations smooth when another window is focused, and hardens Windows single-instance launching and window lifecycle handling.

## Improvements & Fixes in 2.2.2
- **Clean Overview Lighting Canvas On/Off Toggle**:
  - Replaced `"Live"` / `"Static"` controls across the Overview tile header, system tray (`Lighting Canvas Animation`), command palette, Customize drawer, and Settings -> Appearance with clean **On / Off** labels.
  - When **Off**, Overview no longer mounts `CaseStageCanvas` and instead renders a lightweight pure-Iced paused lighting preview showing static scene color swatches (or muted dormant pills when lights are off), active effect/state title, enabled zone/LED counts, and an inline **Turn On** button while hardware lighting stays live.
  - Decoupled the toggle completely from other Overview tiles (such as the Fans tile's spinning fan animation) and from the global UI subscription tick rate.
- **Smooth Unfocused / Tab-Out Frame Rate**:
  - Removed `!self.window_focused` subscription throttling so live rig animations stay smooth when Airglow is visible alongside other active windows or games.
- **Single-Instance Desktop, Focus & Window-Close Hardening**:
  - Scoped Windows single-instance mutex and show-window event names by `current_desktop_name()` (`single_instance_object_names`) for non-default desktops so isolated desktops do not collide with the user's primary desktop session.
  - Added `AllowSetForegroundWindow(ASFW_ANY)` when signaling an already-running instance and `window::gain_focus(id)` on `WindowOpened` so bringing Airglow back to the foreground focuses reliably.
  - Tracked `window::close_events()` via `Message::WindowClosed` so `main_window` state clears cleanly when closed externally.

## Downloads
- **`Airglow.exe`** — Standalone portable Windows x64 executable (double-click to run).
- **`Airglow-2.2.2-windows-x64-portable.zip`** — Portable archive with `Airglow.exe`, `airglow.ico`, `README.md`, and `LICENSE`.
- **`SHA256SUMS.txt`** — SHA-256 verification checksums.
