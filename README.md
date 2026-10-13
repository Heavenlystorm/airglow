<p align="center"><img src="docs/airglow-icon.png" width="96" alt=""></p>

# Airglow

Lighting, fan and sensor control for Windows PCs, in one small app instead of a stack of vendor tools. Free and open
source (MIT), no account, nothing sent anywhere.

![The Overview: a live preview of the lighting, temperatures, load, power and fans](docs/screenshots/overview.png)

What it does:
- Shows your rig live on screen: fans as rings and strips as lines, placed where they sit in your case and lit with
  exactly what the hardware is showing.
- Effects that sweep across every device at once (solid, breathing, colour cycle, rainbow, gradient, comet,
  temperature, screen ambience, music), each with its own settings. Any zone can run its own effect.
- Fan curves you drag into shape. Fans never go below 20 %, and from 90 °C they run at full speed whatever the
  curve says.
- Automations you read as sentences, like "Between 23:00 and 07:00, turn the lights off" or "When
  Cyberpunk2077.exe is running, use Gaming". When a rule stops applying, your own settings come back.
- An Overview of tiles you arrange yourself, for any reading your PC has, with ten minutes of history.
- Lights off while the PC sleeps (many boards keep RGB on), and recovery after sleep or unplugging.

| Lighting | Fans | Automations |
|---|---|---|
| ![Lighting](docs/screenshots/lighting.png) | ![Fans](docs/screenshots/fans.png) | ![Automations](docs/screenshots/automations.png) |

## Getting started
1. Download `Airglow-Setup-<version>.exe` from [Releases](../../releases) and run it. It installs for your Windows
   user only (no admin prompt) and can start with Windows. If SmartScreen says "Windows protected your PC", choose
   More info, Run anyway (see [Is it safe?](#is-it-safe)).
2. Close other RGB software (SignalRGB, iCUE, NZXT CAM, Armoury Crate, ...). Two programs driving the same LEDs
   makes them flicker. Airglow notices them and offers to close them.
3. Open Airglow. The welcome screen lists what it found. Then, depending on your hardware:
   - **Temperatures and power:** install [LibreHardwareMonitor](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor),
     run it, and turn on Options, Remote Web Server, Run. Without it you still get memory, network, disk and
     processor load, but fan curves have no temperature to follow and run at the failsafe speed.
   - **Most RGB devices** (RAM, graphics cards, keyboards, mice, strips, coolers): install
     [OpenRGB](https://openrgb.org), start it with its SDK server on (Settings, SDK Server, Start Server, or run it
     with `--server`). The devices appear in Airglow's Devices tab.
   - **Motherboard fans:** Airglow switches [FanControl](https://getfancontrol.com) profiles per preset.
4. In Devices, check each zone's LED count and shape. Not sure? Press Identify: the first LED blinks green, then a
   white dot walks through every LED in order.

Needs Windows 10 1809 or newer with the Edge WebView2 runtime (built into Windows 11). Airglow is Windows-only.

## Supported hardware
| Connection | Devices |
|---|---|
| Directly over USB | NZXT Control Hub. ASUS Aura USB motherboard controller `0B05:19AF`. |
| Directly over USB, experimental | NZXT RGB & Fan Controller, Smart Device V2, HUE 2 and HUE 2 Ambient; ASUS Aura motherboard controllers `18F3`, `1939`, `1AA6`, `1BED`. These speak the same protocol as the tested ones but haven't been confirmed with Airglow yet. If you own one, please [report how it went](CONTRIBUTING.md#reporting-a-problem). |
| Through OpenRGB | Everything OpenRGB supports. ARGB headers it splits into fans show as fan rings, keyboards as a grid. |
| Windows Dynamic Lighting, experimental | Keyboards, mice and accessories with HID LampArray support. Off by default (Settings, More devices): Windows only lets Airglow light them while its window is in front. |
| Sensors | Everything LibreHardwareMonitor reports. Memory, network, disk and processor load also work without it. |
| Motherboard fans | Through FanControl profiles. |

The full list of directly supported controllers, and how to add one, is in
[`airglow/data/devices.json`](airglow/data/devices.json) and [CONTRIBUTING.md](CONTRIBUTING.md).

## Troubleshooting
- **A device is missing or says "busy":** another RGB program has it open. Close it (check the tray too) and wait
  a few seconds.
- **No temperatures:** LibreHardwareMonitor isn't running or its web server is off. The address is in Settings,
  Sensors (default `http://localhost:8085`).
- **OpenRGB devices don't show:** OpenRGB's SDK server must be running. Airglow retries every 10 seconds.
- **Colours look wrong on a strip:** change its colour order (RGB, GRB, ...) in Devices.
- **Anything else:** Settings, About, **Save a report**, then open an issue with the zip. It holds the logs and
  what Airglow found, with your user name, PC name and device serial numbers taken out.

Settings live in `%APPDATA%\Airglow`, logs in its `logs` folder.

## Is it safe?
- It only listens on `127.0.0.1`, and every change needs a header websites can't send, so a web page can't drive
  your hardware ([SECURITY.md](SECURITY.md)).
- No telemetry, no account. The only internet access is the update check, and only when you press the button.
- Fans fail safe: if sensors stop, Airglow runs them at the failsafe speed; at 90 °C they go to 100 %; when Airglow
  quits it leaves them at the failsafe speed.
- The installer isn't code-signed yet, which is why SmartScreen warns about it. You can build it yourself from
  this repository (below) and compare.

## Building from source
```
pip install -r requirements-dev.txt
python -m unittest discover -s tests -t .     # the tests need no hardware
python run.py                                 # run from source
python tools\dev_server.py --demo             # the UI on http://127.0.0.1:8790 with a simulated rig and sensors
.\build.ps1                                   # tests, then dist\Airglow\ and dist\Airglow-Setup-<version>.exe
```
Building the installer needs [Inno Setup 6](https://jrsoftware.org/isinfo.php). Releasing and signing:
[docs/RELEASING.md](docs/RELEASING.md).

## How it works
- `airglow/devices.py`, `airglow/hw.py`: USB drivers for the NZXT and ASUS controllers (packet builders are pure
  functions, tested against captures from real hardware) and the device layer. Which controllers get a driver comes
  from `airglow/data/devices.json`.
- `airglow/openrgb.py`: an independent client for the OpenRGB SDK protocol (version 5, falls back to 3).
- `airglow/dynlight.py`: Windows Dynamic Lighting devices through `Windows.Devices.Lights`.
- `airglow/lighting/`: zone geometry, effects (numpy, about 30 fps), screen and audio inputs that only run while an
  effect uses them, and the engine that streams changed frames to each device on its own thread.
- `airglow/sensors.py`: every LibreHardwareMonitor sensor by its stable id, plus `auto/...` shortcuts (such as
  `auto/cpu_temp`) that find the right sensor on any PC.
- `airglow/automation.py`: state-based rules; the highest active rule wins, separately for lights and fans.
- `airglow/controller.py`: works out what should happen from your settings, automations, sleep and lock, and runs
  the fan curves and the failsafe.
- `airglow/api.py`: the local HTTP API and a server-sent event stream for the UI.
- `web/`: the interface, plain JavaScript modules with no framework or build step.

The NZXT and ASUS packet formats follow what liquidctl and OpenRGB document for these controllers (credits in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)). Airglow is an independent implementation and includes none of
their code.

## Licence
MIT, see [LICENSE](LICENSE). Bundled components and their licences: [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
