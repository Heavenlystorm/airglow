"""Check the community profiles in profiles/ and rebuild profiles/index.json.

    python tools/profiles.py --check    every profile is valid, named after its id, and unique (CI on pull requests)
    python tools/profiles.py --index    rewrite profiles/index.json (the gallery workflow, after a merge)
"""

import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

from airglow import profiles  # noqa: E402

FOLDER = ROOT / "profiles"
SKIP = {"index.json", "featured.json"}


def load_all():
    """[(path, profile)] for every profile file; raises SystemExit listing every problem."""
    out, problems, seen = [], [], set()
    for f in sorted(FOLDER.glob("*.json")):
        if f.name in SKIP:
            continue
        try:
            p = profiles.validate_shared(json.loads(f.read_text(encoding="utf-8")))
            if f.stem != p["id"]:
                raise ValueError(f"the file should be named {p['id']}.json")
            if p["id"] in seen:
                raise ValueError("another profile has the same id")
            if not p["author"]:
                raise ValueError("author is empty")
            seen.add(p["id"])
            out.append((f, p))
        except (ValueError, json.JSONDecodeError) as e:
            problems.append(f"{f.name}: {e}")
    featured = json.loads((FOLDER / "featured.json").read_text(encoding="utf-8")) if (FOLDER / "featured.json").exists() else []
    problems += [f"featured.json: no profile called {i}" for i in featured if i not in seen]
    if problems:
        raise SystemExit("\n".join(problems))
    return out


def added(path):
    """Date the file first appeared in git (today if it's not committed yet)."""
    r = subprocess.run(["git", "log", "--diff-filter=A", "--follow", "--format=%cs", "--", str(path)],
                       capture_output=True, text=True, cwd=ROOT)
    dates = r.stdout.split()
    return dates[-1] if dates else __import__("datetime").date.today().isoformat()


def build_index(items):
    return {"generated_by": "tools/profiles.py", "profiles": [
        {"id": p["id"], "name": p["name"], "author": p["author"], "description": p["description"],
         "badges": profiles.badges(p), "added": added(f)} for f, p in items]}


if __name__ == "__main__":
    items = load_all()
    if "--index" in sys.argv:
        (FOLDER / "index.json").write_text(json.dumps(build_index(items), indent=2, ensure_ascii=False) + "\n",
                                           encoding="utf-8", newline="\n")
        print(f"index: {len(items)} profiles")
    else:
        print(f"{len(items)} profiles OK")
