; 9/IB SMP installer for Windows (Inno Setup 6): Calcite built with --features ibsmp.
; scripts/package-windows.ps1 -Edition ibsmp passes AppVersion and the binaries.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef Binary
  #define Binary "..\..\target\release\calcite.exe"
#endif
#ifndef LoginBinary
  #define LoginBinary "..\..\target\release\calcite-login.exe"
#endif

[Setup]
; Its own id: installs next to Calcite, never over it.
AppId={{3F6B1C2A-9D4E-4B7A-8C15-91B0E2D7A4C6}
AppName=9/IB SMP
AppVersion={#AppVersion}
AppVerName=9/IB SMP {#AppVersion}
AppPublisher=ByteV0rtex
AppPublisherURL=https://status.bytev0rtex.com
DefaultDirName={autopf}\9IB SMP
DisableProgramGroupPage=yes
; Installs for the current user without asking for admin.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\..\dist
OutputBaseFilename=9IB-SMP-{#AppVersion}-windows-x64-setup
SetupIconFile=9ib.ico
UninstallDisplayIcon={app}\9IB SMP.exe
UninstallDisplayName=9/IB SMP
LicenseFile=..\..\LICENSE
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes

[Tasks]
; On by default: friends start it from the desktop.
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{#Binary}"; DestDir: "{app}"; DestName: "9IB SMP.exe"; Flags: ignoreversion
; Microsoft sign-in window; the launcher looks for it next to itself.
Source: "{#LoginBinary}"; DestDir: "{app}"; DestName: "calcite-login.exe"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\9IB SMP"; Filename: "{app}\9IB SMP.exe"
Name: "{autodesktop}\9IB SMP"; Filename: "{app}\9IB SMP.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\9IB SMP.exe"; Description: "{cm:LaunchProgram,9/IB SMP}"; Flags: nowait postinstall skipifsilent

; The 9/IB instance, accounts and settings live in %APPDATA%\Calcite (shared
; with Calcite on purpose) and are kept on uninstall.
