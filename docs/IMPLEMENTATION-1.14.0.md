# Airglow 1.14.0 implementation

The approved personal customization update and all three creative experiments are integrated on the current
1.13.1 source baseline. Existing telemetry, persistence recovery and hardware-control fixes are retained.

## Features

- Lighting offers 2D and 3D views of configured geometry. Orbit, pan, zoom, keyboard controls and Reset adjust
  the view. Zone depth is local presentation data; Arrange returns to the existing 2D editor. The viewer uses
  incoming LED colors, renders on updates and stops while hidden. It is not a reconstruction of a physical case.
- Studio supports freehand brush/erase masks, numeric LED selection, clear/invert/remove, gesture undo and draft
  recovery. Masks follow quantized LED positions and intersect area masks. Browser and native renderers receive
  matching position metadata. Painting and mask history changes suspend automatic saving/live syncing until an
  explicit action. Save retains the existing behavior of updating an effect that may already be applied.
- Saved-effect blends capture two originals into a portable code effect with a percentage control, independent
  preview, swap, recovery, failed-save retry, attribution and permission preservation. Saving creates a new effect.
- Appearance recipes, personal looks, Overview workspaces, Living Rig framing, Studio variations/Hold/A-B and
  accessible scene previews are included. All twelve designs remain available.

## Verification

Checks run against isolated fixture data, never the installed application's settings or hardware endpoints:

- 747 Python unit tests; browser module/import validation; community effect and profile validation.
- 179 JavaScript state/runtime regressions cover history, recovery, coordinate parity, preview ownership, projection,
  stale requests, persistence failures and viewport changes.
- Browser workflows cover telemetry, all themes, responsive layouts, workspaces, personalization, 3D controls and
  fallback, freehand painting/save/reopen, blend recovery/save failure, and narrow dialogs at 120% interface scale.
- Real Edge worker rendering compares blends with original layer, point, canvas-alpha, WebGL-alpha and stateful
  effects at endpoints and intermediate percentages.
- An isolated native WebView2 host verified exact painted-mask colors and their 50% blend using virtual points.
  No physical lighting was changed by this check.
- Independent review found an Undo/Redo path that could bypass the paint guard after Save. The guard now returns
  restored mask edits to preview-only mode; a dedicated regression passes.
- Integrated rendering checks caught a delayed preview event hiding a failed-save message and narrow header
  overflow at 120% zoom once readings arrived. Save errors now persist, and the narrow header retains readings
  on a separate row. Browser fixtures use the production sandbox renderer through a test transport, avoiding
  native WebView profile locks in CI.

Desktop and narrow screenshots were reviewed. Existing theme glow, uppercase label tracking, device-identification
blink and effect-thumbnail animations were retained as scoped design exceptions, not introduced as decoration.

Release builds and publication use the version-tag GitHub Actions workflow. Installation and hardware evidence
are separate from these source/fixture checks. No clean-PC install or physical thermal/shutdown test is implied.

## Limits

Paint masks contain at most 2,048 unique coordinate pairs; colocated LEDs share a selection and moved LEDs need
repainting. The entire effect retains its 32 KB limit. Generated blends cannot be nested and must fit existing
runtime limits. Appearance/Overview recipes persist through their APIs; Studio variations, blend recovery and
3D depth use local storage on the current PC.
