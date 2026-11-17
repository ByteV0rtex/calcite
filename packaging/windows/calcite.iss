; Calcite installer for Windows (Inno Setup 6).
; scripts/package-windows.ps1 passes AppVersion and Binary.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef Binary
  #define Binary "..\..\target\release\calcite.exe"
#endif

[Setup]
AppId={{E548880D-1912-4936-87E1-597361749756}
AppName=Calcite
AppVersion={#AppVersion}
AppVerName=Calcite {#AppVersion}
AppPublisher=ByteV0rtex
AppPublisherURL=https://github.com/ByteV0rtex/calcite
AppSupportURL=https://github.com/ByteV0rtex/calcite/issues
DefaultDirName={autopf}\Calcite
DisableProgramGroupPage=yes
; Installs for the current user without asking for admin; admins can
; choose all users.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\..\dist
OutputBaseFilename=Calcite-{#AppVersion}-windows-x64-setup
SetupIconFile=..\icons\calcite.ico
UninstallDisplayIcon={app}\calcite.exe
UninstallDisplayName=Calcite
LicenseFile=..\..\LICENSE
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ChangesAssociations=yes
CloseApplications=yes

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "mrpack"; Description: "Open .mrpack modpacks with Calcite"; GroupDescription: "File types:"

[Files]
Source: "{#Binary}"; DestDir: "{app}"; DestName: "calcite.exe"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Calcite"; Filename: "{app}\calcite.exe"
Name: "{autodesktop}\Calcite"; Filename: "{app}\calcite.exe"; Tasks: desktopicon

[Registry]
Root: HKA; Subkey: "Software\Classes\.mrpack\OpenWithProgids"; ValueType: string; ValueName: "Calcite.mrpack"; ValueData: ""; Flags: uninsdeletevalue; Tasks: mrpack
Root: HKA; Subkey: "Software\Classes\Calcite.mrpack"; ValueType: string; ValueName: ""; ValueData: "Modrinth modpack"; Flags: uninsdeletekey; Tasks: mrpack
Root: HKA; Subkey: "Software\Classes\Calcite.mrpack\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\calcite.exe,0"; Tasks: mrpack
Root: HKA; Subkey: "Software\Classes\Calcite.mrpack\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\calcite.exe"" ""%1"""; Tasks: mrpack

[Run]
Filename: "{app}\calcite.exe"; Description: "{cm:LaunchProgram,Calcite}"; Flags: nowait postinstall skipifsilent

; Instances, accounts and settings in %APPDATA%\Calcite are kept on uninstall.
