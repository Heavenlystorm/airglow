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

**Native USB controllers.** Airglow drives some controllers directly, through device plugins in
[`airglow/plugins/`](airglow/plugins/). There's one module per controller family, and nothing else in Airglow
names it: detection, the OpenRGB hand-off, the Devices tab and per-device settings all come from what the module
declares. The contract is at the top of [`airglow/plugins/__init__.py`](airglow/plugins/__init__.py).
- A model from a family Airglow already speaks (NZXT HUE 2 generation, ASUS Aura USB) is one more `Product` line
  in that plugin: USB vendor and product id, name, and its facts (channel counts). Mark it `experimental=True`
  until someone has confirmed it on real hardware.
- If you own an experimental one and it works, say so in an issue (with a diagnostics report) and we'll drop the flag.
- NZXT accessory LED counts are in `airglow/plugins/nzxt_hue2.py` under `ACCESSORIES`.
- A new protocol is a new plugin: packet builders as pure functions in `airglow/devices.py` (with tests built from
  real captures), and a module in `airglow/plugins/` with `PRODUCTS`, a `Device` subclass and `create()`. Give it
  a new `ID_PREFIX`, and never change one after a release: saved zone settings are keyed by it.
- A setting that belongs to one kind of device (a brightness cap, onboard LEDs on or off) goes in the plugin's
  `PARAMETERS`. The Devices tab draws it, and the device reads it from `self.params`.

**Products in the setup guide.** The fans, strips and coolers the setup guide offers are in
[`airglow/data/components/`](airglow/data/components/), one product per line: an `id`, `brand`, `name`, `kind`
(`fan`, `strip`, `ring` or `single`), where it plugs in (`argb` for a standard 5 V header, `nzxt` for HUE 2
accessories), and `leds` (per fan for fans; `null` if it has to be counted). Only add LED counts you've counted on
the real product or found in the maker's documentation; anything else is better counted with the guide's LED counter.

Sources for device facts must be public documentation or open-source projects with a compatible way of learning
from them (we credit liquidctl and OpenRGB). Don't contribute anything taken from closed vendor software.

**Sensors.** A diagnostics report contains `lhm_data.json`, LibreHardwareMonitor's data in the shape the tests use.
If sensor picking is wrong on your PC (wrong CPU temperature, missing graphics card), that file lets us add your
hardware to `tests/` and fix it.

## Sharing an effect
Make it in the Studio and press Share: GitHub opens with the file filled in, and Propose changes turns it into a pull
request. [effects/README.md](effects/README.md) has the rules and what the checks look at. Profiles work the same way
(Lighting, Profiles, Edit, Share).

## Working on the code
```
pip install -r requirements-dev.txt
python -m unittest discover -s tests -t .     # all tests, no hardware needed
python tools/dev_server.py --demo             # the UI on http://127.0.0.1:8790 with a simulated rig and sensors
python run.py                                 # the real app from source
.\build.ps1                                   # tests, then dist\Airglow\ and the release (needs vpk)
```

- Python backend in `airglow/`, plain JavaScript modules in `web/` (no framework, no build step).
- Keep the style of the file you're in. Tests go with every fix, especially anything touching packets or fans.
- Fans are safety-relevant: fan code must fail towards running fans faster (see `fans.py` and the failsafe in
  `controller.py`), never slower.
- The local API only listens on 127.0.0.1 and every write needs the `X-Airglow` header. Keep it that way
  (see SECURITY.md).

## Releases
See [docs/RELEASING.md](docs/RELEASING.md).
