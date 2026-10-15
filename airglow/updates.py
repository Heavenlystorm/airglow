"""Manual update check against GitHub releases. Runs only when the user clicks "Check for updates":
Airglow never contacts the internet on its own."""

import json
import re
import urllib.error
import urllib.request

from . import __version__

# Where releases are published. Empty hides the check in the UI.
UPDATE_REPO = "Heavenlystorm/airglow"


def _parse(v):
    return tuple(int(x) for x in re.findall(r"\d+", v)[:3]) or (0,)


def check(repo=None, current=__version__, timeout=6):
    repo = repo or UPDATE_REPO
    if not repo:
        raise RuntimeError("Update checks aren't set up for this build")
    req = urllib.request.Request(f"https://api.github.com/repos/{repo}/releases/latest",
                                 headers={"Accept": "application/vnd.github+json", "User-Agent": f"Airglow/{current}"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            data = json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as e:
        raise RuntimeError("No published release found yet" if e.code == 404 else f"GitHub answered {e.code}") from e
    except (urllib.error.URLError, OSError) as e:
        raise RuntimeError("Couldn't reach GitHub. Check your internet connection.") from e
    latest = data.get("tag_name", "")
    return {"current": current, "latest": latest.lstrip("v"), "newer": _parse(latest) > _parse(current),
            "url": data.get("html_url"), "notes": (data.get("body") or "")[:2000]}
