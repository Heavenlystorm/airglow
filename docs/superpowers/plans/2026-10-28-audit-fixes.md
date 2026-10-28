# Airglow audit fixes implementation plan

**Goal:** Fix the 19 findings approved in this chat, with regression tests and no new dependencies.

**Architecture:** Keep the Python controller, JSON store, browser modules and worker sandbox. Correct their existing failure handling and state ownership; preserve installed data and public interfaces.

**Tech stack:** Python 3.12 unittest, browser JavaScript, Node VM test harnesses, Windows hardware adapters.

**Spec:** The numbered 19-finding audit and the user's instruction to fix all of it in this chat.

## Constraints and verification

- Preserve existing untracked files. Work on the local repair branch; do not publish or drive real hardware.
- Reproduce each defect with a failing regression, implement the smallest fix, and run the focused tests again.
- Reuse fakes only at hardware/network/GUI boundaries; exercise actual production state and persistence logic.
- Test interrupted operations, delayed replies, disconnected devices, restart persistence and invalid boundary values.
- Run the complete Python suite, browser module checker, all Node regression suites, community validators, and a fresh independent code review before completion.

## Work and file ownership

### Hardware and trial safety

Owner: hardware agent. Files: controller.py, hw.py, devices.py, plugins/, lighting/engine.py, and corresponding Python tests. Coordinate any config.py edits with root first.

- [x] 1: Count sensor/fan failures across intervening lightweight ticks; assert failsafe before shutdown.
- [x] 2: Native failsafe preserves higher cooling duty and known thermal emergency.
- [x] 3: Close failed fan-only USB connections so housekeeping reconnects.
- [x] 5: Trial zone overrides remain transient, including crash/reload and disconnect/discard.
- [x] 7: Persist physical-device identity to local ID mapping, preserving old IDs and absent devices.
- [x] 8: Refresh the path of a known USB identity during rescan.
- [x] 13: OpenRGB exclusions preserve the lighting of fan-only NZXT models in mixed rigs.
- [x] 14: Initialize OpenRGB host/port from saved settings.

### Browser save and navigation behavior

Owner: UI agent. Files: web/js/studio.js, fans.js, main.js, lighting.js and new tools/test-*.mjs.

- [x] 4: Serialize Studio saves for a captured document; late replies cannot mutate another document or discard new edits. Reuse one save path.
- [x] 6: Fan save replies update their original curve and matching draft revision only.
- [x] 17: Lighting can open Studio before Studio has been initialized.

### Effect boundaries

Owner: effects agent. Files: fxformat.py, fxlibrary.py, studioapi.py if needed, web/js/fxrun.js, web/runtime/frame.js, tests/test_studio.py, new worker regression tests.

- [x] 10: Reject non-finite numeric controls and prove an imported bad file cannot poison catalogue JSON.
- [x] 11: Apply revocations to rollback copies, including when the current version is allowed.
- [x] 12: Bound worker initialization independently of readiness and frame ticks; clean up timers.
- [x] 16: Unique generated effect IDs stay within the slug limit.

### Setup, profile transaction and small state corrections

Owner: root. Files: tools.py, api.py, config.py if required, automation.py, history.py and their tests.

- [x] 9: Preserve valid LHM credentials through later helper setup failure; cancellation keeps existing credentials.
- [x] 15: Validate all profile/rule changes before writing and restore prior state on a later write failure.
- [x] 18: Reject an explicit empty day selection, retaining the omitted-field default.
- [x] 19: Missing sensors expire after the history window; returning readings reset their absence count.

## Final checks

- [x] Add new Node regression commands to CI/build checks where relevant.
- [x] All focused regressions have demonstrated red then green.
- [x] Full unit suite and JS tests pass, validators pass, imports parse.
- [x] Independent review findings addressed; final diff and worktree checked.

## Progress

- Baseline from audit: 510 Python tests, 4 Node Overview tests, 27 browser modules, 7 effects and 4 profiles passed. No tracked changes before implementation.
- User's request authorizes the audit's concrete fixes. No additional feature/design scope is introduced.

## Verification results

- Final full suite: **541 Python tests passed** in 24.453 seconds, with normal temporary-file and localhost socket access. Hardware, setup elevation and external services are faked by the tests.
- Browser regression suites: **23 passed** (Overview 4, UI state 12, effect runtime 7). CI explicitly runs all three suites.
- Browser module checker: **27 modules passed**. JavaScript syntax checks passed; **52 Python files parsed**, application imports succeeded and **2 device plugins loaded**.
- Catalogue validation: **7 effects** and **4 profiles** passed.
- Independent review found and closed four further edge cases: same-name NZXT revisions, device-identity persistence failure, repeated/reopened editor saves, and profile activation writes failing after installation. Final independent review had no remaining actionable findings.
- Added 31 Python tests and 19 JavaScript tests relative to the audit baseline. Regression coverage includes failures, retries, restarts, disconnections and delayed responses.
- `git diff --check` passed. Changes are local on `codex/fix-audit-findings`; no commit, push, deployment or real hardware operation was performed.

## Verification limits

- Real USB hardware and the packaged WebView2 application were not exercised. Existing untracked creative work was preserved and is outside these application regression suites.
- Physical identity uses serial numbers when available and USB paths otherwise; a serial-less device moved to another path cannot be distinguished reliably from a new device. Existing mappings cannot reconstruct associations lost before this upgrade.
- Profile Keep restores earlier files when a later write fails. This is best-effort rollback, not a crash-atomic multi-file storage format; process termination or a second storage failure during rollback remains a limitation of the existing JSON store.
