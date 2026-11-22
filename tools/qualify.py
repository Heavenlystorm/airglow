"""Run Airglow's source checks or qualify an already built Windows package."""

import argparse
from collections import deque
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
from zipfile import BadZipFile, ZipFile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
UNIT_TOOLS = (
    "overview", "readings", "ui-races", "ui-state", "effect-runtime",
    "studio-workflow", "workbenches", "personalization-motion", "light-graph",
)
IMPORT_CHECK = """
import pkgutil
from pathlib import Path
from tokenize import open as open_source
import tests
import airglow.app, airglow.window, airglow.effecthost, airglow.tray, airglow.api, airglow.controller
from airglow import plugins

for path in [Path("run.py"), *Path("airglow").rglob("*.py"), *Path("tools").rglob("*.py")]:
    with open_source(path) as source:
        compile(source.read(), str(path), "exec")
expected = {mod.name for mod in pkgutil.iter_modules(plugins.__path__) if not mod.name.startswith("_")}
loaded = {mod.__name__.rsplit(".", 1)[1] for mod in plugins.load()}
if not loaded or loaded != expected:
    raise SystemExit(f"Could not load device plugins: {sorted(expected - loaded)}")
print(f"Startup modules compiled and imported; {len(loaded)} device plugins loaded.")
"""


class CheckFailed(RuntimeError):
    pass


def run_check(output, name, command, env=None):
    log = output / f"{name}.log"
    print(f"Running {name} …", flush=True)
    with log.open("wb") as stream:
        result = subprocess.run(command, cwd=ROOT, env=env, stdout=stream, stderr=subprocess.STDOUT)
    if result.returncode:
        with log.open(encoding="utf-8", errors="replace") as stream:
            print("".join(deque(stream, maxlen=60)), end="", file=sys.stderr)
        raise CheckFailed(f"{name} exited with code {result.returncode}. See {log}")
    print(f"PASS {name} ({log})", flush=True)


def source_checks(output):
    python = [sys.executable, "-B"]
    run_check(output, "python-tests", [*python, "-m", "unittest", "discover", "-s", "tests", "-t", "."])
    run_check(output, "python-imports", [*python, "-c", IMPORT_CHECK])
    run_check(output, "effects-catalog", [*python, "tools/effects.py", "--check"])
    run_check(output, "profiles-catalog", [*python, "tools/profiles.py", "--check"])
    run_check(output, "js-imports", ["node", "--experimental-vm-modules", "tools/check-js.mjs"])
    for script in sorted((ROOT / "tools").glob("*.mjs")):
        run_check(output, f"syntax-{script.stem}", ["node", "--check", script])
    js_tests = [f"tools/test-{name}.mjs" for name in UNIT_TOOLS]
    js_tests.extend(test.relative_to(ROOT).as_posix() for test in sorted((ROOT / "tests").glob("test_*.mjs")))
    run_check(output, "js-tests", ["node", "--experimental-vm-modules", "--test", "--test-timeout=30000", *js_tests])


def browser_checks(output):
    env = dict(os.environ, AIRGLOW_PRODUCTION_UI_OUTPUT=str(output / "production-ui"))
    run_check(output, "production-ui", ["node", "--test", "--test-timeout=120000", "tools/test-production-ui.mjs"], env)
    run_check(output, "scene-audition", ["node", "--test", "--test-timeout=120000", "tools/test-scene-audition.mjs"])
    run_check(output, "browser-fixtures", [sys.executable, "-B", "tools/run-ui-checks.py", "--output", str(output / "fixtures")])


def source(output, part="all"):
    if part in ("checks", "all"):
        source_checks(output)
    if part in ("browser", "all"):
        browser_checks(output)


def executable(path):
    if not path.is_file():
        raise CheckFailed(f"Missing {path}. Build the package first.")
    with path.open("rb") as stream:
        if stream.read(2) != b"MZ":
            raise CheckFailed(f"{path} is not a Windows executable.")


def digest(path, algorithm="sha256"):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, algorithm).hexdigest()


def check_archive(path):
    with ZipFile(path) as archive:
        damaged = archive.testzip()
        if damaged:
            raise CheckFailed(f"{path.name} has a damaged entry: {damaged}")
        files = [f"/{name.casefold()}" for name in archive.namelist()]
        for required in ("airglow.exe", "web/index.html"):
            if not any(name.endswith("/" + required) for name in files):
                raise CheckFailed(f"{path.name} does not contain {required}.")


def release_assets(releases, version):
    from tools.release import channel

    track = channel(version)
    setup = releases / f"Airglow-{track}-Setup.exe"
    portable = releases / f"Airglow-{track}-Portable.zip"
    feed = releases / f"releases.{track}.json"
    executable(setup)
    data = json.loads(feed.read_text(encoding="utf-8-sig"))
    matches = [item for item in data["Assets"] if item["PackageId"] == "Airglow"
               and item["Version"] == version and str(item["Type"]).casefold() in ("full", "1")]
    if len(matches) != 1:
        raise CheckFailed(f"{feed.name} must list one full package for Airglow {version}.")
    asset = matches[0]
    filename = asset["FileName"]
    if Path(filename).name != filename or not filename.endswith(".nupkg"):
        raise CheckFailed("The release feed names an invalid full package.")
    full = releases / filename
    if full.stat().st_size != asset["Size"]:
        raise CheckFailed(f"{full.name} size differs from the release feed.")
    for key in ("SHA256", "SHA1"):
        if asset.get(key):
            if digest(full, key.lower()) != asset[key].lower():
                raise CheckFailed(f"{full.name} {key} differs from the release feed.")
            break
    else:
        raise CheckFailed(f"{feed.name} has no checksum for {full.name}.")
    check_archive(portable)
    check_archive(full)
    return (setup, portable, full, feed)


def package(output, dist):
    from airglow import __version__

    exe = dist / "Airglow" / "Airglow.exe"
    executable(exe)
    web = exe.parent / "_internal" / "web"
    for source_file in (ROOT / "web").rglob("*"):
        if not source_file.is_file():
            continue
        relative = source_file.relative_to(ROOT / "web")
        packed = web / relative
        if not packed.is_file() or packed.read_bytes() != source_file.read_bytes():
            raise CheckFailed(f"Packaged web asset differs from source: {relative}")
    assets = release_assets(dist / "releases", __version__)
    run_check(output, "native-packaged-host", [sys.executable, "-B", "tools/test-effect-host-native.py",
              "--exe", str(exe), "--result", str(output / "native-packaged-host.json")])
    (output / "SHA256SUMS.txt").write_text("".join(f"{digest(path)}  {path.name}\n" for path in assets), encoding="utf-8")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("stage", choices=("source", "package"), default="source", nargs="?")
    parser.add_argument("--part", choices=("all", "checks", "browser"), default="all", help="Run one source part in a separate CI job")
    parser.add_argument("--output", type=Path, help="Directory for this stage's evidence")
    parser.add_argument("--dist", type=Path, default=ROOT / "dist", help="Prebuilt dist directory for the package stage")
    args = parser.parse_args()
    if args.stage == "package" and args.part != "all":
        parser.error("--part only applies to the source stage")
    output = (args.output or ROOT / "output" / "qualification" / args.stage).resolve()
    try:
        output.mkdir(parents=True, exist_ok=True)
        if args.stage == "source":
            source(output, args.part)
        else:
            package(output, args.dist.resolve())
    except (CheckFailed, OSError, ValueError, KeyError, TypeError, BadZipFile) as error:
        print(f"Qualification failed: {error}", file=sys.stderr)
        return 1
    print(f"{args.stage.capitalize()} qualification ({args.part}) passed. Evidence: {output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
