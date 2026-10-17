# Tests never read or write the real Studio effect library in %APPDATA%\Airglow\effects
import tempfile

from airglow import fxlibrary

fxlibrary.LIBRARY = fxlibrary.Library(tempfile.mkdtemp(prefix="airglow-test-effects-"))
