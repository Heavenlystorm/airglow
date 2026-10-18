"""Is the Microsoft Edge WebView2 runtime on this PC?

Airglow's window is Edge WebView2. When the runtime is missing pywebview doesn't fail: it quietly falls back to the old
Internet Explorer engine, which can't run the interface, and the user is left with an empty window and no explanation.
So Airglow checks first, with the same registry entries pywebview looks at (and a few more, so this never says "missing"
when pywebview would have found it), and says what to install.
"""

DOWNLOAD_URL = "https://developer.microsoft.com/microsoft-edge/webview2/"
MIN_MAJOR = 86           # the oldest runtime pywebview accepts

# EdgeUpdate client ids: the Evergreen runtime, and the Beta, Dev and Canary builds (pywebview accepts those too)
CLIENT_IDS = ("{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}", "{2CD8A007-E189-409D-A2C8-9AF4EF3C72AA}",
              "{0D50BFEC-CD6A-4F9A-964C-C7416E3ACB10}", "{65C35B14-6C1D-4122-AC46-7148CC9D6497}")


def _registry_versions(reg=None):
    """Every installed WebView2 version the registry lists ("pv" under each client id): per user, per machine from a
    64-bit program, and per machine on 32-bit Windows."""
    if reg is None:
        import winreg as reg
    for hive, root in (("HKEY_CURRENT_USER", r"SOFTWARE\Microsoft"),
                       ("HKEY_LOCAL_MACHINE", r"SOFTWARE\WOW6432Node\Microsoft"),
                       ("HKEY_LOCAL_MACHINE", r"SOFTWARE\Microsoft")):
        for client in CLIENT_IDS:
            try:
                with reg.OpenKey(getattr(reg, hive), rf"{root}\EdgeUpdate\Clients\{client}") as key:
                    yield str(reg.QueryValueEx(key, "pv")[0])
            except OSError:
                continue


def _major(version):
    try:
        return int(str(version).split(".")[0])
    except ValueError:
        return 0


def installed(versions=None):
    """True when a usable WebView2 runtime is installed. When it can't be checked (not Windows) it says True, so a
    check that doesn't work never stops Airglow from starting."""
    try:
        found = _registry_versions() if versions is None else versions
        return any(_major(v) >= MIN_MAJOR for v in found)
    except ImportError:
        return True
