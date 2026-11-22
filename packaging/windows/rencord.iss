; Per-user by default. {autopf} is %LOCALAPPDATA%\Programs when privileges are lowest,
; and Program Files when the user picks an all-users install.

#define MyAppName "Rencord"
#ifndef MyAppVersion
  #define MyAppVersion "0.6.2"
#endif
#ifndef VersionQuad
  #define VersionQuad "0.6.2.0"
#endif
#define MyAppPublisher "Rabellion"
#define MyAppExeName "Rencord.exe"
#ifndef ExeSource
  #define ExeSource "Rencord.exe"
#endif

[Setup]
AppId={{7F3A9C2E-1B64-4E8D-A5F0-6C2D8E4B91A3}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
VersionInfoVersion={#VersionQuad}
VersionInfoCompany={#MyAppPublisher}
VersionInfoProductName={#MyAppName}
VersionInfoDescription=Rencord
DefaultDirName={autopf}\Rencord
DefaultGroupName=Rencord
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
OutputBaseFilename=Rencord-v{#MyAppVersion}-setup-x64
SetupIconFile=..\..\assets\brand\hexwave.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
UninstallDisplayName={#MyAppName}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64
ArchitecturesInstallIn64BitMode=x64
CloseApplications=force
UsePreviousAppDir=yes
SignTool=rencord
SignedUninstaller=yes

[Tasks]
Name: desktopicon; Description: "Create a desktop shortcut"; GroupDescription: "Additional icons:"; Flags: unchecked

[Files]
Source: "{#ExeSource}"; DestDir: "{app}"; DestName: "{#MyAppExeName}"; Flags: ignoreversion

[Icons]
Name: "{group}\Rencord"; Filename: "{app}\{#MyAppExeName}"
Name: "{autodesktop}\Rencord"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "Launch Rencord"; Flags: nowait postinstall skipifsilent

[Code]
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  DataDir: String;
begin
  if CurUninstallStep <> usUninstall then
    Exit;
  if UninstallSilent then
    Exit;
  DataDir := ExpandConstant('{userappdata}\rencord');
  if DirExists(DataDir) and (MsgBox('Remove Rencord user data in %APPDATA%?', mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES) then
    DelTree(DataDir, True, True, True);
end;
