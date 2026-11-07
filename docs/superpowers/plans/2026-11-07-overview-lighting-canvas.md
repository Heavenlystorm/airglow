# Overview Lighting Canvas Implementation Plan

> **For agentic workers:** Use subagent-driven-development to implement this plan task-by-task.

**Goal:** Make Airglow Overview an expressive lighting canvas with useful, distinct instruments and personal layouts.

**Architecture:** Keep the existing Overview orchestration and persistence. Add focused rendering modules for the rig and profile/fan instruments, and a scoped stylesheet. Extend workspace validation identically on client and server.

**Tech Stack:** Existing Python server, vanilla JavaScript, Canvas/SVG, CSS, Node tests and Playwright fixtures. No new dependencies.

**Spec:** ../specs/2026-11-07-overview-lighting-canvas.md

## Global Constraints

- Preserve existing saved layouts, theme choices, sensor semantics and actual LED colors.
- Browser mutations use isolated fixture 8790/8791; no installed app edits.
- Resize never writes saved state. Explicit controls are the only hardware mutations.
- New presentation fields are validated on server and client, atomically.
- No idle decorative animation; respect motion preferences and hidden views.

## Review Focus

- Invalid appearance/workspace imports cannot partially modify state.
- Zero readings differ from missing/stale data, and gaps never connect across outages.
- Rig removal, view switching and hidden pages dispose or suspend rendering and pending work.
- CSS zoom and narrow resizing preserve desktop spans and usable controls.
- Theme accent customization does not obscure alerts or alter real light colors.

### Task 1: Persist presentation and workspace starters

Files: airglow/config.py, web/js/overview-workspaces.js, tests/test_overview.py, tests/test_overview_workspaces.mjs.
Produces the spec's optional appearance/view/auto schema and starterWorkspace(doc, kind), accepting showcase/monitoring/balanced plus legacy lighting/current.
- [x] Test new fields, invalid data, old-document shape, roundtrips and non-mutating starters.
- [x] Implement mirrored validation and useful starter arrangements using available sensor IDs.
- [x] Run focused Python/Node tests and report exact results.

### Task 2: Live Overview rig

Files: web/js/overview-rig.js, web/js/rig-3d.js if required, web/css/overview-rig.css, tests/test_overview_rig.mjs.
Produces createOverviewRig(host, {onSelect, onViewChange, isVisible}) => {update({zones,frames,view}),dispose()}. It owns its canvas/view/zone controls inside host. It may import existing stage renderer and lazy-load rig-3d; no hardware API calls. Parent sets view from persisted tile and onViewChange saves explicit changes.
- [x] Test fit/lifecycle/state and exact frame forwarding; support sparse and empty rigs.
- [x] Implement attractive framed 2D/3D rig with keyboard zone selection and local view controls.
- [x] Run module tests; root integrates and browser-verifies.

### Task 3: Profile and fan instruments

Files: web/js/overview-instruments.js, web/css/overview-instruments.css, tests/test_overview_instruments.mjs.
Produces profileArtwork(preset) => safe HTML/SVG of actual saved colors/pattern; fanInstrument(fans,curves,{stale}) => safe HTML/SVG of actual curve/current point. No hardware calls or autonomous animation. Root retains existing profile click handlers and fan controls.
- [x] Test real colors/pattern distinctions, escaping, missing/stale/zero fan states and unavailable curves.
- [x] Build thumbnail artwork and fan graph with units and honest data labeling.
- [x] Run focused Node tests.

### Task 4: Compose the canvas and editor

Files: web/js/overview.js, web/js/overview-sensors.js, web/js/readings.js, web/js/main.js, web/js/lighting.js, web/css/overview-canvas.css, web/index.html, tools/test-overview.mjs, tests/test_overview_sensors.mjs.
Consumes Tasks 1–3. Sensor module provides auto presentation using existing readings; explicit legacy styles remain. Integrates all-tile appearance sheet, optional starters, snap feedback and lifecycle.
- [x] Test adaptive instrument selection, missing/zero/history/threshold behavior.
- [x] Implement size-aware composition and presentation controls, preserving existing drag/history/save behavior.
- [x] Integrate rig/profile/fan renderers and scoped styles; make Darkroom data surfaces calm.
- [x] Run affected unit, persistence and responsive tests.

### Task 5: Browser verification and review

Files: tools/test-overview-canvas.mjs, tools/run-ui-checks.py, .github/workflows/tests.yml as required; output/overview-lighting-canvas (untracked evidence only).
- [x] Exercise new behavior in isolated fixture, including all starters, persistence, undo/import, 2D/3D/zone navigation, theme accents and offline states.
- [x] Inspect screenshots at desktop, large, narrow and 120% scale across dark/light themes; fix visible issues.
- [x] Run appropriate regression suites and independent code/design review; resolve material findings.
- [x] Report verified implementation and any limits without claiming a publication/install.
