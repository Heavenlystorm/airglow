# Living Rig and Personal Customization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement the approved research as a distinctive rig-centered interface, personal appearance and workspaces, and reversible Studio exploration.

**Architecture:** Preserve the current Python/vanilla JavaScript/Canvas/SVG stack and all existing themes. Extend bounded settings and Overview documents; isolate pure Studio variation logic from hardware. Integrate modules through existing load/save flows.

**Tech Stack:** Python unittest, JavaScript ES modules, CSS, existing Playwright fixture tools.

**Spec:** `C:/Users/schul/Desktop/Airglow-Visual-Research-2026-11-07/research.html`, approved for implementation by the user in this conversation.

## Global constraints

- Preserve the heavily modified working tree; no reset, checkout, broad commit, install, release or hardware changes. Baseline copied to `output/living-rig-implementation/baseline/`.
- Existing lighting, cooling ownership, reduced motion, high contrast and persisted tile sizing remain functional.
- Appearance accents never recolor LED frames or overwrite warning colors. Presentation work stops when hidden.
- This implementation includes personal looks/workspaces, rig presentation, scene browsing and Studio variations. Full 3D, freehand LED masks and cross-effect blending remain later experiments.
- No framework or dependency replacement. Use fixture processes only for browser verification.

## Interfaces

- Settings `appearance`: `{accent: null | '#rrggbb', density: 'comfortable'|'compact', scale: 90|100|110|120, rig: 'frame'|'minimal', motion: 'system'|'reduced'}`. Defaults are null/comfortable/100/frame/system. All values validated.
- Settings `looks`: up to 12 `{id,name,design,appearance}` records. IDs unique lowercase names up to 32 chars; labels 1–40 chars. Saved looks survive Reset settings.
- Overview retains `tiles,pins` and optionally adds `workspaces` (max 8 `{id,name,tiles,pins}`) and `active_workspace` (null or an existing workspace ID). Old payloads remain accepted and retain existing saved workspaces. Active workspace snapshots follow explicit edits. Reset only resets the current layout, retaining named workspaces.
- `applyAppearance(settings.appearance)` runs at bootstrap and confirmed settings updates, and temporarily during preview. Appearance dialog Cancel restores its baseline; Keep saves through existing settings route.
- Root owns Python validation/store changes and HTML/bootstrap integration. Appearance worker owns design/settings UI modules; Overview worker owns overview/stage modules; Studio worker owns studio modules. Each adds its own focused CSS and tests. No shared CSS/index edits by workers.

## Review focus

- Malformed/imported preferences cannot damage hardware settings or discard the old layout: test bounds, roundtrip and failed writes.
- Quick successive edits/switches cannot let stale saves win: exercise serialized saves and navigation.
- Existing saved layouts and all twelve designs still render, including no-zone and offline cases.
- Keyboard/numeric alternatives remain available, including scene playback and appearance preview cancel.
- Added motion responds to reduced-motion settings; images are stills except for the active scene preview.

## Tasks

### 1. Preference persistence (root)
- [x] Add failing tests in `tests/test_personalization.py` for appearance/looks validation, migration from old Overview, workspace roundtrip, reset preservation and failed-write rollback.
- [x] Extend `airglow/config.py` with bounded validation and compatible persistence.
- [x] Run `python -m unittest tests.test_personalization tests.test_config tests.test_overview`.

### 2. Personal appearance (appearance worker)
- [x] Implement previewable appearance editing and named looks in `web/js/appearance.js`, `web/js/settings.js`, `web/js/design.js`, `web/js/design-boot.js`, and `web/css/appearance.css`.
- [x] Expose accent, density, readable scale, rig framing and motion with Keep/Cancel/Reset and save/rename/delete look operations; local JSON export/import of appearance recipes.
- [x] Add behavioral validation/transaction tests before implementation. Preserve theme selection behavior and failure handling.

### 3. Living Rig + workspaces (Overview worker)
- [x] Extend `web/js/overview.js` and add helper/CSS for named workspaces, import/export, edit history and reversible switching.
- [x] Improve `web/js/stage.js` presentation and Overview lighting composition using actual LED positions and colors; keep minimal presentation available.
- [x] Test original-layout preservation, workspace edits/switches/undo and malformed imported layouts, then capture all major themes/responsive states in final integration.

### 4. Studio exploration (Studio worker)
- [x] Add bounded named variations of a document's creative parameters, parameter locks, restrained variation generation, A/B comparison and same-document interpolation where meaningful.
- [x] Preserve the existing undo/recovery/save contract; no variation action enables hardware preview. Keep numeric and keyboard controls.
- [x] Improve gallery framing and focus/explicit playback using existing still caching. Add pure behavior tests for locks, restoration, interpolation and bounded persistence.

### 5. Integration + review (root)
- [x] Wire added styles/modules into `web/index.html` and bootstrap without changing existing boot behavior.
- [x] Run Python suite, JavaScript suites and existing isolated browser checks; add end-to-end coverage for the new flows.
- [x] Inspect screenshots at desktop/narrow widths and representative themes. Fix visual regressions.
- [x] Obtain an independent review, fix material findings and record verification. Report source changes separately from installed behavior.

## Execution decisions

User's “implement” authorizes execution of the research direction; no repeat approval gate. Work proceeds on the existing `release/1.11.1` checkout because the approved current source includes substantial uncommitted work that a clean worktree would omit. File ownership and a baseline copy protect that state. Commits are deferred to avoid mixing unrelated existing changes.
