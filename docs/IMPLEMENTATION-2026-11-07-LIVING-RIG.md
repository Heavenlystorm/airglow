# Living Rig and personal customization

This records the first implementation stage. Its verification predates the integrated 1.14.0 release candidate;
the previously deferred experiments are now included. See [the release implementation report](IMPLEMENTATION-1.14.0.md)
for the combined scope and validation.

## What is available

- **Settings → Appearance → Customize:** preview a design, interface accent, comfortable/compact density, 90–120% interface size, framed/minimal rig and reduced motion. Keep commits the draft; Cancel/Escape restores the previous appearance. Save up to 12 personal looks, rename/delete them, and import/export JSON appearance recipes. Existing theme choices remain available.
- **Overview → Workspaces:** preserve the existing layout as My workspace; create up to eight named layouts from the current arrangement, Living Rig or Readings starters. Switch, rename, delete, import/export, and undo/redo layout edits. Viewport changes do not write layouts.
- **Living Rig:** a neutral frame and component geometry around the configured LED layout. Rendering retains the incoming LED colors. Sparse single-LED zones remain compact; a minimal presentation is available.
- **Studio → Layers → Variations:** eight named master-control captures per effect, Hold controls, Vary unlocked, A/B capture/recall and a numeric/keyboard blend control. Blend and recall are undoable. Variations stay in local storage on this PC; A/B captures last for the editing session. Saving an effect keeps the current effect parameters.
- **Studio scene cards:** cached stills with pointer/focus and explicit playback. Escape stops playback; OS and app reduced-motion settings suppress it. Leaving Studio invalidates pending gallery work.

Appearance and workspace recipes contain presentation settings only. Appearance saves do not reapply hardware control. Studio exploration does not enable live hardware preview or apply an effect; the existing explicit controls remain responsible for those actions.

## Verification

| Check | Result | Evidence in `output/living-rig-implementation/` |
| --- | --- | --- |
| Full Python suite | 725 passed | `python-full.log` |
| Full JavaScript regression command | 151 passed | `javascript-full.log` |
| Browser module parsing/import resolution | 38 modules passed | `tools/check-js.mjs` |
| Existing visual/theme checks | 135 passed; zero browser exceptions | `visual.log`, `browser/screenshots/` |
| Responsive layout workflow | Passed | `responsive.log` |
| Existing browser workflows | 11 passed | `roadmap.log` |
| New personalization browser workflows | 6 passed | `personalization-browser.log`, `browser/personalization/` |
| Full fixture process restart | Appearance, saved look and active workspace retained | `restart-persistence.log` |
| Whitespace check | Passed | `git diff --check` |

Browser coverage includes failed appearance saves/retry, preview cancellation, saved-look rename/delete/import/export, narrow dialogs at 90/120%, all twelve designs, workspace switch/undo/reload/import, Studio locks/blend/recall/undo, explicit and keyboard scene playback, and both motion preferences. Representative fan-rig captures explicitly use simulated browser data; they are not physical-hardware evidence.

Independent review found scale-coordinate errors in tab/drag/resize/FLIP behavior and incomplete cancellation of delayed gallery responses. Both were fixed and covered by regression tests. Browser testing additionally caught unsupported focus-event property assignment; stable event listeners now handle keyboard preview.

The configured design check reports only two pre-existing advisory patterns (theme stripes and grid backgrounds). Existing theme glows remain intentionally retained under the existing file-scoped exception for `web/index.html`. No new CSS exception was added. The JavaScript suite still emits an existing non-failing effect-runtime test-mock `setPoints` diagnostic; all 151 assertions pass.

The completion hook subsequently ran from the Desktop workspace and missed the repository's scoped exceptions. Review confirmed that the three marquee findings are unchanged lighting-effect thumbnails, the pulse illustrates hardware identification, the radial halo belongs to a lighting thumbnail, and wider tracking belongs to short labels or brand typography rather than paragraphs. The existing exceptions were mirrored through `impeccable hooks ignore-value` in `C:/Users/schul/Desktop/.impeccable/config.json`, restricted to `airglow/web/index.html`. Rechecking from that same root leaves only the two pre-existing advisories. No application code changed during this triage.

## First-stage delivery boundary

At this stage, no commit, package, installation or publication had been performed. The original dirty checkout was preserved; a pre-change copy is under `output/living-rig-implementation/baseline/`. New regression files and the personalization browser suite were included in CI commands. All owned demo processes were stopped and ports 8790/8791 were confirmed closed.

Full 3D rendering, freehand LED masks and cross-effect blending were deferred at this stage and subsequently
included in 1.14.0. Physical lights, fans and packaged WebView2 behavior were not exercised in these first-stage checks.
