# Releasing

Releases are built and published by GitHub Actions ([`release.yml`](../.github/workflows/release.yml)) when a
version tag is pushed. Nothing is built on a laptop.

1. Set the version in `airglow/__init__.py` (the only place it lives). `1.6.0` is a stable release,
   `1.6.0-beta.1` a beta.
2. Give it a section in `CHANGELOG.md` (`## 1.6.0`). A beta without its own section uses `## Unreleased`. The
   section becomes the release notes, on GitHub and in Airglow's update notice.
3. Commit, then tag and push: `git tag v1.6.0` and `git push origin v1.6.0`.
4. The workflow checks the tag matches the version, runs the tests, builds with PyInstaller, packs the Velopack
   release (Setup.exe, the full package, a delta against the channel's previous release, a portable zip and the
   release feed), publishes the GitHub release (betas as pre-releases), and for stable releases updates winget and
   the Scoop bucket.
5. Smoke-test the published Setup.exe on a PC (or VM) that has never had Airglow: install, open, check the Devices
   tab, quit from the tray, uninstall from Apps & features.

`.\build.ps1` builds the same release locally into `dist\releases\` for trying it out. It needs
`dotnet tool install -g vpk --version 1.2.161` (the .NET 8 SDK first, if it isn't there).

## How updates reach users
Airglow uses [Velopack](https://velopack.io). Installs live in `%LOCALAPPDATA%\Airglow` (no admin prompt); the
user's settings stay in `%APPDATA%\Airglow`, which updates never touch.

- With "Check automatically" on (Settings, About; off by default and asked once), Airglow checks a minute after
  start-up and every 6 hours, downloads in the background, and installs on the next start or with "Restart to
  update". With it off, nothing happens until "Check for updates" is pressed.
- The stable channel reads `releases.stable.json`, the beta channel reads both feeds and takes the newer.
- Velopack keys releases by version, so never re-use a version number. A broken release is replaced by a newer
  one; to pull it, delete the GitHub release and its tag.

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
