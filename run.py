"""Launcher (also the PyInstaller entry point)."""

import os
import sys

# Before numpy is first imported: its OpenBLAS otherwise starts a thread per core, and reserves memory for each, for
# arrays of a few hundred numbers that never use them.
for _var in ("OPENBLAS_NUM_THREADS", "OMP_NUM_THREADS"):
    os.environ.setdefault(_var, "1")

from airglow.app import main  # noqa: E402

if __name__ == "__main__":
    code = main()
    sys.stdout and sys.stdout.flush()
    os._exit(code or 0)   # don't wait on library threads (tray, webview) once we've shut down cleanly
