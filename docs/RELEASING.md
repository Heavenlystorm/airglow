# Releasing

Published releases are built by GitHub Actions ([`release.yml`](../.github/workflows/release.yml)) when a version
tag is pushed.

1. Set the version in `airglow/__init__.py` (the only place it lives). `1.6.0` is a stable release,
   `1.6.0-beta.1` a beta.
2. Give it a section in `CHANGELOG.md` (`## 1.6.0`). A beta without its own section uses `## Unreleased`. The
   section becomes the release notes, on GitHub and in Airglow's update notice.
3. Push the candidate branch. The tests workflow checks the installer, portable ZIP and upgrade on separate
   Windows VMs. It opens the app, detects the version and checks that fresh users get Firefly and upgrading keeps Paper.
   After the candidate passes, tag and push with `git tag v1.6.0` and `git push origin v1.6.0`.
4. The workflow requires the same source qualification as normal CI, checks the tag, builds with PyInstaller, then
   packs the Velopack release. The package and native-host checks must pass before publication. Betas are
   pre-releases. The WinGet and Scoop publication steps run when their respective credentials are configured.
   If the first WinGet submission is awaiting moderation, rerun only the release's `winget` job after it merges.
   This submits the existing public installer and leaves its checksum intact.
5. Check the release workflow’s `published-installer` job. It downloads Setup.exe from the public tag, verifies its
   SHA-256 digest, and tests installation, native launch and uninstall on a fresh Windows VM. Use a PC with supported
   hardware for a manual check of the Devices tab and the tray’s Quit command.

`.\build.ps1` runs the source gate and builds a local candidate in `dist\releases\`. Install the build and browser
dependencies listed in [Building from source](../README.md#building-from-source) and
`dotnet tool install -g vpk --version 1.2.161`. Run `python -B tools/qualify.py package` afterwards to verify the
release feed, ZIP and full package and smoke-test the compiled WebView2 effect host. The command writes logs and
artifact checksums under `output/qualification/package`. The installer itself still needs the clean-machine check in step 5.

## How updates reach users
Airglow uses [Velopack](https://velopack.io). Installs live in `%LOCALAPPDATA%\Airglow` (no admin prompt); the
user's settings stay in `%APPDATA%\Airglow`, which updates never touch.

- With "Check automatically" on (Settings, About; off by default and asked once), Airglow checks a minute after
  start-up and every 6 hours, downloads in the background, and installs on the next start or with "Restart to
  update". With it off, nothing happens until "Check for updates" is pressed.
- The stable channel reads `releases.stable.json`, the beta channel reads both feeds and takes the newer.
- Velopack keys releases by version, so never re-use a version number. A broken release is replaced by a newer
  one; to pull it, delete the GitHub release and its tag.
- A new Setup.exe (downloaded, or `winget upgrade`) can be run while Airglow is open. Velopack's Setup ends every
  process running from `%LOCALAPPDATA%\Airglow` (Airglow and its effect host) before replacing the folder. It ends
  them abruptly: there's no hook before that, so Airglow can't quit cleanly and the fans stay at their last speed
  for a moment. A normal Setup starts the new version straight after. A silent one (winget) doesn't, so the install
  hook does, when it replaced an existing install that was running (Airglow leaves a `running` file in
  `%APPDATA%\Airglow` until it quits cleanly) or set to start with Windows. It starts
  `Airglow.exe --after-setup <pid>`, which waits for Setup to exit and then starts in the tray, unless Setup already
  started it. A fresh silent install never starts Airglow. Only the in-app "Restart to update" quits cleanly first.

> **Note for developers:** never run Setup.exe from the shell of a packaged (MSIX) app, such as a packaged terminal
> app. Windows redirects that app's AppData and HKCU writes into the package
> (`%LOCALAPPDATA%\Packages\<app>\LocalCache`), so Airglow ends up installed only inside that app's view: Explorer,
> sign-in and the real "start with Windows" entry don't see it. Setup also can't find the running copy there ("0
> matched directory" in the Velopack log) and fails with "Failed to remove existing application directory". Run
> Setup.exe from Explorer or a normal terminal.

## app-meta.json
[`meta/app-meta.json`](../meta/README.md) is read along with every update check. Raise `minSupported` to tell older
versions to update (community features turn off for them), or set `notice` for a banner. A change is live within
about 5 minutes of reaching `main`.

## Signing
Unsigned installers get a SmartScreen warning ("Windows protected your PC"), and PyInstaller apps are sometimes
flagged by antivirus heuristics. Signing fixes most of that.

- The plan: [SignPath Foundation](https://signpath.org/), free for open-source projects. It signs builds made by
  GitHub Actions from this repository, and the certificate is issued to the project, not to a person. Apply on
  their site; once approved, their signing step goes into `release.yml` before `vpk upload`.
- With any certificate signtool can use (Certum open-source certificate, Azure Trusted Signing): set the
  `AIRGLOW_SIGN` repository secret to a command with `{file}` where the path goes, e.g.
  `signtool sign /fd sha256 /tr http://timestamp.digicert.com /td sha256 /a "{file}"`. `vpk` then signs Airglow.exe,
  the bundled binaries, Update.exe and Setup.exe. The same variable works for `build.ps1` locally.

Until one of these is in place the workflow publishes unsigned releases and says so in its run summary.

## winget
The first submission is by hand, once a stable release exists:
```
winget install wingetcreate
wingetcreate new https://github.com/Heavenlystorm/airglow/releases/download/v1.6.0/Airglow-stable-Setup.exe
```
Package identifier `Heavenlystorm.Airglow`, installer type `exe`, silent switch `--silent`, scope `user`. After
that's merged into `microsoft/winget-pkgs`, the workflow opens the update pull request for every stable release. It
needs a classic GitHub token with the `public_repo` scope as the `WINGET_TOKEN` repository secret, and a fork of
`microsoft/winget-pkgs` under the token's account.

## Scoop
The bucket is a separate repository, `Heavenlystorm/scoop-airglow`, with the manifest at `bucket/airglow.json`. The
workflow rewrites it for every stable release from the portable zip. It needs a fine-grained token with
"Contents: read and write" on that repository as the `SCOOP_BUCKET_TOKEN` secret. Users install with:
```
scoop bucket add airglow https://github.com/Heavenlystorm/scoop-airglow
scoop install airglow
```
Scoop installs don't update themselves; `scoop update airglow` does it.
