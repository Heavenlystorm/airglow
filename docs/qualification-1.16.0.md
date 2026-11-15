# Airglow 1.16.0 qualification

The results below are from local qualification of the unsigned 1.16.0-beta.1 build and a local upgrade
from 1.15.0. Stable 1.16.0 has its own source and package gates. The stable publication result is pending
in the GitHub Actions release workflow. Local qualification used separate data, loopback listeners and
synthetic devices. A brief read-only check followed the beta upgrade.

## What changed

- Restart waits for the exact preceding process; partial startup and repeated Quit clean up safely. WebView2
  preflight happens before hardware acquisition, and effect windows remain hidden and non-activating.
- Store changes use validated recoverable transactions. Helper repair stages downloads and restores files/tasks
  on failure. Managed cooling immediately uses the configured missing-temperature failsafe without reducing a
  higher existing duty or taking over firmware-owned channels.
- Lighting waits for the exact required renderer instances and a successful save. Failed replacements retain
  the preceding scene. Changes fade, Off cancels pending work, and Undo affects lighting only. Retry cannot
  overwrite a newer selection or a different app session.
- Lighting cards and scene auditions use production renderers, configured geometry and a separate preview
  capacity. Simulated inputs are labelled. Ctrl+K provides searchable, explicitly labelled actions.
- All five tile styles and twelve themes have distinct, meaningful presentations. Keyboard navigation, enlarged
  interface scale, accent contrast, hidden/reduced-motion behavior and animation cancellation are corrected.

## Evidence

These results apply to the beta build. The package gate writes checksums and logs for each build.

| Check | Result |
| --- | --- |
| Python regressions and startup imports | 854 tests passed; startup modules and both device plugins loaded |
| JavaScript regressions and imports | 227 tests passed; 49 browser modules parsed and resolved |
| Focused browser checks | 12 production UI and 8 audition checks passed |
| Existing browser fixture suites | All 9 passed; 135 visual checks with zero browser errors and no fixture API errors |
| Appearance coverage | 12 themes, 5 styles, widths 390/700/1280/1920/3840, scale 90/100/120; reduced motion and keyboard checks |
| Animation sampling | 180 frames during 15 layout changes: 4.3 ms p95 callback interval at approximately 240 Hz; no overlapping long task above 100 ms |
| Native application restart | 10 handoffs across 11 real app/WebView2/tray processes; isolated settings and fake hardware; no orphan process or containment kill |
| Packaged native renderer | Package gate passed; 282 nonzero frames, 187 distinct frames in 12.06 seconds; no unexpected exit or visible/taskbar window |
| Native sandbox bootstrap fault checks | Both passed. One blocked script load recovered. Two blocked script loads terminated the effect and preserved the preceding scene. |
| Mixed native soak on the beta package | Stopped as requested at 23m 19.52s after 100 rapid switches, 70 mixed scene switches and 9,050 audition frames, with no recorded errors. The two-hour run was waived for this qualification. |
| Installed upgrade and available hardware | Silent Setup exited with code 0. After a manual background launch, all 17 read-only checks passed on 1.16.0-beta.1. |

The native restart harness used actual application lifecycle code, isolated fake hardware, a unique mutex and
an owned loopback listener. It did not restart Windows, exercise physical power events or replace the installed app.
The soak uses real packaged native renderers with synthetic audio, temperature, screen and lighting devices.
It exercises 100 rapid switches, all four live Studio groups and separate four-group scene auditions.

An earlier endurance run exposed an intermittent sandbox page bootstrap failure. A single failed local script
request reproduced that failure before effect code ran. Bootstrap now permits one fresh sandbox attempt within
the same eight-second total budget. Effect-code errors and worker hangs fail without a bootstrap retry.
The rebuilt native single-failure check recovered and ran 32.01 seconds with 207 valid audition frames and no
errors. The double-failure check made two sandbox attempts, then reported the error, preserved the preceding
Solid output and produced zero frames for the failing effect. The cause of the original navigation failure
remains unknown.

The local beta upgrade required a separate manual background launch after silent Setup. The cause of the
missed automatic relaunch is unknown. The installed executable matched the qualified SHA-256. The saved
scene was preserved and lighting remained Off. The read-only check also confirmed the cooling profile,
the 30% failsafe setting, the startup entry, online native and OpenRGB lighting devices, available
LibreHardwareMonitor and hub fan channels, and 15 healthy controls.

## Limits and remaining user checks

- Windows Sandbox/Hyper-V was unavailable. Fresh install, repair, uninstall/reinstall, offline install and missing
  WebView2 installer flows have not been exercised in disposable Windows machines. Unit/fault-injection checks
  are not a substitute for that matrix.
- The measured display cadence was approximately 240 Hz; the specific 60 Hz performance target is unverified.
- Real helper replacement/rollback and physical device unplug/replug were not exercised. Existing healthy
  helpers are retained during the application upgrade.
- Live lighting effect changes were skipped after installation. The installed lighting check read the saved
  and effective Off state and device health.
- Reboot and sleep/resume remain unverified. After each, check that one Airglow instance starts, the saved
  lighting and cooling return, devices reconnect, and no effect window appears in the taskbar.
- Tests establish the documented behavior on this machine and these fixtures; they do not establish that every
  environment or future failure is covered.

## Local recovery

The local qualification retained a private, hash-verified backup of the previous installation, durable
settings and effects, and the startup entry. It excluded runtime WebView profiles, caches, logs and live
PID and port markers from the settings snapshot.

Keep this backup private. If recovery is needed, first quit Airglow, preserve the new installation and data in a
separate folder, restore the backed-up installation and durable data to the manifest's original paths, and restore
the recorded startup value. Never copy a stale `running` or `port` marker. Do not remove working helper programs.
The local qualification did not exercise a downgrade or backup restore.

See [Releasing](RELEASING.md) for the stable installer, portable ZIP, package feed and qualification steps.
