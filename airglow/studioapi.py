"""The Studio's part of the local API (/api/studio/...): your effects, the draft on your lights, sharing, the
community gallery and the external editor. api.route() hands every /api/studio/ request here."""

import os
import re
import shutil
import subprocess
from pathlib import Path

from . import fxformat, fxgallery, fxlibrary
from .lighting import effects, studio


class StudioError(Exception):
    def __init__(self, status, message):
        super().__init__(message)
        self.status = status


def _summary(eff, with_source=True):
    """What the UI needs about an effect."""
    out = {k: eff.get(k) for k in ("fx_id", "id", "name", "author", "description", "tags", "license", "remix_of",
                                   "permissions", "params", "schema", "kind", "layers", "code", "origin", "sha256")}
    if with_source:
        out["source"] = eff["source"]
    return out


def _lib():
    return fxlibrary.library()


def _parts(body):
    """The Studio's builder state -> the parts fxformat.to_html takes."""
    tags = body.get("tags") or []
    if isinstance(tags, str):
        tags = [t.strip().lower() for t in tags.split(",") if t.strip()]
    parts = {"name": str(body.get("name") or "").strip() or "Untitled effect",
             "author": str(body.get("author") or "").strip().lower(),
             "description": str(body.get("description") or "").strip(), "tags": tags,
             "license": str(body.get("license") or "").strip(), "remix_of": str(body.get("remix_of") or "").strip()}
    if body.get("layers") is not None:
        parts["layers"] = body["layers"]
    else:
        parts["code"], parts["params"] = str(body.get("code") or ""), body.get("params") or []
    return parts


def _source(body):
    """A request carries either a whole file ("source") or builder parts."""
    if isinstance(body.get("source"), str):
        return body["source"]
    return fxformat.to_html(_parts(body))


def _documents():
    """The user's Documents folder (it can be moved, e.g. to OneDrive)."""
    import ctypes
    buf = ctypes.create_unicode_buffer(260)
    if ctypes.windll.shell32.SHGetFolderPathW(None, 5, None, 0, buf) == 0:          # CSIDL_PERSONAL
        return Path(buf.value)
    return Path.home() / "Documents"


def editor_dir():
    return _documents() / "Airglow" / "Effects"


def route(server, method, path, body, query):
    ctrl, hooks = server.ctrl, server.hooks
    try:
        return _route(ctrl, hooks, method, path, body or {}, query or {})
    except fxformat.FormatError as e:
        raise StudioError(400, str(e)) from None
    except RuntimeError as e:
        raise StudioError(502, str(e)) from None


def _route(ctrl, hooks, method, path, body, query):
    lib = _lib()
    if method == "GET":
        if path == "/api/studio/effects":
            return {"rev": lib.rev, "effects": [_summary(e) for e in lib.all()], "host": studio.HOST.status()}
        if path == "/api/studio/gallery":
            data = fxgallery.listing()
            data["revoked_now"] = lib.apply_revocations(data["revoked"])
            installed = {e["id"]: e["sha256"] for e in lib.all() if e["origin"] == "community"}
            for e in data["effects"]:
                e["installed"] = e["id"] in installed
                e["update"] = e["installed"] and installed[e["id"]] != e["sha256"]
            return data
        if path == "/api/studio/gallery/effect":
            source, entry = fxgallery.fetch((query.get("id") or [""])[0])
            return {"entry": entry, "effect": _summary({**fxformat.validate(source), "fx_id": None, "origin": "gallery"})}
        if path == "/api/studio/editor":
            name = (query.get("file") or [""])[0]
            if not re.fullmatch(r"[a-z0-9\-.]{1,60}\.html", name):
                raise StudioError(400, "bad file name")
            f = editor_dir() / name
            if not f.is_file():
                raise StudioError(404, "The file isn't there any more.")
            return {"mtime": f.stat().st_mtime_ns, "source": f.read_text(encoding="utf-8")}
        raise StudioError(404, "not found")

    if path == "/api/studio/build":                  # builder parts -> a checked effect file
        source = _source(body)
        return {"source": source, "effect": _summary({**fxformat.validate(source), "fx_id": None, "origin": "draft"}, False)}
    if path == "/api/studio/check":
        return {"effect": _summary({**fxformat.validate(_source(body)), "fx_id": None, "origin": "draft"}, False),
                "flags": fxformat.scan(_source(body))}
    if path == "/api/studio/draft":                  # "Also on my lights"
        if not body.get("lights"):
            if ctrl.trial and ctrl.trial.get("kind") == "studio":
                ctrl.end_trial()
            lib.clear_draft()
            return {"lights": False}
        eff = lib.set_draft(_source(body), based_on=body.get("fx_id"))
        studio.HOST.retry(fxlibrary.DRAFT)
        params = effects.clean_values(eff["schema"], body.get("values"))
        scene = {"effect": fxlibrary.DRAFT, "params": params,
                 "brightness": ctrl.base_scene.get("brightness", 100), "on": True}
        ctrl.try_profile(scene, f"Studio: {eff['name']}", kind="studio")
        return {"lights": True}
    if path == "/api/studio/save":
        eff = lib.save_mine(_source(body), body.get("fx_id") or None)
        studio.HOST.retry(eff["fx_id"])
        return {"effect": _summary(eff)}
    if path == "/api/studio/delete":
        fx_id = str(body.get("fx_id"))
        if ctrl.base_scene.get("effect") == fx_id:
            raise StudioError(409, "That effect is on your lights right now. Pick another one first.")
        lib.delete(fx_id)
        return {"ok": True}
    if path == "/api/studio/use":
        fx_id = str(body.get("fx_id"))
        eff = lib.get(fx_id)
        if not eff:
            raise StudioError(404, "There's no such effect.")
        if ctrl.trial and ctrl.trial.get("kind") == "studio":
            ctrl.end_trial()
        studio.HOST.retry(fx_id)
        ctrl.set_scene({"effect": fx_id, "params": effects.clean_values(eff["schema"], body.get("values")), "on": True})
        return {"scene": ctrl.base_scene}
    if path == "/api/studio/retry":
        studio.HOST.retry(body.get("fx_id"))
        return {"ok": True}
    if path == "/api/studio/share":
        login = str(body.get("author") or "").strip().lower().lstrip("@")
        if not fxformat.LOGIN.fullmatch(login) or login == "me":
            raise StudioError(400, "Add your GitHub user name, so the gallery can credit you.")
        lic = str(body.get("license") or "MIT")
        if lic not in fxformat.LICENSES:
            raise StudioError(400, "Pick an open-source licence.")
        eff = lib.get(str(body.get("fx_id")))
        if not eff:
            raise StudioError(404, "Save the effect first.")
        parts = {"name": eff["name"], "author": login, "description": str(body.get("description") or eff["description"]).strip(),
                 "tags": body.get("tags") if isinstance(body.get("tags"), list) else eff["tags"],
                 "license": lic, "remix_of": eff["remix_of"], "permissions": eff["permissions"]}
        if not parts["description"]:
            raise StudioError(400, "Add a short description: what does it look like?")
        if eff["kind"] == "layers":
            parts["layers"] = eff["layers"]
        else:
            parts["code"], parts["params"] = eff["code"], eff["params"]
        source = fxformat.to_html(parts)
        slug = fxformat.slug(eff["name"])
        url, filled = fxgallery.share_url(source, login, slug)
        if body.get("open", True) and hooks.get("open_url") and fxgallery.is_share_url(url):
            hooks["open_url"](url)
        return {"url": url, "filled": filled, "source": source, "path": f"effects/{login}/{slug}.html",
                "flags": fxformat.scan(source)}
    if path == "/api/studio/open-share":            # after the page put a long effect on the clipboard
        url = str(body.get("url"))
        if not fxgallery.is_share_url(url):
            raise StudioError(400, "not a share address")
        if hooks.get("open_url"):
            hooks["open_url"](url)
        return {"ok": True}
    if path == "/api/studio/install":
        source, entry = fxgallery.fetch(str(body.get("id")))
        if any(r["id"] == entry["id"] and (not r["sha256"] or entry["sha256"] in r["sha256"]) for r in fxgallery.listing()["revoked"]):
            raise StudioError(409, "That effect was taken down, so it can't be installed.")
        eff = lib.install(source, entry["sha256"])
        studio.HOST.retry(eff["fx_id"])
        return {"effect": _summary(eff)}
    if path == "/api/studio/rollback":
        return {"effect": _summary(lib.rollback(str(body.get("fx_id"))))}
    if path == "/api/studio/editor":                 # open the effect in your own editor; the Studio reloads on save
        source = _source(body)
        eff = fxformat.validate(source)
        folder = editor_dir()
        folder.mkdir(parents=True, exist_ok=True)
        f = folder / f"{fxformat.slug(eff['name'])}.html"
        f.write_text(source, encoding="utf-8", newline="\n")
        code = shutil.which("code")
        try:
            if code:
                subprocess.Popen([code, str(f)], creationflags=0x08000000)
            else:
                os.startfile(str(folder))  # noqa: S606 - our own folder in Explorer
        except OSError:
            pass
        return {"file": f.name, "path": str(f), "editor": "VS Code" if code else "Explorer",
                "mtime": f.stat().st_mtime_ns}
    raise StudioError(404, "not found")
