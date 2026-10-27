# Changelog

## 1.10.2
- Fan failsafe commands reach every selected LibreHardwareMonitor channel, including setups with more than six
  channels. Full-speed thermal protection bypasses normal retry backoff, and changing curves no longer starve
  later channels.
- An unreadable settings file stays protected during automatic device discovery and later saves. Restart Airglow
  to load it again; a damaged file that was safely set aside can still be replaced with new settings.
- Overview saves run one at a time. Older responses no longer overwrite names, tiles or pins edited while a save
  is pending.

## 1.10.1
A performance release for the two grained designs. No fan speed and no light is changed.

- **Spore and Darkroom cost less to draw.** Their speckle was built at runtime by an SVG turbulence filter on a
  pseudo-element, composited through `mix-blend-mode`: every tile carried a full-size noise raster and a layer of its
  own, re-blended whenever the reading in the tile changed. The speck is now a 64px tile baked once
  (`tools/make_grain.py`, `web/img/grain-*.png`) and painted as the panel's own background: no filter, no blend mode,
  no extra layer. The texture is unchanged (measured: the same speck contrast on screen).
- **The matte designs no longer blur what is behind a control.** Kiln, Lumen, Midnight, Spore, Rim and Darkroom drop
  the two 6px backdrop blurs (`chip-btn`, `tile-tools`), which read as glass on paper, plaster and print anyway. A
  blur over the live stage has to be recomposited every time the lights redraw.
- **Measured on a 1280x800 window:** the renderer's main thread stays at about 1.2 ms per second of wall time on the
  Overview in every design, and with an animated effect on screen the new designs no longer sit above Graphite (all
  within run-to-run noise of each other, 4.4 to 5.6 % of one core). The idle UI was never the cost; the animated
  stage is, and it costs the same in every design.

## 1.10.0
- **Six more interface designs, each its own system rather than a recolour.** Settings, General now offers twelve:
  Graphite, ASCII, Paper, Amber CRT, High contrast and Neon are joined by **Kiln** (a near-black workshop, cream
  serif figures and one ember accent; a curve turns ember, then white-hot, when its chip runs warm), **Lumen**
  (violet-black, lowercase serif headings over a plain sans, a blueprint grid inside the charts, one brass line),
  **Midnight** (midnight blue to every edge, square surfaces, wide light heads and figures, one azure signal),
  **Spore** (coffee-black with a speck grain on the tiles, a soft serif, one oyster-pink fill), **Rim** (warm black,
  a hairline above each tile with corner ticks, every measurement in mono, an orange signal and a sky-blue second)
  and **Darkroom** (print-black, hairline condensed capitals, film grain, one amber safelight).
- **The structure changes with the type.** Kiln and Spore set their readings in a serif; Midnight and Rim cut every
  corner square; Darkroom rules a hairline under each tile head; Rim and Darkroom grain their wells. Each design
  keeps its one accent for what is selected or switched on, and the six designs that came before are unchanged.
- **The gallery previews carry each design's own mark**, not just its colours: Kiln's heat ramp on the meter,
  Spore's grain, Rim's corner ticks, Lumen's grid, Darkroom's capitals.
- **Your lights keep their real colours** in all twelve, and the window's title bar takes the design's ground.

## 1.9.4
No fan speed was changed in this release.

- **The log no longer fills with tracebacks when a page is closed.** 1.9.3 caught the write error together with
  the state build, so a closed live-update connection logged "state event failed" with a stack trace every couple
  of seconds for the rest of the session, and the handler for it never ended. A write to a closed client ends that
  stream again, as it did before 1.9.3.
- **The log now says why the fans are at the speed they are at** - "fans at 26% (the curve), temperature 43 °C",
  or "(failsafe 30%, no reading)", "(full speed, 95 °C on the hottest part)", "(lowest speed, no reading yet)" -
  whenever the speed changes by 5 points or more. If they ever run faster than you set them, the log will name the
  input that asked for it.

## 1.9.3
The rest of the review's findings.

- **A "too many Studio effects" notice no longer turns off the effect that is live.** Running a fifth Studio effect
  marked the effect off by its id, and every zone using that id then drew nothing. The notice is advisory now, and
  it clears as soon as the effect draws again.
- **The elevated half of helper setup only runs files from Airglow's own downloads folder.** The job file it is
  handed sits in a folder other programs can write, and it names the installers to copy and run; anything pointing
  elsewhere on the disk is refused before a byte is copied.
- **Effect files whose name isn't a valid effect id are no longer listed** - they appeared in the Studio but could
  never be played or deleted.
- **Dynamic Lighting:** if Windows refuses the colours for a device, that is reported on the device once, instead of
  logging a traceback thirty times a second while the device stays dark.

## 1.9.2
A bug-fix release after a full review of the app. No new features.

- **Security: a web page could drive your lights and fans.** A request that didn't carry Airglow's header was
  refused before its body was read, and on a kept-alive connection that unread body was then parsed as the next
  request - so a page could smuggle one that did carry the header, and reach settings, effects, fan speeds or the
  helper setup. Refused requests now close the connection, and the effect frame may only load scripts from Airglow
  itself rather than any program on this PC. Effect console output is rate-limited so an effect can't cycle the log.
- **Your settings could be reset by one failed read.** A config file that was momentarily locked (an antivirus
  scan, the search indexer) or half-written was treated as missing, and the defaults were then written over it.
  A damaged file is now kept as `.corrupt`, writes are flushed to disk before they replace anything, and nothing is
  rewritten after a read that failed.
- **Renaming an Overview tile could lose the end of the name.** The tile settings sheet held a copy that was replaced
  whenever the layout was saved; typing a little more wrote to the copy nobody shows.
- **The window could stop updating until you switched tabs.** A closed live connection was reused, and the watchdog
  that should have recovered it switched itself off after firing once. A failed zone refresh could also freeze every
  tile behind a false "Airglow isn't responding".
- **Lights that stayed at their default colours:** lighting-only NZXT controllers (HUE 2, HUE 2 Ambient, N7 boards)
  never re-asked for their LED report, and an ASUS Aura board that didn't answer its config query accepted colours
  and sent nothing. A re-plugged Dynamic Lighting device was never picked up until a restart.
- **Fans:** a stalled LibreHardwareMonitor could hold up the fan loop for up to a minute, and a channel it refused was
  retried every two seconds forever. Channels now back off, each pass is bounded, and the failsafe speed still goes
  out immediately. OpenRGB being slow no longer drops every OpenRGB device.
- **Studio:** the gallery preview no longer renders a second copy of the whole rig every frame (and no longer holds
  the frame lock while it does), the backdrop grid is a quarter of the work and its time is counted in the frame
  meter, a preview can no longer leave the lights on an effect that was marked "busy", and the hover preview stops
  when you move off a card.
- **Smaller fixes:** "Open in my editor" actually starts VS Code (its `.cmd` shim can't be spawned directly), the
  zone inspector no longer breaks on an effect that was deleted, unavailable sensors stop drawing a flat line as if
  they were still reporting, the notice bar is announced to screen readers, Ctrl+1-7 no longer changes the view under
  an open dialog, and the tray poll stops converting every LED to hex while the window is hidden.
- **Under the hood:** the test suite now runs every test in three modules that silently skipped their last class,
  CI compiles and imports every module (a syntax error in the window or effect-host code used to ship green), and a
  new check parses each browser module the way the browser does and verifies its imports resolve.

## 1.9.1
- **Six interface designs.** Graphite, ASCII, Paper, Amber CRT, High contrast and Neon, chosen in Settings, General
  with a live miniature of each. Every design is a full palette plus its own structure — character frames and block
  meters in ASCII, hairline paper panels, scanlines and amber phosphor, 2px rules and thick focus rings, neon tubes —
  not a hue rotation. Your lights keep their real colours in every design, the choice is kept per PC, and the Windows
  title bar follows it.
- **A Cost per hour tile.** The processor and graphics power Airglow already reads becomes €/h at the tariff you set.
  It starts at the German household average of 37.97 ct/kWh (Destatis, first half of 2026), is editable in Settings,
  and the reading can be pinned, graphed and picked in Add a tile like any other. Fresh installs get the tile on the
  Overview; existing layouts add it with Customize, Add a tile, Cost per hour.

## 1.9.0
- **The Studio works with no lights set up.** Previews and gallery cards draw a labelled sample case, so an effect
  can be made on a fresh PC; the starting points (code templates included) now show real live stills instead of a
  placeholder, and **Surprise me** makes a valid effect in one click.
- **Undo, redo, autosave and Revert.** Ctrl+Z / Ctrl+Shift+Z undo a whole slider drag as one step, saved effects
  autosave after a pause, and **Revert** goes back to where the session started (or your last Save); autosave does
  not move that point. **Save** is the primary button while there are changes; **Share** is secondary.
- **A palette and effect-wide knobs.** One palette, speed, brightness and music reactivity apply to the whole effect,
  and any of them can be exposed as a control. Layers opt into the palette with one tick, and a layer's colours can be
  exposed too. **See it as a user** shows the panel people get in Lighting.
- **Controls stay with their layer.** A layer has a stable id, so exposing a setting and then reordering the stack no
  longer moves the control to another layer or resets anyone's value. Files from 1.8 keep their old ids.
- **Any layer can follow the music**, by strength or by speed, not just the dedicated music layers.
- **Comet and Chase layers**, and each LED now knows its position on its own fan or strip (`led.local`, `led.fan`,
  `led.zone`) plus `ctx.rig` for the whole layout.
- **Code effects declare controls next to the code** with `airglow.params({...})`; the Studio writes them into the
  file on save. `console.log` output and errors appear under the preview, an error's line can be clicked to jump to
  it, and `docs/effect-api.d.ts` gives editors the same shapes.
- **A debug strip in the preview**: pause, quarter speed, restart the clock, an x/y grid with LED numbers and zone
  names, and Save canvas as PNG.
- **Check before sharing**: name, description, controls, music permission, size, reviewer scan, strobing faster than
  3 times a second and frame time, in one sheet, with the exact file shown under it. Share runs the check first.
  Export an effect to a file, import one, or drop it on the Studio.
- **The community gallery teaches**: "How it's made" shows a shared effect's controls and layers before installing,
  and **Learn** in the Studio has the rules, the API and copyable recipes.
- **Smaller fixes:** the effect's More sheet no longer adds a second click handler every time it opens; going from
  Layers to Code and back survives reopening the effect; a code effect that reads the music now asks for the audio
  permission instead of playing silent on the lights; sliders take a typed value and have a reset; layers can be
  moved with buttons, duplicated with Ctrl+D and deleted with Delete; the size in the check reads in bytes under 1 KB.

## 1.8.2
- **Overview graphs draw from the first reading.** The sparklines and big graphs no longer wait for six samples before
  drawing, so a cold start shows the trend immediately instead of a blank chart for the first 12 seconds.
- **No ghost digits behind the live numbers.** The faint unlit-segment figures behind temperatures, fan speeds and the
  setup counter are gone; only the current value remains.

## 1.8.1
- **Starting the UI is lighter.** Overview now loads only the code it needs; Lighting, Studio, Fans, Devices, Settings
  and the setup guide are fetched the first time you open them, then kept for the rest of the session.
- **Unchanged files are no longer re-downloaded in full.** The page, scripts, styles, font and images carry an ETag, so
  a later launch revalidates them with a 304 instead of reading and sending the whole file again.
- **The display font no longer blocks the first paint.** Martian Mono swaps in as it arrives instead of hiding the text.
- **Showing the window refreshes immediately.** Coming back from the tray fetches a current state instead of showing the
  readings from before the window was hidden. Nothing extra runs while it is hidden.

## 1.8.0
- **A "Why" strip says what changed the lights or fans.** When an automation is holding the lights dark, the fans are at
  the failsafe speed, automations are paused, or a rule has taken over, Airglow says so under the top bar and offers
  Pause all for an hour or Resume. It follows you across every tab.
- **The lighting stage works from the keyboard.** Every zone is a focusable button (Enter or Space selects it), and the
  Lighting inspector has a "Lighting for" picker, so a zone can get its own effect without the mouse.
- **The first-run guide has one way forward.** It now has one primary action, a close button and a step marker; closing
  it (or pressing Esc) finishes onboarding instead of asking again next launch. Installing OpenRGB and the sensor helper
  is a secondary action.
- **Settings is easier to navigate.** A section rail jumps between groups, the page remembers where you were, and a small
  Saved note sits in the rail. The column is centered on large screens.
- **Undo after a delete.** Automation rules, Overview tiles and Studio layers can be brought back from the toast.
- **A keyboard shortcut sheet** (`?` or F1, also Settings, About) lists the tab shortcuts, Ctrl+S, the fan-curve keys and
  the rest. The tabs now carry their Ctrl+1–7 shortcuts for assistive technology.
- **Accessibility fixes:** Settings, Devices and the automation builder name their controls; dialogs have accessible
  names and a Tab loop; text fields and colour swatches have a clear 2 px focus ring; two low-contrast labels were raised
  above 4.5:1.
- Overview's **Add a tile** is in the Customize bar (and back in the tab order), and choosing a zone no longer requires
  the mouse.

## 1.7.1
- **"Start OpenRGB" starts OpenRGB with the rights it needs.** Graphics cards, memory and onboard RGB are reached over
  I2C/SMBus, which goes through the PawnIO driver and therefore needs administrator rights: a copy started normally
  shows "One or more I2C/SMBus interfaces failed to initialize" and shows none of those devices. The button now starts
  it through the same sign-in task Airglow registers (which already runs with highest rights, so nothing is asked);
  without that task — a copy someone installed themselves — Windows is asked for permission once instead of starting
  a copy that can't see half its devices.
- **A winget problem no longer holds up the Scoop manifest.** The winget step is allowed to fail on its own, so a
  missing winget package (or a token GitHub no longer accepts) doesn't skip the Scoop update and doesn't fail an
  otherwise good release.

## 1.7.0
- **The effect host is restarted properly.** It used to be started again a few milliseconds after the old process was
  killed, while WebView2 still held its profile: the new host could fail-fast crash on start (three starts in 78 ms,
  then the effect was turned off for the rest of the session). Airglow now waits for the old process to go, then backs
  off (1.5 s, 3 s, 6 s, 12 s) before trying again. An effect that kills the host before it has drawn a single frame is
  turned off on its own, so the other Studio effects keep running.
- **Quitting doesn't start an effect host.** The lighting thread drew one more frame after the Studio host had been
  stopped, which started a fresh host that found Airglow already gone. The host is now closed before the engine stops.
- **Fan failsafe speed defaults to 30% instead of 60%** (and can be set from 30%). Windows' shutdown notification and
  the shutdown itself both reached the fan code, so the failsafe speed was written twice a few hundred milliseconds
  apart; it's written once now.
- **A still scene runs at 4 frames a second.** "Temperature" is an animated effect but its colours only change when a
  temperature does, so the engine used to redraw it 30 times a second for nothing. Any change brings the full rate back
  on the next frame.
- **Less work in the window.** LED glows reuse one gradient per colour instead of building one per LED per frame (150
  on the Overview, 15 times a second); the Studio preview only repaints when the effect sent a new frame; the Fans
  graph is only rebuilt when something it shows changes (and keeps the focus on a curve point); the Overview measures
  tile titles only when their text or the window changes.
- **The effect host stops piling up requests.** If Airglow falls behind, it skips a frame instead of queueing another
  POST (up to four are in flight), and effects that draw on a 2D canvas are read back directly instead of through a
  second canvas.
- **A launch during a quit starts Airglow.** It used to find the single-instance mutex taken, fail to reach the copy
  that was already shutting down, and exit without a window. It now waits for that copy to let go and takes over. The
  tray's new Restart Airglow uses the same path.
- **Unsaved Studio work is kept.** Leaving the editor (or closing the window) with "Not saved" no longer throws the
  effect away: a draft is kept and offered on the Studio's home page, and Ctrl+S saves.
- **OpenRGB isn't silent any more.** When it's enabled but not reachable, a notice at the top says which lights are
  dark and offers Start OpenRGB (Airglow's own copy). Settings can start it too.
- **Other fixes:** no temperature readings means a one-off notice that the fans are at the failsafe speed; a stalled
  state stream is noticed after 12 seconds instead of showing old readings for ever; API calls give up after 8 seconds;
  the Studio's community grid no longer leaves an observer behind on every search; the music thumbnail used the wrong
  animation (two keyframes had the same name).
- **New:** Reset settings in Settings (zones, presets, curves and the overview are kept), an Effect submenu in the
  tray, "Use for all" for the motherboard/cooler fan rows, and Ctrl+S in the Studio.

## 1.6.9
- **Studio effects take less than half the processor time.** With an animated Studio effect and the window in the
  tray, Airglow, its effect host and their WebView2 processes went from about a fifth of a core to under a
  tenth. The effect file was checked on disk for every zone on every frame; WebView2 handed every frame Airglow's
  effect host sent back to Python and back for nothing; and frames now go back as plain bytes, only when they changed.
- Closed to the tray, Airglow's window kept drawing the live preview 15 times a second, because WebView2 wasn't
  told it was hidden. Now it is: the page stops, and WebView2 lets go of most of its memory (about 120 MB to 10 MB).
- Graphics cards and memory on the slow I²C bus are sent at most 5 updates a second instead of 10. With an animated
  effect, OpenRGB went from about 13% of a core to about 6% on an RTX 5080.
- Airglow and its effect host each start one numerical worker thread instead of one per processor core. Airglow's
  main process went from 42 threads to 27 and from 585 MB of reserved memory to about 100 MB.
- LibreHardwareMonitor set up by Airglow reads only what Airglow uses: no network adapters, power supplies or
  batteries, every 2 seconds instead of every second, drive health every 30 seconds, and 30 seconds of history
  instead of 12 hours. This applies when Airglow installs or reinstalls it. A LibreHardwareMonitor you installed
  yourself is left as it is.
- A copy that can't update itself now says why, running from source or a build that wasn't installed, and links
  to the releases page.

## 1.6.8
- **Much less processor time with OpenRGB.** Graphics cards and memory sit on a slow bus, and an animated effect
  sent them 30 updates a second: one RTX 5080 kept OpenRGB busy on a whole core. They now get at most 10 a second,
  only when the colour really changes, and each device gets one update per frame instead of one per zone. With an
  animated effect, Airglow, OpenRGB and the effect together went from about a full core to about a sixth of one.
- A still scene (Solid or Off) is drawn 4 times a second instead of 30. Airglow then uses almost no processor time.
  Anything that moves, an identify flash or the setup guide gets the full 30 again right away.
- Without LibreHardwareMonitor, every temperature check held up automations and fans for 3 seconds. Temperatures
  are now read on their own, every 12 seconds until LibreHardwareMonitor answers, and the default address is
  `http://127.0.0.1:8085` (a saved "localhost" changes to it).
- When Airglow sets up OpenRGB, OpenRGB leaves alone the controllers Airglow drives itself (the ASUS Aura board and
  the NZXT hub), so the two programs no longer both open them.
- A hub or board controller plugged in after Airglow started, or one that wasn't ready at sign-in, is found within
  15 seconds instead of only after a restart.
- A running GNU C compiler (gcc.exe) is no longer taken for Gigabyte Control Center, so "Close it" can't end it.
  For RGB programs that run as Windows services, Airglow now says to stop them in Services instead of offering to
  close them.
- Empty NZXT hub channels no longer turn into enabled 40-LED strips on some starts. The hub answers "what's
  plugged in" about 0.6 seconds after it's asked, and Airglow only waited 0.5; once lighting started, the answer
  could get lost. Airglow now waits for it, asks again if it doesn't come, and a channel the hub never reported
  keeps the setup you saved.

## 1.6.7
- Setting up the helper programs on a PC without the Visual C++ runtime stopped at its first step with "isn't signed
  by Microsoft", so neither LibreHardwareMonitor nor OpenRGB was installed. The runtime is now checked and installed.
- A PC without the Edge WebView2 runtime used to get an empty window and no explanation. The installer now adds the
  runtime when it's missing, and Airglow tells you where to get it if it's started without it.

## 1.6.6
- Fans driven through LibreHardwareMonitor: if a speed change didn't get through (LibreHardwareMonitor busy for a
  moment), Airglow forgot that fan, so "Leave alone", quitting and the failsafe speed all skipped it and it stayed
  at its last speed. It now keeps trying, and hands the fan back to its own curve even after a failed change.
- A Studio effect edited and saved twice in quick succession, keeping the same file size, now plays the new version
  on your lights instead of the old one.

## 1.6.5
- Fan safety: "full speed at 90 °C" now watches the hottest part, whatever the curve follows. With the curve on the
  graphics card (or the new coolant reading), a processor at 95 °C used to leave the fans on the curve.
- **Motherboard fan headers and liquid coolers.** The Fans tab lists every fan and pump LibreHardwareMonitor can
  drive: the board's own fan headers, and the pumps and fans of NZXT Kraken X3, Z3, 2023 and 2024 Elite, older
  Kraken X coolers, MSI CoreLiquid, Aquacomputer D5 Next and Aquastream XT pumps, Arctic and Razer fan controllers
  and the NZXT Grid+. Each one is left alone until you pick: follow the fan curve, or hold a steady speed (pumps).
  Fan safety applies to them too: full speed at 90 °C, at least the failsafe speed without temperatures, and when
  Airglow quits they go back to their BIOS or cooler curve.
- Fans can follow the **coolant temperature** of an AIO or custom loop (Settings, Fans), and so can "running hot"
  automations. The Fans tab shows it next to the processor and graphics card.
- An ASUS Aura board no longer hides the rest of ASUS from OpenRGB: ROG and TUF graphics cards, memory, keyboards,
  mice and monitors used to disappear from Airglow as soon as the board was found.
- NZXT RGB & Fan Controllers whose lighting Airglow can't drive (the 3+6 channel revisions) are no longer hidden from
  OpenRGB either, so OpenRGB lights them while Airglow runs their fans.
- New, experimental: the HUE 2 lighting built into NZXT N7 motherboards.
- More liquid coolers are recognised in the setup guide (Corsair H110i GT, MSI CoreLiquid, ASUS ROG Ryuo, Ryujin
  and Strix LC, Aquacomputer pumps), and it says when a cooler's pump and fans can follow Airglow.
- Logitech G HUB, SteelSeries GG and Engine, MSI Mystic Light's LEDKeeper and the current Lian Li L-Connect 3 are
  recognised as programs that fight over the same lights. Peripheral software only counts while Airglow lights that
  brand's devices, so nobody is nagged about the program their mouse needs.
- Fans set up on one controller no longer start out drawn on top of each other on the Lighting stage: every kit of
  fans gets a column of its own.
- Windows Dynamic Lighting devices appear in the Devices tab, so their zones can be named, switched and checked.
- Long OpenRGB strips (WLED, E1.31, long ARGB runs) of more than 300 LEDs, and fans with more than 64 LEDs, can be
  saved; they used to be refused with an error.
- An OpenRGB message that stops half way no longer freezes the lights; Airglow reconnects instead.
- FanControl is also found when it was installed with Scoop or into your user folder.

## 1.6.2
- Fan safety: Airglow now sets the speed of every fan port on the NZXT hub. Before, a fan that read 0 RPM, for
  example one that had stalled at a low speed, was left out, so it stayed at that speed however hot the PC got and
  the failsafe skipped it too.
- A Studio effect that's deleted, or taken down from the gallery, no longer breaks what uses it. Profiles that use it
  are kept (they used to be dropped at the next start), zones show a plain colour until it's back, and the log gets
  one line instead of filling up within minutes. The Studio won't delete an effect a profile or zone still uses.
- Lights on OpenRGB devices no longer freeze when the connection drops in the middle of a frame (it can happen when
  the PC wakes from sleep).

## 1.6.1
- "Lights off when the PC sleeps or shuts down" now really leaves them off at shutdown. Before, the next frame lit
  them again; if the shutdown is cancelled, they come back on.
- Trying a profile (or "Also on my lights" in the Studio) is never saved. A restart while trying used to keep the
  borrowed look and put the fans on Balanced; now you get back what you had. Picking a profile while trying one
  ends the trial, so Discard no longer undoes it.
- After waking from sleep, devices from OpenRGB get every zone sent again, and Airglow no longer gathers a
  background thread per device each time.
- The tray tooltip calls a profile's own fan curve by the profile's name.
- Temperatures in automations follow your °C or °F setting, in the editor and in the Lighting tab.
- Overview tiles keep their titles readable when a long processor or graphics card name sits next to them.
- On the Lighting stage, zones at the edge of the case show whole, names step away from fans they would cover, and
  long effect names fit their cards.
- Saving settings retries when another program holds the file for a moment, instead of failing.
- Two profiles saved under the same name no longer clash, and automations with the same id are refused.
- Malformed requests to the automations API get a clear error instead of an internal one.

## 1.6.0
- **The Studio**, a new tab for making your own effects. Pick a starting point (or remix any effect), stack layers
  such as gradients, plasma, waves, sparkles, a scanner, breathing and three that follow the music, choose how each
  mixes in and which area of the case it lights, and watch it on a picture of your own case, and on your real lights
  with "Also on my lights". Tick "Show as a control" to let others tweak a setting. "Test beat" plays a fake beat so
  music effects can be built in silence. Saved effects work everywhere built-in ones do.
- Effects can also be written in JavaScript: switch the editor to Code (CodeMirror, with errors and log output under
  the preview), start from a canvas, per-LED or shader template, or edit in VS Code and see every save live.
- Share sends an effect to the community gallery as a pull request on GitHub, where it's checked automatically and
  reviewed. Community effects (Studio, Community effects) play on your own layout before you install them; updates
  keep the previous version for one rollback, and effects that are taken down are turned off.
- Every Studio effect runs sandboxed in a separate effect host process, without network, files or access to Airglow.
  One that hangs, is too slow or uses too much memory is turned off with the reason, and Lighting offers Try again.
- Airglow can keep itself up to date. Turn on "Check automatically" (Airglow asks once; it's off unless you say yes)
  and it looks for a new version a minute after it starts and every 6 hours, downloads it in the background and
  installs it the next time it starts, or right away with "Restart to update". Updates are small, since only what
  changed is downloaded. With it off, Airglow still only goes online when you press a button.
- A beta channel in Settings, About, for new features first.
- A new installer (Velopack). Installing over 1.5 or older moves Airglow to it: the old copy and its shortcuts go,
  your settings, profiles and helper programs stay, and "Start with Windows" keeps pointing at Airglow.
- Installing a new version while Airglow is open works without quitting first, and after a winget upgrade Airglow
  starts again in the tray.
- Sharing a long profile works: GitHub only takes about 8 KB in its address, so a longer profile goes on the
  clipboard, and Airglow tells you to paste it on the page that opens.
- Releases are built and published by GitHub Actions from a version tag.

## 1.5.0
- Presets are now profiles, and you can share them. A profile is the lighting look plus, optionally, effects for
  particular areas (front fans, top, graphics card...), its own fan curve, and the automations that switch to it.
  Areas are keyed by where lights sit, so a shared profile lands on the right lights on any rig.
- A community gallery (Lighting, Profiles, Browse): profiles from `profiles/` in this repository, featured first.
  Each one plays live on your own layout before you touch anything; "Try it" puts it on your real lights with Keep
  and Discard, and Discard puts everything back. Fan curves and automations only come along if you tick them.
- Share sends a profile to the gallery: GitHub opens with the file filled in, and saving it proposes a pull
  request. Profiles are checked automatically and the gallery's index is rebuilt when one is merged.

## 1.4.0
- A guided setup for your lights, on first run, whenever new lights show up, and from Devices. For each header the
  lights blink so you know which ones you're setting up; you pick the product (NZXT accessories are already
  detected), or count the LEDs with a slider that lights them one by one with the last one in amber, then drag it to
  where it sits on the Lighting stage (or snap it to Front, Top, Rear and so on first) while the guide waits beside
  it. Built-in lighting (graphics cards, boards, RAM, keyboards) only needs the last step, and a device's zones
  move together. Effects then sweep across the case as it really is. Products live in `airglow/data/components/`,
  one per line.
- Recognises USB-connected AIO coolers (NZXT Kraken, Corsair Hydro and iCUE, ASUS ROG Ryujin, MSI Coreliquid and
  Asetek-based ones) by name, from liquidctl's public list. A pump ring wired to an ARGB header is set up like any
  other light.
- Zone names on the Lighting stage no longer print on top of each other when zones sit close together.
- New social preview image.

## 1.3.1
- Fans no longer spin up for half a minute after you sign in. Airglow usually starts before LibreHardwareMonitor
  has temperatures ready; it used to treat that as a lost sensor and run the fans at the failsafe speed. Until the
  first temperature arrives (at most 45 seconds) the fans now stay at the lowest speed of your curve.

## 1.3.0
- Device plugins: each directly driven controller family is one module in `airglow/plugins/` that declares its USB
  ids, how to pick the right interface, what it leaves out of OpenRGB and its own settings. The NZXT HUE 2 and
  ASUS Aura USB drivers moved there, and `airglow/data/devices.json` is gone. Saved settings carry over unchanged.
- Per-device settings, drawn from what the plugin declares: a brightness limit for NZXT hubs, and switching an ASUS
  board's own LEDs off while its ARGB headers stay lit.
- A new look: an instrument-panel design with clearer readings and calmer surfaces.
- Much lighter on CPU and GPU while the window is open. Overview readings now update in place instead of gliding,
  effect thumbnails animate when you point at them (or tab to them), and the live preview only redraws when the
  colours change. On a test PC the Overview went from 60-90% of one CPU core to about 6%.

## 1.2.0
- Automatic setup for LibreHardwareMonitor and OpenRGB (welcome screen, and Settings, Helper programs). Airglow
  downloads pinned versions, checks their checksums, installs them into Program Files behind one Windows admin
  prompt, and starts them at sign-in. LibreHardwareMonitor's web server is set to answer only on this PC, with a
  password. The PawnIO driver and the Visual C++ runtime are installed too if they're missing. Uninstalling Airglow
  offers to remove them.

## 1.1.0
- More hardware, from a data file (`airglow/data/devices.json`): NZXT RGB & Fan Controller, Smart Device V2 and
  HUE 2 controllers, four more ASUS Aura motherboard controllers (all marked experimental), every HUE 2
  accessory's LED count, and fan control across several fan controllers.
- OpenRGB protocol 5: ARGB headers OpenRGB splits into fans show as fan rings, and keyboards get a grid layout.
- Windows Dynamic Lighting devices (experimental, off by default).
- Live updates over one event stream instead of polling; nothing runs in the page while it's hidden in the tray.
- Settings, About: **Save a report** for bug reports, with personal details removed.
- If the usual port is taken, Airglow uses the next free one.
- Settings files carry a format version and are upgraded once.
- Fixes: zone settings saved every second for some NZXT fan chains, pinned fan readings broke the tray tooltip,
  editing a preset an automation uses didn't apply, several resets at once after waking from sleep.
- Pause automations for 15 minutes, an hour or until 06:00; temperature rules no longer flicker at their limit;
  the power button says when an automation is holding the lights off.

## 1.0.0
First release.
