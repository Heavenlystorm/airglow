; Airglow installer (Inno Setup 6). Per-user install, no admin prompt. Built by build.ps1.
; AppVersion is passed in by build.ps1 (from airglow/__init__.py), so the version lives in one place.

#define AppName "Airglow"
#ifndef AppVersion
  #define AppVersion "0.0.0-dev"
#endif
#define AppExe "Airglow.exe"

[Setup]
AppId={{982CBB98-0B0F-43C0-A986-30BC3E9EA9F0}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=The Airglow contributors
AppCopyright=Copyright (c) 2026 The Airglow contributors. MIT licence.
DefaultDirName={localappdata}\Programs\Airglow
DisableDirPage=auto
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=dist
OutputBaseFilename=Airglow-Setup-{#AppVersion}
SetupIconFile=airglow.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
LicenseFile=LICENSE
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
CloseApplications=no

[Tasks]
Name: "startup"; Description: "Start Airglow with Windows, so lighting and fan curves apply as soon as you sign in"
Name: "desktopicon"; Description: "Add a desktop shortcut"

[Files]
Source: "dist\Airglow\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[InstallDelete]
; files from older versions that are no longer part of the app
Type: filesandordirs; Name: "{app}\_internal\web"

[Icons]
Name: "{userprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; Comment: "Lighting, fans and sensors"
Name: "{userdesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon; Comment: "Lighting, fans and sensors"

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Airglow"; ValueData: """{app}\{#AppExe}"" --background"; Flags: uninsdeletevalue; Tasks: startup

[Run]
Filename: "{app}\{#AppExe}"; Description: "Open Airglow now"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/IM {#AppExe} /F"; Flags: runhidden; RunOnceId: "StopAirglow"

[Code]
// Close a running copy before files are replaced (it holds the USB devices and its own files open).
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Code: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/IM {#AppExe} /F', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Sleep(800);
  Result := '';
end;

// Helper programs Airglow set up (Settings, Helper programs) live in Program Files and start at sign-in as admin,
// so removing them needs the admin prompt; ask first, and skip it for silent uninstalls.
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Code: Integer;
begin
  if (CurUninstallStep = usUninstall) and not UninstallSilent and DirExists(ExpandConstant('{commonpf64}\Airglow Tools')) then
    if MsgBox('Airglow set up LibreHardwareMonitor and OpenRGB on this PC. Remove them too?' + #13#10#13#10 +
              'Windows will ask for permission.', mbConfirmation, MB_YESNO) = IDYES then
      ShellExec('runas', ExpandConstant('{app}\{#AppExe}'), '--remove-tools', '', SW_HIDE, ewWaitUntilTerminated, Code);
end;
