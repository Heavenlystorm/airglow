"""Studio effects: one self-contained HTML file per effect (docs/EFFECT-STUDIO.md).

    <title>Aurora Wave</title>
    <meta name="airglow-effect" content="1">
    <meta name="author" content="jane-dev">  ... description, tags, license, remix-of, permissions
    <meta property="speed" label="Speed" type="number" min="0.1" max="5" step="0.1" default="1">
    <script type="application/json" id="layers">[...]</script>        a builder effect, or
    <script> airglow.effect({ render(ctx, frame) { ... } }) </script>  a code effect

The same checks run in the app (saving, installing) and on pull requests to effects/ (tools/effects.py). Nothing
here runs effect code: effects only ever run inside the sandboxed runtime (web/runtime/).
"""

import hashlib
import html
import json
import re
from functools import lru_cache
from html.parser import HTMLParser

from . import lightsetup, paths
from .lighting import effects

API_VERSIONS = (1,)
MAX_BYTES = 32_000
MAX_PARAMS = 16
MAX_LAYERS = 12
MAX_TAGS = 5
CANVAS_W, CANVAS_H = 320, 200
PERMISSIONS = ("audio",)
# OSI-approved licences the gallery accepts (SPDX ids)
LICENSES = ("MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "MPL-2.0", "0BSD", "Unlicense",
            "GPL-2.0-or-later", "GPL-3.0-or-later", "LGPL-3.0-or-later", "AGPL-3.0-or-later")
ALLOWED_TAGS = {"html", "head", "body", "meta", "title", "script"}

LOGIN = re.compile(r"[a-z0-9](?:[a-z0-9]|-(?=[a-z0-9])){0,38}")        # a GitHub login, lowercased
SLUG = re.compile(r"[a-z0-9][a-z0-9\-]{0,39}")
EFFECT_ID = re.compile(rf"(?:{LOGIN.pattern}|me)\.{SLUG.pattern}")
PARAM_ID = re.compile(r"[a-zA-Z][a-zA-Z0-9_]{0,31}")
TAG = re.compile(r"[a-z0-9][a-z0-9\-]{0,19}")
_HEX = re.compile(r"#[0-9a-fA-F]{6}")


class FormatError(ValueError):
    pass


def slug(name):
    return re.sub(r"[^a-z0-9]+", "-", str(name).lower()).strip("-")[:40] or "effect"


def make_id(author, name):
    return f"{author or 'me'}.{slug(name)}"


@lru_cache(maxsize=1)
def layer_types():
    data = json.loads((paths.WEB_DIR / "runtime" / "layers.json").read_text(encoding="utf-8"))
    return data["types"], tuple(data["blends"])


# ---------------------------------------------------------------- reading the file

class _Reader(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.title, self.meta, self.props, self.scripts, self.errors = None, {}, [], [], []
        self._in, self._buf, self._attrs = None, [], {}

    def handle_starttag(self, tag, attrs):
        a = {k.lower(): (v if v is not None else "") for k, v in attrs}
        if tag not in ALLOWED_TAGS:
            self.errors.append(f"<{tag}> isn't allowed: an effect is only <meta>, <title> and <script> tags")
        elif tag == "meta":
            if "property" in a:
                self.props.append(a)
            elif "name" in a:
                self.meta[a["name"].lower()] = a.get("content", "")
        elif tag in ("title", "script"):
            if tag == "script" and "src" in a:
                self.errors.append("scripts can't load other files (src=...); put everything in the one file")
            self._in, self._buf, self._attrs = tag, [], a

    def handle_endtag(self, tag):
        if tag == self._in:
            text = "".join(self._buf)
            if tag == "title":
                self.title = text.strip()
            else:
                self.scripts.append((self._attrs, text))
            self._in = None

    def handle_data(self, data):
        if self._in:
            self._buf.append(data)
        elif data.strip():
            self.errors.append("text outside a tag isn't allowed")


def _text(value, name, longest, required=False):
    v = (value or "").strip()
    if len(v) > longest or (required and not v):
        raise FormatError(f"{name} must be {'1 to ' if required else 'up to '}{longest} characters")
    return v


def _number(a, key, name):
    try:
        return float(a[key])
    except (KeyError, TypeError, ValueError):
        raise FormatError(f"control '{name}' needs a number for {key}") from None


def parse_param(a):
    """One <meta property=...> control -> {"id", "label", "type", ...} in the file's own terms."""
    pid = a.get("property", "")
    if not PARAM_ID.fullmatch(pid):
        raise FormatError(f"control id '{pid}' must be letters, digits and _ (starting with a letter)")
    label = _text(a.get("label") or pid, f"the label of '{pid}'", 32, required=True)
    kind = a.get("type", "")
    out = {"id": pid, "label": label, "type": kind}
    if kind == "number":
        lo, hi = _number(a, "min", pid), _number(a, "max", pid)
        step = _number(a, "step", pid) if "step" in a else 1.0
        default = _number(a, "default", pid)
        if not lo < hi or step <= 0 or not lo <= default <= hi:
            raise FormatError(f"control '{pid}' needs min < max, a positive step and a default in between")
        out.update(min=lo, max=hi, step=step, default=default)
    elif kind == "color":
        if not _HEX.fullmatch(a.get("default", "")):
            raise FormatError(f"control '{pid}' needs a default colour like #12abef")
        out["default"] = a["default"].lower()
    elif kind == "boolean":
        if a.get("default", "false") not in ("true", "false"):
            raise FormatError(f"control '{pid}' needs default=\"true\" or \"false\"")
        out["default"] = a.get("default", "false") == "true"
    elif kind == "select":
        opts = [o.strip() for o in a.get("options", "").split(",") if o.strip()]
        if not 2 <= len(opts) <= 12 or any(len(o) > 32 for o in opts) or len(set(opts)) != len(opts):
            raise FormatError(f"control '{pid}' needs 2 to 12 different options, comma separated")
        default = a.get("default", opts[0])
        if default not in opts:
            raise FormatError(f"control '{pid}' has a default that isn't one of its options")
        out.update(options=opts, default=default)
    else:
        raise FormatError(f"control '{pid}' has type '{kind}'; use number, color, boolean or select")
    return out


def to_schema(params):
    """File controls -> the parameter schema built-in effects use, so the UI draws them the same way."""
    out = []
    for p in params:
        if p["type"] == "number":
            out.append(effects.p_range(p["id"], p["label"], p["default"], p["min"], p["max"], p["step"]))
        elif p["type"] == "color":
            out.append(effects.p_color(p["id"], p["label"], p["default"]))
        elif p["type"] == "boolean":
            out.append(effects.p_toggle(p["id"], p["label"], p["default"]))
        else:
            out.append(effects.p_choice(p["id"], p["label"], p["default"], [(o, o) for o in p["options"]]))
    return out


# ---------------------------------------------------------------- builder layers

def _layer_param_to_file(spec, pid, label):
    """A layer setting exposed as a control, in the file's terms."""
    kind = spec["type"]
    if kind == "range":
        return {"id": pid, "label": label, "type": "number", "min": float(spec["min"]), "max": float(spec["max"]),
                "step": float(spec["step"]), "default": float(spec["default"])}
    if kind == "color":
        return {"id": pid, "label": label, "type": "color", "default": spec["default"]}
    if kind == "toggle":
        return {"id": pid, "label": label, "type": "boolean", "default": bool(spec["default"])}
    if kind == "choice":
        return {"id": pid, "label": label, "type": "select", "options": [o["value"] for o in spec["options"]],
                "default": spec["default"]}
    raise FormatError(f"{spec['label']} can't be a control")


def clean_layers(raw):
    """Validate builder layers. Returns (layers, params): params are the settings marked "Show as a control"."""
    types, blends = layer_types()
    if not isinstance(raw, list) or not 1 <= len(raw) <= MAX_LAYERS:
        raise FormatError(f"a builder effect has 1 to {MAX_LAYERS} layers")
    layers, params = [], []
    for i, layer in enumerate(raw):
        if not isinstance(layer, dict) or layer.get("type") not in types:
            raise FormatError(f"layer {i + 1} has an unknown type")
        t = types[layer["type"]]
        try:
            values = effects.clean_values(t["params"], layer.get("params"))
        except ValueError as e:
            raise FormatError(f"layer {i + 1} ({t['name']}): {e}") from None
        opacity = layer.get("opacity", 100)
        if not isinstance(opacity, (int, float)) or not 0 <= opacity <= 100:
            raise FormatError(f"layer {i + 1}: opacity is 0 to 100")
        if layer.get("blend", "normal") not in blends:
            raise FormatError(f"layer {i + 1}: blend is one of {', '.join(blends)}")
        mask = layer.get("mask", "all")
        if mask != "all" and mask not in lightsetup.SPOTS:
            raise FormatError(f"layer {i + 1}: unknown area '{mask}'")
        expose = layer.get("expose") or {}
        if not isinstance(expose, dict):
            raise FormatError(f"layer {i + 1}: expose lists settings shown as controls")
        clean_expose = {}
        for key, label in expose.items():
            spec = next((s for s in t["params"] if s["id"] == key), None)
            if spec is None or spec["type"] == "colors":
                raise FormatError(f"layer {i + 1}: '{key}' can't be shown as a control")
            label = _text(label if isinstance(label, str) else spec["label"], "a control label", 32, required=True)
            clean_expose[key] = label
            params.append(_layer_param_to_file({**spec, "default": values[key]}, f"l{i}_{key}", label))
        layers.append({"type": layer["type"], "enabled": bool(layer.get("enabled", True)),
                       "opacity": int(opacity) if float(opacity).is_integer() else round(float(opacity), 1), "blend": layer.get("blend", "normal"), "mask": mask,
                       "params": values, "expose": clean_expose})
    if len(params) > MAX_PARAMS:
        raise FormatError(f"at most {MAX_PARAMS} settings can be shown as controls")
    return layers, params


def layers_need_audio(layers):
    types, _ = layer_types()
    return any(types[layer["type"]].get("audio") and layer["enabled"] for layer in layers)


# ---------------------------------------------------------------- the whole file

def validate(text):
    """Check an effect file. Returns the effect as a dict (with "source", the file itself); raises FormatError."""
    if not isinstance(text, str):
        raise FormatError("an effect is a text file")
    size = len(text.encode("utf-8"))
    if size > MAX_BYTES:
        raise FormatError(f"an effect can be at most {MAX_BYTES // 1000} KB (this one is {size // 1000} KB)")
    r = _Reader()
    try:
        r.feed(text)
        r.close()
    except Exception as e:  # html.parser is lenient, but never let a bad file through as a crash
        raise FormatError(f"couldn't read the file: {e}") from None
    if r.errors:
        raise FormatError(r.errors[0])
    m = r.meta
    try:
        api = int(m.get("airglow-effect", ""))
    except ValueError:
        raise FormatError('this isn\'t an Airglow effect (it needs <meta name="airglow-effect" content="1">)') from None
    if api not in API_VERSIONS:
        raise FormatError(f"this effect needs a newer Airglow (effect version {api})")
    name = _text(r.title, "the title", 40, required=True)
    author = (m.get("author") or "").strip().lower()
    if author and not LOGIN.fullmatch(author):
        raise FormatError("author must be a GitHub user name")
    license_ = (m.get("license") or "").strip()
    if license_ and license_ not in LICENSES:
        raise FormatError(f"licence must be one of {', '.join(LICENSES)}")
    tags = [t.strip().lower() for t in (m.get("tags") or "").split(",") if t.strip()]
    if len(tags) > MAX_TAGS or not all(TAG.fullmatch(t) for t in tags):
        raise FormatError(f"up to {MAX_TAGS} tags, each lowercase letters, digits and dashes")
    perms = [p.strip() for p in (m.get("permissions") or "").split(",") if p.strip()]
    if any(p not in PERMISSIONS for p in perms):
        raise FormatError("the only permission an effect can ask for is audio")
    remix_of = (m.get("remix-of") or "").strip()
    if remix_of and not (EFFECT_ID.fullmatch(remix_of) or remix_of in effects.EFFECTS):
        raise FormatError("remix-of must be an effect id like jane-dev.aurora-wave")

    data = [s for a, s in r.scripts if a.get("type", "").lower() == "application/json" and a.get("id") == "layers"]
    code = [s for a, s in r.scripts if a.get("type", "text/javascript").lower() in ("", "text/javascript", "module")]
    if len(data) + len(code) != len(r.scripts):
        raise FormatError('scripts are either JavaScript or <script type="application/json" id="layers">')
    if bool(data) == bool(code) or len(data) > 1 or len(code) > 1:
        raise FormatError("an effect has either one layers script (builder) or one JavaScript script (code)")
    if data:
        try:
            layers, params = clean_layers(json.loads(data[0]))
        except json.JSONDecodeError as e:
            raise FormatError(f"the layers aren't valid JSON: {e.msg}") from None
        if layers_need_audio(layers) and "audio" not in perms:
            perms.append("audio")
        code_text = None
    else:
        layers, code_text = None, code[0]
        params = [parse_param(a) for a in r.props]
        if len(params) > MAX_PARAMS:
            raise FormatError(f"an effect can have at most {MAX_PARAMS} controls")
        if len({p["id"] for p in params}) != len(params):
            raise FormatError("two controls share an id")
    return {"api": api, "id": make_id(author, name), "name": name, "author": author,
            "description": _text(m.get("description"), "the description", 200), "tags": tags,
            "license": license_, "remix_of": remix_of, "permissions": perms, "params": params,
            "schema": to_schema(params), "kind": "layers" if layers is not None else "code", "layers": layers,
            "code": code_text, "source": text, "sha256": hashlib.sha256(text.encode("utf-8")).hexdigest(),
            "bytes": size}


# ---------------------------------------------------------------- writing the file

def _attr(v):
    return html.escape(str(v), quote=True)


def _num(v):
    return str(int(v)) if float(v).is_integer() else str(v)


def to_html(eff):
    """Write an effect file from its parts (name, author, description, tags, license, remix_of, permissions and
    either layers or code + params). The result always passes validate()."""
    lines = ["<!doctype html>", '<meta charset="utf-8">', f"<title>{html.escape(eff['name'].strip())}</title>",
             f'<meta name="airglow-effect" content="{API_VERSIONS[-1]}">']
    for key, meta in (("author", "author"), ("description", "description"), ("license", "license"),
                      ("remix_of", "remix-of")):
        if eff.get(key):
            lines.append(f'<meta name="{meta}" content="{_attr(eff[key])}">')
    if eff.get("tags"):
        lines.append(f'<meta name="tags" content="{_attr(", ".join(eff["tags"]))}">')
    layers = eff.get("layers")
    params = eff.get("params") or []
    perms = list(eff.get("permissions") or [])
    if layers is not None:
        layers, params = clean_layers(layers)
        if layers_need_audio(layers) and "audio" not in perms:
            perms.append("audio")
    if perms:
        lines.append(f'<meta name="permissions" content="{_attr(", ".join(perms))}">')
    for p in params:
        a = f'property="{_attr(p["id"])}" label="{_attr(p["label"])}" type="{p["type"]}"'
        if p["type"] == "number":
            a += f' min="{_num(p["min"])}" max="{_num(p["max"])}" step="{_num(p["step"])}" default="{_num(p["default"])}"'
        elif p["type"] == "boolean":
            a += f' default="{"true" if p["default"] else "false"}"'
        elif p["type"] == "select":
            a += f' options="{_attr(",".join(p["options"]))}" default="{_attr(p["default"])}"'
        else:
            a += f' default="{_attr(p["default"])}"'
        lines.append(f"<meta {a}>")
    lines.append("")
    if layers is not None:
        body = ",\n".join(json.dumps(layer, ensure_ascii=False) for layer in layers).replace("</", "<\\/")
        lines.append(f'<script type="application/json" id="layers">\n[\n{body}\n]\n</script>')   # a layer per line
    else:
        code = (eff.get("code") or "").replace("</script", "<\\/script")
        lines.append(f"<script>\n{code.strip()}\n</script>")
    text = "\n".join(lines) + "\n"
    validate(text)
    return text


# ---------------------------------------------------------------- for reviewers

_FLAGS = [
    (re.compile(r"\bfetch\s*\("), "uses fetch() (blocked in the sandbox)"),
    (re.compile(r"XMLHttpRequest|WebSocket|EventSource|RTCPeerConnection|sendBeacon"), "tries to open a connection (blocked)"),
    (re.compile(r"\bimportScripts\s*\(|\bimport\s*\("), "tries to load other code (blocked)"),
    (re.compile(r"\beval\s*\(|\bnew\s+Function\b|\bFunction\s*\("), "builds code from text (eval or Function)"),
    (re.compile(r"\batob\s*\(|\\x[0-9a-f]{2}(?:\\x[0-9a-f]{2}){8,}", re.IGNORECASE), "contains encoded text"),
    (re.compile(r"\b(?:localStorage|indexedDB|caches|navigator\.storage)\b"), "tries to store data (blocked)"),
]


def scan(text):
    """Things a reviewer should look at. The sandbox stops all of them anyway; this makes intent visible."""
    flags = [msg for rx, msg in _FLAGS if rx.search(text)]
    if any(len(line) > 400 for line in text.splitlines()):
        flags.append("has very long lines (minified or generated code is hard to review)")
    return flags
