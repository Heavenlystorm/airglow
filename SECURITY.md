# Security

Airglow controls fans, so a way for another program or a website to change fan speeds or lighting is a security bug.

## How Airglow protects its local API
- The HTTP server listens on `127.0.0.1` only.
- Every request must carry a `Host` header for that address, which blocks DNS rebinding.
- Every write must carry the `X-Airglow: 1` header. A web page on another origin can't send it without a CORS
  preflight, and Airglow never approves one.
- Requests are size-limited and validated; fan curves can't go below 20 %, and at 90 °C fans run at full speed
  whatever a curve says.

Programs already running as you on the same PC can still talk to the API, like they could to any app you run.

## Studio effects
Effects made in the Studio are JavaScript, and community effects come from strangers, so they run sandboxed:
- Each effect runs in a Web Worker inside an `<iframe sandbox="allow-scripts">` without same-origin access. Its page
  is served with a Content Security Policy that blocks every connection (`connect-src 'none'`), `eval`, and any
  script but its own. It can't reach Airglow's API, the network, files, or the page around it; all it can send back
  is LED colours.
- On the real lights, effects run in a separate effect host process with its own WebView2 data folder. A stuck,
  slow or memory-hungry effect is stopped, and the process is ended if it stops answering.
- Community effects arrive through reviewed pull requests, are checked automatically, and are verified against the
  gallery index's SHA-256 when installed. Maintainers can revoke one, and Airglow turns it off.

A way for an effect to get out of that sandbox is a security bug; please report it as below.

## Updates
- Airglow only looks for updates on its own if you turned on "Check automatically". Otherwise it checks when you
  press the button.
- Updates come from this repository's GitHub releases over HTTPS. Velopack checks every downloaded package against
  the checksum in the release feed (SHA-256) before installing it.
- Releases are built by GitHub Actions from a tag, never on someone's PC. Code signing is planned (see
  [docs/RELEASING.md](docs/RELEASING.md)); until then the installer is unsigned.

## Reporting
Please don't open a public issue for a vulnerability. Use GitHub's private vulnerability reporting
(Security tab, "Report a vulnerability") and include steps to reproduce. You'll get an answer within a week.
