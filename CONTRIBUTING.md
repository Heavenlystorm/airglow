# Contributing to Airglow

Thanks for helping. The most useful thing you can do is try Airglow on your PC and tell us what happened, because
every rig is different and nobody owns all the hardware.

## Reporting a problem
1. In Airglow, open **Settings**, scroll to **About** and press **Save a report**. Explorer opens with a zip that
   holds what Airglow found, its settings, a sensor list and the logs. Your Windows user name, PC name and device
   serial numbers are removed before it's written, but have a look inside before you share it.
2. Open an issue, say what you expected and what happened, and attach the zip.

## Hardware support
**Through OpenRGB.** Most devices work this way. If OpenRGB can control it, Airglow can. Problems that also happen in
OpenRGB itself belong in the [OpenRGB tracker](https://gitlab.com/CalcProgrammer1/OpenRGB/-/issues).

**Native USB controllers.** Airglow drives a few controllers directly. Which ones, and their channel counts, are
listed in [`airglow/data/devices.json`](airglow/data/devices.json):
- A model from a family Airglow already speaks (NZXT HUE 2 generation, ASUS Aura USB) can usually be added as one
  JSON entry: USB vendor and product id, name, channel counts. Mark it `"experimental": true` until someone has
  confirmed it on real hardware.
- If you own an experimental one and it works, say so in an issue (with a diagnostics report) and we'll drop the flag.
- NZXT accessory LED counts are in the same file under `nzxt_accessories`.
- A new protocol needs a driver: packet builders as pure functions in `airglow/devices.py` (with tests built from
  real captures), and a `Device` subclass in `airglow/hw.py`.

Sources for device facts must be public documentation or open-source projects with a compatible way of learning
from them (we credit liquidctl and OpenRGB). Don't contribute anything taken from closed vendor software.

**Sensors.** A diagnostics report contains `lhm_data.json`, LibreHardwareMonitor's data in the shape the tests use.
If sensor picking is wrong on your PC (wrong CPU temperature, missing graphics card), that file lets us add your
hardware to `tests/` and fix it.

## Working on the code
```
pip install -r requirements-dev.txt
python -m unittest discover -s tests -t .     # all tests, no hardware needed
python tools/dev_server.py --demo             # the UI on http://127.0.0.1:8790 with a simulated rig and sensors
python run.py                                 # the real app from source
.\build.ps1                                   # tests, then dist\Airglow\ and the installer (needs Inno Setup 6)
```

- Python backend in `airglow/`, plain JavaScript modules in `web/` (no framework, no build step).
- Keep the style of the file you're in. Tests go with every fix, especially anything touching packets or fans.
- Fans are safety-relevant: fan code must fail towards running fans faster (see `fans.py` and the failsafe in
  `controller.py`), never slower.
- The local API only listens on 127.0.0.1 and every write needs the `X-Airglow` header. Keep it that way
  (see SECURITY.md).

## Releases
See [docs/RELEASING.md](docs/RELEASING.md).
