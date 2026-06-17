; Inno Setup script for AppSwitcher — per-user install, no admin needed.
; Build: ISCC.exe installer.iss   (after PyInstaller produces dist\AppSwitcher)

#define AppName "AppSwitcher"
#define AppVersion "1.0"
#define AppExe "AppSwitcher.exe"

[Setup]
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=AppSwitcher
DefaultDirName={localappdata}\Programs\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
OutputDir=Output
OutputBaseFilename={#AppName}-Setup
SetupIconFile=icon.ico
Compression=lzma2
SolidCompression=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
WizardStyle=modern

[Tasks]
Name: "startup"; Description: "Start {#AppName} automatically when Windows starts"; GroupDescription: "Startup:"

[Files]
Source: "dist\AppSwitcher\*"; DestDir: "{app}"; Flags: recursesubdirs ignoreversion

[Icons]
Name: "{group}\{#AppName}";            Filename: "{app}\{#AppExe}"
Name: "{group}\Uninstall {#AppName}";  Filename: "{uninstallexe}"

; Single autostart mechanism: HKCU Run key with --startup (no GUI on boot).
; Matches the in-app "Start with Windows" toggle, so there's never a duplicate.
[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; \
  ValueType: string; ValueName: "AppSwitcher"; \
  ValueData: """{app}\{#AppExe}"" --startup"; \
  Tasks: startup; Flags: uninsdeletevalue

[Run]
Filename: "{app}\{#AppExe}"; Description: "Launch {#AppName} now"; Flags: nowait postinstall skipifsilent
