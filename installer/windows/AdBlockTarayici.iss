; Inno Setup 6 Script - AdBlock Tarayıcı (Python PyQt6 Edition)
#define MyAppName "AdBlock Tarayıcı"
#define MyAppVersion "0.1.0"
#define MyAppPublisher "AdBlock Browser"
#define MyAppExeName "AdBlockTarayici.exe"

[Setup]
AppId={{C3A91D2E-4F7B-48E1-9D3A-7C8F12B5E609}}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={autopf}\AdBlockTarayici
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
OutputDir=..\..\dist\installer
OutputBaseFilename=AdBlockTarayici-Setup-{#MyAppVersion}-x64
SetupIconFile=..\..\rust-browser\assets\app.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
WizardStyle=modern
WizardResizable=no
WizardImageFile=..\..\rust-browser\assets\installer\wizard_sidebar.bmp,..\..\rust-browser\assets\installer\wizard_sidebar_200.bmp
WizardSmallImageFile=..\..\rust-browser\assets\installer\wizard_header.bmp,..\..\rust-browser\assets\installer\wizard_header_200.bmp
Compression=lzma2/ultra64
SolidCompression=yes
ArchitecturesAllowed=x64
ArchitecturesInstallIn64BitMode=x64
VersionInfoVersion={#MyAppVersion}.0
VersionInfoProductName={#MyAppName}
VersionInfoDescription={#MyAppName} Kurulum Sihirbazı

[Languages]
Name: "turkish"; MessagesFile: "compiler:Languages\Turkish.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "..\..\dist\AdBlockTarayici\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Registry]
Root: HKA; Subkey: "Software\Microsoft\Windows\CurrentVersion\App Paths\{#MyAppExeName}"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppExeName}"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Microsoft\Windows\CurrentVersion\App Paths\{#MyAppExeName}"; ValueType: string; ValueName: "Path"; ValueData: "{app}"

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent
