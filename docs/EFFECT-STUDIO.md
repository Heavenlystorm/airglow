# Effect Studio: make and share an effect in 10 minutes

Confirmed 2026-10-17 and built the same day. It replaces M2, M3, M4 and M6 in [STAGE1.md](STAGE1.md). What's
verified and what isn't is at the end.

## The 10-minute path
1. **Studio tab, pick a starting point** (Blank, Aurora, Ember, Ocean, Lights and sirens, Party, or a code template),
   or **Remix** any effect: from Lighting, from your own, or from the community gallery. The editor opens with your
   own case on screen.
2. **Stack layers**: add a wave on top, sparkles, a pulse on the beat. Pick colours, speed, direction, how strongly
   each layer mixes in, and whether it lights the whole case or one area (front fans, graphics card...). Every change
   shows on screen at once, and on your real lights too with **Also on my lights**.
3. **Choose what others can tweak**: tick "Show as a control" under a setting, and it becomes a control in everyone's
   effect panel. You can rename it in the effect's details.
4. **Save**. It's in your effects and in Lighting's effect list, and works in profiles and automations like any other
   effect. **Use on my lights** puts it on.
5. **Share**: your GitHub name, a description, tags and a licence. GitHub opens in your browser with the file filled
   in; press Propose changes, then Create pull request. That's the end of the 10 minutes.
6. The checks run on the pull request by themselves. Once a maintainer approves and merges it, it's in every
   Airglow's community gallery within about 10 minutes.

People who code switch the editor to **Code** (the layers become a small script they can change) or start from a code
template, and can edit in VS Code with the preview reloading on every save.

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
| The Studio UI, starting points, remix recipes | `web/js/studio.js`, `web/js/studio-start.js` |
| Registry checks, index, headless render test, CI | `tools/effects.py`, `tools/effects-render.mjs`, `.github/workflows/effects.yml` |
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
<meta property="l2_color" label="Star colour" type="color" default="#e8fff6">

<script type="application/json" id="layers">            <!-- a builder effect: one layer per line -->
[
{"type": "gradient", "enabled": true, "opacity": 100, "blend": "normal", "mask": "all", "params": {...}, "expose": {}},
...
]
</script>
```
A code effect has a plain `<script>` instead, and its controls are `<meta property>` tags (types `number`, `color`,
`boolean`, `select`; at most 16). Anything else in the file (other tags, scripts with `src`) is refused.

### The effect API (version 1)
```js
airglow.effect({
  init(ctx) {},                     // once
  render(ctx, frame) {},            // draw on ctx.canvas (320 x 200, 2d or webgl2); each LED takes the colour under it
  point(led, frame) { return [r, g, b]; },   // or answer per LED: led = {x, y (0..1), px, py, area, index}
  frame(frame) {},                  // optional, once per frame before point()
  onParamChange(id, value, ctx) {}, // optional
  dispose(ctx) {},                  // optional
});
// ctx.params (your controls), ctx.log(...) (shown under the preview), ctx.width, ctx.height
// frame.time, frame.dt (seconds), frame.audio = {level, bass, beat, bands: Float32Array(64)} or null
// AirglowLayers comes with every effect, so code can reuse the layer renderer.
```
The canvas has the same 1.6 : 1 shape as Airglow's stage, so it covers the case without stretching. `point()` effects
see exactly where each LED is, including the area it sits in.

## Layers
Colour: Solid, Gradient (straight or round, 2 to 4 colours, drifting), Rainbow, Colour cycle. Motion: Plasma, Wave,
Scanner, Sparkles, Breathing. Music: Beat pulse, Spectrum, Bass glow. Every layer has a strength (0 to 100 %), a mix
mode (Cover, Add light, Brighten, Darken) and a "Where" (the whole case or one area). At most 12 layers.

The catalogue is one file, `web/runtime/layers.json`, which the builder's controls, the renderer and the validator
all read. The renderer uses the same maths as the built-in effects, so **Remix** of Solid, Breathing, Colour cycle,
Rainbow wave, Gradient, Music spectrum and Music pulse looks the same as the original. Comet, Temperature and Screen
ambience have no layer equivalent (they need each fan's ring, the sensors, or the screen).

## Running effects safely
Effects are code from strangers, so none of it runs with access to Airglow, the network or files.

- **The runtime**: each effect runs in a Web Worker with an `OffscreenCanvas`, inside an `<iframe sandbox="allow-scripts">`
  (no same-origin) whose page is served with a CSP that allows only its own scripts and a worker made from a blob:
  no `connect-src`, no `eval`, no images or styles. The worker's only way out is posting LED colours (and, for the
  Studio, a picture of the canvas) to the frame, which passes only those on.
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
- The gallery is only fetched while the community view is open (and when installing), like the profile gallery.
  Index signing (Ed25519) is left for later; reviewed pull requests, SHA-256 and revocation cover version 1.

## Verified, and not yet
Verified on this PC (Windows 11, WebView2), with the demo rig of the dev server:
- The sandbox and its limits, with hostile test effects (above).
- Builder, Test beat, code mode with CodeMirror, Save, Use on my lights, community browse and install, Lighting's
  Create and Remix buttons, all in a real WebView2 window.
- "Also on my lights" and saved effects play through the effect host process; the engine gets their colours.
- The headless render test passes the six starter effects and fails an endless loop and a slow effect.
- 330 unit tests, including the Studio's routes, the host's supervision and the registry tools.

Not yet:
- The 10-minute run with a person who has never seen the Studio.
- Real hardware (the demo rig is simulated) and a long run on the lights.
- The CI workflow itself, and a real pull request through Share (needs the branch on GitHub).
- Index signing, and seed effects beyond the first six (M7 wants 15 to 20).
