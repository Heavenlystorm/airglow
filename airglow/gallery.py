"""The community profile gallery: profiles/ in the Airglow repository on GitHub.

Airglow contacts GitHub only while the gallery is open. Each profile is one JSON file (airglow/profiles.py has the
format); profiles/index.json lists them and is rebuilt by a workflow whenever a profile is merged
(tools/profiles.py); profiles/featured.json is a hand-picked list of ids. Sharing never uploads anything from the
app: it opens GitHub's "new file" page in your browser with the profile filled in, which becomes a pull request.
"""

import json
import os
import re
import time
import urllib.error
import urllib.parse
import urllib.request

from . import __version__, profiles
from .updates import UPDATE_REPO

BRANCH = "main"
FOLDER = "profiles"
CACHE_SECONDS = 600
_cache = {}


def _raw(name, repo=None):
    # AIRGLOW_GALLERY_URL points the gallery somewhere else, e.g. the repo's own profiles/ folder (file:///...) for the
    # dev server, so profiles can be tried before they're on GitHub
    base = os.environ.get("AIRGLOW_GALLERY_URL") or f"https://raw.githubusercontent.com/{repo or UPDATE_REPO}/{BRANCH}/{FOLDER}"
    return f"{base.rstrip('/')}/{name}"


def _get_json(url, timeout=8):
    hit = _cache.get(url)
    if hit and time.monotonic() - hit[0] < CACHE_SECONDS:
        return hit[1]
    req = urllib.request.Request(url, headers={"User-Agent": f"Airglow/{__version__}"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            raw = resp.read(profiles.MAX_BYTES * 40 + 1)
    except urllib.error.HTTPError as e:
        raise RuntimeError("That isn't in the gallery (any more)." if e.code == 404 else f"GitHub answered {e.code}") from e
    except (urllib.error.URLError, OSError) as e:
        raise RuntimeError("Couldn't reach the gallery. Check your internet connection.") from e
    try:
        data = json.loads(raw.decode("utf-8"))
    except ValueError as e:
        raise RuntimeError("The gallery sent something Airglow couldn't read.") from e
    _cache[url] = (time.monotonic(), data)
    return data


def listing():
    """{"profiles": [{id, name, author, description, badges, added}], "featured": [ids]}, featured first."""
    index = _get_json(_raw("index.json"))
    try:
        featured = [i for i in _get_json(_raw("featured.json")) if isinstance(i, str)]
    except RuntimeError:
        featured = []
    items = []
    for e in (index.get("profiles") or []) if isinstance(index, dict) else []:
        if not isinstance(e, dict) or not isinstance(e.get("id"), str) or not profiles._ID.fullmatch(e["id"]):
            continue
        items.append({"id": e["id"], "name": str(e.get("name", e["id"]))[:40], "author": str(e.get("author", ""))[:40],
                      "description": str(e.get("description", ""))[:200], "added": str(e.get("added", ""))[:10],
                      "badges": [b for b in (e.get("badges") or []) if b in ("music", "screen", "fans",
                                                                         "automations", "areas")]})
    items.sort(key=lambda e: e["added"], reverse=True)                                  # newest first...
    items.sort(key=lambda e: featured.index(e["id"]) if e["id"] in featured else len(featured))   # ...featured on top
    return {"profiles": items, "featured": [i for i in featured if any(e["id"] == i for e in items)]}


def fetch(pid):
    """One profile from the gallery, validated."""
    if not isinstance(pid, str) or not profiles._ID.fullmatch(pid):
        raise ValueError("unknown profile")
    data = profiles.validate_shared(_get_json(_raw(f"{pid}.json")))
    if data["id"] != pid:
        raise RuntimeError("That gallery file doesn't match its name.")
    return {**data, "badges": profiles.badges(data)}


MAX_URL = 8000          # GitHub answers 414 above about 8,190 characters (measured 2026-10-17)


def share_file(shared):
    return json.dumps(shared, indent=2, ensure_ascii=False) + "\n"


def share_url(shared, repo=None):
    """GitHub's new-file page with the profile filled in; saving it there proposes a pull request. Returns
    (url, filled): a profile too long for the address gets the page with only the file name, to paste into."""
    base = f"https://github.com/{repo or UPDATE_REPO}/new/{BRANCH}/{FOLDER}?"
    url = base + urllib.parse.urlencode({"filename": f"{shared['id']}.json", "value": share_file(shared)})
    if len(url) <= MAX_URL:
        return url, True
    return base + urllib.parse.urlencode({"filename": f"{shared['id']}.json"}), False


def is_share_url(url, repo=None):
    return bool(re.fullmatch(rf"https://github\.com/{re.escape(repo or UPDATE_REPO)}/new/{BRANCH}/{FOLDER}\?.*", url, re.S))
