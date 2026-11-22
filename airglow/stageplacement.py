"""Scope Stage Arrange writes to zone centres and orientation."""

from .lighting.geometry import CANVAS_H, CANVAS_W


class PlacementConflict(ValueError):
    pass


def clean_placements(body):
    items = body.get("placements")
    if not isinstance(items, list) or not items:
        raise ValueError("Choose at least one zone to arrange.")
    clean = {}
    for item in items:
        if not isinstance(item, dict):
            raise ValueError("Invalid zone placement.")
        key = item.get("key")
        if not isinstance(key, str) or not key or key in clean:
            raise ValueError("Each placement needs a different zone.")
        poses = []
        for which in ("expected", "position"):
            pose = item.get(which)
            if not isinstance(pose, dict) or set(pose) != {"x", "y", "angle"}:
                raise ValueError("A placement needs X, Y and angle.")
            for axis, low, high in (("x", -.2, CANVAS_W + .2), ("y", -.2, CANVAS_H + .2), ("angle", -360, 360)):
                value = pose[axis]
                if type(value) not in (int, float) or not low <= value <= high:
                    raise ValueError(f"Placement {axis} must be between {low} and {high}.")
            poses.append(dict(pose))
        clean[key] = tuple(poses)
    return clean


def merge_placements(zones, changes):
    updated = {}
    for key, (expected, position) in changes.items():
        cfg = zones.get(key)
        if not cfg:
            raise PlacementConflict("An arranged zone is no longer available. Cancel and reopen Arrange.")
        current = {axis: cfg["layout"][axis] for axis in position}
        if current == position:  # Replaying an acknowledged write after a lost response is harmless.
            continue
        if current != expected:
            raise PlacementConflict(f'{cfg["name"]} moved during this edit. Cancel and reopen Arrange to use the latest position.')
        updated[key] = {**cfg, "layout": {**cfg["layout"], **position}}
    return updated
