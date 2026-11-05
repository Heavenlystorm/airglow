# Tests never read or write the real Studio effect library in %APPDATA%\Airglow\effects
import tempfile
import atexit

from airglow import fxlibrary

_effects = tempfile.TemporaryDirectory(prefix="airglow-test-effects-")
atexit.register(_effects.cleanup)
fxlibrary.LIBRARY = fxlibrary.Library(_effects.name)
