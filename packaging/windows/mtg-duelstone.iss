; Inno Setup 6 script for the Windows installer. build-windows.sh stages the game into one folder and
; passes AppVersion, StageDir (that folder) and OutputDir.
;
; Installs for the current user by default (%LOCALAPPDATA%\Programs, no administrator prompt); the
; first page offers all users instead. Card images are not part of it: the last page can start
; fetch-offline.exe, and the Start menu keeps a shortcut to it.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef StageDir
  #define StageDir "..\..\target\windows\MTG Duelstone"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\target\windows"
#endif

[Setup]
; Identifies the installation for upgrades and the uninstaller: never change it.
AppId={{8B0454AB-2AA5-4B32-980C-A38228A08F03}
AppName=MTG Duelstone
AppVersion={#AppVersion}
AppVerName=MTG Duelstone {#AppVersion}
AppPublisher=MTG Duelstone
AppPublisherURL=https://github.com/birkmann/mtg-duelstone
AppSupportURL=https://github.com/birkmann/mtg-duelstone/issues
DefaultDirName={autopf}\MTG Duelstone
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
SetupIconFile=mtg-duelstone.ico
UninstallDisplayIcon={app}\mtg-duelstone.exe
UninstallDisplayName=MTG Duelstone
OutputDir={#OutputDir}
OutputBaseFilename=mtg-duelstone-windows-x86_64-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern

[Tasks]
Name: desktopicon; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{#StageDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[InstallDelete]
; An upgrade replaces the bundled runtime and data instead of mixing old and new files.
Type: filesandordirs; Name: "{app}\jre"
Type: filesandordirs; Name: "{app}\forge"
Type: filesandordirs; Name: "{app}\assets"
Type: filesandordirs; Name: "{app}\web"

[Icons]
Name: "{autoprograms}\MTG Duelstone"; Filename: "{app}\mtg-duelstone.exe"
Name: "{autoprograms}\MTG Duelstone - Download card images"; Filename: "{app}\fetch-offline.exe"; IconFilename: "{app}\mtg-duelstone.exe"
Name: "{autodesktop}\MTG Duelstone"; Filename: "{app}\mtg-duelstone.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\fetch-offline.exe"; Description: "Download the card images now (about 9 GB)"; Flags: postinstall nowait skipifsilent unchecked
Filename: "{app}\mtg-duelstone.exe"; Description: "{cm:LaunchProgram,MTG Duelstone}"; Flags: postinstall nowait skipifsilent
