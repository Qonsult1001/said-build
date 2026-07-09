; Inno Setup script for said (Windows installer).
; Compiled in CI by iscc; produces said-setup-<version>-x64.exe.
; Installs said.exe + said-mcp.exe, adds the install dir to the user PATH, and
; registers an uninstaller. No admin required (per-user install).
;
; The compiler is invoked as:
;   iscc /DMyAppVersion=0.11.1 /DBinDir=..\..\dist packaging\windows\said.iss

#ifndef MyAppVersion
  #define MyAppVersion "0.0.0"
#endif
#ifndef BinDir
  #define BinDir "..\..\dist"
#endif

[Setup]
AppId={{7C3E9F2A-6B1D-4E58-9A2C-SAIDBRAIN0001}
AppName=said
AppVersion={#MyAppVersion}
AppPublisher=said
AppPublisherURL=https://github.com/Qonsult1001/said-build
DefaultDirName={localappdata}\Programs\said
DefaultGroupName=said
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputBaseFilename=said-setup-{#MyAppVersion}-x64
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ChangesEnvironment=yes

[Files]
Source: "{#BinDir}\said.exe";     DestDir: "{app}"; Flags: ignoreversion
Source: "{#BinDir}\said-mcp.exe"; DestDir: "{app}"; Flags: ignoreversion

[Tasks]
Name: "addtopath"; Description: "Add said to my PATH (so `said` works in any terminal)"; GroupDescription: "Setup:"

[Registry]
; Append the install dir to the USER Path when the task is selected.
Root: HKCU; Subkey: "Environment"; ValueType: expandsz; ValueName: "Path"; \
  ValueData: "{olddata};{app}"; Check: NeedsAddPath('{app}'); Tasks: addtopath

[Icons]
Name: "{group}\said (help)"; Filename: "{cmd}"; Parameters: "/k said --help"

[Code]
function NeedsAddPath(Param: string): Boolean;
var
  OrigPath: string;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', OrigPath) then
  begin
    Result := True; exit;
  end;
  Result := Pos(';' + ExpandConstant(Param) + ';', ';' + OrigPath + ';') = 0;
end;
