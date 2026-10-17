"""The community effect gallery: effects/ in the Airglow repository on GitHub.

Each effect is effects/<github-login>/<slug>.html. effects/index.json lists them with their SHA-256 and is rebuilt by
a workflow when a pull request is merged (tools/effects.py); its "revoked" list turns effects off everywhere.
Airglow contacts GitHub only while the Studio's community view is open (and when installing), like the profile
gallery. Sharing never uploads anything from the app: it opens GitHub's "new file" page with the effect filled in.
"""

import json
import os
import re
import time
import urllib.error
import urllib.parse
import urllib.request

from . import __version__, fxformat
from .updates import UPDATE_REPO

BRANCH = "main"
FOLDER = "effects"
CACHE_SECONDS = 600
_cache = {}


def _raw(rel, repo=None):
    # AIRGLOW_EFFECTS_URL points the gallery somewhere else, e.g. this checkout's effects/ (file:///...) for the dev
    # server, so effects can be tried before they're on GitHub
    base = os.environ.get("AIRGLOW_EFFECTS_URL") or f"https://raw.githubusercontent.com/{repo or UPDATE_REPO}/{BRANCH}/{FOLDER}"
    return f"{base.rstrip('/')}/{rel}"


def _get(url, limit, timeout=8):
    hit = _cache.get(url)
    if hit and time.monotonic() - hit[0] < CACHE_SECONDS:
        return hit[1]
    req = urllib.request.Request(url, headers={"User-Agent": f"Airglow/{__version__}"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            raw = resp.read(limit + 1)
    except urllib.error.HTTPError as e:
        raise RuntimeError("That isn't in the gallery (any more)." if e.code == 404 else f"GitHub answered {e.code}") from e
    except (urllib.error.URLError, OSError) as e:
        raise RuntimeError("Couldn't reach the gallery. Check your internet connection.") from e
    if len(raw) > limit:
        raise RuntimeError("The gallery sent more than expected.")
    _cache[url] = (time.monotonic(), raw)
    return raw


def clean_index(data):
    """effects/index.json -> {"effects": [...], "revoked": [...]}, dropping anything malformed."""
    items, revoked = [], []
    for e in data.get("effects", []) if isinstance(data, dict) else []:
        if not isinstance(e, dict) or not isinstance(e.get("id"), str) or not fxformat.EFFECT_ID.fullmatch(e["id"]):
            continue
        author, _, slug = e["id"].partition(".")
        if author == "me" or not re.fullmatch(r"[0-9a-f]{64}", str(e.get("sha256", ""))):
            continue
        items.append({"id": e["id"], "name": str(e.get("name", e["id"]))[:40], "author": author,
                      "description": str(e.get("description", ""))[:200],
                      "tags": [t for t in e.get("tags", []) if isinstance(t, str) and fxformat.TAG.fullmatch(t)][:5],
                      "kind": "code" if e.get("kind") == "code" else "layers",
                      "audio": bool(e.get("audio")), "sha256": e["sha256"], "path": f"{author}/{slug}.html",
                      "added": str(e.get("added", ""))[:10], "updated": str(e.get("updated", ""))[:10],
                      "featured": bool(e.get("featured")), "remix_of": str(e.get("remix_of") or "")[:90]})
    for r in data.get("revoked", []) if isinstance(data, dict) else []:
        if isinstance(r, dict) and isinstance(r.get("id"), str) and fxformat.EFFECT_ID.fullmatch(r["id"]):
            revoked.append({"id": r["id"], "sha256": [s for s in r.get("sha256", []) if isinstance(s, str)],
                            "reason": str(r.get("reason", ""))[:200]})
    items.sort(key=lambda e: e["updated"] or e["added"], reverse=True)          # newest first...
    items.sort(key=lambda e: not e["featured"])                                  # ...featured on top
    return {"effects": items, "revoked": revoked}


def listing():
    try:
        data = json.loads(_get(_raw("index.json"), 2_000_000).decode("utf-8"))
    except ValueError as e:
        raise RuntimeError("The gallery sent something Airglow couldn't read.") from e
    return clean_index(data)


def fetch(effect_id):
    """One effect's file from the gallery, checked against the index's SHA-256. Returns (source, index entry)."""
    entry = next((e for e in listing()["effects"] if e["id"] == effect_id), None)
    if entry is None:
        raise RuntimeError("That effect isn't in the gallery (any more).")
    source = _get(_raw(entry["path"]), fxformat.MAX_BYTES).decode("utf-8")
    import hashlib
    if hashlib.sha256(source.encode("utf-8")).hexdigest() != entry["sha256"]:
        _cache.pop(_raw(entry["path"]), None)
        raise RuntimeError("That effect changed since the gallery was listed. Try again in a few minutes.")
    return source, entry


MAX_URL = 8000          # GitHub answers 414 above about 8,190 characters (measured 2026-10-17)


def share_url(source, login, slug, repo=None):
    """GitHub's new-file page with the effect filled in; saving it there proposes a pull request. Returns
    (url, filled): an effect too long for the address gets the page with only the file name, to paste into."""
    base = f"https://github.com/{repo or UPDATE_REPO}/new/{BRANCH}/{FOLDER}/{login}?"
    url = base + urllib.parse.urlencode({"filename": f"{slug}.html", "value": source})
    if len(url) <= MAX_URL:
        return url, True
    return base + urllib.parse.urlencode({"filename": f"{slug}.html"}), False


def is_share_url(url, repo=None):
    return bool(re.fullmatch(rf"https://github\.com/{re.escape(repo or UPDATE_REPO)}/new/{BRANCH}/{FOLDER}/[a-z0-9\-]+\?.*",
                             url, re.DOTALL))
