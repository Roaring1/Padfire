; Padfire per-user installer. Keeps the Qt DLLs in separate replaceable files.
#define MyAppName "Padfire"
#define MyAppVersion "2.4-dev"
#define MyAppPublisher "Roaring1"
#define MyAppExeName "Padfire.exe"

[Setup]
AppId={{7A7D8749-47DB-4D89-A2BA-F20E8AC89C10}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={localappdata}\Programs\Padfire
DefaultGroupName=Padfire
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=..\..\dist\installer
OutputBaseFilename=Padfire-Setup-{#MyAppVersion}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UninstallDisplayIcon={app}\{#MyAppExeName}

[Files]
Source: "..\..\dist\Padfire\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\..\dist\PadfireCLI\*"; DestDir: "{app}\CLI"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\..\THIRD_PARTY_NOTICES.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Padfire"; Filename: "{app}\{#MyAppExeName}"
Name: "{autodesktop}\Padfire"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "Launch Padfire"; Flags: postinstall nowait skipifsilent
