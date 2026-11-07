# Lighting Canvas Overview

The Overview now centers on your configured rig with selectable 2D and 3D views. Choose a lighting zone from the preview to open its controls. Brightness and power remain directly accessible.

Under **Workspaces → New workspace**, select **Showcase**, **Monitoring**, or **Balanced**. These create new arrangements and keep your previous workspace. Existing layouts and explicit Number, Gauge, Graph and Bar choices stay intact.

New sensor tiles use **Instrument** presentation: temperature scales with Airglow warning marks, segmented memory/load scales, and power/network histories. Taller tiles reveal history, statistics and an optional second reading. Airglow warning marks are display thresholds, not manufacturer thermal limits.

Use **Customize → Tile settings** on any tile for a theme-following or custom accent and Standard, Feature tile or Quiet emphasis. Instrument tiles can also change readings or add a comparison. Layout Undo/Redo includes these edits, and exported workspaces carry them.

Profiles show saved colors and pattern artwork. Scripted or reactive effects use a labeled palette/fallback; thumbnails never run effects on hardware. Fan tiles reveal the selected curve when enlarged, distinguish requested duty from reported channel duty, and show measured RPM only when available.

Dragging and resizing show the destination cell. Keyboard movement and resizing remain available. Window size and interface scale change presentation without rewriting the saved layout. Darkroom keeps its texture around the lighting canvas and uses calmer reading surfaces.

Verification: 751 Python tests, 208 JavaScript tests, 46 browser-module checks and all nine isolated browser suites passed. Tested viewport layouts include narrow windows at 120% scale and 4K. Screenshots and automated hardware-shaped fixtures validate rendering and interactions; physical fan/LED behavior and an installed release were not changed or tested by this implementation.

Known existing issue: a narrow window at 120% interface scale shows a blank strip at the bottom. The global appearance sizing file is unchanged by this work; the horizontal-fit checks do not cover that viewport-height issue.
