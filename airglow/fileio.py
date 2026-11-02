"""Atomic UTF-8 text writes shared by local settings and effect files."""

import os
import tempfile
import time
from pathlib import Path


def atomic_write_text(path, text):
    """Publish a complete file, retry brief Windows locks, and clean up every failed temporary write.

    The caller owns validation, protected-file checks and any multi-file transaction. A unique file in the
    destination directory keeps concurrent writers independent and the final replacement on the same volume.
    """
    path = Path(path)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", newline="\n", dir=path.parent,
                                         prefix=f".{path.name}.", suffix=".tmp", delete=False) as stream:
            temporary = Path(stream.name)
            stream.write(text)
            stream.flush()
            os.fsync(stream.fileno())
        for wait in (0.05, 0.1, 0.2, 0.4, None):
            try:
                temporary.replace(path)
                return
            except PermissionError:
                if wait is None:
                    raise
                time.sleep(wait)
    finally:
        if temporary is not None:
            try:
                temporary.unlink(missing_ok=True)
            except OSError:
                pass
