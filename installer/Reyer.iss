; Inno Setup script. Build: powershell -ExecutionPolicy Bypass -File build.ps1 -Installer
; Expects the portable folder dist\Reyer to exist (build.ps1 creates it).
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{6B1F3A52-9C7E-4D58-8E0A-2F4C7D91B3A6}
AppName=Reyer
AppVersion={#AppVersion}
AppPublisher=Artem Shevchenko
AppPublisherURL=https://github.com/reyjendiz/Reyer
DefaultDirName={autopf}\Reyer
DefaultGroupName=Reyer
DisableProgramGroupPage=yes
UninstallDisplayIcon={app}\Reyer.exe
SetupIconFile=..\assets\icon\reyer.ico
LicenseFile=..\LICENSE
OutputDir=..\dist
OutputBaseFilename=Reyer-{#AppVersion}-windows-x64-setup
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
Compression=lzma2/ultra
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"
Name: "ukrainian"; MessagesFile: "compiler:Languages\Ukrainian.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; Flags: unchecked
Name: "assoc"; Description: "Register Reyer for video and audio files (choose it in Default apps afterwards)"

[Files]
Source: "..\dist\Reyer\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\Reyer"; Filename: "{app}\Reyer.exe"
Name: "{autodesktop}\Reyer"; Filename: "{app}\Reyer.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\Reyer.exe"; Description: "{cm:LaunchProgram,Reyer}"; Flags: nowait postinstall skipifsilent
; silent installs come from the in-app updater: start the new version again
Filename: "{app}\Reyer.exe"; Flags: nowait runasoriginaluser skipifnotsilent

[UninstallDelete]
Type: filesandordirs; Name: "{app}"

[Code]
const
  VideoExts = '.mov .qt .m4v .mp4 .mkv .avi .wmv .webm .flv .ts .m2ts .mts .vob .3gp .3g2 .mpg .mpeg .m2v .ogv .asf .mxf .f4v .divx .rm .rmvb';
  AudioExts = '.m4a .m4b .m4r .caf .aif .aiff .aifc .mp3 .flac .wav .aac .ogg .oga .opus .wma .ape .wv .ac3 .eac3 .dts .mka .alac .mp2 .tta';

// same registrations as tools\register_associations.ps1, under HKCU or HKLM depending on install mode
procedure SetProgId(Id, Name, IconFile: String);
var
  Key: String;
begin
  Key := 'Software\Classes\' + Id;
  RegWriteStringValue(HKA, Key, '', Name);
  RegWriteStringValue(HKA, Key, 'FriendlyTypeName', Name);
  RegWriteStringValue(HKA, Key + '\DefaultIcon', '', ExpandConstant('{app}\icons\') + IconFile);
  RegWriteStringValue(HKA, Key + '\shell\open\command', '', '"' + ExpandConstant('{app}\Reyer.exe') + '" "%1"');
end;

procedure ForEachExt(List, ProgId: String; Remove: Boolean);
var
  S, Ext: String;
  P: Integer;
begin
  S := List + ' ';
  while S <> '' do
  begin
    P := Pos(' ', S);
    Ext := Copy(S, 1, P - 1);
    Delete(S, 1, P);
    if Ext = '' then Continue;
    if Remove then
      RegDeleteValue(HKA, 'Software\Classes\' + Ext + '\OpenWithProgids', ProgId)
    else
    begin
      RegWriteStringValue(HKA, 'Software\Reyer\Capabilities\FileAssociations', Ext, ProgId);
      RegWriteStringValue(HKA, 'Software\Classes\' + Ext + '\OpenWithProgids', ProgId, '');
    end;
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssPostInstall) and WizardIsTaskSelected('assoc') then
  begin
    SetProgId('Reyer.Video', 'Video (Reyer)', 'reyer_video.ico');
    SetProgId('Reyer.Audio', 'Audio (Reyer)', 'reyer_audio.ico');
    RegWriteStringValue(HKA, 'Software\Reyer\Capabilities', 'ApplicationName', 'Reyer');
    RegWriteStringValue(HKA, 'Software\Reyer\Capabilities', 'ApplicationDescription', 'A small Liquid Glass video and audio player.');
    ForEachExt(VideoExts, 'Reyer.Video', False);
    ForEachExt(AudioExts, 'Reyer.Audio', False);
    RegWriteStringValue(HKA, 'Software\RegisteredApplications', 'Reyer', 'Software\Reyer\Capabilities');
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
  begin
    ForEachExt(VideoExts, 'Reyer.Video', True);
    ForEachExt(AudioExts, 'Reyer.Audio', True);
    RegDeleteKeyIncludingSubkeys(HKA, 'Software\Classes\Reyer.Video');
    RegDeleteKeyIncludingSubkeys(HKA, 'Software\Classes\Reyer.Audio');
    RegDeleteKeyIncludingSubkeys(HKA, 'Software\Reyer');
    RegDeleteValue(HKA, 'Software\RegisteredApplications', 'Reyer');
  end;
end;
