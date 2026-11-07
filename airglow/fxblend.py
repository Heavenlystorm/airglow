"""Compile two saved effect snapshots into one portable, sandboxed code effect."""

import json
import math

from . import fxformat, paths
from .lighting import effects

MARKER = "Airglow saved-effects blend v1"


def _json(value):
    return json.dumps(value, ensure_ascii=True, separators=(",", ":"), allow_nan=False).replace("</", "<\\/")


def _endpoint(raw, slot):
    if not isinstance(raw, dict) or not isinstance(raw.get("source"), str):
        raise fxformat.FormatError(f"Choose a saved effect for {slot}.")
    eff = fxformat.validate(raw["source"])
    if MARKER in eff["source"]:
        raise fxformat.FormatError("A blend can contain two original effects, not another blend.")
    values = raw.get("values", {})
    if not isinstance(values, dict):
        raise fxformat.FormatError(f"{slot}'s control values must be an object.")
    try:
        for p in eff["schema"]:
            if p["type"] == "range" and p["id"] in values:
                value = values[p["id"]]
                if isinstance(value, bool) or not math.isfinite(float(value)):
                    raise ValueError(f"{p['label']} must be a finite number")
        values = effects.clean_values(eff["schema"], values)
    except (ValueError, TypeError, OverflowError) as error:
        raise fxformat.FormatError(f"{slot}: {error}") from None
    credit = {key: eff[key] for key in ("id", "name", "author", "license", "sha256")}
    if eff["kind"] == "layers":
        config = {"v": 2, "layers": eff["layers"], "master": eff["master"], "expose": eff["expose"]}
        code = f"airglow.effect(AirglowLayers.effect({_json(config)}, {{}}));"
    else:
        code = eff["code"]
    # Separate lexical scopes keep endpoint declarations and registrations independent. No eval is needed.
    compiled = f"endpoint((airglow) => {{\n{code}\n}}, {_json(values)}, {_json(credit)})"
    return eff, compiled


def build(body):
    if not isinstance(body, dict):
        raise fxformat.FormatError("A blend needs two saved effects.")
    amount = body.get("amount", 50)
    if isinstance(amount, bool) or not isinstance(amount, (int, float)) or not math.isfinite(amount) or not 0 <= amount <= 100:
        raise fxformat.FormatError("Blend must be a number from 0 to 100.")
    name = body.get("name", "Blended effect")
    if not isinstance(name, str) or not name.strip() or len(name.strip()) > 40:
        raise fxformat.FormatError("Name the blend using 1 to 40 characters.")
    a, code_a = _endpoint(body.get("a"), "A")
    b, code_b = _endpoint(body.get("b"), "B")
    if a["sha256"] == b["sha256"]:
        raise fxformat.FormatError("Choose two different effects for A and B.")
    template = (paths.WEB_DIR / "runtime" / "blend.js").read_text(encoding="utf-8")
    code = template.replace("/*__AIRGLOW_BLEND_ENDPOINTS__*/", code_a + ",\n" + code_b)
    return fxformat.to_html({
        "name": name.strip(), "description": f"Blend of {a['name']} and {b['name']}. Endpoint snapshots and credits are included.",
        "code": code, "permissions": sorted(set(a["permissions"] + b["permissions"])),
        "license": a["license"] if a["license"] == b["license"] else "",
        "params": [{"id": "blend", "label": "Blend toward B", "type": "number", "min": 0, "max": 100,
                    "step": 1, "default": amount}],
    })
