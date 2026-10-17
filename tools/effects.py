"""Check the community effects in effects/ and rebuild effects/index.json.

    python tools/effects.py --check                          every effect is valid, named right, and unique
    python tools/effects.py --check --author jane-dev --changed-since origin/main
                                                             ...and on a pull request, it only touches jane-dev's folder
    python tools/effects.py --scan [files]                   what a reviewer should look at (Markdown, for the run summary)
    python tools/effects.py --index                          rewrite effects/index.json (after a merge)

Each effect is effects/<github-login>/<slug>.html, where the author in the file is that login and the slug comes
from its title. Format and limits: airglow/fxformat.py. Moderation lists that maintainers edit by hand:
effects/featured.json (ids shown first) and effects/revoked.json (turned off on every PC).
"""

import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

from airglow import fxformat  # noqa: E402

FOLDER = ROOT / "effects"


def files():
    return sorted(p for p in FOLDER.glob("*/*.html"))


def check_file(path):
    """The effect in path, or raise ValueError saying what's wrong."""
    rel = path.relative_to(FOLDER).as_posix()
    login = path.parent.name
    try:
        eff = fxformat.validate(path.read_text(encoding="utf-8"))
    except (fxformat.FormatError, UnicodeDecodeError) as e:
        raise ValueError(f"{rel}: {e}") from None
    if not fxformat.LOGIN.fullmatch(login) or login == "me":
        raise ValueError(f"{rel}: the folder must be a GitHub user name")
    if eff["author"] != login:
        raise ValueError(f'{rel}: <meta name="author"> must be "{login}", the folder it\'s in')
    if path.stem != fxformat.slug(eff["name"]):
        raise ValueError(f"{rel}: name the file {fxformat.slug(eff['name'])}.html, after its title")
    if not eff["license"]:
        raise ValueError(f"{rel}: needs a licence (<meta name=\"license\" content=\"MIT\">)")
    if not eff["description"]:
        raise ValueError(f"{rel}: needs a description")
    return eff


def load_all():
    out, problems, seen = [], [], set()
    for f in files():
        try:
            eff = check_file(f)
            if eff["id"] in seen:
                raise ValueError(f"{f.relative_to(FOLDER).as_posix()}: another effect has the id {eff['id']}")
            seen.add(eff["id"])
            out.append((f, eff))
        except ValueError as e:
            problems.append(str(e))
    for name in ("featured.json", "revoked.json"):
        path = FOLDER / name
        if path.exists():
            data = json.loads(path.read_text(encoding="utf-8"))
            ids = data if name == "featured.json" else [r.get("id") for r in data]
            if name == "featured.json":
                problems += [f"featured.json: no effect called {i}" for i in ids if i not in seen]
    stray = [p.relative_to(FOLDER).as_posix() for p in FOLDER.rglob("*") if p.is_file()
             and p.suffix != ".html" and p.name not in ("README.md", "index.json", "featured.json", "revoked.json")]
    problems += [f"{s}: only .html effect files go in effects/" for s in stray]
    if problems:
        raise SystemExit("\n".join(problems))
    return out


def changed(since):
    r = subprocess.run(["git", "diff", "--name-only", "--diff-filter=ACMR", f"{since}...HEAD", "--", "effects"],
                       capture_output=True, text=True, cwd=ROOT, check=True)
    return [ROOT / line for line in r.stdout.split() if line.endswith(".html")]


def check_owner(paths, author):
    """On a pull request, every changed effect must be in the author's own folder."""
    author = author.lower()
    wrong = [p.relative_to(FOLDER).as_posix() for p in paths if p.parent.name != author]
    if wrong:
        raise SystemExit("\n".join(f"{w}: you can only add or change effects in effects/{author}/" for w in wrong))


def scan_report(paths):
    lines = []
    for p in paths:
        eff = fxformat.validate(p.read_text(encoding="utf-8"))
        flags = fxformat.scan(eff["source"])
        kind = "layers" if eff["kind"] == "layers" else "code"
        lines.append(f"### {eff['name']} (`{p.relative_to(ROOT).as_posix()}`)")
        lines.append(f"{kind}, {eff['bytes']} bytes, licence {eff['license']}"
                     + (", uses audio" if "audio" in eff["permissions"] else "")
                     + (f", remix of {eff['remix_of']}" if eff["remix_of"] else ""))
        lines += [f"- :warning: it {f}" for f in flags] or ["- nothing for the reviewer to look at in particular"]
        lines.append("")
    return "\n".join(lines) or "No effects changed."


def git_dates(path):
    r = subprocess.run(["git", "log", "--follow", "--format=%cs", "--", str(path)], capture_output=True, text=True, cwd=ROOT,
                       check=False)
    dates = r.stdout.split()
    today = __import__("datetime").date.today().isoformat()
    return (dates[-1] if dates else today), (dates[0] if dates else today)


def build_index(items):
    featured = json.loads((FOLDER / "featured.json").read_text(encoding="utf-8")) if (FOLDER / "featured.json").exists() else []
    revoked = json.loads((FOLDER / "revoked.json").read_text(encoding="utf-8")) if (FOLDER / "revoked.json").exists() else []
    effects = []
    for f, eff in items:
        added, updated = git_dates(f)
        effects.append({"id": eff["id"], "name": eff["name"], "author": eff["author"], "description": eff["description"],
                        "tags": eff["tags"], "kind": eff["kind"], "audio": "audio" in eff["permissions"],
                        "license": eff["license"], "remix_of": eff["remix_of"],
                        # the bytes exactly as GitHub serves them, which is what Airglow checks after downloading
                        "sha256": hashlib.sha256(f.read_bytes()).hexdigest(), "bytes": f.stat().st_size,
                        "added": added, "updated": updated, "featured": eff["id"] in featured})
    return {"generated_by": "tools/effects.py", "schema": 1, "effects": effects, "revoked": revoked}


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--author")
    ap.add_argument("--changed-since")
    ap.add_argument("--scan", nargs="*")
    ap.add_argument("--index", action="store_true")
    a = ap.parse_args(argv)
    if a.check:
        items = load_all()
        if a.author and a.changed_since:
            check_owner(changed(a.changed_since), a.author)
        print(f"{len(items)} effects are valid")
    if a.scan is not None:
        paths = [Path(p).resolve() for p in a.scan] if a.scan else ([*changed(a.changed_since)] if a.changed_since else files())
        print(scan_report(paths))
    if a.index:
        out = FOLDER / "index.json"
        out.write_text(json.dumps(build_index(load_all()), indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        print(f"wrote {out.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
