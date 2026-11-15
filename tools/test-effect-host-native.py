"""Windows smoke test: the real helper must render without exposing a native window.

Uses an isolated data directory and synthetic LEDs; never opens lighting hardware.
Run from the repository: python tools/test-effect-host-native.py [--exe Airglow.exe]
"""

import argparse
import ctypes
from ctypes import wintypes
import functools
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
from airglow import fxformat


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe')
    parser.add_argument('--entry', default=str(ROOT / 'run.py'))
    parser.add_argument('--effect')
    parser.add_argument('--seconds', type=float, default=12)
    parser.add_argument('--result')
    args = parser.parse_args()
    if args.effect:
        effect = fxformat.validate(Path(args.effect).read_text(encoding='utf-8'))
    else:
        effect = fxformat.validate(fxformat.to_html({
            'name': 'Native smoke', 'code': 'airglow.effect({point(p,f){return [128+127*Math.sin(f.time),128,64]}});'}))
    params = {item['id']: item['default'] for item in effect['schema'] if 'default' in item}
    load = {'type': 'load', 'id': 'smoke', 'kind': effect['kind'], 'code': effect['code'],
            'layers': effect['layers'], 'params': params,
            'points': [[i * 10, 100, None, i / 31, 0, 'test'] for i in range(32)]}
    received = {'frames': 0, 'nonzero_frames': 0, 'problems': [], 'distinct_frames': set()}

    class Handler(SimpleHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_GET(self):
            if not self.path.startswith('/api/fx/host/stream'):
                return super().do_GET()
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.end_headers()
            def send(kind, body):
                self.wfile.write(f'event: {kind}\ndata: {json.dumps(body)}\n\n'.encode())
                self.wfile.flush()
            try:
                send('load', load)
                started = time.monotonic()
                while True:
                    send('tick', {'time': time.monotonic() - started, 'dt': 1 / 30, 'audio': None})
                    time.sleep(1 / 30)
            except (ConnectionError, OSError):
                pass

        def do_POST(self):
            body = self.rfile.read(int(self.headers.get('Content-Length', 0)))
            if self.headers.get('Content-Type') == 'application/octet-stream' and body:
                received['frames'] += 1
                colours = body[3 + body[0]:]
                received['nonzero_frames'] += bool(any(colours))
                received['distinct_frames'].add(colours)
            elif self.headers.get('Content-Type') == 'application/json':
                data = json.loads(body)
                received['problems'].extend(data.get('problems', {}).values())
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            self.wfile.write(b'{"ok":true}')

    server = ThreadingHTTPServer(('127.0.0.1', 0), functools.partial(Handler, directory=str(ROOT / 'web')))
    threading.Thread(target=server.serve_forever, daemon=True).start()
    user32 = ctypes.WinDLL('user32', use_last_error=True)
    callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    user32.IsWindowVisible.argtypes = [wintypes.HWND]
    user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
    user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
    user32.GetWindowRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
    user32.GetWindowLongW.argtypes = [wintypes.HWND, ctypes.c_int]
    visible = set()
    switchers = set()
    visibility = {'samples': 0, 'first_seconds': None, 'last_seconds': None}
    with tempfile.TemporaryDirectory(prefix='airglow-native-host-') as data:
        env = dict(os.environ, APPDATA=data)
        command = [args.exe] if args.exe else [sys.executable, args.entry]
        command += ['--effect-host', f'http://127.0.0.1:{server.server_port}/runtime/host.html#token=native-smoke',
                    '--parent', str(os.getpid())]
        with open(Path(data) / 'stderr.log', 'w', encoding='utf-8') as err:
            proc = subprocess.Popen(command, env=env, stdout=err, stderr=err, creationflags=0x08000000)
            @callback_type
            def check(hwnd, _):
                owner = wintypes.DWORD()
                user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
                if owner.value == proc.pid and user32.IsWindowVisible(hwnd):
                    title = ctypes.create_unicode_buffer(512)
                    user32.GetWindowTextW(hwnd, title, 512)
                    style = user32.GetWindowLongW(hwnd, -20)
                    if style & 0x40000 or not style & 0x80:
                        switchers.add(title.value)
                    rect = wintypes.RECT()
                    user32.GetWindowRect(hwnd, ctypes.byref(rect))
                    left, top = user32.GetSystemMetrics(76), user32.GetSystemMetrics(77)
                    right = left + user32.GetSystemMetrics(78)
                    bottom = top + user32.GetSystemMetrics(79)
                    if rect.right <= left or rect.bottom <= top or rect.left >= right or rect.top >= bottom:
                        return True
                    visible.add(title.value)
                    elapsed = round(time.monotonic() - started, 3)
                    visibility['samples'] += 1
                    if visibility['first_seconds'] is None:
                        visibility['first_seconds'] = elapsed
                    visibility['last_seconds'] = elapsed
                return True
            started = time.monotonic()
            try:
                while time.monotonic() - started < args.seconds and proc.poll() is None:
                    user32.EnumWindows(check, 0)
                    time.sleep(0.01)
                exit_code = proc.poll()
            finally:
                import psutil
                try:
                    children = psutil.Process(proc.pid).children(recursive=True)
                    proc.terminate()
                    proc.wait(timeout=10)
                    for child in children:
                        try:
                            child.kill()
                        except psutil.Error:
                            pass
                    psutil.wait_procs(children, timeout=5)
                except psutil.NoSuchProcess:
                    pass
        result = {**received, 'distinct_frames': len(received['distinct_frames']),
                  'visible_windows': sorted(visible), 'unexpected_exit': exit_code,
                  'taskbar_windows': sorted(switchers),
                  'visibility': visibility,
                  'duration': round(time.monotonic() - started, 2),
                  'stderr': (Path(data) / 'stderr.log').read_text(encoding='utf-8')}
        result['passed'] = (exit_code is None and not visible and not switchers and received['nonzero_frames'] > 10
                            and result['distinct_frames'] > 2 and not received['problems'])
        print(json.dumps(result, indent=2))
        if args.result:
            Path(args.result).write_text(json.dumps(result, indent=2), encoding='utf-8')
    server.shutdown()
    return 0 if result['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())
