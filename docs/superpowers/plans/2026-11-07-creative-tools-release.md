# Creative tools and 1.14.0 release plan

> Implement with subagent-driven-development and verification-before-completion. The user explicitly approved all three deferred experiments plus GitHub publication and installation; no repeat approval gate is required.

**Goal:** Ship personal customization together with interactive 3D rig viewing, freehand LED masks and cross-effect blending as Airglow 1.14.0, then install it with settings retained.

**Architecture:** Keep the Python/vanilla JavaScript stack and existing v1 effect format. Separate camera presentation from hardware geometry. Add optional bounded painted masks to layers. Compile blends into standard self-contained code effects from validated snapshots.

**Spec:** Approved visual research at `C:/Users/schul/Desktop/Airglow-Visual-Research-2026-11-07/research.html`; user's follow-up explicitly includes the three previously deferred features.

## Constraints and decisions

- Release branch `release/1.14.0` uses the already-existing clean checkout `.worktrees/release-1.13.1`, based on current `origin/main` 62e534d. Preserve the original dirty root checkout and all newer reliability/telemetry fixes.
- No dependency/CDN addition, telemetry, guessed physical case model, or implicit hardware activation. Existing explicit Save/Use/Live actions retain their documented behavior.
- 3D uses real perspective projection, extruded configured geometry, depth ordering, actual incoming LED colors, camera orbit/pan/zoom, keyboard/reset and optional local per-zone display depth. Default 2D remains available. No continuous decorative camera motion or hidden rendering.
- Painted masks use optional `paint: {v:1,points:[[x,y],...]}` on a layer. Coordinates are normalized effect coordinates quantized to1e-4, signed integers bounded to[-100000,100000], max2048 unique points. The mask intersects the existing area mask. Colocated LEDs share selection; moved LEDs require repainting. Brush/erase, numeric selection, clear/invert and undo operate on a preview draft. Existing effect file size limit remains enforced.
- Cross-effect blends snapshot two validated endpoint effects into a standard code effect with numeric blend0..100. Reject nesting, oversized output and unsupported endpoints explicitly. Preserve permissions/provenance and existing worker execution limits. Preview uses existing sandbox and never implicitly applies.
- Maintain accessibility, reduced motion, all12 designs, saved layouts and effect recovery. New data is bounded and failed writes leave prior state intact.

## Tasks and ownership

- [x] Root: port only this session's personalization changes using its saved baseline; preserve newer source and regression tests; run merged checks.
- [x] Rig worker: `rig-3d.js`, `rig-3d.css`, `stage.js` wiring and projection/camera/lifecycle tests.
- [x] Painting worker: `studio-paint.js`, painting CSS, `studio.js` hooks, `layers.js`, layer validation in `fxformat.py`, meaningful runtime/gesture/persistence tests.
- [x] Blend worker: `fxblend.py`, build-only `studioapi.py` route, `studio-blend.js`, CSS and generated-runtime/API tests. Coordinate Studio hooks with painting worker.
- [x] Root: add integration links/tests, isolated browser checks and screenshots at desktop/narrow/custom scale, independent review and fixes, full Python/JS/browser/effect/profile validation.
- [ ] Root: version/changelog, release metadata check, commit only release scope, publish source/tag, confirm successful workflows and published assets.
- [ ] Root: download/hash/check release assets and feed; back up installed settings/effects, update via existing updater, verify exact version and retained state. Record distribution boundaries accurately.

## Review focus

- Unsupported/code/canvas endpoints fail honestly; blending preserves both endpoints and midpoint, canvas alpha and controls without escaping worker restrictions.
- Painting and blending remain coherent through undo/revert, reload/recovery, save failure, effect limits and navigation while requests resolve.
- Painting cannot bypass live-preview gating through later edits/autosave; malformed imported masks are rejected before replacing a saved effect.
- Camera/pointer coordinates remain correct under interface zoom; large rigs stay bounded and hidden viewers do no unnecessary work.
- Publication must retain the latest1.13.1 fixes, include all new browser assets and preserve installed settings/effects.

## Evidence

- Latest stable verified via GitHub release metadata: v1.13.1, published2026-11-05; baseline724 Python tests passed before porting.
- Browser/native preview fixtures must retain the1.13.1 fix disabling both native preview hosts.
- Primary rendering reference: https://developer.mozilla.org/en-US/docs/Web/API/WebGL_API/WebGL_best_practices (resource lifecycle); CPU projection chosen to avoid additional WebGL resource/context requirements.
