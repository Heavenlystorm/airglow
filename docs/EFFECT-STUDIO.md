# Effect Studio: make and share an effect in 10 minutes

Confirmed 2026-10-17 and built the same day. It replaces M2, M3, M4 and M6 in [STAGE1.md](STAGE1.md).
Updated 2026-10-23 for 1.9: a sample case when no lights are set up, real previews for every template, a palette and
effect-wide knobs, stable control ids, music reaction on any layer, Comet and Chase, undo/redo and autosave, the
in-app check, export/import, the gallery's "How it's made", the Learn guide, and `airglow.params` in code.
What's verified and what isn't is at the end.

## The 10-minute path
1. **Studio, pick a starting point** (Blank, Aurora, Ember, Ocean, Lights and sirens, Party, Two knobs, or a code
   template), press **Surprise me**, or **Remix** any effect: from Lighting, your own, or the community gallery. The
   editor opens with your own case on screen; before any lights are set up it uses a labelled sample case, so you can
   start on a fresh PC.
2. **Stack layers**: add a wave on top, sparkles, a comet around each fan, a pulse on the beat. Pick colours, speed,
   direction, how strongly each layer mixes in, and whether it lights the whole case or one area (front fans, graphics
   card...). Layers marked **palette** take their colour from the effect's one palette, and any layer can follow the
   music. Every change shows on screen at once, and on your real lights too with **Also on my lights**.
3. **Choose what others can tweak**: tick "Show as a control" under a setting — a layer's setting or one of the
   effect-wide ones (palette, speed, brightness, reactivity). It becomes a control in everyone's effect panel, and a
   reorder no longer moves it to another layer. **See it as a user** shows that panel and previews it live.
4. **Save**. It's in your effects and in Lighting's effect list, and works in profiles and automations like any other
   effect. Saved effects autosave as you keep editing; **Revert** goes back to where the session started (or your last
   Save), and Ctrl+Z / Ctrl+Shift+Z undo and redo (a slider drag is one step). **Use on my lights** puts it on.
5. **Check** (or press **Share**, which runs it first): name, description, controls, music permission, size,
   reviewer scan, strobing and frame time, in one sheet, with the exact file that will be sent under it.
6. **Share**: your GitHub name, a description, tags and a licence. GitHub opens in your browser with the file filled
   in; press Propose changes, then Create pull request. That's the end of the 10 minutes.

People who code switch the editor to **Code** (the layers become a small script they can change, and the generated
block can be read back into layers even after reopening) or start from a code template, declare controls with
`airglow.params({...})`, and can edit in VS Code with the preview reloading on every save. **Export .html** takes an
effect out; importing one, or dropping it on the Studio, opens it.

## The parts
| Part | Where |
|---|---|
| The effect file format, checks, review scan | `airglow/fxformat.py` |
| Your effects, installed ones, the draft, revocation | `airglow/fxlibrary.py` |
| The community gallery and the share link | `airglow/fxgallery.py` |
| Studio API (`/api/studio/...`) | `airglow/studioapi.py` |
| Effects on the real lights (effect host, supervision) | `airglow/lighting/studio.py`, `airglow/effecthost.py` |
| The sandbox: frame page, worker runtime, layer renderer, layer catalogue | `web/runtime/` |
| Running an effect from any page | `web/js/fxrun.js` (the host page uses it too: `web/js/fxhost.js`) |
| The Studio UI, starting points, randomiser, remix recipes | `web/js/studio.js`, `web/js/studio-start.js` |
| Registry checks, index, headless render test, CI | `tools/effects.py`, `tools/effects-render.mjs`, `.github/workflows/effects.yml` |
| Type declarations for editors | `docs/effect-api.d.ts` |
| Community effects and moderation lists | `effects/` |

## The effect file
One self-contained HTML file, at most 32 KB. Builder effects are 1 to 3 KB.

```html
<!doctype html>
<meta charset="utf-8">
<title>Aurora</title>
<meta name="airglow-effect" content="1">              <!-- Effect API major version -->
<meta name="author" content="jane-dev">                <!-- GitHub login; empty for effects that stay on your PC -->
<meta name="description" content="Slow northern lights drifting across the case.">
<meta name="license" content="MIT">                    <!-- an OSI licence; required in the gallery -->
<meta name="tags" content="ambient, calm">
<meta name="remix-of" content="sam.northern-glow">     <!-- optional credit -->
<meta name="permissions" content="audio">              <!-- only "audio" exists; added by itself for music layers -->
<meta property="palette" label="Your colours" type="colors" default="#00ffa3,#2f5bff">

<script type="application/json" id="layers">
{
  "v": 2,
  "layers": [
    {"id": "sky", "type": "gradient", "enabled": true, "opacity": 100, "blend": "normal", "mask": "all",
     "palette": true, "react": {"what": "speed", "amount": 30},
     "params": {"colors": ["#00ffa3", "#2f5bff"], "speed": 20, "angle": 15, "scale": 50},
     "expose": {"speed": "Drift"}},
    {"id": "stars", "type": "sparkles", "opacity": 70, "blend": "add", "params": {...}, "expose": {}}
  ],
  "master": {"palette": ["#00ffa3", "#2f5bff"], "speed": 100, "brightness": 100, "react": 50},
  "expose": {"palette": "Your colours", "speed": "Pace"}
}
</script>
```

The layers script may also be the original plain list (`[...]`), which is what v1 files contain; they draw and load
exactly as before. A layer's `id` is optional: files without one keep their old `l<number>_<setting>` control ids, and
the editor gives every layer an id when it touches it, so a control stays on its own layer when the stack is
reordered. `palette: true` makes a layer take its colours from `master.palette`; `react` makes it follow the music
(strength or speed). `master.speed` is 0 to 200 % (100 is normal), brightness 0 to 100, and reactivity shifts the
analysed audio up or down. `expose` names the effect-wide controls (palette, speed, brightness, react), which become
the `m_palette`, `m_speed`, `m_brightness` and `m_react` controls.

A code effect has a plain `<script>` instead. Its controls are `<meta property>` tags (types `number`, `color`,
`colors`, `boolean`, `select`; at most 16) — or `airglow.params({...})` in the script, which the Studio writes out as
those tags when it saves. Anything else in the file (other tags, scripts with `src`) is refused.

### The effect API (version 1)
```js
airglow.effect({
  init(ctx) {},                     // once
  render(ctx, frame) {},            // draw on ctx.canvas (320 x 200, 2d or webgl2); each LED takes the colour under it
  point(led, frame) { return [r, g, b]; },   // or answer per LED
  frame(frame) {},                  // optional, once per frame before point()
  onParamChange(id, value, ctx) {}, // optional
  dispose(ctx) {},                  // optional
});
airglow.params({                    // optional: controls next to the code
  speed: { label: "Speed", type: "number", min: 0.1, max: 3, step: 0.1, default: 1 },
  palette: { label: "Colours", type: "colors", default: ["#00ffa3", "#2f5bff"] },
});
// ctx.params (your controls), ctx.log(...) and console.log(...) (shown under the preview), ctx.width, ctx.height
// ctx.rig = { count, zones: [{name, area, count, x0, y0, x1, y1}] } (positions 0..1), so an effect adapts to the rig
// led = {x, y (0..1), px, py, local (0..1 on its own fan or strip), fan, zone, area, index}
// frame.time, frame.dt (seconds), frame.audio = {level, bass, beat, bands: Float32Array(64)} or null
// AirglowLayers comes with every effect, so code can reuse the layer renderer (and the palette/master settings).
```
The canvas has the same 1.6 : 1 shape as Airglow's stage, so it covers the case without stretching. `point()` effects
see exactly where each LED is, including the area it sits in and its place on its own fan or strip. An error shows
the line in the file; click it in the Studio to jump there. `docs/effect-api.d.ts` has the same shape for editors.

## Layers
Colour: Solid, Gradient (straight or round, 2 to 4 colours, drifting), Rainbow, Colour cycle. Motion: Plasma, Wave,
Scanner, Sparkles, Breathing, Comet (a head and tail around each fan and along each strip), Chase (bands running
around each fan). Music: Beat pulse, Spectrum, Bass glow. Every layer has a strength (0 to 100 %), a mix mode (Cover,
Add light, Brighten, Darken) and a "Where" (the whole case or one area). At most 12 layers. At most 16 controls,
effect-wide and per layer together.

The catalogue is one file, `web/runtime/layers.json`, which the builder's controls, the renderer and the validator
all read. The renderer uses the same maths as the built-in effects, so **Remix** of Solid, Breathing, Colour cycle,
Rainbow wave, Gradient, Music spectrum, Music pulse, Comet and the chases looks like the original. Temperature and
Screen ambience have no layer equivalent (they need the sensors or the screen).

A layer can be soloed to see it alone, moved with the buttons or by dragging, and shows its palette (a gradient chip)
and music reaction in its row.

## Controls people can change
- Each layer setting can be exposed, including a layer's colours. The label is renamed in the effect panel.
- The effect-wide palette, speed, brightness and reactivity can each be exposed once, so a shared effect can be
  retuned in one place instead of per layer.
- Ids are stable: `l<layer id>_<setting>`, `m_<knob>`. Old files keep their `l<index>_<setting>` ids.
- **See it as a user** shows the panel the effect gets in Lighting, with your labels and values, and previews changes.

## Trying it before sharing
- **Undo/redo** (`Ctrl+Z`, `Ctrl+Shift+Z`, or `Ctrl+Y`) with a drag or a burst of typing as one step; **Revert** back
  to where the session started or your last Save; saved effects **autosave** after a pause without moving that point.
  Your work is also kept as a draft if you leave.
- **Text values and reset**: every slider has a number you can type, and a `↺` back to its default. Layers move with
  the arrow buttons as well as the mouse, and can be duplicated (`Ctrl+D`) or deleted (`Delete`).
- **Debug strip**: pause, quarter speed, restart the clock at 0 s, an overlay with the x/y grid, LED numbers and zone
  names, and Save canvas as PNG.
- **Check** (top bar, and Share runs it first) shows the same list the reviewers see: name, description, controls,
  music permission, size, reviewer scan, strobing above 3 times a second, and frame time, with the exact file that
  will be sent. **Export** writes that .html; importing one, or dropping it on the Studio, opens it.
- **Learn** has the rules, the API and copyable recipes.

## Running effects safely
Effects are code from strangers, so none of it runs with access to Airglow, the network or files.

- **The runtime**: each effect runs in a Web Worker with an `OffscreenCanvas`, inside an `<iframe sandbox="allow-scripts">`
  (no same-origin) whose page is served with a CSP that allows only its own scripts and a worker made from a blob:
  no `connect-src`, no `eval`, no images or styles. The worker's only way out is posting LED colours (and, for the
  Studio, a picture of the canvas) to the frame, which passes only those on. `console.*` is routed into the Studio's
  console, nothing else of the console exists.
- **Tested in WebView2**: `fetch`, `WebSocket`, `importScripts`, `eval` and `data:` fetches all fail, and there's no
  `document`, `window` or `parent`. An endless loop is stopped within about 3.5 seconds; an effect over 20 ms a frame
  for 30 frames in a row is stopped.
- **On your lights**: the effect host is a separate Airglow process (`Airglow.exe --effect-host`) with its own hidden
  WebView2 and data folder. Its page runs every Studio effect that's on (at most 4) in the same kind of sandboxed frame,
  driven by a frame clock from the engine over `/api/fx/host/stream` (with a random token); colours come back in one
  POST per frame. The lights trail the effect by one frame. It starts with the first Studio effect and stops 30 s
  after the last. If it stops answering for 4 s it's restarted (at most 3 times a minute); if any one WebView2 process
  goes over 512 MB the effects are turned off. A turned-off effect leaves its LEDs dark and Airglow says why, with a
  Try again button.
- Built-in Python effects are unchanged.

## Sharing and the community gallery
- Shared effects live in this repository: `effects/<github-login>/<slug>.html`, the slug coming from the title.
- **Share** opens GitHub's new-file page with the file in its address. GitHub refuses addresses over about 8,190
  characters (measured), which holds an effect of roughly 5 KB. A longer one opens the page with just the file name
  and puts the effect on the clipboard to paste.
- **Checks on every pull request** (`.github/workflows/effects.yml`): the file is valid, named after its title, has
  a licence and description, and only touches the author's own folder; the review scan (fetch, connections, eval,
  encoded or minified code) goes into the run summary; and every changed effect runs for 10 seconds in headless
  Chromium with the real runtime, a 400-LED test rig and the test beat, failing on errors, a stuck effect or more
  than 20 ms a frame on average.
- **On merge**: `effects/index.json` is rebuilt with each file's SHA-256 (of the exact bytes GitHub serves; effect
  files are kept LF). Airglow checks it when installing; an update keeps the previous version for one rollback.
- **Moderation**: `effects/featured.json` lists effects shown first; `effects/revoked.json` takes effects down. Airglow
  turns off revoked installed effects when the community gallery loads, and refuses to install them.
- **In the gallery**: pointing at a card plays it on your case, and **How it's made** shows the description, controls
  and layers before you install it. **Remix** opens it in the Studio without installing.
- The gallery is only fetched while the community view is open (and when installing), like the profile gallery.
  Index signing (Ed25519) is left for later; reviewed pull requests, SHA-256 and revocation cover version 1.

## Verified, and not yet
Verified on this PC (Windows 11, WebView2), with the demo rig of the dev server:
- The sandbox and its limits, with hostile test effects.
- Builder, Test beat, code mode with CodeMirror, Save, Use on my lights, community browse and install, Lighting's
  Create and Remix buttons, all in a real WebView2 window.
- "Also on my lights" and saved effects play through the effect host process; the engine gets their colours.
- The headless render test passes the starter effects and fails an endless loop and a slow effect.

Verified for 1.9 on 2026-10-23 with headless Edge against the dev server and its sample rig: the sample-case preview,
every template's live still (including code templates), the editor with the palette and master knobs, an exposed
master knob appearing in **See it as a user**, saving and reopening with stable ids, autosave, undo after a slider
and a reorder, solo, the code round trip, `console.log` and the clickable error line, the check sheet (including a
5.2 Hz strobe being flagged), importing a gallery file, the gallery's "How it's made", and the Learn recipes.
482 Python tests pass.

Not yet:
- The 10-minute run with a person who has never seen the Studio.
- Real hardware (the demo rig is simulated) and a long run on the lights; the WebView2 pass was not repeated for 1.9.
- The CI workflow itself, and a real pull request through Share (needs the branch on GitHub).
- Index signing, and more seed effects (M7 wants 15 to 20; there are 6 in the gallery plus the templates).
