# Builds Airglow: runs the tests, packages dist\Airglow\ with PyInstaller, then the Velopack release in dist\releases\
# (Setup.exe, the full and delta packages, a portable zip and the release feed). The version comes from
# airglow\__init__.py; versions like 1.6.0-beta.1 go to the beta channel.
#
# Velopack's packer is a .NET tool:  dotnet tool install -g vpk --version 1.2.161
# Without it the build stops after PyInstaller, and dist\Airglow\ is ready to run.
#
# To sign, set AIRGLOW_SIGN to a command with {file} where the path goes, e.g.
#   $env:AIRGLOW_SIGN = 'signtool sign /fd sha256 /tr http://timestamp.digicert.com /td sha256 /a "{file}"'
# vpk then signs Airglow.exe, the bundled binaries, Update.exe and Setup.exe with it.
#
# Releases are built by .github/workflows/release.yml when a version tag is pushed, not with this script; it's for
# trying a build locally. -Delta downloads the channel's latest release first, so a delta package is made too.
param(
  [switch]$SkipTests,
  [switch]$Delta
)
# Not "Stop": Windows PowerShell 5.1 would treat any log line a tool writes to stderr as a failure. Every step
# checks its exit code instead.
$ErrorActionPreference = "Continue"
Set-Location $PSScriptRoot

if (-not $SkipTests) {
  python -m unittest discover -s tests -t . -q
  if ($LASTEXITCODE -ne 0) { throw "Tests failed; not building." }
}

$version = (python tools\version_info.py).Trim()
if ($LASTEXITCODE -ne 0) { throw "Couldn't read the version." }
$channel = (python tools\release.py channel).Trim()

python -m PyInstaller --noconfirm --clean --distpath dist --workpath build\work Airglow.spec
if ($LASTEXITCODE -ne 0) { throw "PyInstaller failed." }

if (-not (Get-Command vpk -ErrorAction SilentlyContinue)) {
  Write-Warning "vpk not found; skipping the Velopack release. dist\Airglow\ is ready to run."
  return
}

$out = "dist\releases"
New-Item -ItemType Directory -Force $out | Out-Null
$repo = "https://github.com/Heavenlystorm/airglow"
if ($Delta) {
  $extra = @(); if ($channel -eq "beta") { $extra += "--pre" }
  if ($env:GITHUB_TOKEN) { $extra += @("--token", $env:GITHUB_TOKEN) }
  vpk download github --repoUrl $repo --channel $channel --outputDir $out @extra
  if ($LASTEXITCODE -ne 0) { Write-Warning "No earlier $channel release to make a delta from; packing a full release only." }
}

python tools\release.py notes $version dist\notes.md
if ($LASTEXITCODE -ne 0) { throw "CHANGELOG.md has no notes for $version." }

$pack = @("pack", "--packId", "Airglow", "--packVersion", $version, "--packDir", "dist\Airglow", "--mainExe", "Airglow.exe",
          "--packTitle", "Airglow", "--packAuthors", "The Airglow contributors", "--icon", "airglow.ico",
          "--channel", $channel, "--releaseNotes", "dist\notes.md", "--outputDir", $out)
if ($env:AIRGLOW_SIGN) { $pack += @("--signTemplate", $env:AIRGLOW_SIGN.Replace("{file}", "{{file}}")) }
vpk @pack
if ($LASTEXITCODE -ne 0) { throw "vpk pack failed." }
Get-ChildItem $out -Filter "Airglow-*" | Where-Object { $_.Name -match "Setup\.exe$|Portable\.zip$|\.nupkg$" } |
  ForEach-Object { "Built {0} ({1:N1} MB)" -f $_.FullName, ($_.Length / 1MB) }
