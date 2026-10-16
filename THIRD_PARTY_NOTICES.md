# Third-party software

Airglow's installer bundles the following open-source components. Each keeps its own licence.

| Component | Version | Licence | Notes |
|---|---|---|---|
| Python | 3.12 | PSF License | Runtime |
| hidapi (Python bindings + library) | 0.15.0 | BSD (dual-licensed BSD/GPLv3; used under BSD) | USB HID access |
| pywebview | 6.2.1 | BSD 3-Clause | App window |
| pythonnet / clr_loader | 3.2.1 / 0.3.1 | MIT | .NET bridge for the window |
| Microsoft Edge WebView2 loader | bundled with pywebview | Microsoft redistributable licence | Uses the WebView2 runtime that ships with Windows 11 |
| bottle, proxy_tools | 0.13.4 / 0.1.0 | MIT | pywebview dependencies |
| pystray | 0.19.5 | LGPL 3.0 | Tray icon. Shipped as plain `.py` files in `_internal\pystray`, so you can replace it with a modified version |
| Pillow | 12.2.0 | MIT-CMU (HPND) | Tray icon drawing |
| NumPy | 2.4.3 | BSD 3-Clause (with 0BSD, MIT, Zlib, CC0 parts) | Effect rendering |
| mss | 10.1.0 | MIT | Screen ambience capture |
| SoundCard | 0.4.6 | BSD 3-Clause | Music-reactive effects (WASAPI loopback) |
| cffi / pycparser | 2.1.1 / 3.11 | MIT-0 / BSD 3-Clause | SoundCard dependencies |
| psutil | 7.2.2 | BSD 3-Clause | Running-program detection for automations |
| pywinrt (winrt-runtime and Windows.* projections) | 3.2.1 | MIT | Windows Dynamic Lighting devices |
| typing_extensions | 4.16.0 | PSF 2.0 | Dependency |
| Martian Mono (variable, Latin subset) | 1.1.0 via @fontsource-variable/martian-mono 5.2.5 | SIL Open Font License 1.1 | Numerals in the interface. Licence text in `web/fonts/OFL-martian-mono.txt` |
| PyInstaller bootloader | 6.22.3 | GPL 2.0 with the bootloader exception | Allows distributing the packaged app under any licence |

Airglow talks to these programs if you run them, but does not include them:
- **LibreHardwareMonitor** (MPL 2.0), for temperature and power readings over its local web server.
- **OpenRGB** (GPL 2.0), over its network SDK protocol. Airglow's client is an independent implementation of the published protocol.
- **FanControl**, for motherboard fan profiles.

Device facts (USB ids, channel counts, accessory LED counts, packet layouts) for the NZXT and ASUS Aura controllers
follow the public documentation and source of these projects. Airglow is an independent implementation and includes
none of their code:
- **liquidctl** (GPL 3.0), https://github.com/liquidctl/liquidctl: NZXT Smart Device V2 / RGB & Fan Controller / HUE 2 family.
- **OpenRGB** (GPL 2.0), https://gitlab.com/CalcProgrammer1/OpenRGB: NZXT HUE 2 accessory table, ASUS Aura USB ids,
  and the SDK protocol (`Documentation/OpenRGBSDK.md`).
