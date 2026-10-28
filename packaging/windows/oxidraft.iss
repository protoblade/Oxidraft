; Inno Setup script for the Windows installer, built by the release workflow:
;   ISCC.exe /DAppVersion=0.5.1 /DBinDir=path\to\release packaging\windows\oxidraft.iss
;
; Installs per user by default (no admin rights needed), into
; %LOCALAPPDATA%\Programs\Oxidraft, so the app's built-in updater can replace
; oxidraft.exe later without elevation.

#ifndef AppVersion
  #error Pass the version: /DAppVersion=x.y.z
#endif
#ifndef BinDir
  #define BinDir "..\..\target\x86_64-pc-windows-msvc\release"
#endif

[Setup]
AppId={{191D51D7-4CC0-4F73-BDBF-7B7744ADF248}
AppName=Oxidraft
AppVersion={#AppVersion}
AppVerName=Oxidraft {#AppVersion}
AppPublisher=The Oxidraft contributors
AppPublisherURL=https://github.com/protoblade/Oxidraft
AppSupportURL=https://github.com/protoblade/Oxidraft/issues
AppUpdatesURL=https://github.com/protoblade/Oxidraft/releases
DefaultDirName={autopf}\Oxidraft
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\..\dist
OutputBaseFilename=Oxidraft-{#AppVersion}-windows-x64-setup
SetupIconFile=..\..\assets\icon.ico
UninstallDisplayIcon={app}\oxidraft.exe
UninstallDisplayName=Oxidraft
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ChangesAssociations=yes
; The app may be running when updating: ask to close it.
CloseApplications=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "dxfassoc"; Description: "Open .dxf drawings with Oxidraft"; GroupDescription: "File associations:"

[Files]
Source: "{#BinDir}\oxidraft.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE-MIT"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE-APACHE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Oxidraft"; Filename: "{app}\oxidraft.exe"
Name: "{autodesktop}\Oxidraft"; Filename: "{app}\oxidraft.exe"; Tasks: desktopicon

[Registry]
; Always offer Oxidraft in "Open with" for .dxf files.
Root: HKA; Subkey: "Software\Classes\Applications\oxidraft.exe\SupportedTypes"; ValueType: string; ValueName: ".dxf"; ValueData: ""; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\oxidraft.exe\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\oxidraft.exe"" ""%1"""; Flags: uninsdeletekey
; With the task: make Oxidraft the program that opens .dxf files.
Root: HKA; Subkey: "Software\Classes\.dxf\OpenWithProgids"; ValueType: string; ValueName: "Oxidraft.dxf"; ValueData: ""; Flags: uninsdeletevalue; Tasks: dxfassoc
Root: HKA; Subkey: "Software\Classes\Oxidraft.dxf"; ValueType: string; ValueName: ""; ValueData: "DXF Drawing"; Flags: uninsdeletekey; Tasks: dxfassoc
Root: HKA; Subkey: "Software\Classes\Oxidraft.dxf\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\oxidraft.exe,0"; Tasks: dxfassoc
Root: HKA; Subkey: "Software\Classes\Oxidraft.dxf\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\oxidraft.exe"" ""%1"""; Tasks: dxfassoc

[Run]
Filename: "{app}\oxidraft.exe"; Description: "{cm:LaunchProgram,Oxidraft}"; Flags: nowait postinstall skipifsilent
