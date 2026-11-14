; Inno Setup script for the Windows installer (built by CI; locally: iscc /DVersion=0.4.7 packaging\windows\wireant.iss).
; Installs for the current user by default (no administrator rights needed), or for all users
; if chosen; Start menu entry, optional desktop shortcut, .maa/.nec/.ez file types, and an
; entry in Settings -> Apps to uninstall it.

#ifndef Version
  #define Version "0.0.0"
#endif
#ifndef SourceExe
  #define SourceExe "..\..\target\release\wireant.exe"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\dist"
#endif

[Setup]
AppId={{E89BB01A-6011-4002-82AC-5B89A6F40F89}
AppName=wireant
AppVersion={#Version}
AppVerName=wireant {#Version}
AppPublisher=wireant
AppPublisherURL=https://github.com/tanganyika/wireant
AppSupportURL=https://github.com/tanganyika/wireant/issues
AppUpdatesURL=https://github.com/tanganyika/wireant/releases
DefaultDirName={autopf}\wireant
DefaultGroupName=wireant
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
LicenseFile=..\..\LICENSE
SetupIconFile=..\wireant.ico
UninstallDisplayIcon={app}\wireant.exe
UninstallDisplayName=wireant
OutputDir={#OutputDir}
OutputBaseFilename=wireant-{#Version}-windows-x64-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ChangesAssociations=yes
CloseApplications=yes

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"
Name: "assoc"; Description: "Open .maa, .nec and .ez antenna files with wireant"; GroupDescription: "File types:"

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\wireant"; Filename: "{app}\wireant.exe"
Name: "{autodesktop}\wireant"; Filename: "{app}\wireant.exe"; Tasks: desktopicon

[Registry]
; HKA = HKCU for a per-user install, HKLM for all users
Root: HKA; Subkey: "Software\Classes\wireant.antenna"; ValueType: string; ValueName: ""; ValueData: "Antenna model"; Flags: uninsdeletekey; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\wireant.antenna\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\wireant.exe,0"; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\wireant.antenna\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\wireant.exe"" ""%1"""; Tasks: assoc
; .maa and .ez open with wireant by double-click (unless the user has chosen another program);
; .nec only gets "Open with" as other NEC programs use it too
Root: HKA; Subkey: "Software\Classes\.maa"; ValueType: string; ValueName: ""; ValueData: "wireant.antenna"; Flags: uninsdeletevalue; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\.ez"; ValueType: string; ValueName: ""; ValueData: "wireant.antenna"; Flags: uninsdeletevalue; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\.maa\OpenWithProgids"; ValueType: string; ValueName: "wireant.antenna"; ValueData: ""; Flags: uninsdeletevalue; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\.nec\OpenWithProgids"; ValueType: string; ValueName: "wireant.antenna"; ValueData: ""; Flags: uninsdeletevalue; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\.ez\OpenWithProgids"; ValueType: string; ValueName: "wireant.antenna"; ValueData: ""; Flags: uninsdeletevalue; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\Applications\wireant.exe\SupportedTypes"; ValueType: string; ValueName: ".maa"; ValueData: ""; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\wireant.exe\SupportedTypes"; ValueType: string; ValueName: ".nec"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\wireant.exe\SupportedTypes"; ValueType: string; ValueName: ".ez"; ValueData: ""

[Run]
Filename: "{app}\wireant.exe"; Description: "{cm:LaunchProgram,wireant}"; Flags: nowait postinstall skipifsilent
