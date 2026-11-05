"""Local LHM requests with a total deadline, including trickling headers and bodies."""

import http.client
import ipaddress
import json
import socket
import threading
import time
import urllib.parse
from concurrent.futures import Future


_LOOKUPS = {}
_DNS_LOCK = threading.Lock()
_DNS_SLOTS = threading.BoundedSemaphore(4)


def _addresses(host, port, deadline):
    try:
        ip = ipaddress.ip_address(host)
        family = socket.AF_INET6 if ip.version == 6 else socket.AF_INET
        return [(family, socket.SOCK_STREAM, 0, "", (host, port))]
    except ValueError:
        pass
    # getaddrinfo has no timeout. Coalesce named-host lookups, bound their workers, and cache successful results.
    key = (host, port)
    with _DNS_LOCK:
        now = time.monotonic()
        future, expires = _LOOKUPS.get(key, (None, 0))
        if future is None or future.done() and now >= expires:
            if not _DNS_SLOTS.acquire(blocking=False):
                raise TimeoutError("LibreHardwareMonitor name lookups are still busy")
            future = Future()
            for old, (lookup, until) in list(_LOOKUPS.items()):
                if lookup.done() and (now >= until or len(_LOOKUPS) >= 64):
                    del _LOOKUPS[old]
            _LOOKUPS[key] = (future, now + 60)

            def resolve():
                try:
                    future.set_result(socket.getaddrinfo(host, port, type=socket.SOCK_STREAM))
                except Exception as error:
                    future.set_exception(error)
                    with _DNS_LOCK:
                        _LOOKUPS.pop(key, None)
                finally:
                    _DNS_SLOTS.release()
            threading.Thread(target=resolve, name="lhm-dns", daemon=True).start()
    return future.result(timeout=max(0, deadline - time.monotonic()))


def _connect(address, deadline):
    host, port = address
    problem = None
    for family, kind, protocol, _, target in _addresses(host, port, deadline):
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("LibreHardwareMonitor deadline exceeded")
        sock = socket.socket(family, kind, protocol)
        try:
            sock.settimeout(remaining)
            sock.connect(target)
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("LibreHardwareMonitor deadline exceeded")
            sock.settimeout(remaining)       # HTTPS uses the remaining time for its TLS handshake, too
            return sock
        except OSError as error:
            problem = error
            sock.close()
    raise OSError(f"couldn't connect to LibreHardwareMonitor: {problem}")


def read_json(url, *, timeout, headers=None, method="GET", body=None):
    address = urllib.parse.urlsplit(url)
    if address.scheme not in ("http", "https") or not address.hostname:
        raise OSError("expected an HTTP or HTTPS LibreHardwareMonitor address")
    deadline = time.monotonic() + timeout
    connection = http.client.HTTPSConnection if address.scheme == "https" else http.client.HTTPConnection
    conn = connection(address.hostname, address.port, timeout=timeout)
    conn._create_connection = lambda address, timeout, source_address=None: _connect(address, deadline)
    timer = None
    try:
        conn.connect()
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("LibreHardwareMonitor deadline exceeded")
        sock = conn.sock
        sock.settimeout(remaining)

        def abort():
            # Keep the socket reference: HTTPConnection clears conn.sock for Connection: close responses.
            try:
                sock.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
        timer = threading.Timer(remaining, abort)
        timer.daemon = True
        timer.start()
        target = urllib.parse.urlunsplit(("", "", address.path or "/", address.query, ""))
        conn.request(method, target, body=body, headers=headers or {})
        with conn.getresponse() as response:
            if response.status != 200:
                raise OSError(f"LibreHardwareMonitor answered {response.status}")
            data = response.read()
        if time.monotonic() >= deadline:
            raise TimeoutError("LibreHardwareMonitor deadline exceeded")
        return json.loads(data.decode("utf-8"))
    except http.client.HTTPException as e:
        raise OSError(str(e)) from e
    finally:
        if timer is not None:
            timer.cancel()
        conn.close()
