# Builds Airglow: runs the tests, packages dist\Airglow\ with PyInstaller, then the Setup.exe with Inno Setup.
# The version comes from airglow\__init__.py. To sign the exe and the installer, set AIRGLOW_SIGN to a command
# with {file} where the path goes, e.g.  $env:AIRGLOW_SIGN = 'signtool sign /fd sha256 /a "{file}"'
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

python -m unittest discover -s tests -t . -q
if ($LASTEXITCODE -ne 0) { throw "Tests failed; not building." }

$version = (python tools\version_info.py).Trim()
if ($LASTEXITCODE -ne 0) { throw "Couldn't read the version." }

function Sign($file) {
  if (-not $env:AIRGLOW_SIGN) { return }
  $cmd = $env:AIRGLOW_SIGN.Replace("{file}", $file)
  Invoke-Expression $cmd
  if ($LASTEXITCODE -ne 0) { throw "Signing $file failed." }
}

python -m PyInstaller --noconfirm --clean --distpath dist --workpath build\work Airglow.spec
if ($LASTEXITCODE -ne 0) { throw "PyInstaller failed." }
Sign "dist\Airglow\Airglow.exe"

$iscc = @("$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe", "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { Write-Warning "Inno Setup not found; skipping the installer. dist\Airglow\ is ready to run."; return }
& $iscc /Q "/DAppVersion=$version" installer.iss
if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed." }
Sign "dist\Airglow-Setup-$version.exe"
Get-ChildItem "dist\Airglow-Setup-$version.exe" | ForEach-Object { "Built {0} ({1:N1} MB)" -f $_.FullName, ($_.Length / 1MB) }
