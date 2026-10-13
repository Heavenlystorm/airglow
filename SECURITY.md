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

## Reporting
Please don't open a public issue for a vulnerability. Use GitHub's private vulnerability reporting
(Security tab, "Report a vulnerability") and include steps to reproduce. You'll get an answer within a week.
