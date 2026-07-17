#ifndef AppVersion
  #error AppVersion must be supplied by the package command.
#endif
#define OutputName "BabelForge-eXecutor-" + AppVersion + "-win-Setup"

[Setup]
AppId={{29D1334D-14E5-4A01-AEF3-2C1BFC81E08B}
AppName=BabelForge eXecutor
AppVersion={#AppVersion}
AppPublisher=BabelForge
AppCopyright=Copyright (C) 2026 BabelForge
DefaultDirName={localappdata}\BabelForge eXecutor
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\dist
OutputBaseFilename={#OutputName}
Compression=lzma2/ultra64
SolidCompression=yes
ChangesEnvironment=yes
UninstallDisplayName=BabelForge eXecutor
UninstallDisplayIcon={app}\bfx.exe
VersionInfoVersion={#AppVersion}
VersionInfoProductName=BabelForge eXecutor
VersionInfoDescription=BabelForge eXecutor command-line interface

[Files]
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\THIRD_PARTY_NOTICES.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\bfx.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\.pixi\envs\runtime\*"; DestDir: "{app}\runtime"; Excludes: "__pycache__\*,*.pyc,*.pdb,test\*,tests\*,testing\*"; Flags: ignoreversion recursesubdirs createallsubdirs

[Code]
const
  UserEnvironmentKey = 'Environment';
  UserPathValue = 'Path';

function NormalizePath(Value: String): String;
begin
  Result := Lowercase(Trim(Value));
  while (Length(Result) > 3) and (Result[Length(Result)] = '\') do
    Delete(Result, Length(Result), 1);
end;

function PathContains(const PathValue, Entry: String): Boolean;
var
  Remaining, Part: String;
  Separator: Integer;
begin
  Result := False;
  Remaining := PathValue;
  while Remaining <> '' do
  begin
    Separator := Pos(';', Remaining);
    if Separator = 0 then
    begin
      Part := Remaining;
      Remaining := '';
    end
    else
    begin
      Part := Copy(Remaining, 1, Separator - 1);
      Delete(Remaining, 1, Separator);
    end;

    if NormalizePath(Part) = NormalizePath(Entry) then
    begin
      Result := True;
      Exit;
    end;
  end;
end;

function RemovePathEntry(const PathValue, Entry: String): String;
var
  Remaining, Part: String;
  Separator: Integer;
begin
  Result := '';
  Remaining := PathValue;
  while Remaining <> '' do
  begin
    Separator := Pos(';', Remaining);
    if Separator = 0 then
    begin
      Part := Remaining;
      Remaining := '';
    end
    else
    begin
      Part := Copy(Remaining, 1, Separator - 1);
      Delete(Remaining, 1, Separator);
    end;

    if (Part <> '') and (NormalizePath(Part) <> NormalizePath(Entry)) then
    begin
      if Result <> '' then
        Result := Result + ';';
      Result := Result + Part;
    end;
  end;
end;

procedure AddInstallDirectoryToPath;
var
  PathValue: String;
begin
  if not RegQueryStringValue(HKCU, UserEnvironmentKey, UserPathValue, PathValue) then
    PathValue := '';

  if not PathContains(PathValue, ExpandConstant('{app}')) then
  begin
    if PathValue <> '' then
      PathValue := PathValue + ';';
    RegWriteExpandStringValue(HKCU, UserEnvironmentKey, UserPathValue,
      PathValue + ExpandConstant('{app}'));
  end;
end;

procedure RemoveInstallDirectoryFromPath;
var
  Existing, Updated: String;
begin
  if RegQueryStringValue(HKCU, UserEnvironmentKey, UserPathValue, Existing) then
  begin
    Updated := RemovePathEntry(Existing, ExpandConstant('{app}'));
    if Updated <> Existing then
    begin
      if Updated = '' then
        RegDeleteValue(HKCU, UserEnvironmentKey, UserPathValue)
      else
        RegWriteExpandStringValue(HKCU, UserEnvironmentKey, UserPathValue, Updated);
    end;
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    AddInstallDirectoryToPath;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RemoveInstallDirectoryFromPath;
end;
