"""Studio effects on this PC: yours (%APPDATA%\\Airglow\\effects\\mine) and installed community effects
(effects\\community), one HTML file each (airglow/fxformat.py), plus the draft the Studio is editing.

Every effect is known to the rest of Airglow as "fx:<file name>", e.g. "fx:me.aurora-wave" or
"fx:jane-dev.aurora-wave". Your own keep their file name when you rename them, so profiles and automations that use
them keep working.
"""

import hashlib
import logging
import os
import threading
from contextlib import contextmanager
from pathlib import Path

from . import fileio, fxformat, paths

log = logging.getLogger(__name__)

PREFIX = "fx:"
DRAFT = "fx:draft"
ORIGINS = ("mine", "community")


class Library:
    def __init__(self, root=None):
        self.root = Path(root) if root else paths.data_dir() / "effects"
        self._lock = threading.RLock()
        self._cache = {}              # path -> (mtime_ns, size, effect)
        self._draft = None
        self.rev = 0                  # bumps on every change, so the UI knows to reload the effect list

    @contextmanager
    def transaction(self):
        """Keep a reviewed local-library decision and its commit in one critical section."""
        with self._lock:
            yield self

    def _dir(self, origin, create=True):
        d = self.root / origin
        if create:
            d.mkdir(parents=True, exist_ok=True)
        return d

    def _read(self, path, origin):
        st = path.stat()
        hit = self._cache.get(path)
        if hit and hit[0] == st.st_mtime_ns and hit[1] == st.st_size:
            return hit[2]
        try:
            eff = fxformat.validate(path.read_text(encoding="utf-8"))
        except (OSError, UnicodeDecodeError, fxformat.FormatError) as e:
            log.warning("skipping effect %s: %s", path.name, e)
            eff = None
        else:
            eff = {**eff, "fx_id": PREFIX + path.stem, "origin": origin, "path": str(path)}
        self._cache[path] = (st.st_mtime_ns, st.st_size, eff)
        return eff

    # ------------------------------------------------------------ reading
    def all(self):
        """Every effect on this PC (yours first), newest change first within each."""
        out = []
        with self._lock:
            for origin in ORIGINS:
                d = self._dir(origin, create=False)
                files = sorted(d.glob("*.html"), key=lambda p: p.stat().st_mtime, reverse=True) if d.is_dir() else []
                # get() only accepts a stem that is a valid effect id, so listing anything else made a card that
                # could never be played or deleted ("Aurora.html" in the effects folder, or a stray "ME.FOO.HTML").
                files = [p for p in files if fxformat.EFFECT_ID.fullmatch(p.stem)]
                out += [e for e in (self._read(p, origin) for p in files) if e]
        return out

    def get(self, fx_id):
        """An effect by its fx: id, or None."""
        if fx_id == DRAFT:
            return self._draft
        if not isinstance(fx_id, str) or not fx_id.startswith(PREFIX):
            return None
        stem = fx_id[len(PREFIX):]
        if not fxformat.EFFECT_ID.fullmatch(stem):
            return None
        with self._lock:
            for origin in ORIGINS:
                path = self.root / origin / f"{stem}.html"
                if path.is_file():
                    return self._read(path, origin)
        return None

    # ------------------------------------------------------------ the Studio's draft
    def set_draft(self, source, based_on=None):
        """What the Studio is editing, so "Also on my lights" can play it before it's saved."""
        eff = fxformat.validate(source)
        with self._lock:
            self._draft = {**eff, "fx_id": DRAFT, "origin": "draft", "based_on": based_on}
        return self._draft

    def clear_draft(self):
        self._draft = None

    # ------------------------------------------------------------ your own
    def save_mine(self, source, fx_id=None):
        """Save one of yours: a new one (fx_id None) gets a file named after it; an existing one keeps its file."""
        eff = fxformat.validate(source)
        with self._lock:
            mine = self._dir("mine")
            if fx_id:
                old = self.get(fx_id)
                if not old or old["origin"] != "mine":
                    raise fxformat.FormatError("that isn't one of your effects")
                path = Path(old["path"])
            else:
                slug = fxformat.slug(eff["name"])
                stem, n = f"me.{slug}", 2
                while (mine / f"{stem}.html").exists() or (self.root / "community" / f"{stem}.html").exists():
                    suffix = f"-{n}"
                    stem, n = f"me.{slug[:40 - len(suffix)]}{suffix}", n + 1
                path = mine / f"{stem}.html"
            _write(path, source)
            self._cache.pop(path, None)   # a same-size edit inside one file-time tick keeps (mtime, size)
            self.rev += 1
            return self._read(path, "mine")

    def delete(self, fx_id):
        """Delete one of yours, or uninstall a community effect."""
        eff = self.get(fx_id)
        if not eff or eff["origin"] not in ORIGINS:
            raise fxformat.FormatError("there's no such effect")
        with self._lock:
            Path(eff["path"]).unlink(missing_ok=True)
            prev = Path(eff["path"] + ".prev")
            prev.unlink(missing_ok=True)
            self._cache.pop(Path(eff["path"]), None)
            self.rev += 1

    def install_pack(self, items):
        """Install a validated copy-only batch without publishing a partial library on ordinary failures.

        Community provenance/revocation checks belong to the caller; checksums are checked here too. Existing
        effects are never replaced. Mine copies receive new IDs, leaving every profile reference intact.
        """
        if not isinstance(items, list) or len(items) > 24:
            raise fxformat.FormatError("a scene pack holds up to 24 effects")
        validated = []
        for item in items:
            if not isinstance(item, dict) or item.get("origin") not in ORIGINS or item.get("fx_id"):
                raise fxformat.FormatError("scene packs may only install new mine or community effects")
            source = item.get("source")
            eff = fxformat.validate(source)
            if item["origin"] == "community":
                if not eff["author"] or eff["sha256"] != item.get("sha256"):
                    raise fxformat.FormatError("the community effect does not match its author and checksum")
            validated.append((item["origin"], source, eff, item.get("copy") is True))
        with self._lock:
            planned, stems = [], set()
            for origin, source, eff, copy in validated:
                if origin == "community":
                    stem = eff["id"]
                    if not copy and (stem in stems or any((self.root / folder / f"{stem}.html").exists() for folder in ORIGINS)):
                        raise fxformat.FormatError("that community effect is already installed; keep it or make a copy")
                    author, slug = eff["id"].split(".", 1)
                    n = 2
                    while stem in stems or any((self.root / folder / f"{stem}.html").exists() for folder in ORIGINS):
                        suffix = f"-{n}"
                        stem, n = f"{author}.{slug[:40 - len(suffix)]}{suffix}", n + 1
                else:
                    slug, n = fxformat.slug(eff["name"]), 2
                    stem = f"me.{slug}"
                    while stem in stems or any((self.root / folder / f"{stem}.html").exists() for folder in ORIGINS):
                        suffix = f"-{n}"
                        stem, n = f"me.{slug[:40 - len(suffix)]}{suffix}", n + 1
                stems.add(stem)
                planned.append((self.root / origin / f"{stem}.html", source, origin))
            written = []
            cache = dict(self._cache)
            try:
                for path, source, origin in planned:
                    self._dir(origin)
                    _write(path, source)
                    written.append(path)
                installed = [self._read(path, origin) for path, _, origin in planned]
                if any(eff is None for eff in installed):
                    raise OSError("an installed effect could not be read back")
            except Exception as error:
                self._cache = cache
                failures = []
                for path in reversed(written):
                    try:
                        path.unlink(missing_ok=True)
                    except OSError:
                        failures.append(path.name)
                if failures:
                    raise OSError(f"Scene pack failed ({error}); could not remove {', '.join(failures)}.") from error
                raise
            if installed:
                self.rev += 1
            return installed

    # ------------------------------------------------------------ community
    def install(self, source, sha256):
        """Install (or update) a community effect after checking it's exactly what the gallery index lists. The
        version it replaces is kept as .prev for one rollback."""
        if hashlib.sha256(source.encode("utf-8")).hexdigest() != sha256:
            raise fxformat.FormatError("the download doesn't match the gallery's checksum, so it wasn't installed")
        eff = fxformat.validate(source)
        if not eff["author"]:
            raise fxformat.FormatError("a community effect needs an author")
        with self._lock:
            path = self._dir("community") / f"{eff['id']}.html"
            if path.exists():
                pending, backup = Path(str(path) + ".next"), Path(str(path) + ".prev.next")
                promoted = False
                try:
                    # Prepare both files while the working effect and its rollback remain available.
                    _write(pending, source)
                    _write(backup, path.read_text(encoding="utf-8"))
                    os.replace(pending, path)
                    promoted = True
                    try:
                        os.replace(backup, str(path) + ".prev")
                    except OSError:
                        os.replace(backup, path)       # restore the active version if its rollback cannot be kept
                        raise
                finally:
                    pending.unlink(missing_ok=True)
                    if not promoted:
                        backup.unlink(missing_ok=True)
                    # If even restoration failed, leave .prev.next on disk for recovery.
                self._cache.pop(Path(str(path) + ".prev"), None)
            else:
                _write(path, source)
            self._cache.pop(path, None)
            self.rev += 1
            return self._read(path, "community")

    def apply_revocations(self, revoked):
        """Turn off installed community effects the gallery has revoked (all versions, or the listed ones).
        Include rollback copies; keep files as .revoked so nothing is lost. Returns active effects turned off."""
        out = []
        with self._lock:
            folder = self._dir("community", create=False)
            changed = False
            for path in [*folder.glob("*.html"), *folder.glob("*.html.prev")]:
                if not fxformat.EFFECT_ID.fullmatch(path.name.removesuffix(".prev").removesuffix(".html")):
                    continue
                eff = self._read(path, "community")
                if not eff:
                    continue
                hit = next((r for r in revoked if r["id"] == eff["id"] and (not r["sha256"] or eff["sha256"] in r["sha256"])), None)
                if hit:
                    os.replace(path, str(path) + ".revoked")
                    self._cache.pop(path, None)
                    changed = True
                    log.warning("community effect %s revoked: %s", eff["id"], hit["reason"])
                    if path.suffix == ".html":
                        out.append({"fx_id": eff["fx_id"], "name": eff["name"], "reason": hit["reason"]})
            if changed:
                self.rev += 1
        return out

    def rollback(self, fx_id):
        with self._lock:
            eff = self.get(fx_id)
            prev = Path(eff["path"] + ".prev") if eff else None
            if not prev or not prev.exists():
                raise fxformat.FormatError("there's no earlier version to go back to")
            os.replace(prev, eff["path"])
            self._cache.pop(Path(eff["path"]), None)
            self.rev += 1
            return self._read(Path(eff["path"]), eff["origin"])


def _write(path, text):
    fileio.atomic_write_text(path, text)


LIBRARY = None


def library():
    global LIBRARY
    if LIBRARY is None:
        LIBRARY = Library()
    return LIBRARY
