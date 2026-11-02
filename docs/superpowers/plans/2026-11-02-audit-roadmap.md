# Airglow audit roadmap Implementation Plan

> **For agentic workers:** Use focused task briefs and the dispatching-parallel-agents workflow for disjoint file ownership, then independent review. Steps use checkbox syntax for tracking. Preserve the existing dirty checkout; do not commit.

**Goal:** Implement every engineering/UX recommendation and four functional product MVPs from the approved audit.

**Architecture:** Existing Python controller and native ES modules, with domain-specific routes and bounded local services. Independent workers own runtime tools, profiles/migration/persistence, and Studio; the coordinator owns shared UI, cooling controls, CI and integration.

**Tech Stack:** Python 3.12, unittest, native JavaScript ES modules, Node tests, existing Playwright fixture, Windows/WebView2.

**Spec:** `docs/superpowers/specs/2026-11-02-audit-roadmap-design.md`

## Global Constraints

- Preserve the previous working tree and all ten audit fix groups; no commit/push/release or installed-app change.
- Keep seven main views, twelve themes, saved layout semantics, existing controls and keyboard shortcuts.
- Use local bounded storage; no account, new remote service, paid dependency or telemetry.
- Never change fan ownership, speeds or thermal limits merely by recording, previewing or importing.
- Existing JSON request limit is 256 KiB; each Studio effect is at most 32 KB.
- Coordinate changes through the file ownership below. Do not edit another worker's files.

## Review Focus

- Failed persistence must preserve visible state and the previous durable data (Tasks 1–3, injected write failures).
- Devices can disconnect/reconnect during pending output; successful status cannot describe retired writers (Task 1, event-controlled tests).
- Import payloads may be malformed, oversized, duplicated or reference missing devices/effects (Tasks 2–3, validate/preview/apply tests).
- A late save or rerender cannot erase a newer draft or keyboard focus (Tasks 3–4, delayed promises and browser keyboard tests).
- The tools must work on a narrow screen with no telemetry, no profiles or a restarted app (all tasks, fixture/browser tests).

## Task 1: Runtime status and cooling workbench

**Own:** `airglow/controller.py`, `airglow/sensors.py`, `airglow/lighting/engine.py`, new `airglow/controlstatus.py`, `airglow/cooling.py`, `airglow/operationsapi.py`, new `web/js/control-center.js`, `web/js/cooling-workbench.js`, `web/css/workbench.css`, focused tests and `tests/test_controller.py`, `tests/test_engine.py`, `tests/test_sensors.py`.

**Interfaces:** `operationsapi.route(server, method, path, body, query=None)` handles `/api/control/*` and `/api/cooling/*`, importing ApiError lazily if needed. Add compact `control` to Controller.snapshot. UI exports `openControlCenter()` and `openCoolingWorkbench()`; coordinator wires entry buttons and optional tile. Workbench CSS is included by coordinator. Use shared `fileio.atomic_write_text(path, text)` once Task 2 lands, or write a small adapter pending it.

- [x] Add failing runtime success/age/ownership, writer retirement, failed controller mutation and bounded recorder tests.
- [x] Implement status/timeline, successful-work timestamps and safe runtime mutation publishing.
- [x] Implement opt-in 1,800-sample/12-session recorder, comparison, draft replay and JSON/CSV export.
- [x] Build functional control-center and cooling UI with failure/empty/loading states and scoped styling.
- [x] Run focused Python/JS tests; report interfaces, test evidence and rendering hooks.

## Task 2: Profiles, migration and persistence

**Own:** `airglow/config.py`, `airglow/api.py`, `airglow/fxlibrary.py`, new `airglow/fileio.py`, `airglow/profileapi.py`, `airglow/rigbundle.py`, `airglow/rigapi.py`, `web/js/profiles.js`, `web/js/settings.js`, new `web/js/rig-migration.js`, `web/css/migration.css`, focused tests and `tests/test_api.py`, `tests/test_config.py`, `tests/test_studio.py`.

**Interfaces:** Dispatch `/api/control/*` and `/api/cooling/*` to Task 1 operationsapi (call-time import); preserve Studio dispatch. Extract profile route family into profileapi. Dispatch `/api/rig/*` to rigapi. Add `control` to config TILE_TYPES for the optional Overview tile. Shared utility is `fileio.atomic_write_text(path, text)`; callers serialize JSON themselves and retain protected-file checks. Coordinator handles index CSS includes.

- [x] Add failing stable-ID profile update and atomic write/rollback tests.
- [x] Implement update-current/save-new with scope UI and retained editing target.
- [x] Extract profile route family and shared file-write utility without breaking existing endpoints/tests.
- [x] Add validated bounded versioned rig export, import preview/manual mapping/apply and tests on a second simulated rig.
- [x] Add Settings migration controls and accessible preview/mapping UI; verify failure preservation and fan ownership exclusions.
- [x] Run focused suites; report contracts and evidence.

## Task 3: Studio creator workflow

**Own:** `web/js/studio.js`, new `web/js/studio-document.js`, `web/js/studio-gallery.js` and other focused Studio modules, `web/css/studio-workflow.css`, `airglow/studioapi.py`, new `airglow/scenepacks.py`, focused new tests (do not modify Task 2 test_studio.py).

**Interfaces:** Keep existing Studio public exports and lazy loading. Pack routes live under `/api/studio/packs/*`. Use existing fxlibrary public methods; do not edit fxlibrary.py. Coordinator includes CSS. Root-owned core may gain mutation events but existing exports remain compatible.

- [x] Add failing state/preview cleanup, document/history, palette and scene-pack validation tests.
- [x] Split document/history/persistence and gallery/publishing responsibilities into focused modules.
- [x] Implement persistent preview/live/applied states, guided remix, favorite palettes and progressive metadata/control exposure.
- [x] Implement validated portable scene packs with preview/conflicts and explicit import, no automatic hardware application.
- [x] Fix focus and narrow editor/menu behavior; run focused tests and report browser flows to verify.

## Task 4: Shared UX, fan controls and CI (coordinator)

**Own:** `web/js/core.js`, `web/js/main.js`, `web/js/fans.js`, `web/js/automations.js`, `web/js/overview.js`, `web/index.html`, existing CSS, `.github/workflows/tests.yml`, `tools/dev_server.py`, browser/JS tests not owned above.

- [x] Add failing tests for resource-serialized mutations, persistent error recovery and obsolete retry suppression.
- [x] Implement shared request state/queue and visible persistent status/recovery near the active surface.
- [x] Replace fan bulk-copy with explicit targets, pump opt-in and honest partial results; add curve point keyboard action.
- [x] Preserve automation focus; add compact narrow telemetry/navigation and remove redundant eyebrows.
- [x] Wire control center, optional status tile, cooling, Studio and migration into existing views.
- [x] Consolidate common CSS tokens and avoid new theme overrides.
- [x] Add isolated fixture runner and Windows browser CI with artifacts.

## Task 5: Integration, review and delivery

- [x] Run all Python/JS tests, import/module checks, effect/profile validators and whitespace checks.
- [x] Run visual/responsive and new end-to-end workflows against an isolated fixture; inspect wide/narrow results and contrasting themes.
- [x] Obtain independent source review of the actual baseline-to-current patch; fix findings and rerun affected checks.
- [x] Add a recommendation-by-recommendation implementation ledger and honest physical-hardware limits to the audit.
- [x] Stop owned fixture processes and deliver the implemented features, evidence and any remaining external verification.
