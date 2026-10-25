# Stage 1: self-updating app and community effects

This is the Stage 1 spec rewritten against Airglow as it is at 1.5.0. Where the original draft and the code
disagreed, the code and the decisions below win. Status: **confirmed 2026-10-17**. M1 is being built.

## What Stage 1 delivers
- A signed Airglow that installs in one step (Setup.exe or `winget install Airglow`) and, if you allow it, keeps
  itself up to date with small delta downloads.
- Effects written in JavaScript that anyone can build with a text editor and the app, test live, and publish to
  every Airglow user through a reviewed, signed registry in this repository.
- Running cost: €0 a month. No domain is needed for Stage 1.

## Decisions taken
| Question | Decision |
|---|---|
| App stack | Stays Python 3.12 + pywebview on WebView2. Effects run in WebView2; the host stays Python. |
| Going online | Opt-in. A first-run question, off by default: "Check for updates and new community effects automatically". The same switch is in Settings. Manual buttons always work. With it off, Airglow still never contacts anything on its own. |
| Existing numpy effects | Stay as they are. JS effects are added next to them and appear in the same effect picker. |
| Registry location | This repository (`effects/` next to `profiles/`), served as static files from GitHub Pages. No R2, no CDN account, no separate repo. |
| App name | Airglow. Repo `Heavenlystorm/airglow`. |
| Data folder | Everything the user owns stays in `%APPDATA%\Airglow` as today. Velopack owns `%LOCALAPPDATA%\Airglow` and deletes it on uninstall, so nothing of ours lives there. |
| Distribution | GitHub only: Releases, winget and a Scoop bucket. No website or domain in Stage 1. |
| Signing | Unsigned until SignPath approves the project. The release workflow has the slot ready. |
| Effect file extension | `.agfx` |
| Concurrent JS effects | At most 4 |
| Canvas | Fixed 320 × 200, stretched over the rig |
| Profiles and effects | One signed community index from M5 on |

## Principles (unchanged from the draft)
- Offline first: installed effects, profiles, devices and fan curves work with no network.
- Effects are untrusted code. Every JS effect runs sandboxed, including ours.
- Static over servers: everything Airglow downloads is a static file on GitHub (Releases or Pages).
- One source of truth: the files in this repository. Indexes are built by CI, never edited by hand.

## Milestones
1. **M1 Release pipeline**: tag-to-release in CI, Velopack, signing slot, winget, `app-meta.json`. Done when an
   older Airglow installed from winget updates itself.
2. **M2 Effect runtime v1**: sandboxed effect host, canvas sampling, lifecycle, params, audio, resource limits.
   Done when the example effects run on real lights and the hostile test effects are contained.
3. **M3 Creator tooling**: New effect, templates, hot reload, dev panel, Validate, Export, `effect-api.d.ts`.
4. **M4 Registry**: `effects/` in this repo, CI checks, headless render test, signed index on Pages.
5. **M5 Community browser**: one Community view for profiles and effects, with install, update, remove and
   revocation.
6. **M6 In-app Publish**: GitHub device flow, then fork, commit and pull request from inside Airglow.
7. **M7 Launch prep**: privacy wording, content rules, seed effects.

M1 is specified in detail below. **M2, M3, M4 and M6 became the Effect Studio** ([EFFECT-STUDIO.md](EFFECT-STUDIO.md)), which is built; its design wins where it differs from the M2 section and the later-milestone notes below (for example, `eval` is blocked, and one effect host process runs every Studio effect).

---

## M1 Release pipeline

### Today
`build.ps1` runs locally: tests, PyInstaller (`Airglow.spec`), optional `AIRGLOW_SIGN` command, Inno Setup
(`installer.iss`). CI builds the installer as an artifact but never publishes. `updates.py` checks the latest
GitHub release when the user presses the button and only links to the download.

### Workflow on tag `vX.Y.Z` (`.github/workflows/release.yml`)
1. `windows-latest`: install `requirements-dev.txt`, run the unit tests, check the tag matches
   `airglow/__init__.py` (fail otherwise).
2. PyInstaller builds `dist\Airglow\` as today.
3. Sign `Airglow.exe` (see Signing). Skipped, with a warning in the run summary, while no signing secrets exist.
4. `vpk pack --packId Airglow --packVersion X.Y.Z --packDir dist\Airglow --mainExe Airglow.exe --icon airglow.ico`
   with `--channel stable`, or `--channel beta` for tags like `v1.6.0-beta.1`. This produces
   `Airglow-win-Setup.exe`, a full package, a delta against the previous release, and the release feed.
5. Sign the Setup.exe.
6. `vpk upload github` attaches everything to the GitHub release (beta tags as pre-releases) with the changelog
   section for that version as release notes.
7. Submit to winget with `winget-releaser` (stable only).
8. Update the Scoop bucket (`Heavenlystorm/scoop-airglow`) to the new portable zip (stable only).

Releases are never built on a laptop again. `build.ps1` stays for local test builds and runs `vpk pack` instead
of Inno Setup when `vpk` is installed.

### Signing
- First choice: SignPath Foundation (free for open source, signs inside GitHub Actions). Applying is your step;
  the workflow has the slot ready and runs unsigned until the secrets exist.
- Fallbacks: Certum open-source certificate or Azure Trusted Signing, through the same slot (`vpk --signTemplate`).
- Until signed, the README keeps its SmartScreen note.

### Updating (Velopack Python SDK, `velopack` on PyPI)
- `velopack.App().run()` is the first thing `run.py` does, so install, update and uninstall hooks work.
- Only with the online switch on: check a minute after start-up and every 6 hours, download in the background, apply on the
  next restart or when the user presses "Restart to update". The tray tooltip says when an update is waiting.
- With the switch off: Settings, About keeps "Check for updates", which now downloads and applies instead of only
  linking to GitHub.
- Channel: stable or beta, in Settings, About. Beta checks both feeds and takes the newer, so a beta user gets
  the next stable release too. Switching back to stable never downgrades: you stay on the beta until a stable
  release is newer.
- Scoop installs (portable zip under `scoop\apps`) don't update themselves; Settings says to run
  `scoop update airglow` instead.
- Updates never touch `%APPDATA%\Airglow`.

### Moving existing users off Inno Setup
Existing installs live in `%LOCALAPPDATA%\Programs\Airglow` (Inno), Velopack installs to `%LOCALAPPDATA%\Airglow`.
- 1.6.0 ships as a Velopack release only. The Velopack install hook removes the old install itself: it stops
  any Airglow running from the old folder, deletes the folder, the old Start menu and desktop shortcuts (only
  those pointing there) and the old "Apps & features" entry. It doesn't run the old uninstaller, because that one
  kills every `Airglow.exe`, including the new one. LibreHardwareMonitor and OpenRGB stay installed. Settings in
  `%APPDATA%\Airglow` are untouched.
- The "Start with Windows" Run key is rewritten to the new path (`autostart.py`).
- Old 1.5.0 installs don't update themselves (they have no updater). Their manual "Check for updates" already links
  to the release page, where the new Setup.exe does the move.
- A fresh install starts with Windows (the Inno installer's default). A moved install keeps whatever the user had.
- The Velopack uninstall hook takes over what `installer.iss` did on uninstall: removing the Run key and offering
  to remove the helper programs. The offer runs as a small detached script, so it isn't cut off by Velopack's hook
  time limit and doesn't hold files in the folder being deleted.
- `installer.iss` and Inno Setup are gone from the build.

### `app-meta.json` (`meta/app-meta.json` in this repo, read from `raw.githubusercontent.com` like the gallery)
```json
{ "schema": 1, "minSupported": "1.6.0", "effectApi": [1], "notice": null }
```
There's no `latest` field: Velopack's release feed already says what the latest version is. The file is edited by
hand (it's a switchboard, not a build artifact), and a test checks its shape.
Fetched with the update check (so only when the switch is on or the button is pressed), cached in
`%APPDATA%\Airglow\cache\`. A failed fetch is ignored.
- Below `minSupported`: a banner asks to update and the Community view is disabled. Everything local keeps working.
- `effectApi`: Effect API major versions this release line runs.
- `notice`: optional banner text for outages and security advisories.

### winget and Scoop
- winget: the first manifest (`Heavenlystorm.Airglow`) goes into `microsoft/winget-pkgs` by hand once, with
  `wingetcreate`. After that,
  `winget-releaser` opens the update PR on each stable tag. It needs a classic token with `public_repo` scope stored
  as the `WINGET_TOKEN` secret.
- Scoop: `Heavenlystorm/scoop-airglow` holds `airglow.json`, pointing at the release's portable zip. The release
  workflow rewrites it on each stable tag, using the `SCOOP_BUCKET_TOKEN` secret. `scoop bucket add airglow
  https://github.com/Heavenlystorm/scoop-airglow` then `scoop install airglow`.

### M1 done when
- Pushing `v1.6.0-beta.1` produces a pre-release with Setup.exe, full and delta packages, with no manual step.
- A machine with 1.5.0 (Inno) runs the new Setup.exe and ends up with one Airglow, settings and helper programs
  intact, starting with Windows from the new path.
- A machine on 1.6.0-beta.1 with the online switch on updates itself to beta.2 within 6 hours, and with the switch
  off does not contact GitHub until "Check for updates" is pressed.
- Installing needs no admin prompt.

---

## M2 Effect runtime v1

> Superseded by [EFFECT-STUDIO.md](EFFECT-STUDIO.md), kept for the history of the decisions.


### Shape
JS effects draw onto a fixed **320 × 200** canvas. Airglow already knows where every LED sits (`z.pos`, 0..1
across the whole rig, set by the guided light setup), so each LED maps to `(x · 319, y · 199)` on the canvas. No
new layout view is needed.

A JS effect plugs into the existing engine as one more `Effect` subclass (`JsEffect` in
`airglow/lighting/jseffect.py`). The engine keeps its 30 fps clock. Each tick it sends the effect host the time,
parameters and (if allowed) audio analysis, and uses the newest colours the host has sent back. That adds one
frame of latency, which is fine for lighting.

### The effect host process
Each running JS effect gets its own child process: `Airglow.exe --effect-host <effect folder>`.
- The process builds a hidden WinForms form and a WebView2 control directly through pythonnet, using the WebView2
  assemblies pywebview already ships. It doesn't use pywebview's window, so no `js_api` bridge or host objects are
  injected.
- Its own WebView2 user data folder (`%APPDATA%\Airglow\effect-host\`) means its own WebView2 browser process,
  separate from the app UI.
- Airglow talks to it over the child's stdin/stdout: length-prefixed JSON for control messages, raw bytes for
  colour frames (3 bytes per LED).
- Killing the process tree is the hard stop for hangs and memory.
- At most 4 JS effects run at once. A 5th asks to turn one off. Per-area effects in profiles can be JS effects.

### Sandbox
- The effect folder is served read-only from `https://fx.airglow/` (`SetVirtualHostNameToFolderMapping`). The
  runtime shim is served from a second mapping, `https://runtime.airglow/`.
- `WebResourceRequested` cancels every request not aimed at those two hosts. A strict CSP sits behind it
  (`default-src 'self' https://runtime.airglow; connect-src 'none'`). `eval` stays allowed, since shaders and
  effect code sometimes need it and there's no network to abuse it with.
- Blocked: navigation away, new windows, downloads, context menu, host objects, DevTools (except in dev mode),
  status bar, autofill, zoom.
- `PermissionRequested` denies everything (camera, mic, location, clipboard, notifications, ...).
- Messages from the page follow a fixed schema (`frame`, `log`, `error`, `ready`). Anything else is dropped and
  logged.

### Lifecycle (`effect.js`, ES module)
```js
export function init(ctx) {}                      // once, after load
export function render(ctx, frame) {}             // every frame
export function onParamChange(id, value, ctx) {}  // optional
export function dispose(ctx) {}                   // optional, before unload
```
`ctx.canvas`, `ctx.width`, `ctx.height`, `ctx.params`, `ctx.log(...)`, `frame.time`, `frame.dt`, and
`frame.audio` (only with the `audio` permission) work exactly as in the original draft. The shim samples the LED
positions after each `render` (2D with `getImageData`, WebGL2 with `readPixels` into a reused buffer) and posts one
`Uint8Array` back.

### Audio
The existing `AudioAnalyzer` (WASAPI loopback, 16 bands) grows to 64 log-spaced bands plus a beat flag (energy
onset over the low bands). Existing effects read 16 bands averaged down from the 64, so they look the same. Only
the analysis crosses into the effect, never samples.

### Params
`number`, `color`, `boolean`, `select`, at most 16. They map onto the UI's existing control types (`range`,
`color`, `toggle`, `choice`), so the effect panel builds controls for JS effects the same way it does now. Values
are saved per effect id like built-in effect params.

### Resource limits
- Frame budget: `render` over 20 ms for 30 frames in a row pauses the effect and tells the user why.
- Hang: no frame for 3 s kills the host process. The effect stays off until re-enabled, and the zones fall back to
  the profile's base look.
- Memory: the host process tree above 512 MB is killed (`psutil`).
- A crashing host never takes lighting down. The engine treats a missing frame as "keep the last colours".

### Where effects live
- `%APPDATA%\Airglow\effects\<id>\<version>\`: installed effects (M5).
- `Documents\Airglow\Effects\dev\<id>\`: effects under development (M3).
- `web/effects/examples/`: five example effects bundled with the app (Gradient, Particles, Audio bars, WebGL
  shader, Blank). These are the M2 test material and later the M3 templates.

Effect package format and `manifest.json` are as in the original draft, with `.agfx` as the file extension.

### Hostile test effects (`tests/fixtures/hostile-effects/`)
fetch, XHR, WebSocket, `<img src="https://...">`, `window.open`, navigation, `file://` read, an infinite loop in
`render`, an infinite loop in `init`, a 2 GB allocation, a clipboard read, a camera request. An automated test
runs each one through the real host on the Windows CI runner and checks it was contained and Airglow kept running.

### M2 done when
- The five examples run on real lights and in the live preview at 30 fps with about 100 LEDs.
- Every hostile effect achieves nothing it tried and is stopped within the limits above, with the app responsive.
- With no JS effect on, no effect host process runs.

---

## Later milestones: changes from the draft
- **M3**: the dev preview draws the user's real LED positions from the light setup. `harness.html` (browser-only
  testing) moves up to M3, since it's also what the M4 headless test runs.
- **M4**: effects live in `effects/<login>/<slug>/` in this repository, checked by the same kind of workflow as
  `profiles/` today. On merge, CI builds `.agfx` packages and publishes them, with `index.json` and its Ed25519
  signature, to the `gh-pages` branch at immutable paths (`/fx/<id>/<version>.agfx`). GitHub Pages serves with a
  10-minute cache, so there's nothing to purge. Verifying the signature needs one new dependency (`cryptography`
  or PyNaCl).
- **M5**: one Community view for profiles and effects. The profile gallery moves into the same signed index later
  .
- **M6**: profiles keep their current "opens GitHub in your browser" share flow, which works without OAuth. Effects
  need the device flow, because a zip can't go through GitHub's "new file" page.
- **M7**: Sentry is dropped from Stage 1. The README's "no telemetry" promise stays. Impressum and privacy policy
  are only needed if a website beyond the GitHub repo appears.

## Still to do outside the code
- Apply to SignPath Foundation, then add its secrets.
- Create the `WINGET_TOKEN` and `SCOOP_BUCKET_TOKEN` secrets and the `Heavenlystorm/scoop-airglow` repo.
- Submit the first winget manifest by hand with `wingetcreate` (steps in `docs/RELEASING.md`).
