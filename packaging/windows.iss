[Setup]
AppId=Lamina
AppName=Lamina
AppVersion={#Version}
DefaultDirName={localappdata}\Programs\Lamina
PrivilegesRequired=lowest
UninstallDisplayIcon={app}\lamina-viewer.exe
SetupIconFile=..\assets\icon.ico
OutputDir=..\target\installers
OutputBaseFilename=lamina-{#Version}-{#Arch}-setup
Compression=lzma2/fast
WizardStyle=modern
[Files]
Source: "{#Bin}\lamina.exe"; DestDir: "{app}"
Source: "{#Bin}\lamina-viewer.exe"; DestDir: "{app}"
[Icons]
Name: "{userprograms}\Lamina"; Filename: "{app}\lamina-viewer.exe"; WorkingDir: "{app}"
