"""Run browser checks against an owned, isolated demo process; never use the installed application."""

import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parent.parent
BASE = "http://127.0.0.1:8790"
SUITES = {
    "telemetry": ["node", "--test", "tools/test-telemetry-ui.mjs"],
    "visual": ["node", "tools/test-visual.mjs"],
    "responsive": ["node", "--test", "tools/test-responsive-overview.mjs"],
    "roadmap": ["node", "--test", "tools/test-roadmap.mjs"],
    "personalization": ["node", "--test", "tools/test-personalization-browser.mjs"],
    "rig3d": ["node", "--test", "tools/test-rig-3d-browser.mjs"],
    "theatre-capture": ["node", "--test", "tools/test-theatre-capture.mjs"],
    "creative": ["node", "--test", "tools/test-creative-browser.mjs"],
    "studio-screen": ["node", "--test", "tools/test-studio-screen.mjs"],
    "blend-renderer": ["node", "tools/test-blend-renderer.mjs"],
    "overview-canvas": ["node", "--test", "tools/test-overview-canvas.mjs"],
    "overview-companions": ["node", "--test", "tools/test-overview-companions.mjs"],
    "overview-light": ["node", "--test", "tools/test-overview-light.mjs"],
    "desk-display": ["node", "--test", "tools/test-desk-display-browser.mjs"],
    "profile-library": ["node", "--test", "tools/test-profile-library.mjs"],
    "profile-capacity": ["node", "--test", "tools/test-profile-capacity.mjs"],
    "community-profiles": ["node", "--test", "tools/test-community-profile-navigation.mjs"],
    "first-use-workspaces": ["node", "--test", "tools/test-first-use-workspaces.mjs"],
    "evening-workflow": ["node", "--test", "tools/test-evening-workflow.mjs"],
    "split-workspace": ["node", "--test", "tools/test-split-workspace.mjs"],
    "wind-down": ["node", "--test", "tools/test-wind-down.mjs"],
    "windows-shortcut": ["node", "--test", "tools/test-window-shortcut.mjs"],
    "telemetry-sessions": ["node", "--test", "tools/test-session-lab.mjs"],
    "curve-sandbox": ["node", "--test", "tools/test-curve-sandbox.mjs"],
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--suite", choices=["all", *SUITES], default="all")
    parser.add_argument("--output", default="output/browser-checks")
    parser.add_argument("--serve-only", action="store_true", help="Keep the isolated fixture running for a focused check")
    args = parser.parse_args()
    output = (ROOT / args.output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    for port in (8790, 8791):
        with socket.socket() as probe:
            try:
                probe.bind(("127.0.0.1", port))
            except OSError:
                raise SystemExit(f"Port {port} is occupied. Stop the other test fixture first; no existing process was changed.")
    fixture = Path(tempfile.mkdtemp(prefix="fixture-", dir=output))
    env = os.environ.copy()
    env.update(APPDATA=str(fixture / "roaming"), TEMP=str(fixture), TMP=str(fixture),
               AIRGLOW_FIXTURE="1", AIRGLOW_FIXTURE_DATA=str(fixture / "data"), PYTHONUNBUFFERED="1")
    # Browser imports need the caller's Playwright install, not the fixture's empty APPDATA.
    check_env = os.environ.copy()
    check_env.update(AIRGLOW_FIXTURE_URL=BASE, AIRGLOW_VISUAL_OUTPUT=str(output / "screenshots"), AIRGLOW_ROADMAP_OUTPUT=str(output / "roadmap"),
                     AIRGLOW_RESPONSIVE_OUTPUT=str(output / "responsive"), AIRGLOW_TELEMETRY_OUTPUT=str(output / "telemetry"),
                     AIRGLOW_PERSONALIZATION_OUTPUT=str(output / "personalization"),
                     AIRGLOW_RIG3D_OUTPUT=str(output / "rig-3d"), AIRGLOW_CREATIVE_OUTPUT=str(output / "creative"))
    check_env["AIRGLOW_CANVAS_OUTPUT"] = str(output / "overview-canvas")
    check_env["AIRGLOW_COMPANIONS_OUTPUT"] = str(output / "overview-companions")
    check_env["AIRGLOW_CAPTURE_OUTPUT"] = str(output / "theatre-capture")
    check_env["AIRGLOW_LIGHT_OUTPUT"] = str(output / "overview-light")
    check_env["AIRGLOW_DESK_OUTPUT"] = str(output / "desk-display")
    check_env["AIRGLOW_SHORTCUT_OUTPUT"] = str(output / "windows-shortcut")
    check_env["AIRGLOW_WIND_DOWN_OUTPUT"] = str(output / "wind-down")
    check_env["AIRGLOW_LIBRARY_OUTPUT"] = str(output / "profile-library")
    check_env["AIRGLOW_SPLIT_OUTPUT"] = str(output / "split-workspace")
    check_env["AIRGLOW_SESSION_OUTPUT"] = str(output / "telemetry-sessions")
    check_env["AIRGLOW_CURVE_OUTPUT"] = str(output / "curve-sandbox")
    result = {"fixture": str(fixture), "checks": {}}
    with (output / "fixture.log").open("w", encoding="utf-8") as log:
        server = subprocess.Popen([sys.executable, "-u", "tools/dev_server.py", "--demo"], cwd=ROOT, env=env,
                                  stdout=log, stderr=subprocess.STDOUT,
                                  creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        result["pid"] = server.pid
        try:
            deadline = time.monotonic() + 45
            while time.monotonic() < deadline:
                if server.poll() is not None:
                    raise RuntimeError("Fixture exited during startup; see fixture.log")
                try:
                    with urllib.request.urlopen(BASE + "/api/settings", timeout=1) as response:
                        settings = json.load(response)
                    if settings["settings"]["lhm_url"] == "http://127.0.0.1:8791":
                        break
                except (OSError, urllib.error.URLError):
                    pass
                time.sleep(0.1)
            else:
                raise RuntimeError("Fixture did not become ready; see fixture.log")
            print(f"Isolated fixture ready at {BASE}; PID {server.pid}; data {fixture}", flush=True)
            if args.serve_only:
                return server.wait()
            for name in SUITES if args.suite == "all" else [args.suite]:
                with (output / f"{name}.log").open("w", encoding="utf-8") as check_log:
                    check = subprocess.run(SUITES[name], cwd=ROOT, env=check_env, stdout=check_log,
                                           stderr=subprocess.STDOUT, timeout=300)
                result["checks"][name] = check.returncode
                print(f"{name}: {'PASS' if check.returncode == 0 else 'FAIL'} ({output / (name + '.log')})", flush=True)
            return int(any(result["checks"].values()))
        finally:
            server.terminate()
            try:
                server.wait(timeout=10)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait(timeout=5)
            result["fixture_stopped"] = server.poll() is not None
            (output / "results.json").write_text(json.dumps(result, indent=2), encoding="utf-8")


if __name__ == "__main__":
    sys.exit(main())
