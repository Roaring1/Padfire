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
; LICENSE and THIRD_PARTY_NOTICES.md are shipped from the GUI one-folder payload.

[Icons]
Name: "{autoprograms}\Padfire"; Filename: "{app}\{#MyAppExeName}"
Name: "{autodesktop}\Padfire"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "Launch Padfire"; Flags: postinstall nowait skipifsilent

[UninstallRun]
; Remove the optional per-user sign-in entry before the CLI is deleted.
Filename: "{app}\CLI\PadfireCLI.exe"; Parameters: "--uninstall"; RunOnceId: "RemovePadfireStartup"; Flags: runhidden skipifdoesntexist


[Code]
const
  VC_RUNTIME_KEY = 'SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64';
  VC_RUNTIME_DOWNLOAD_PAGE = 'https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist?view=msvc-170';

function HasRequiredVCRedist: Boolean;
var
  Installed, Major, Minor, Build: Cardinal;
begin
  Result :=
    RegQueryDWordValue(HKLM64, VC_RUNTIME_KEY, 'Installed', Installed) and
    (Installed = 1) and
    RegQueryDWordValue(HKLM64, VC_RUNTIME_KEY, 'Major', Major) and
    RegQueryDWordValue(HKLM64, VC_RUNTIME_KEY, 'Minor', Minor) and
    RegQueryDWordValue(HKLM64, VC_RUNTIME_KEY, 'Bld', Build);
  if not Result then
    Exit;
  Result :=
    (Major > 14) or
    ((Major = 14) and
      ((Minor > 44) or ((Minor = 44) and (Build >= 35211))));
end;

function InitializeSetup: Boolean;
var
  ErrorCode: Integer;
begin
  Result := HasRequiredVCRedist;
  if Result then
    Exit;
  if WizardSilent then
  begin
    Log('Required Microsoft Visual C++ x64 Redistributable 14.44.35211 or newer is not installed.');
    Result := False;
    Exit;
  end;
  if MsgBox(
    'Padfire requires the Microsoft Visual C++ x64 Redistributable version 14.44.35211 or newer. ' +
    'Setup is per-user and will not install system runtimes automatically. ' +
    'Open Microsoft''s official download page now?',
    mbError, MB_YESNO
  ) = IDYES then
    ShellExec('open', VC_RUNTIME_DOWNLOAD_PAGE, '', '', SW_SHOWNORMAL, ewNoWait, ErrorCode);
  Result := False;
end;
