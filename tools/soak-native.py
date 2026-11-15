"""Mixed scene/preview soak: production controller and native renderers, synthetic devices and inputs.

No USB, real sensor server, screen/audio capture, installed app, or user settings are used.
--exe runs the packaged effect host; omit it to exercise the source entry point.
"""
import argparse
import base64
import hashlib
import json
import logging
import math
import os
from pathlib import Path
import shutil
import subprocess
import sys
import threading
import time
from types import SimpleNamespace

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
HOST_ASSETS = ("runtime/host.html", "runtime/frame.html", "runtime/frame.js", "runtime/worker.js",
               "runtime/layers.js", "runtime/layers.json", "js/fxhost.js", "js/fxrun.js")


def effect_host_exe(path):
    exe = path.resolve(strict=True)
    current = exe.parent / "current"
    if (exe.parent / ".portable").is_file() or (
            (exe.parent / "Update.exe").is_file() and (current / "sq.version").is_file()):
        # Velopack's stable shortcut launcher exits after starting the real executable.
        target = current / exe.name
        if not target.is_file():
            raise ValueError(f"Velopack layout has no effect-host executable at {target}")
        exe = target.resolve(strict=True)
        if not exe.is_relative_to(current.resolve(strict=True)):
            raise ValueError("The Velopack executable is outside its current directory")
    if not exe.is_file():
        raise ValueError(f"Effect-host executable is not a file: {exe}")
    return exe


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seconds", type=float, default=7200)
    parser.add_argument("--exe", type=Path)
    parser.add_argument("--output", type=Path, default=ROOT / "output/qualification/soak")
    args = parser.parse_args()
    try:
        exe = effect_host_exe(args.exe) if args.exe else None
        web = exe.parent / "_internal" / "web" if exe else ROOT / "web"
        if not all((web / asset).is_file() for asset in HOST_ASSETS):
            raise ValueError(f"The effect-host web assets are missing from {web}")
    except (OSError, ValueError) as exc:
        parser.error(str(exc))
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    stop_request = output / "stop.request"
    if stop_request.exists():
        parser.error(f"Choose a fresh output directory; this soak was stopped at {stop_request}")
    # Must precede every Airglow import: blocks external sockets, hardware and unapproved children.
    from tests import isolation
    from tests.fakes import FakeLightDevice, FakeManager, signals
    from airglow import api, config, fileio, fxformat, fxlibrary, paths, previewhost
    from airglow.controller import Controller
    from airglow.lighting import studio
    import numpy as np
    import psutil

    paths.WEB_DIR = web
    logging.basicConfig(filename=output / "soak.log", filemode="w", level=logging.INFO,
                        format="%(asctime)s %(levelname)s %(name)s %(message)s")
    result = {"scope": "Production controller, Store, engine, API and native WebView2 renderers; synthetic devices/inputs",
              "host_entry": str(exe or ROOT / "run.py"), "host_web": str(web), "passed": False, "stopped_early": False,
              "host_asset_sha256": {asset: hashlib.sha256((web / asset).read_bytes()).hexdigest() for asset in HOST_ASSETS},
              "switches": 0, "rapid_switches": 0,
              "preview_frames": 0, "samples": [], "errors": []}
    store = config.Store()  # tests bootstrap made a fresh temporary per-user data directory
    dev = FakeLightDevice("soak", "Synthetic rig", zones=tuple((f"z{i}", 24) for i in range(4)))
    manager = FakeManager(dev)
    manager.maintain(True)
    store.ensure_zones([dev.info()])
    lib = fxlibrary.Library(store.dir / "effects")
    fxlibrary.LIBRARY = lib
    made = [lib.save_mine(fxformat.to_html({"name": f"Soak {i}",
             "code": f"airglow.effect({{point(p,f){{return [128+127*Math.sin(f.time+{i}),128+127*Math.sin(f.time*.7+p.x*.03),128+127*Math.cos(f.time+p.y*.02)]}}}});"})) for i in range(5)]
    ctrl = Controller(store, manager=manager, read_sensors=lambda: {"cpu_temp": 60, "gpu_temp": 55}, signals=signals,
                      async_writes=True)
    ctrl.sensors = {"cpu_temp": 60, "gpu_temp": 55}
    sample = SimpleNamespace(bands=np.linspace(.1, .9, 64), bass=.6, level=.5, beat=False)
    ctrl.engine.audio = SimpleNamespace(frame=sample, active=True, error=None, touch=lambda: None, stop=lambda: None)
    ctrl.engine.screen = SimpleNamespace(frame=SimpleNamespace(sample=previewhost._sample_screen), active=True,
                                        error=None, touch=lambda **_: None, stop=lambda: None)

    class SoakHandler(api.Handler):
        def log_request(self, code="-", size="-"):
            path = getattr(self, "path", "").partition("?")[0]  # don't put the host token in evidence
            if path.startswith("/runtime/") or path in ("/js/fxhost.js", "/js/fxrun.js",
                                                         "/api/fx/host/stream", "/api/fx/preview/stream"):
                logging.info("host HTTP %s %s from loopback port %d", code, path, self.client_address[1])

    server = api.Server(0, ctrl, store)
    server.RequestHandlerClass = SoakHandler
    origin = f"http://127.0.0.1:{server.server_address[1]}"
    children = []
    host_log = output / "host-process.log"
    host_log.write_bytes(b"")

    def spawn(url):
        if not url.startswith(origin + "/runtime/host.html#token="):
            raise ValueError("The soak may launch only its own effect-host endpoint")
        command = [str(exe)] if exe else [sys.executable, "-B", str(ROOT / "run.py")]
        command += ["--effect-host", url, "--parent", str(os.getpid())]
        with host_log.open("ab") as log:
            child = isolation._fixture_process(subprocess.Popen, command, cwd=ROOT, creationflags=0x08000000,
                                               stdout=log, stderr=subprocess.STDOUT)
        children.append(child)
        return child

    studio.HOST = studio.EffectHost(lib, spawn=spawn)
    studio.HOST.url = origin
    studio._looked_up.clear()
    server.previews.close()
    server.previews = previewhost.PreviewHost(origin, spawn=spawn)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    started = time.monotonic()
    previous = None
    preview_id = None
    preview_leds = 24
    keys = list(store.zones())
    own = psutil.Process()
    owned = {}
    next_tree = started

    def remember_tree():
        for process in own.children(recursive=True):
            owned[process.pid] = process

    def host_snapshot(host):
        with host._lock:
            info, child = host.status(), host.proc
        if child is None:
            return info
        info.update(pid=child.pid, returncode=child.poll())
        try:
            process = psutil.Process(child.pid)
            info["rss"] = process.memory_info().rss
            info["workers"] = []
            for worker in process.children(recursive=True):
                try:
                    info["workers"].append({"pid": worker.pid, "name": worker.name(), "rss": worker.memory_info().rss})
                except psutil.Error:
                    continue
        except psutil.Error:
            pass
        return info

    def stop_children():
        """Collect surviving test workers by ancestry and the exact isolated WebView2 data paths."""
        data = store.dir.resolve()
        profiles = {data / name / "EBWebView" for name in ("effect-host", "preview-host")}
        for process in psutil.process_iter(("name", "cmdline")):
            if (process.info["name"] or "").casefold() != "msedgewebview2.exe":
                continue
            for arg in process.info["cmdline"] or ():
                if arg.startswith("--user-data-dir=") and Path(arg.partition("=")[2].strip('"')) in profiles:
                    owned[process.pid] = process
        alive = []
        for process in owned.values():
            if process.is_running():
                try:
                    process.kill()
                    alive.append(process)
                except psutil.NoSuchProcess:
                    pass
        _, alive = psutil.wait_procs(alive, timeout=6)
        if alive:
            raise RuntimeError(f"Isolated effect-host workers did not exit: {[process.pid for process in alive]}")
        for child in children:
            child.wait(timeout=2)

    def activate(index):
        nonlocal preview_id, preview_leds
        if preview_id:
            server.previews.route("stop", {"id": preview_id}); preview_id = None
        # Four simultaneous native effects on every fourth selection; no outgoing instances linger.
        four = index % 4 == 0
        for i, key in enumerate(keys):
            zone = store.zones()[key]
            zone["override"] = {"effect": made[i]["fx_id"], "params": {}} if four else None
            store.set_zone(key, zone, 48)
        ctrl.engine.invalidate()
        effect = made[index % 5]["fx_id"] if index % 4 == 1 else ("rainbow", "gradient", "spectrum", "temperature")[index % 4]
        ctrl.set_scene({"effect": effect, "brightness": 60 + index % 40, "on": True})
        if four:
            points, selections = [], []
            for i, key in enumerate(keys):
                cfg = store.zones()[key]
                zone = ctrl.engine._geometry(key, cfg["layout"], cfg)
                points.extend(studio._points(zone))
                selections.append({"key": key, "count": zone.n, "id": made[i]["fx_id"], "params": {}, "layout": cfg["layout"]})
            preview_leds = len(points)
            audition = {"kind": "scene", "id": effect, "brightness": 35, "zones": selections}
        else:
            points = [[i * 10, 100, None, i / 23, 0, "audition"] for i in range(24)]
            preview_leds = len(points)
            audition = {"kind": "code", "code": made[4]["code"], "params": {}}
        preview_id = server.previews.route("load", {"effect": audition, "points": points})["id"]
        result["switches"] += 1

    try:
        ctrl.engine.start()
        # Real native replies race this rapid sequence; stale frames must never activate an earlier scene.
        for i in range(100):
            ctrl.set_scene({"effect": made[i % 5]["fx_id"]})
            result["rapid_switches"] += 1
        activate(0)
        next_switch, next_sample, preview_at = started + 20, started + 1, 0
        while time.monotonic() - started < args.seconds:
            if stop_request.is_file():
                result["stopped_early"] = True
                logging.info("Soak stopped by %s", stop_request)
                break
            now = time.monotonic()
            if now >= next_tree:
                remember_tree()
                next_tree = now + 2
            state = ctrl.scene_status()
            if state["status"] == "failed":
                raise RuntimeError(state["error"])
            if not ctrl.engine.healthy:
                raise RuntimeError("Lighting engine stopped")
            if len(studio.HOST.instances) > 4:
                raise RuntimeError("Live renderer capacity exceeded")
            ctrl.sensors["cpu_temp"] = 55 + 15 * math.sin((now - started) / 20)
            sample.bass = .3 + .3 * math.sin((now - started) * 3)
            if now >= preview_at:
                frame = server.previews.route("tick", {"id": preview_id, "time": now - started, "dt": .1})
                if frame.get("problem"):
                    raise RuntimeError(str(frame["problem"]))
                if frame.get("colors"):
                    colors = base64.b64decode(frame["colors"])
                    if len(colors) != preview_leds * 3 or not any(colors):
                        raise RuntimeError("Invalid audition frame")
                    result["preview_frames"] += 1
                preview_at = now + .1
            if now >= next_switch:
                if state["status"] != "ready":
                    raise RuntimeError("Scene did not become ready before the next selection")
                activate(result["switches"])
                next_switch = now + 20
            if now >= next_sample:
                rss = own.memory_info().rss
                host_rss = 0
                for process in own.children(recursive=True):
                    try:
                        host_rss += process.memory_info().rss
                    except psutil.NoSuchProcess:
                        continue
                result["samples"].append({"seconds": round(now - started), "rss": rss, "native_rss": host_rss,
                                          "live_instances": len(studio.HOST.instances), "threads": own.num_threads()})
                result["elapsed"] = round(now - started, 2)
                fileio.atomic_write_text(output / "progress.json", json.dumps(result, indent=2))
                print(f"{result['elapsed']}s, {result['switches']} scenes, {result['preview_frames']} audition frames", flush=True)
                next_sample = now + 60
            # Fake devices retain their write history for assertions; bound it for an endurance run.
            if len(dev.writes) > 100:
                dev.writes[:] = dev.writes[-20:]
            previous = ctrl.engine.frames
            time.sleep(.03)
        if not result["stopped_early"]:
            if not previous or result["preview_frames"] < 10:
                raise RuntimeError("Insufficient rendered frames")
            result["passed"] = True
    except Exception as exc:
        result["errors"].append(f"{type(exc).__name__}: {exc}")
        logging.exception("Soak failed")
    finally:
        result["elapsed"] = round(time.monotonic() - started, 2)
        result["hosts_before_cleanup"] = {
            "live": host_snapshot(studio.HOST),
            "preview": host_snapshot(server.previews.host),
        }
        for step in (remember_tree, studio.HOST.close, ctrl.shutdown, server.shutdown, server.server_close, stop_children):
            try:
                step()
            except Exception as exc:
                result["errors"].append(f"Cleanup {step.__name__}: {exc}")
                logging.exception("Cleanup %s failed", step.__name__)
        thread.join(timeout=3)
        runtime_log = store.dir / "logs" / "effect-host.log"
        if runtime_log.is_file():
            try:
                shutil.copyfile(runtime_log, output / "effect-host.log")
            except OSError as exc:
                result["errors"].append(f"Could not preserve effect-host log: {exc}")
        if result["errors"]:
            result["passed"] = False
        fileio.atomic_write_text(output / "result.json", json.dumps(result, indent=2))
    print(json.dumps({key: value for key, value in result.items() if key != "samples"}, indent=2))
    return 0 if result["passed"] else 2 if result["stopped_early"] and not result["errors"] else 1


if __name__ == "__main__":
    sys.exit(main())
