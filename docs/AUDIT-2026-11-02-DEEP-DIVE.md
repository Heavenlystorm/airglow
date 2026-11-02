# Airglow code and product review

Airglow is a Windows control app for lighting, cooling and telemetry, with an unusually capable effect editor. Its strongest foundation is the separation between hardware discovery, rendering, per-device writes and effect execution. Its recurring weakness is state crossing those boundaries: a saved setting, a pending request, a displayed value and a physical output do not always mean the same thing.

**Implementation update, 2 November 2026:** the user subsequently approved all recommendations and all four product MVPs. They are now implemented in the local source. See the [implementation and coverage report](IMPLEMENTATION-2026-11-02.md) for current behavior, 691 Python / 88 JavaScript tests, browser evidence and remaining physical-hardware limits. The audit below preserves the original findings and recommendations.

This review fixed ten defect groups in the local checkout. The recommended next investment is reliable, understandable control: make ownership, pending changes, recovery and profile scope obvious. Keep the live-tile Overview and existing visual choices.

The review covers the working tree on 2 November 2026, based on `c5d724b` / `release/1.11.1`, including the changes already present when this task began. Those earlier audit and analytics changes were preserved and are not counted as new fixes. The assumed workload is one Windows PC with several lighting controllers, optional helper programs and one main UI. This is not a claim that every supported physical device was tested.

## Architecture

```text
run.py
  airglow/app.py                  startup, process lifecycle and shutdown
    config.Store                 settings, profiles, automations, overview, current selection
    Controller                   manual state + automations + power events + fan policy
      LhmPoller/SystemSensors    telemetry collection
      DeviceManager              discovery and reconnection
      Engine                     lighting frames
        DeviceWriter per device  bounded pending frames; slow I/O isolated
        Studio Host              separate process for custom JavaScript effects
    API Server                   loopback HTTP, JSON commands, SSE state and frames
    Window / Tray                native WebView2 shell and tray controls
      web/js/main.js             routing, state refresh, status and notifications
        Overview / Lighting / Studio / Fans / Automations / Devices / Settings
```

| Area | Main source | Responsibility and important boundary |
|---|---|---|
| Startup and Windows integration | [app.py](../airglow/app.py), [window.py](../airglow/window.py), [tray.py](../airglow/tray.py), [system.py](../airglow/system.py), [install.py](../airglow/install.py) | Creates the store, controller, server, updater and UI; manages single-instance behavior, power events, installation hooks and shutdown. Closing the window and quitting the controller are distinct actions. |
| Persistence | [config.py](../airglow/config.py), [profiles.py](../airglow/profiles.py) | Validation, migrations and JSON persistence. Store locking and atomic replacement are useful; compound operations must keep their entire read/validate/write sequence under that lock. |
| Control policy | [controller.py](../airglow/controller.py), [automation.py](../airglow/automation.py), [fans.py](../airglow/fans.py) | Separates manual/base settings from effective settings. Highest matching automation wins independently for lighting and fans. Power events and thermal rules apply additional constraints. |
| Hardware | [hw.py](../airglow/hw.py), [devices.py](../airglow/devices.py), [plugins](../airglow/plugins/), [openrgb.py](../airglow/openrgb.py), [dynlight.py](../airglow/dynlight.py) | Native HID drivers and protocol clients behind a device/zone interface. Discovery, stable identity, reconnection and actual writes must agree about which device owns a zone. |
| Telemetry and cooling output | [sensors.py](../airglow/sensors.py), [history.py](../airglow/history.py), [lhmfans.py](../airglow/lhmfans.py), [fancontrol.py](../airglow/fancontrol.py) | LHM sensors plus Windows metrics, portable sensor aliases, bounded history, fan-header/pump output and optional external fan-profile integration. Missing readings are meaningful states. |
| Lighting | [lighting/engine.py](../airglow/lighting/engine.py), [geometry.py](../airglow/lighting/geometry.py), [effects.py](../airglow/lighting/effects.py), [inputs.py](../airglow/lighting/inputs.py), [lightsetup.py](../airglow/lightsetup.py) | Converts configured zone geometry, effects and inputs to colors. Preview colors and wire color order are separate. Per-device writers keep slow hardware out of the render loop. |
| Studio | [studioapi.py](../airglow/studioapi.py), [fxformat.py](../airglow/fxformat.py), [fxlibrary.py](../airglow/fxlibrary.py), [lighting/studio.py](../airglow/lighting/studio.py), [effecthost.py](../airglow/effecthost.py), [web/runtime](../web/runtime/) | Validates effects, maintains local/community files, executes previews and controls a separate effect-host process. Draft, saved effect, preview and hardware application are separate states. |
| Local API | [api.py](../airglow/api.py) | Loopback-only server with Host checks, a custom mutation header, bounded JSON requests, static assets, SSE and token-authenticated effect-host traffic. Centralizes many unrelated application commands. |
| UI | [web/js](../web/js/), [web/index.html](../web/index.html) | Native ES modules, shared `S` state and a small event bus; expensive screens load lazily. Screen-specific drafts and request queues coexist with server refreshes. |
| Design system | [app.css](../web/css/app.css), [designs.css](../web/css/designs.css), [surfaces.css](../web/css/surfaces.css), [experience.css](../web/css/experience.css) | Shared controls, twelve themes and successive layout/refinement layers. The current cascade works, but increases the cost of predicting which rule controls a surface. |
| Distribution and optional services | [updates.py](../airglow/updates.py), [tools.py](../airglow/tools.py), [gallery.py](../airglow/gallery.py), [fxgallery.py](../airglow/fxgallery.py), [release workflow](../.github/workflows/release.yml) | Updates, helper installation and community downloads. Failures here should remain isolated from hardware control. |

### Main flows

**Changing a light.** A UI gesture posts a scene or zone change. Validation and persistence run in Python; the controller computes effective state. The engine renders geometry and colors, applies output color order, suppresses unchanged frames and hands writes to the device worker. SSE brings state and preview frames back to the interface. A UI acknowledgment alone does not prove that a physical write succeeded.

**Following a fan curve.** LHM is polled separately; the controller periodically derives temperatures and calculates target duty, thermal overrides and ramp-limited duty. Native hubs and selected LHM channels receive commands. A curve target, the current commanded duty and measured RPM are three different quantities. The repaired editor now uses commanded duty for its “Now” marker.

**Automation.** Rules are reevaluated against process, time, lock, idle and temperature signals. The highest applicable lighting rule and fan rule override manual settings; those settings return when the conditions stop. This is a good foundation for explainable behavior and a recent-changes timeline.

**Creating an effect.** Studio maintains a draft, renders a preview, saves a validated effect file, and can explicitly apply it to hardware. Real-light execution uses a separate WebView2 process and sandboxed workers. Gallery effects are checksum-checked; revocation and rollback are supported. These are valuable boundaries to preserve.

## Bugs fixed

Each group has a regression that demonstrated the defect before its fix. Tests use isolated stores, fake hardware or loopback fixture servers.

| # | Defect and consequence | Fix and evidence |
|---|---|---|
| 1 | A zone switched off stopped receiving frames, leaving its last color on the LEDs. Even shutdown blackout skipped it. | The engine remembers zones it has driven and sends black when they are disabled. Never-enabled peripherals remain untouched. Tests cover disable/re-enable, immediate blackout, throttled asynchronous output, identify expiry and OpenRGB whole-device frames. [engine.py](../airglow/lighting/engine.py), [tests](../tests/test_engine.py) |
| 2 | An interrupted OpenRGB packet left the connected byte stream misaligned; subsequent discovery could repeatedly fail. A partial notification header could remain queued forever. Replies from a different controller could also be accepted. | Discard interrupted/invalid transports and match both packet type and controller index. Preserve the intentional protocol-0 no-reply fallback. Tests include partial header/body, EOF, invalid framing, a fresh connection, stale controller replies and real-socket notification stalls. [openrgb.py](../airglow/openrgb.py), [tests](../tests/test_openrgb.py) |
| 3 | An effect upgrade moved the active file away before writing its replacement. A disk or permission failure removed the effect from the library and made normal rollback inaccessible. | Stage replacement and recovery files first. Restore the active file if backup promotion fails. Writes now flush and sync before replacement. Tests preserve current content, prior rollback, lookup and revision across staging and promotion failures. [fxlibrary.py](../airglow/fxlibrary.py), [tests](../tests/test_studio.py) |
| 4 | Two “Save current as profile” requests could both succeed while one profile vanished from disk. | ID allocation, limit validation and append now share one Store transaction. Tests cover concurrent API saves, duplicate names, the 24-profile boundary and failed writes. [config.py](../airglow/config.py), [api.py](../airglow/api.py) |
| 5 | Diagnostics failed to remove device serials when OpenRGB identifiers contained Unicode characters. | The scrubber now matches the same Unicode alphanumeric identifier alphabet as the device layer. Tests cover accented/CJK device names and serials, and stable replacement across repeated references. [diagnostics.py](../airglow/diagnostics.py), [tests](../tests/test_diagnostics.py) |
| 6 | A truncated update-metadata response could escape error handling and leave the updater permanently “checking.” For beta users, a failed feed also discarded usable updates from the other feed. | Metadata fetching is inside the updater error boundary; HTTP protocol failures fall back to cached metadata. The same updater can retry after failure. Stable and beta feeds are checked independently: an available update can download despite the other feed failing; an incomplete check with no update still reports an error. [updates.py](../airglow/updates.py), [tests](../tests/test_updates.py) |
| 7 | The sensor picker hid legitimate names such as Temperature, Tctl/Tdie and Total Power because a memory-timing expression was case-insensitive. | Limit that expression to its intended capitalization. Real readings remain visible while timing and threshold entries stay filtered. [sensors.py](../airglow/sensors.py), [tests](../tests/test_sensors.py) |
| 8 | The shared slider helper batched edits but allowed overlapping saves; an earlier slow request could finish after a later edit. | Allow one in-flight save and merge pending patches. Tests demonstrate bounded requests, final-edit preservation and continuation after failure. This fixes callers using `coalesce`; it is not a global transaction guarantee for every UI action. [core.js](../web/js/core.js), [tests](../tools/test-ui-state.mjs) |
| 9 | The fan graph labeled an unsaved draft calculation “Now,” also ignoring ramp-limited controller output. | The marker uses current commanded duty, redraws when duty changes at a fixed temperature and disappears when duty is unknown. Editing the draft does not change the live marker. [fans.js](../web/js/fans.js), [tests](../tools/test-ui-state.mjs) |
| 10 | Binary effect-frame requests accepted negative Content-Length, causing an unbounded-until-EOF read instead of rejecting the request. | Reject negative lengths and close the connection. Tests exercise negative, oversized and malformed lengths. This is request hardening; no new browser CSRF bypass was established. [api.py](../airglow/api.py), [tests](../tests/test_api.py) |

## Engineering improvements

1. **Make mutation ownership explicit.** Put compound operations in Store/controller methods, following the new profile append. Standardize how UI modules represent draft, saving, saved and failed states. Finish auditing independent scene/effect/zone mutations that bypass the shared coalescer before introducing a global queue.
2. **Report progress, not only thread existence.** Add timestamps for the last completed controller cycle, sensor sample and device write. Distinguish “disconnected,” “reading is stale,” “command queued” and “output failed.” The lighting engine already has a completed-frame health check worth extending to the other workers.
3. **Keep hardware identity and ownership testable.** Formalize writer retirement, reconnect and off transitions. Add event-driven tests for a device disappearing with queued writes and returning with new descriptors. Keep firmware compatibility claims separate from fake-device correctness.
4. **Consolidate file-write mechanics carefully.** Store and effect-library writes currently maintain separate atomic replacement behavior. A small shared file utility can standardize flushing, cleanup and Windows retry behavior. Multi-file transactions still need explicit handling; ordinary exception rollback is not proof of power-loss atomicity.
5. **Split large modules by responsibility.** Extract Studio document/history/persistence, editor rendering and gallery/publishing into clear modules. Split API route families around existing domains. Keep existing ES modules and Python architecture; this review found no reason to introduce a new frontend framework or database.
6. **Make browser regressions a CI gate.** CI currently checks Python, JavaScript behavior, module imports and content validity, but does not run the current visual/responsive fixture suites. Add a Windows fixture job with browser artifacts. Include narrow Studio editors, focus preservation, failed saves and delayed replies, beyond outer-container overflow.
7. **Reduce CSS override ambiguity incrementally.** Consolidate shared spacing, sizing and control tokens before merging surface rules. Preserve twelve themes, real LED colors, keyboard focus, reduced motion and saved layout geometry. Avoid changing all theme rules in one refactor.

## UI and UX recommendations

The current design is recognizably Airglow: real rig geometry, lighting previews, telemetry and fan curves lead the interface. The “Why” bar, profile trials, sensor aliases and recoverable Studio drafts are strengths. A wholesale visual replacement would discard useful work.

| Priority | Improvement | Concrete user benefit | Scope |
|---|---|---|---|
| First | **Update this profile / Save as new** | Refining Gaming updates the profile its automations already reference, rather than silently creating another ID. Show whether it includes lighting, zones and fans. | Medium |
| First | **Explicit bulk fan/pump targets** | “Use for all” currently includes pumps despite fan-oriented wording. Show the actual targets and make pump inclusion explicit; preserve backend safety policy. | Medium |
| First | **Stable keyboard editing** | Preserve control identity and focus when automation fields rebuild. Add an explicit keyboard-accessible Add curve point action. Source reveals likely focus gaps; targeted interaction tests should precede implementation. | Small–medium |
| Next | **Persistent Studio operating state** | Always say “Preview only,” “Live draft preview” or “Saved effect applied.” Explain Save, Use and Share in those terms. Current empty-library copy overstates live application. | Small–medium |
| Next | **Cooling controls earlier in narrow windows** | At 390px, four telemetry cards occupy most of the first screen and the curve begins near the bottom. Collapse summary readings into a compact strip on the Fans screen. | Small |
| Next | **Visible overflow navigation** | Devices and Settings are initially offscreen in the narrow horizontal tab strip. Add a clear overflow affordance or compact navigation pattern. | Small |
| Next | **Progressive Studio detail** | Put creative controls ahead of publishing metadata. Show control-exposure options when requested; move description/tags nearer Share. | Medium |
| Later | **Persistent recovery messages** | Failed saves and device output failures should remain actionable near the affected control instead of depending only on a disappearing toast. | Medium |

The two independent UI assessments combined current source, thirteen inspected fixture screenshots and the bundled detector. The detector emitted 20 matches, but these are not 20 bugs: effect thumbnails were mistaken for marquees, setup blinking represents real hardware identification, and some grids belong to charts. Three tiny decorative eyebrow lines duplicate useful headings and can be removed in a future copy pass. Current screenshots do not establish motion quality because the fixture uses reduced motion.

## Product directions

These are hypotheses derived from the existing capabilities, not validated market demand or claims that competitors lack them.

**1. A dependable PC control center — recommended first.** Build on the existing “Why” explanation with a local event timeline: which rule took control, which sensor went stale, which device recovered, and the last successful output. MVP: an optional status tile, per-device owner/status and a small recent-events panel with direct recovery actions. Validate by measuring whether testers can explain an unexpected light/fan state and recover without restarting. Risk: noisy status; keep normal operation quiet and make details optional.

**2. A quiet-cooling workbench.** Record an opt-in local session of temperatures, RPM and commanded duty; compare two saved curves under a repeatable workload and preview draft targets without applying them. Reuse bounded sensor history and the curve editor. MVP: record/stop, annotate workload, compare sessions and export a report. Validate repeatable temperatures and fan behavior, not a claimed reduction in noise: RPM alone is not an acoustic measurement. Keep thermal limits and pump ownership explicit.

**3. A lighting instrument for creators.** Develop Studio into a guided remix workflow with clear preview/application states, effect-wide macros, favorite palettes and portable scene packs. MVP: first-remix guidance and a pack containing effects plus area mapping and optional controls. Validate time from opening Studio to a saved usable effect and successful reuse on a different rig. Risk: editor complexity; keep advanced authoring inside Studio and basic Lighting immediate.

**4. A portable rig setup and migration kit.** Export a human-readable setup bundle and guide users through remapping devices when moving PCs or replacing a controller. Preserve intent such as “top fans” while showing unresolved hardware matches. MVP: export/import with a preview of changes and manual mapping; never silently transfer fan ownership. This builds on stable IDs, physical placement, profiles and diagnostics. Validate against a second simulated rig before physical-device trials.

The practical sequence is reliability and ownership visibility, profile editing and bulk-control clarity, then one product experiment. Avoid expanding all four directions simultaneously.

## Known limits and follow-up work

- **Serialless native controllers:** current plugin matching can collapse two serialless NZXT devices of the same product into one. Distinguishing physical devices from multiple interfaces needs representative enumeration captures; blindly treating every HID path as a separate controller could introduce duplicate ownership. Left unchanged and explicitly tracked.
- **Cooling latency:** LHM writes are bounded but sequential. Measure worst-case output latency separately from the nominal control cadence; do not infer physical thermal behavior from unit tests.
- **UI questions still needing interaction evidence:** focus after automation/Studio re-rendering, the Add a layer menu inside scroll containers, full narrow Studio editing, and realistic dense rig-label readability. Recommendations above distinguish these from fixed bugs.
- **Security scope:** source review and regression tests cover local API framing, host checks, diagnostics, effect validation and relevant sandbox code. This was not a fresh browser sandbox penetration test or a release-signing audit.
- **Physical scope:** no installed application patch, fan-speed command, live settings change, real sleep/resume experiment, firmware qualification or clean-PC installer test was performed. No commit, push or release was created.

## Verification and artifacts

- Baseline: **590 Python tests, 57 JavaScript tests passed**.
- Updated suite: **611 Python tests, 60 JavaScript tests passed**.
- Visual fixture: **135 checks passed, zero failures and zero browser errors** across representative views, sizes and themes.
- Responsive Overview regression passed through phone, portrait, ultrawide and 4K, including layout persistence and resizing.
- Browser-module validation checked **27 modules**. Python compilation, **7 effect validations**, **4 profile validations**, and `git diff --check` passed.
- An independent review checked the changes against the saved pre-task source. Its remaining OpenRGB notification case was reproduced with a real socket pair and fixed; the full Python suite passed afterward.
- Testing used a dedicated fixture data folder and process-local app-data path. Screenshots are from the fixture, not the user's running app.

The patch is in the working tree. [New changes only](../output/deep-dive-2026-11-02/new-fixes.diff) separates this work from the changes already present. [Evidence folder](../output/deep-dive-2026-11-02/) contains the baseline snapshot, test logs, detector output and fixture screenshots.
