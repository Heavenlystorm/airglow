# Airglow 2.2.3 — Cached Board & Live Temps When Canvas Animation Is Off

Airglow 2.2.3 refines the **Off** state of the Overview **Lighting canvas animation** toggle so the full rig board and live CPU/GPU temperatures remain visible with zero lighting animation and minimal GPU/CPU work.

## Improvements in 2.2.3
- **Full Rig Board & Live Temps When Animation Is Off**:
  - Added `animate_lighting: bool` to `CaseStageCanvas` and `board_overlay_cache` to `StageDragState`.
  - When **Off** (`animate_lighting == false`), Layer 1 (`static_cache`) keeps the full chassis, motherboard PCB, VRM, AIO housing/hoses, GPU shroud, PSU bulkhead, keyboard, and static 7-blade fans + motor hubs cached.
  - Layer 2 skips ambient RGB wash/spill, PCB trace pulses, coolant glints, airflow ribbons, spinning blades, and volumetric LEDs, instead drawing only the live AIO LCD CPU temperature ring/readout, GPU shroud temperature micro-badge, and hover zone outline/tooltip into `board_overlay_cache` (re-tessellating only on resize, theme/style change, hover movement, or rounded temperature changes).
  - Updated Overview, Customize drawer, Command Palette, and Settings descriptions to reflect that turning canvas animation **Off** keeps the rig board and temperatures visible.

## Downloads
- **`Airglow.exe`** — Standalone portable Windows x64 executable (double-click to run).
- **`Airglow-2.2.3-windows-x64-portable.zip`** — Portable archive with `Airglow.exe`, `airglow.ico`, `README.md`, and `LICENSE`.
- **`SHA256SUMS.txt`** — SHA-256 verification checksums.
