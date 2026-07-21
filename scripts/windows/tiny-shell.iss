#ifndef MyAppVersion
  #define MyAppVersion "0.0.0"
#endif

#ifndef SourceExe
  #error SourceExe must point to the compiled tiny-shell.exe
#endif

#ifndef SetupIcon
  #error SetupIcon must point to tiny-shell.ico
#endif

#ifndef LicenseFile
  #error LicenseFile must point to the project license
#endif

#ifndef OutputDir
  #define OutputDir "."
#endif

#ifndef OutputBaseFilename
  #define OutputBaseFilename "tiny-shell-setup"
#endif

[Setup]
AppId={{8E091D1C-6D7C-4C29-9CA2-8B3D84A42CF8}
AppName=tiny-shell
AppVersion={#MyAppVersion}
AppVerName=tiny-shell {#MyAppVersion}
AppPublisher=tiny-shell contributors
AppPublisherURL=https://github.com/ynx-official/tiny-shell
AppSupportURL=https://github.com/ynx-official/tiny-shell/issues
AppUpdatesURL=https://github.com/ynx-official/tiny-shell/releases
DefaultDirName={localappdata}\Programs\tiny-shell
DefaultGroupName=tiny-shell
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#OutputDir}
OutputBaseFilename={#OutputBaseFilename}
SetupIconFile={#SetupIcon}
UninstallDisplayIcon={app}\tiny-shell.exe
LicenseFile={#LicenseFile}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "chinesesimplified"; MessagesFile: "compiler:Languages\ChineseSimplified.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "tiny-shell.exe"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\tiny-shell"; Filename: "{app}\tiny-shell.exe"
Name: "{autodesktop}\tiny-shell"; Filename: "{app}\tiny-shell.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\tiny-shell.exe"; Description: "{cm:LaunchProgram,tiny-shell}"; Flags: nowait postinstall skipifsilent
