<p align="center"><img src="docs/airglow-icon.png" width="96" alt=""></p>

# Airglow

Free lighting, fan and sensor control for Windows PCs. One small app instead of a stack of vendor tools: see your
rig lit up live on screen, keep an eye on the readings you care about, shape your fan curves, and let simple rules
handle the rest.

*Airglow is the faint light of the upper atmosphere, the glow at the edge of the night sky. Its green comes from
oxygen at 557.7 nm, which is where the colour in the icon comes from.*

- **An Overview you arrange yourself.** Tiles for any reading your PC has (temperatures, load, power, clocks,
  memory, drives, network, fans) as a number with a ten-minute graph, a gauge, a large graph comparing two
  readings, or a bar. Drag tiles to move them, drag a corner to resize, swap the reading in two clicks. Pin up to
  four readings to the top bar and the tray tooltip.
- **Live preview of your actual rig.** Your fans are drawn as rings and your strips as lines, where you put them,
  lit with exactly what the hardware is showing.
- **Effects that sweep across every device:** solid, breathing, colour cycle, rainbow wave, gradient, comet,
  temperature, screen ambience and music, each with its own settings. Any zone can run its own effect.
- **Fan curves you can drag,** with a live marker for the current temperature, a 20% floor so fans never stall,
  and full speed from 90 °C whatever the curve says.
- **Automations written as sentences:** "Between 23:00 and 07:00, turn the lights off." "When Cyberpunk2077.exe is
  running, use Gaming." When a rule stops applying, your own settings come back by themselves.
- **Lights off when the PC sleeps** (many boards keep RGB on in sleep), optional lights off while locked, and
  recovery after sleep or unplugging.
- **Private by default.** No account, no cloud, no telemetry. Update checks happen only when you press the button.

## Supported hardware
| Connection | Devices |
|---|---|
| Built in, over USB | NZXT Control Hub and RGB & Fan Controller (lighting and fans, accessory LED counts detected), ASUS Aura USB motherboards |
| Through [OpenRGB](https://openrgb.org) | Hundreds more: RAM, graphics cards, keyboards, mice, strips, coolers. Run OpenRGB with its SDK server on, and the devices appear in Airglow |
| Sensors | Every sensor [LibreHardwareMonitor](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor) reports (with its remote web server on). Memory, network and disk activity also work without it |
| Motherboard fans | [FanControl](https://getfancontrol.com) profiles, switched per preset |

Close other RGB software first (SignalRGB, iCUE, NZXT CAM, Armoury Crate…). Airglow detects them and offers to close them.

## Install
Download `Airglow-Setup-x.y.z.exe` and run it. It installs for your user only (no admin prompt), adds a Start
menu entry, and can start with Windows. Uninstall from Windows Settings, Apps.

Requires Windows 10 1809 or newer with the Edge WebView2 runtime (built into Windows 11).

## Build from source
```
pip install hidapi pystray pillow pywebview numpy mss soundcard psutil pyinstaller
python -m unittest discover -s tests -t .   # 142 tests
python run.py                               # run from source
.\build.ps1                                 # tests, then dist\Airglow\ and dist\Airglow-Setup-x.y.z.exe
python tools\dev_server.py                  # UI on :8790 with simulated devices and your real sensors
```
Building the installer needs [Inno Setup 6](https://jrsoftware.org/isinfo.php).

## How it works
- `airglow/devices.py`: USB drivers for the NZXT hub and ASUS Aura. Packet builders are pure functions with tests based on captures from real hardware.
- `airglow/openrgb.py`: an independent OpenRGB SDK client (protocol version 3, works with OpenRGB 0.6 to 1.x).
- `airglow/lighting/`: zone geometry, effects (numpy, around 30 fps), screen and audio inputs that only run while an effect uses them, and the engine that streams changed frames.
- `airglow/sensors.py`: every LibreHardwareMonitor sensor by its stable id, Windows readings, and portable `auto/...` shortcuts (such as `auto/cpu_temp`) that resolve to the right sensor on any PC. `airglow/history.py` keeps ten minutes of all of them.
- `airglow/automation.py`: state-based rules. The highest active rule wins, separately for lights and for fans.
- `airglow/controller.py`: works out the effective scene from your settings, automations, sleep and lock. Also runs the fan curves and the failsafe.
- `airglow/api.py`: local HTTP API on `127.0.0.1:8787`. Writes need a custom header and a matching Host, so websites can't drive your hardware.
- `web/`: the interface (plain JavaScript modules, no framework, works offline). Motion follows the Windows "Animation effects" setting.

Settings live in `%APPDATA%\Airglow`; logs in its `logs` folder (rotating). For troubleshooting, set
`AIRGLOW_DEBUG_CRASHES=1` to record native crash stacks, or `AIRGLOW_DEBUG_STACKS=10` to dump all threads after 10 seconds.

The NZXT and ASUS packet formats follow what liquidctl and OpenRGB document for these controllers (credits in
`THIRD_PARTY_NOTICES.md`). Airglow is an independent implementation and includes none of their code.

## Licence
MIT. See `LICENSE`. Bundled components and their licences are listed in `THIRD_PARTY_NOTICES.md`.
