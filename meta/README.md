# app-meta.json

Airglow reads this file whenever it checks for updates (only if the user turned on "Check automatically", or pressed
"Check for updates"), from `raw.githubusercontent.com`, and keeps the last good copy. Edit it by hand; a test checks
its shape.

| Field | What it does |
|---|---|
| `schema` | Always `1`. |
| `minSupported` | Versions below this show a banner asking to update, and community features turn off. Lighting, fans and devices keep working. |
| `effectApi` | Effect API major versions the current release line runs. |
| `notice` | Banner text for outages or security advisories (at most 300 characters), or `null`. |

A change is live for everyone at their next check; GitHub caches the raw file for about 5 minutes.
