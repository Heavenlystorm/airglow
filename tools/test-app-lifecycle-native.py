"""Qualify actual Windows app restart handoffs without touching installed Airglow.

Runs app.main, the real hidden Window/WebView2, native tray and HTTP server, controller loops,
Tray._restart, exact-process handoff wait, and app cleanup. Only the fixture process command
may spawn from Python; devices, sensors, capture, updater and startup registration are isolated.
Ten restarts means eleven complete application processes. No reboot or installed app action.
"""

import argparse
import ctypes
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import threading
import time
import traceback
import uuid

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
CREATE_NO_WINDOW = 0x08000000
RESERVED_PORTS = {8787, 8790, 8791}


def is_protected(args, pid, created):
    return (args.protected_pid is not None and pid == args.protected_pid and
            args.protected_started is not None and abs(created - args.protected_started) < .001)


def protection_args(args):
    return (["--protected-pid", str(args.protected_pid), "--protected-started", str(args.protected_started)]
            if args.protected_pid is not None else [])


def write_json(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + f".{os.getpid()}.tmp")
    temporary.write_text(json.dumps(value, indent=2), encoding="utf-8")
    for attempt in range(40):
        try:
            temporary.replace(path)
            return
        except PermissionError:
            if attempt == 39:
                raise
            time.sleep(.025)


def color_for(generation):
    return f"#{(generation * 19 + 31) % 220 + 20:02x}507b"


def child(args):
    # This installs the existing hardware, subprocess, external-network and registry-write guards
    # before any production application module is imported.
    import tests  # noqa: F401
    from tests import isolation
    from tests.fakes import FakeLightDevice, FakeManager, FakeNzxt, signals
    from airglow import app, autostart, install, paths, system, tools, updates
    from airglow import tray as tray_module, window as window_module
    from airglow.config import Store
    from airglow.controller import Controller
    import psutil
    import urllib.request

    run_dir = Path(args.run_dir).resolve()
    data = run_dir / "data"
    data.mkdir(parents=True, exist_ok=True)
    paths.data_dir = lambda: data
    app.MUTEX_NAME = args.mutex
    app.PORT_TRIES = 1
    os.environ["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = "--disable-background-networking --disable-component-update --no-first-run"
    evidence = {"generation": args.generation, "pid": os.getpid(), "started_at": system.current_process_started_at(),
                "events": [], "blocked_operations": [], "errors": [], "scope": "real hidden app Window and tray; fixture hardware"}
    report = run_dir / f"child-{args.generation:02d}.json"
    lock = threading.Lock()

    def record(event, **fields):
        with lock:
            evidence["events"].append({"event": event, "at": time.time(), **fields})
            write_json(report, evidence)

    original_blocked = isolation._blocked

    def blocked(operation):
        evidence["blocked_operations"].append(operation)
        write_json(report, evidence)
        return original_blocked(operation)

    isolation._blocked = blocked
    app.message_box = lambda message: (_ for _ in ()).throw(RuntimeError(message))
    install.run_hooks = lambda: record("install_hooks_suppressed")
    autostart.repair = lambda: record("autostart_repair_suppressed")
    updates.apply_pending = lambda argv: record("pending_update_suppressed")
    updates.Updater.start = lambda self: record("updater_start_suppressed")
    Controller._warm_up_inputs = staticmethod(lambda: None)

    if args.generation == 0:
        seed = Store(data)
        seed._save_config({"port": args.port})
        seed.update_settings({"online_checks": False, "openrgb_enabled": False,
                              "dynamic_lighting_enabled": False, "lighting_transition_ms": 0})
        seed.save_state(scene={"effect": "solid", "params": {"color": color_for(-1)}, "brightness": 73, "on": True})

    objects = {}

    class FixtureController(Controller):
        def __init__(self, store, **kwargs):
            light, fan = FakeLightDevice("native-qualification", "Qualification LEDs"), FakeNzxt()
            manager = FakeManager(light, fan)
            manager.maintain(False)
            super().__init__(store, manager=manager, read_sensors=lambda: {"online": True, "cpu_temp": 48,
                             "gpu_temp": 44, "cpu_power": 35, "gpu_power": 20}, read_flat=lambda: [],
                             signals=signals, start_power_events=False, async_writes=True)
            self._system.read = lambda: {"ram_load": 30, "cpu_load": 20}
            objects["ctrl"], objects["light"] = self, light
            record("controller_created", saved_scene=self.base_scene)

    app.Controller = FixtureController
    original_start_server = app.start_server

    def start_server(*arguments, **kwargs):
        server = original_start_server(*arguments, **kwargs)
        if server:
            objects["server"] = server
            record("api_created", port=server.server_address[1], mutex_owned=bool(app._mutex))
        return server

    app.start_server = start_server
    original_wait = system.wait_for_process_exit

    def wait_for_exit(pid, started_at, timeout):
        if is_protected(args, pid, started_at / 10_000_000 - 11644473600):
            raise AssertionError("Refusing to wait on the installed Airglow")
        record("handoff_wait_started", previous_pid=pid, previous_started_at=started_at)
        outcome = original_wait(pid, started_at, timeout)
        record("handoff_wait_finished", previous_pid=pid, previous_started_at=started_at, success=outcome)
        return outcome

    system.wait_for_process_exit = wait_for_exit
    original_window = window_module.Window

    class ObservedWindow(original_window):
        def __init__(self, *arguments, **kwargs):
            kwargs["start_hidden"] = True
            super().__init__(*arguments, **kwargs)
            objects["window"] = self
            self.win.events.loaded += lambda: record("native_page_loaded")

    window_module.Window = ObservedWindow
    original_popen = subprocess.Popen

    def next_command(extra):
        if args.generation >= args.restarts:
            raise AssertionError("Final generation cannot restart")
        if len(extra) != 3 or extra[0] != "--restart-from-pid" or int(extra[1]) != os.getpid():
            raise AssertionError("Unexpected restart command")
        if int(extra[2]) != evidence["started_at"]:
            raise AssertionError("Restart did not pass exact process creation time")
        return sys.executable, ["-B", str(Path(__file__).resolve()), "--child", "--run-dir", str(run_dir),
                                "--generation", str(args.generation + 1), "--restarts", str(args.restarts),
                                "--port", str(args.port), "--mutex", args.mutex, *protection_args(args), *extra]

    tools.self_command = next_command

    def fixture_spawn(command, **kwargs):
        if not isinstance(command, list) or command[:3] != [sys.executable, "-B", str(Path(__file__).resolve())]:
            raise AssertionError(f"Unexpected Python subprocess: {command!r}")
        if "--child" not in command or str(run_dir) not in command:
            raise AssertionError("Restart child is not bound to this fixture")
        proc = isolation._fixture_process(original_popen, command, **kwargs)
        record("restart_child_spawned", child_pid=proc.pid, child_started=psutil.Process(proc.pid).create_time(), command=command)
        return proc

    # Tray uses the subprocess module shared by other Python code; any unrelated Popen is rejected.
    subprocess.Popen = fixture_spawn
    original_tray_start = tray_module.Tray.start

    def request(path, body=None):
        req = urllib.request.Request(f"http://127.0.0.1:{args.port}{path}",
                                     data=json.dumps(body).encode() if body is not None else None,
                                     headers={"X-Airglow": "1", "Content-Type": "application/json"})
        with urllib.request.urlopen(req, timeout=5) as response:
            return json.load(response)

    def exercise(tray):
        try:
            deadline = time.monotonic() + args.start_timeout
            while time.monotonic() < deadline:
                window = objects.get("window")
                if window and window.win.events.loaded.is_set():
                    break
                time.sleep(.05)
            else:
                raise AssertionError("Native WebView2 page did not load")
            native = window.win.evaluate_js("({ready:document.readyState, title:document.title, hasPower:!!document.querySelector('#power')})")
            if not native or not native.get("hasPower"):
                raise AssertionError(f"Native app page did not initialize: {native!r}")
            hwnd = window.win.native.Handle.ToInt64()
            native_visible = bool(ctypes.windll.user32.IsWindowVisible(ctypes.c_void_p(hwnd)))
            if native_visible:
                raise AssertionError("Qualification app window unexpectedly became visible")
            state = request("/api/state")
            expected = color_for(args.generation - 1)
            if state["scene"]["params"]["color"] != expected:
                raise AssertionError(f"Saved scene lost across restart: expected {expected}, got {state['scene']}")
            response = request("/api/scene", {"effect": "solid", "params": {"color": color_for(args.generation)},
                                              "brightness": 73, "on": True})
            if response["scene_status"]["status"] != "ready":
                raise AssertionError(f"Solid fixture scene not ready: {response}")
            deadline = time.monotonic() + 5
            while not objects["light"].writes and time.monotonic() < deadline:
                time.sleep(.05)
            if not objects["light"].writes:
                raise AssertionError("Actual engine/writer produced no fixture output")
            descendants = [{"pid": p.pid, "started": p.create_time(), "name": p.name()}
                           for p in psutil.Process().children(recursive=True)]
            if not any("msedgewebview2" in p["name"].lower() for p in descendants):
                raise AssertionError("No actual WebView2 child process observed")
            record("qualified_ready", native=native, native_visible=native_visible,
                   retained_color=expected, saved_color=color_for(args.generation),
                   scene_status=response["scene_status"], api_port=args.port, mutex_owned=bool(app._mutex),
                   descendants=descendants, fake_writes=len(objects["light"].writes))
            time.sleep(.15)
            if args.generation < args.restarts:
                # Exercise the exact production tray callback, not a synthetic direct Popen/quit sequence.
                tray._restart(None, None)
                tray._restart(None, None)  # duplicate request must not spawn a second application
            else:
                record("final_quit_requested")
                tray.quit_app()
        except BaseException:
            evidence["errors"].append(traceback.format_exc())
            write_json(report, evidence)
            tray.quit_app()

    def start_tray(tray):
        original_tray_start(tray)
        objects["tray"] = tray
        threading.Thread(target=exercise, args=(tray,), name="qualification", daemon=True).start()

    tray_module.Tray.start = start_tray
    argv = ["--background"]
    if args.restart_from_pid:
        argv = ["--restart-from-pid", *args.restart_from_pid, "--background"]
    record("app_main_entered", argv=argv, data_dir=str(data), mutex=args.mutex)
    code = app.main(argv)
    server, ctrl, tray = objects.get("server"), objects.get("ctrl"), objects.get("tray")
    clean = bool(code == 0 and not app._mutex and server and server.socket.fileno() == -1 and ctrl and
                 ctrl._stop.is_set() and not ctrl.healthy and tray and tray._stop.is_set() and not install.running_marker().exists())
    record("app_main_returned", code=code, cleanup_verified=clean, mutex_released=not bool(app._mutex),
           api_socket_closed=bool(server and server.socket.fileno() == -1),
           controller_stopped=bool(ctrl and ctrl._stop.is_set()), running_marker_cleared=not install.running_marker().exists())
    # Match run.py: library threads are not joined after production app.main cleanup.
    sys.stdout.flush()
    sys.stderr.flush()
    os._exit(0 if clean and not evidence["errors"] and not evidence["blocked_operations"] else 1)


def parent(args):
    import psutil
    if os.name != "nt":
        raise RuntimeError("This qualification requires Windows and actual WebView2")
    token = uuid.uuid4().hex[:12]
    result_path = Path(args.result).resolve()
    run_dir = result_path.parent / f"native-app-lifecycle-{token}"
    run_dir.mkdir(parents=True)
    mutex = rf"Local\AirglowQualification-{token}"
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    if port in RESERVED_PORTS:
        raise AssertionError("Refusing a reserved port")
    # Capture the optional live process identity before launching any fixture. A missing/inaccessible
    # explicitly protected process fails here instead of silently disabling its protection.
    args.protected_started = psutil.Process(args.protected_pid).create_time() if args.protected_pid is not None else None
    command = [sys.executable, "-B", str(Path(__file__).resolve()), "--child", "--run-dir", str(run_dir),
               "--generation", "0", "--restarts", str(args.restarts), "--port", str(port), "--mutex", mutex,
               "--start-timeout", str(args.start_timeout), *protection_args(args)]
    started = time.monotonic()
    timed_out = False
    with (run_dir / "process-output.log").open("w", encoding="utf-8") as output:
        first = subprocess.Popen(command, cwd=ROOT, stdout=output, stderr=output, creationflags=CREATE_NO_WINDOW)
        first_started = psutil.Process(first.pid).create_time()
        deadline = started + args.timeout
        while time.monotonic() < deadline:
            final = run_dir / f"child-{args.restarts:02d}.json"
            if final.exists():
                state = json.loads(final.read_text(encoding="utf-8"))
                if any(e["event"] == "app_main_returned" for e in state["events"]):
                    break
            reports = [json.loads(p.read_text(encoding="utf-8")) for p in sorted(run_dir.glob("child-*.json"))]
            if any(r.get("errors") or r.get("blocked_operations") or any(e["event"] == "app_main_returned" and not e.get("cleanup_verified")
                   for e in r["events"]) for r in reports):
                break
            if first.poll() is not None and not reports:
                break
            if reports:
                latest = reports[-1]
                if not psutil.pid_exists(latest["pid"]) and not any(e["event"] == "restart_child_spawned" for e in latest["events"]):
                    break
            time.sleep(.1)
        else:
            timed_out = True
    reports = [json.loads(p.read_text(encoding="utf-8")) for p in sorted(run_dir.glob("child-*.json"))]
    owned = {}
    for report in reports:
        owned[report["pid"]] = report["started_at"] / 10_000_000 - 11644473600
        for event in report["events"]:
            for proc in event.get("descendants", []):
                owned[proc["pid"]] = proc["started"]
            if event["event"] == "restart_child_spawned":
                owned[event["child_pid"]] = event["child_started"]
    owned[first.pid] = first_started

    def survivors():
        remaining = []
        for pid, created in owned.items():
            if is_protected(args, pid, created):
                raise AssertionError("Installed app entered fixture process ownership")
            try:
                proc = psutil.Process(pid)
                if abs(proc.create_time() - created) < .001:
                    remaining.append(proc)
            except psutil.NoSuchProcess:
                pass
        return remaining

    end = time.monotonic() + 10
    while survivors() and time.monotonic() < end:
        time.sleep(.1)
    alive = survivors()
    orphaned = [{"pid": p.pid, "name": p.name()} for p in alive]
    cleanup_kills = []
    if alive:
        # Failure containment only, scoped to exact recorded fixture PIDs/creation times.
        for process in alive:
            try:
                children = process.children(recursive=True)
                for target in [*reversed(children), process]:
                    if is_protected(args, target.pid, target.create_time()):
                        raise AssertionError("Refusing installed process cleanup")
                    target.kill()
                    cleanup_kills.append(target.pid)
            except psutil.NoSuchProcess:
                pass
        psutil.wait_procs(alive, timeout=5)
    with socket.socket() as sock:
        sock.settimeout(.3)
        port_closed = sock.connect_ex(("127.0.0.1", port)) != 0
    try:
        protected_unchanged = (is_protected(args, args.protected_pid, psutil.Process(args.protected_pid).create_time())
                               if args.protected_pid is not None else True)
    except psutil.NoSuchProcess:
        protected_unchanged = False
    clean_reports = [r for r in reports if not r["errors"] and not r["blocked_operations"] and
                     any(e["event"] == "qualified_ready" for e in r["events"]) and
                     any(e["event"] == "app_main_returned" and e["cleanup_verified"] for e in r["events"])]
    handoffs = [e for r in reports for e in r["events"] if e["event"] == "handoff_wait_finished" and e["success"]]
    spawns = [e for r in reports for e in r["events"] if e["event"] == "restart_child_spawned"]
    result = {"passed": len(clean_reports) == args.restarts + 1 and len(handoffs) == args.restarts and
              len(spawns) == args.restarts and not orphaned and port_closed and protected_unchanged and not timed_out,
              "scope": "Actual Windows app.main + hidden production Window/WebView2 + production native Tray._restart + API + cleanup; fake devices/sensors and suppressed updater/autostart/install hooks",
              "requested_restarts": args.restarts, "clean_app_processes": len(clean_reports), "exact_process_handoffs": len(handoffs),
              "restart_spawns": len(spawns), "timed_out": timed_out, "orphaned_processes": orphaned,
              "failure_containment_kills": cleanup_kills, "api_port": port, "api_listener_closed": port_closed,
              "unique_mutex": mutex, "protected_pid": args.protected_pid,
              "protected_started": args.protected_started, "protected_process_unchanged": protected_unchanged,
              "duration_seconds": round(time.monotonic() - started, 3), "artifact_dir": str(run_dir), "children": reports}
    write_json(result_path, result)
    print(json.dumps({k: v for k, v in result.items() if k != "children"}, indent=2))
    return 0 if result["passed"] else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--result", default=str(ROOT / ".superpowers/sdd/2026-11-15-production-readiness/native-app-lifecycle-result.json"))
    parser.add_argument("--restarts", type=int, default=10)
    parser.add_argument("--timeout", type=float, default=300)
    parser.add_argument("--start-timeout", type=float, default=25)
    parser.add_argument("--protected-pid", type=int, help="Optional existing process to protect and verify unchanged")
    parser.add_argument("--protected-started", type=float, help=argparse.SUPPRESS)
    parser.add_argument("--child", action="store_true")
    parser.add_argument("--run-dir")
    parser.add_argument("--generation", type=int, default=0)
    parser.add_argument("--port", type=int)
    parser.add_argument("--mutex")
    parser.add_argument("--restart-from-pid", nargs=2)
    args = parser.parse_args()
    if not 0 <= args.restarts <= 30:
        parser.error("restarts must be between zero and thirty")
    if args.protected_pid is not None and args.protected_pid <= 0:
        parser.error("protected-pid must be positive")
    if args.child and args.protected_pid is not None and args.protected_started is None:
        parser.error("a child requires the protected process creation time")
    return child(args) if args.child else parent(args)


if __name__ == "__main__":
    sys.exit(main())
