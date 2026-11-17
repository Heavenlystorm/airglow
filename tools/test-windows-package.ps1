# Smoke-test the real packaged application on one fresh GitHub-hosted Windows runner per scenario.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('clean','portable','upgrade')][string]$Scenario,
    [Parameter(Mandatory)][string]$Version,
    [string]$Setup, [string]$Portable, [string]$Manifest,
    [string]$PreviousSetup, [string]$PreviousVersion,
    [string]$ExpectedDesign = 'firefly',
    [string]$OutputDirectory = 'output/installer-smoke'
)

$ErrorActionPreference = 'Stop'
if (-not $IsWindows -or $env:GITHUB_ACTIONS -ne 'true' -or
    $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or $env:ImageOS -notlike 'win*') {
    throw 'The package lifecycle test requires a disposable GitHub-hosted Windows runner.'
}
$reg = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Airglow'
$installRoot = Join-Path $env:LOCALAPPDATA 'Airglow'
$profileRoot = Join-Path $env:APPDATA 'Airglow'
if ((Test-Path $reg) -or (Test-Path $installRoot) -or (Test-Path $profileRoot) -or
    (Get-Process Airglow -ErrorAction SilentlyContinue)) { throw 'This runner already contains Airglow.' }

$report = [ordered]@{ scenario=$Scenario; version=$Version; runner=$env:ImageOS; checks=@(); passed=$false }
$output = New-Item -ItemType Directory -Force -Path $OutputDirectory
$api = 'http://127.0.0.1:8787/api/settings'
$installedExe = Join-Path $installRoot 'current/Airglow.exe'
$winget = $null

function Record([string]$check, $data) {
    $report.checks += [ordered]@{ check=$check; evidence=$data }
    Write-Host "$check - $($data | ConvertTo-Json -Compress)"
}
function Invoke-Setup([string]$path, [string[]]$arguments) {
    $process = Start-Process -FilePath (Resolve-Path -LiteralPath $path) -ArgumentList $arguments -PassThru -WindowStyle Hidden
    if (-not $process.WaitForExit(180000)) { throw "Timed out running $(Split-Path -Leaf $path)." }
    if ($process.ExitCode -ne 0) { throw "$(Split-Path -Leaf $path) exited with $($process.ExitCode)." }
}
function Invoke-WinGet([string[]]$arguments) {
    $text = & $winget @arguments 2>&1 | Out-String
    if ($LASTEXITCODE -ne 0) { throw "WinGet exited with $LASTEXITCODE. $text" }
    return $text
}
function Assert-Registration([string]$expectedVersion) {
    $entry = Get-ItemProperty -LiteralPath $reg
    if ($entry.DisplayVersion -ne $expectedVersion -or $entry.DisplayName -ne 'Airglow' -or
        $entry.Publisher -ne 'The Airglow contributors' -or $entry.InstallLocation -ne $installRoot -or
        $entry.QuietUninstallString -ne ('"{0}\Update.exe" --uninstall --silent' -f $installRoot) -or
        -not (Test-Path -LiteralPath $installedExe)) { throw "Unexpected registration for $expectedVersion." }
    Record 'Per-user registration' ($entry | Select-Object DisplayName,Publisher,DisplayVersion,InstallLocation,QuietUninstallString)
}
function Wait-Launch([string]$entryExe, [string]$realExe, [string]$expectedVersion, [string]$theme) {
    Start-Process -FilePath (Resolve-Path -LiteralPath $entryExe) -WindowStyle Hidden
    $until = [datetime]::UtcNow.AddSeconds(100)
    do {
        $appProcesses = @(Get-Process Airglow -ErrorAction SilentlyContinue | Where-Object Path -eq $realExe)
        if ($appProcesses.Count) {
            try { $view = Invoke-RestMethod -Uri $api -TimeoutSec 3 } catch { $view = $null }
            if ($view) {
                if ($view.version -ne $expectedVersion -or ($theme -and $view.settings.design -ne $theme)) {
                    throw "Unexpected running app: version $($view.version), design $($view.settings.design)."
                }
                if ($view.data_dir -ne $profileRoot) { throw "Unexpected data directory $($view.data_dir)." }
                $edge = @(Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" |
                    Where-Object ParentProcessId -in $appProcesses.Id)
                if ($edge.Count) {
                    Record 'Executable, API and native WebView2' @{
                        executable=$realExe; version=$view.version; design=$view.settings.design
                        processIds=@($appProcesses.Id); webView2Pids=@($edge.ProcessId)
                    }
                    return
                }
            }
        }
        Start-Sleep -Milliseconds 500
    } while ([datetime]::UtcNow -lt $until)
    throw "Timed out waiting for $entryExe, its settings API and WebView2 process."
}
function Stop-TestApp([string]$realExe) {
    $apps = @(Get-Process Airglow -ErrorAction SilentlyContinue | Where-Object Path -eq $realExe)
    foreach ($app in $apps) {
        Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue
        [void]$app.WaitForExit(10000)
    }
}

try {
    if ($Manifest) {
        if ($Scenario -ne 'clean') { throw 'A WinGet manifest is only used for the clean installation.' }
        $winget = (Get-Command winget.exe -ErrorAction SilentlyContinue).Source
        if (-not $winget) {
            $appInstaller = Get-AppxPackage Microsoft.DesktopAppInstaller
            if ($appInstaller) { $winget = Join-Path $appInstaller.InstallLocation 'winget.exe' }
        }
        if (-not $winget) { throw 'WinGet is missing from the test runner.' }
        Record 'WinGet version' (Invoke-WinGet @('--version')).Trim()
        [void](Invoke-WinGet @('settings','--enable','LocalManifestFiles'))
        [void](Invoke-WinGet @('validate','--manifest',$Manifest))
        [void](Invoke-WinGet @('install','--manifest',$Manifest,'--scope','user','--silent',
            '--disable-interactivity','--accept-source-agreements','--accept-package-agreements'))
        $listing = Invoke-WinGet @('list','--name','Airglow','--exact','--accept-source-agreements','--disable-interactivity')
        if ($listing -notmatch "Airglow[^\r\n]*$([regex]::Escape($Version))") { throw "WinGet did not detect Airglow. $listing" }
        Record 'WinGet local manifest install and detection' ($listing -split '[\r\n]+' | Where-Object { $_ -match 'Airglow' } | Select-Object -Last 1)
    } elseif ($Scenario -eq 'portable') {
        $pkg = Join-Path $env:RUNNER_TEMP 'Airglow portable smoke'
        Expand-Archive -LiteralPath $Portable -DestinationPath $pkg
        if (-not (Test-Path -LiteralPath (Join-Path $pkg '.portable'))) { throw 'Portable marker missing.' }
        $realExe = Join-Path $pkg 'current/Airglow.exe'
        Wait-Launch (Join-Path $pkg 'Airglow.exe') $realExe $Version $ExpectedDesign
        Stop-TestApp $realExe
        if ((Test-Path $reg) -or (Test-Path $installedExe)) { throw 'Portable launch created an installed copy.' }
        Record 'Portable registration check' 'No installer registration or installed application'
    } else {
        if ($Scenario -eq 'upgrade') {
            Invoke-Setup $PreviousSetup @('--silent')
            Assert-Registration $PreviousVersion
            Wait-Launch $installedExe $installedExe $PreviousVersion ''
            $paper = Invoke-RestMethod -Method Post -Uri $api -Headers @{'X-Airglow'='1'} -ContentType 'application/json' -Body '{"design":"paper"}'
            $saved = Get-Content (Join-Path $profileRoot 'config.json') -Raw | ConvertFrom-Json
            if ($paper.settings.design -ne 'paper' -or $saved.design -ne 'paper') { throw 'Paper was not saved before upgrading.' }
            Record 'Previous version saved Paper' $PreviousVersion
            Stop-TestApp $installedExe
        }
        Invoke-Setup $Setup @('--silent')
    }
    if ($Scenario -ne 'portable') {
        Assert-Registration $Version
        $theme = if ($Scenario -eq 'upgrade') { 'paper' } else { $ExpectedDesign }
        Wait-Launch $installedExe $installedExe $Version $theme
        Stop-TestApp $installedExe
        if ($Manifest) {
            [void](Invoke-WinGet @('uninstall','--manifest',$Manifest,'--silent','--disable-interactivity','--accept-source-agreements'))
        } else {
            Invoke-Setup (Join-Path $installRoot 'Update.exe') @('--uninstall','--silent')
        }
        $until = [datetime]::UtcNow.AddSeconds(30)
        while (((Test-Path $reg) -or (Test-Path $installedExe)) -and [datetime]::UtcNow -lt $until) { Start-Sleep -Milliseconds 300 }
        if ((Test-Path $reg) -or (Test-Path $installedExe)) { throw 'Uninstall left the registration or installed executable.' }
        Record 'Uninstall verification' 'Registration and installed executable removed'
    }
    $report.passed = $true
} catch {
    $report.error = $_.Exception.Message
    $logs = Join-Path $profileRoot 'logs'
    if (Test-Path -LiteralPath $logs) { Copy-Item -LiteralPath $logs -Destination $output.FullName -Recurse }
    throw
} finally {
    $report | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $output.FullName "$Scenario.json") -Encoding utf8
}
