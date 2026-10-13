# Releasing

1. Set the version in `airglow/__init__.py` (the only place it lives) and move the "Unreleased" notes in
   `CHANGELOG.md` under it.
2. Run `.\build.ps1`. It runs the tests, writes the exe's file properties from that version, builds `dist\Airglow\`
   with PyInstaller and `dist\Airglow-Setup-<version>.exe` with Inno Setup 6.
3. Smoke-test the installer on a PC (or VM) that has never had Airglow: install, open, check the Devices tab,
   quit from the tray, uninstall.
4. Tag the commit `v<version>` and publish a GitHub release with the installer and its SHA-256
   (`Get-FileHash dist\Airglow-Setup-*.exe`).

## Signing
Unsigned installers get a SmartScreen warning ("Windows protected your PC") and PyInstaller apps are sometimes flagged
by antivirus heuristics. Signing fixes most of that.

- Free for open-source projects: [SignPath Foundation](https://signpath.org/). It signs builds made by GitHub
  Actions from this repository, and the certificate is issued to the project, not to a person.
- With your own certificate: set `AIRGLOW_SIGN` before running `build.ps1`, using `{file}` where the path goes:
  `$env:AIRGLOW_SIGN = 'signtool sign /fd sha256 /tr http://timestamp.digicert.com /td sha256 /a "{file}"'`.
  Both the exe and the installer are signed.

## Update checks
`airglow/updates.py` has `UPDATE_REPO`. Set it to `owner/repo` once releases are published there; the Settings
button only appears when it's set, and Airglow never checks on its own.
