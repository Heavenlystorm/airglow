# Overview Lighting Canvas

Approved direction: a creative, customizable Lighting Canvas. The screen answers “How does my PC look and run right now?” The actual rig is its signature; its actual light colors own the focal surface. Keep the existing Airglow shell, typography and twelve themes.

## Scope

- A closely framed live rig, selectable 2D/3D views, accessible zone selection and direct brightness/power controls.
- Distinct sensor instruments: temperature reading/history/known threshold, segmented load/memory, readable power/network timelines with units and honest gaps. Small tiles prioritize readings, medium adds history, large adds comparisons and detail.
- Fans show the active curve and measured operating point/RPM alongside existing profile controls. Never imply a detected channel is a confirmed physical fan.
- Profiles show their saved lighting colors/patterns, explicit click to apply. Preview never changes hardware.
- Optional Showcase, Monitoring and Balanced workspace starters; retain all existing layouts and dimensions until the user edits them.
- All tiles support appearance emphasis (standard/focus/quiet) and an optional accent. Sensor presentation also offers automatic instruments alongside existing Number/Gauge/Graph/Bar choices.
- Snap guides and responsive placement feedback while editing. Keyboard editing remains supported.
- Darkroom sensor surfaces become calmer; framing retains its identity. Motion follows real lighting or interaction and honors reduced motion.

## Contracts

No new dependencies. No telemetry. No hardware mutations except existing explicit control actions. Presentation never changes actual LED colors or sensor safety semantics. Resize alone never persists layout. Existing workspace v1 imports and exports remain supported. Validate all new presentation fields on server and client; invalid documents fail atomically.

Tile fields: optional `appearance: {emphasis: "standard"|"focus"|"quiet", accent: "#rrggbb"|null}`; optional lighting `view: "2d"|"3d"`; sensor `style: "auto"|"spark"|"ring"|"graph"|"bar"`. Omitted fields preserve old document shape. New sensor tiles and optional starters choose auto; old explicit styles remain usable.

Main worktree is the existing isolated release-1.13.1 worktree, branch feat/overview-lighting-canvas. Do not change the dirty original checkout or installed app. Browser writes must target isolated fixture 8790/8791 only. Publication is outside this implementation task.
