# Changelog

## 1.6.5
- Fan safety: "full speed at 90 °C" now watches the hottest part, whatever the curve follows. With the curve on the
  graphics card (or the new coolant reading), a processor at 95 °C used to leave the fans on the curve.
- **Motherboard fan headers and liquid coolers.** The Fans tab lists every fan and pump LibreHardwareMonitor can
  drive: the board's own fan headers, and the pumps and fans of NZXT Kraken X3, Z3, 2023 and 2024 Elite, older
  Kraken X coolers, MSI CoreLiquid, Aquacomputer D5 Next and Aquastream XT pumps, Arctic and Razer fan controllers
  and the NZXT Grid+. Each one is left alone until you pick: follow the fan curve, or hold a steady speed (pumps).
  Fan safety applies to them too: full speed at 90 °C, at least the failsafe speed without temperatures, and when
  Airglow quits they go back to their BIOS or cooler curve.
- Fans can follow the **coolant temperature** of an AIO or custom loop (Settings, Fans), and so can "running hot"
  automations. The Fans tab shows it next to the processor and graphics card.
- An ASUS Aura board no longer hides the rest of ASUS from OpenRGB: ROG and TUF graphics cards, memory, keyboards,
  mice and monitors used to disappear from Airglow as soon as the board was found.
- NZXT RGB & Fan Controllers whose lighting Airglow can't drive (the 3+6 channel revisions) are no longer hidden from
  OpenRGB either, so OpenRGB lights them while Airglow runs their fans.
- New, experimental: the HUE 2 lighting built into NZXT N7 motherboards.
- More liquid coolers are recognised in the setup guide (Corsair H110i GT, MSI CoreLiquid, ASUS ROG Ryuo, Ryujin
  and Strix LC, Aquacomputer pumps), and it says when a cooler's pump and fans can follow Airglow.
- Logitech G HUB, SteelSeries GG and Engine, MSI Mystic Light's LEDKeeper and the current Lian Li L-Connect 3 are
  recognised as programs that fight over the same lights. Peripheral software only counts while Airglow lights that
  brand's devices, so nobody is nagged about the program their mouse needs.
- Fans set up on one controller no longer start out drawn on top of each other on the Lighting stage: every kit of
  fans gets a column of its own.
- Windows Dynamic Lighting devices appear in the Devices tab, so their zones can be named, switched and checked.
- Long OpenRGB strips (WLED, E1.31, long ARGB runs) of more than 300 LEDs, and fans with more than 64 LEDs, can be
  saved; they used to be refused with an error.
- An OpenRGB message that stops half way no longer freezes the lights; Airglow reconnects instead.
- FanControl is also found when it was installed with Scoop or into your user folder.

## 1.6.2
- Fan safety: Airglow now sets the speed of every fan port on the NZXT hub. Before, a fan that read 0 RPM, for
  example one that had stalled at a low speed, was left out, so it stayed at that speed however hot the PC got and
  the failsafe skipped it too.
- A Studio effect that's deleted, or taken down from the gallery, no longer breaks what uses it. Profiles that use it
  are kept (they used to be dropped at the next start), zones show a plain colour until it's back, and the log gets
  one line instead of filling up within minutes. The Studio won't delete an effect a profile or zone still uses.
- Lights on OpenRGB devices no longer freeze when the connection drops in the middle of a frame (it can happen when
  the PC wakes from sleep).

## 1.6.1
- "Lights off when the PC sleeps or shuts down" now really leaves them off at shutdown. Before, the next frame lit
  them again; if the shutdown is cancelled, they come back on.
- Trying a profile (or "Also on my lights" in the Studio) is never saved. A restart while trying used to keep the
  borrowed look and put the fans on Balanced; now you get back what you had. Picking a profile while trying one
  ends the trial, so Discard no longer undoes it.
- After waking from sleep, devices from OpenRGB get every zone sent again, and Airglow no longer gathers a
  background thread per device each time.
- The tray tooltip calls a profile's own fan curve by the profile's name.
- Temperatures in automations follow your °C or °F setting, in the editor and in the Lighting tab.
- Overview tiles keep their titles readable when a long processor or graphics card name sits next to them.
- On the Lighting stage, zones at the edge of the case show whole, names step away from fans they would cover, and
  long effect names fit their cards.
- Saving settings retries when another program holds the file for a moment, instead of failing.
- Two profiles saved under the same name no longer clash, and automations with the same id are refused.
- Malformed requests to the automations API get a clear error instead of an internal one.

## 1.6.0
- **The Studio**, a new tab for making your own effects. Pick a starting point (or remix any effect), stack layers
  such as gradients, plasma, waves, sparkles, a scanner, breathing and three that follow the music, choose how each
  mixes in and which area of the case it lights, and watch it on a picture of your own case, and on your real lights
  with "Also on my lights". Tick "Show as a control" to let others tweak a setting. "Test beat" plays a fake beat so
  music effects can be built in silence. Saved effects work everywhere built-in ones do.
- Effects can also be written in JavaScript: switch the editor to Code (CodeMirror, with errors and log output under
  the preview), start from a canvas, per-LED or shader template, or edit in VS Code and see every save live.
- Share sends an effect to the community gallery as a pull request on GitHub, where it's checked automatically and
  reviewed. Community effects (Studio, Community effects) play on your own layout before you install them; updates
  keep the previous version for one rollback, and effects that are taken down are turned off.
- Every Studio effect runs sandboxed in a separate effect host process, without network, files or access to Airglow.
  One that hangs, is too slow or uses too much memory is turned off with the reason, and Lighting offers Try again.
- Airglow can keep itself up to date. Turn on "Check automatically" (Airglow asks once; it's off unless you say yes)
  and it looks for a new version a minute after it starts and every 6 hours, downloads it in the background and
  installs it the next time it starts, or right away with "Restart to update". Updates are small, since only what
  changed is downloaded. With it off, Airglow still only goes online when you press a button.
- A beta channel in Settings, About, for new features first.
- A new installer (Velopack). Installing over 1.5 or older moves Airglow to it: the old copy and its shortcuts go,
  your settings, profiles and helper programs stay, and "Start with Windows" keeps pointing at Airglow.
- Installing a new version while Airglow is open works without quitting first, and after a winget upgrade Airglow
  starts again in the tray.
- Sharing a long profile works: GitHub only takes about 8 KB in its address, so a longer profile goes on the
  clipboard, and Airglow tells you to paste it on the page that opens.
- Releases are built and published by GitHub Actions from a version tag.

## 1.5.0
- Presets are now profiles, and you can share them. A profile is the lighting look plus, optionally, effects for
  particular areas (front fans, top, graphics card...), its own fan curve, and the automations that switch to it.
  Areas are keyed by where lights sit, so a shared profile lands on the right lights on any rig.
- A community gallery (Lighting, Profiles, Browse): profiles from `profiles/` in this repository, featured first.
  Each one plays live on your own layout before you touch anything; "Try it" puts it on your real lights with Keep
  and Discard, and Discard puts everything back. Fan curves and automations only come along if you tick them.
- Share sends a profile to the gallery: GitHub opens with the file filled in, and saving it proposes a pull
  request. Profiles are checked automatically and the gallery's index is rebuilt when one is merged.

## 1.4.0
- A guided setup for your lights, on first run, whenever new lights show up, and from Devices. For each header the
  lights blink so you know which ones you're setting up; you pick the product (NZXT accessories are already
  detected), or count the LEDs with a slider that lights them one by one with the last one in amber, then drag it to
  where it sits on the Lighting stage (or snap it to Front, Top, Rear and so on first) while the guide waits beside
  it. Built-in lighting (graphics cards, boards, RAM, keyboards) only needs the last step, and a device's zones
  move together. Effects then sweep across the case as it really is. Products live in `airglow/data/components/`,
  one per line.
- Recognises USB-connected AIO coolers (NZXT Kraken, Corsair Hydro and iCUE, ASUS ROG Ryujin, MSI Coreliquid and
  Asetek-based ones) by name, from liquidctl's public list. A pump ring wired to an ARGB header is set up like any
  other light.
- Zone names on the Lighting stage no longer print on top of each other when zones sit close together.
- New social preview image.

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
