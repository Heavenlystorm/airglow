# Changelog

## 1.3.1
- Fans no longer spin up for half a minute after you sign in. Airglow usually starts before LibreHardwareMonitor
  has temperatures ready; it used to treat that as a lost sensor and run the fans at the failsafe speed. Until the
  first temperature arrives (at most 45 seconds) the fans now stay at the lowest speed of your curve.

## 1.3.0
- Device plugins: each directly driven controller family is one module in `airglow/plugins/` that declares its USB
  ids, how to pick the right interface, what it leaves out of OpenRGB and its own settings. The NZXT HUE 2 and
  ASUS Aura USB drivers moved there, and `airglow/data/devices.json` is gone. Saved settings carry over unchanged.
- Per-device settings, drawn from what the plugin declares: a brightness limit for NZXT hubs, and switching an ASUS
  board's own LEDs off while its ARGB headers stay lit.
- A new look: an instrument-panel design with clearer readings and calmer surfaces.
- Much lighter on CPU and GPU while the window is open. Overview readings now update in place instead of gliding,
  effect thumbnails animate when you point at them (or tab to them), and the live preview only redraws when the
  colours change. On a test PC the Overview went from 60-90% of one CPU core to about 6%.

## 1.2.0
- Automatic setup for LibreHardwareMonitor and OpenRGB (welcome screen, and Settings, Helper programs). Airglow
  downloads pinned versions, checks their checksums, installs them into Program Files behind one Windows admin
  prompt, and starts them at sign-in. LibreHardwareMonitor's web server is set to answer only on this PC, with a
  password. The PawnIO driver and the Visual C++ runtime are installed too if they're missing. Uninstalling Airglow
  offers to remove them.

## 1.1.0
- More hardware, from a data file (`airglow/data/devices.json`): NZXT RGB & Fan Controller, Smart Device V2 and
  HUE 2 controllers, four more ASUS Aura motherboard controllers (all marked experimental), every HUE 2
  accessory's LED count, and fan control across several fan controllers.
- OpenRGB protocol 5: ARGB headers OpenRGB splits into fans show as fan rings, and keyboards get a grid layout.
- Windows Dynamic Lighting devices (experimental, off by default).
- Live updates over one event stream instead of polling; nothing runs in the page while it's hidden in the tray.
- Settings, About: **Save a report** for bug reports, with personal details removed.
- If the usual port is taken, Airglow uses the next free one.
- Settings files carry a format version and are upgraded once.
- Fixes: zone settings saved every second for some NZXT fan chains, pinned fan readings broke the tray tooltip,
  editing a preset an automation uses didn't apply, several resets at once after waking from sleep.
- Pause automations for 15 minutes, an hour or until 06:00; temperature rules no longer flicker at their limit;
  the power button says when an automation is holding the lights off.

## 1.0.0
First release.
