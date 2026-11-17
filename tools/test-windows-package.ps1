# Smoke-test the real packaged application on one fresh GitHub-hosted Windows runner per scenario.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('clean','portable','upgrade')][string]$Scenario,
    [Parameter(Mandatory)][string]$Version,
    [string]$Setup, [string]$Portable, [string]$Manifest, [switch]$DetectExistingSetup,
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
function Confirm-RunnerPagingNotice {
    Add-Type -AssemblyName UIAutomationClient
    $desktop = [Windows.Automation.AutomationElement]::RootElement
    $all = [Windows.Automation.Condition]::TrueCondition
    foreach ($window in $desktop.FindAll([Windows.Automation.TreeScope]::Children, $all)) {
        if ($window.Current.Name -ne 'System Properties' -or $window.Current.ClassName -ne '#32770') { continue }
        $items = $window.FindAll([Windows.Automation.TreeScope]::Descendants, $all)
        $notice = @($items | Where-Object { $_.Current.Name -like 'Windows created a temporary paging file on your computer because of a problem that occurred with your paging file configuration*' })
        $buttons = @($items | Where-Object { $_.Current.ControlType -eq [Windows.Automation.ControlType]::Button })
        if ($notice.Count -eq 1 -and $buttons.Count -eq 1 -and $buttons[0].Current.Name -eq 'OK') {
            Record 'Runner notification before acknowledgement' @{ title=$window.Current.Name; message=$notice[0].Current.Name; button='OK' }
            $buttons[0].GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
            return $true
        }
    }
    return $false
}
function Invoke-WinGet([string[]]$arguments) {
    Write-Host "Running WinGet $($arguments -join ' ')"
    $info = [Diagnostics.ProcessStartInfo]::new($winget)
    $info.UseShellExecute = $false
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    foreach ($argument in $arguments) { $info.ArgumentList.Add($argument) }
    $proc = [Diagnostics.Process]::Start($info)
    $stdout = $proc.StandardOutput.ReadToEndAsync()
    $stderr = $proc.StandardError.ReadToEndAsync()
    if ($arguments[0] -eq 'install' -and $env:AIRGLOW_QA_ACK_PAGING_NOTICE -eq '1') {
        for ($attempt=0; $attempt -lt 20 -and -not $proc.HasExited; $attempt++) {
            if (Confirm-RunnerPagingNotice) { break }
            Start-Sleep -Seconds 1
        }
    }
    $done = $proc.WaitForExit(180000)
    if (-not $done) {
        $processes = @(Get-CimInstance Win32_Process)
        $descendants = @($proc.Id)
        for ($level=0; $level -lt 12; $level++) {
            $children = @($processes | Where-Object { $_.ParentProcessId -in $descendants -and $_.ProcessId -notin $descendants })
            if (-not $children) { break }
            $descendants += $children.ProcessId
        }
        $related = @($processes | Where-Object {
            $_.ProcessId -in $descendants -or $_.Name -in 'smartscreen.exe','consent.exe','AppInstaller.exe' -or $_.Name -like '*Airglow*'
        } | Select-Object Name,ProcessId,ParentProcessId,CommandLine)
        Record 'WinGet timeout process tree' $related
        $windows = @(Get-Process | Where-Object MainWindowTitle | Select-Object ProcessName,MainWindowTitle)
        Record 'Visible windows at timeout' $windows
        Add-Type -AssemblyName UIAutomationClient
        $desktop = [Windows.Automation.AutomationElement]::RootElement
        $topWindows = $desktop.FindAll([Windows.Automation.TreeScope]::Children, [Windows.Automation.Condition]::TrueCondition)
        $nativeWindow = (Get-Process -Id $proc.Id).MainWindowHandle
        $nativeElement = if ($nativeWindow -ne [IntPtr]::Zero) { [Windows.Automation.AutomationElement]::FromHandle($nativeWindow) }
        if ($nativeElement) {
            $items = $nativeElement.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)
            Record 'WinGet main HWND' @{ title=$nativeElement.Current.Name; class=$nativeElement.Current.ClassName; labels=@($items | ForEach-Object { $_.Current.Name } | Where-Object { $_ } | Select-Object -Unique -First 30) }
        }
        foreach ($window in $topWindows) {
            if ($window.Current.ProcessId -notin $related.ProcessId -and $window.Current.ClassName -ne '#32770') { continue }
            $items = $window.FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition)
            $labels = @($items | ForEach-Object { $_.Current.Name } | Where-Object { $_ } | Select-Object -Unique -First 30)
            Record 'WinGet dialog' @{ title=$window.Current.Name; class=$window.Current.ClassName; pid=$window.Current.ProcessId; labels=$labels }
        }
        Record 'Registration at timeout' ([bool](Test-Path $reg))
        $proc.Kill($true)
        $proc.WaitForExit()
    }
    $text = $stdout.GetAwaiter().GetResult() + $stderr.GetAwaiter().GetResult()
    if (-not $done) { throw "WinGet $($arguments[0]) timed out. $text" }
    if ($proc.ExitCode -ne 0) { throw "WinGet $($arguments[0]) exited with $($proc.ExitCode). $text" }
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
        if ($DetectExistingSetup) {
            Invoke-Setup $Setup @('--silent')
            Record 'Installation route' 'Direct silent Setup.exe'
        } else {
            [void](Invoke-WinGet @('install','--manifest',$Manifest,'--scope','user','--silent',
                '--disable-interactivity','--accept-source-agreements','--accept-package-agreements','--verbose-logs'))
            Record 'Installation route' 'WinGet local manifest'
        }
        $listing = Invoke-WinGet @('list','--name','Airglow','--exact','--accept-source-agreements','--disable-interactivity')
        if ($listing -notmatch "Airglow[^\r\n]*$([regex]::Escape($Version))") { throw "WinGet did not detect Airglow. $listing" }
        Record 'WinGet registration detection' ($listing -split '[\r\n]+' | Where-Object { $_ -match 'Airglow' } | Select-Object -Last 1)
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
    if ($Manifest) {
        $diag = Join-Path $env:LOCALAPPDATA 'Packages/Microsoft.DesktopAppInstaller_8wekyb3d8bbwe/LocalState/DiagOutputDir'
        if (Test-Path -LiteralPath $diag) {
            Get-ChildItem -LiteralPath $diag -Filter '*.log' | Copy-Item -Destination $output.FullName
        }
        Get-ChildItem -LiteralPath $env:TEMP,$env:LOCALAPPDATA -Filter '*velopack*.log' -Recurse -ErrorAction SilentlyContinue |
            Select-Object -First 4 | Copy-Item -Destination $output.FullName
    }
    throw
} finally {
    $report | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $output.FullName "$Scenario.json") -Encoding utf8
}
