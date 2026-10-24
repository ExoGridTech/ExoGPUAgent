; ExoGPUAgent Windows installer (Inno Setup 6)
; Phase 2B — localhost only, native SCM service, no NVIDIA/Tailscale installers.

#define MyAppName "ExoGPUAgent"
#define MyAppVersion "0.4.0"
#define MyAppPublisher "ExoGridTech"
#define MyAppURL "https://github.com/ExoGridTech/ExoGPUAgent"
#define MyAppExeName "exogpuagent.exe"

#ifndef SourceExe
  #define SourceExe "..\..\target\release\exogpuagent.exe"
#endif

[Setup]
AppId={{A3E7C2F1-8B4D-4E6A-9C21-0F7E2B91A402}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
LicenseFile=..\..\LICENSE
OutputDir=..\..\dist\windows-installer
OutputBaseFilename=ExoGPUAgent-Setup-v{#MyAppVersion}-win64
Compression=lzma
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
PrivilegesRequired=admin
CloseApplications=yes
UninstallDisplayIcon={app}\{#MyAppExeName}
SetupLogging=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "{#MyAppExeName}"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\docs\phase-2b-windows.md"; DestDir: "{app}\docs"; Flags: ignoreversion skipifsourcedoesntexist
Source: "scripts\Check-Port.ps1"; DestDir: "{app}\scripts"; Flags: ignoreversion
Source: "scripts\Verify-Health.ps1"; DestDir: "{app}\scripts"; Flags: ignoreversion
Source: "scripts\Bootstrap-GuiAuth.ps1"; DestDir: "{app}\scripts"; Flags: ignoreversion

[Icons]
Name: "{group}\{#MyAppName} Doctor"; Filename: "{app}\{#MyAppExeName}"; Parameters: "doctor"
Name: "{group}\{#MyAppName} Mini GUI Setup"; Filename: "{app}\{#MyAppExeName}"; Parameters: "gui setup"
Name: "{group}\Uninstall {#MyAppName}"; Filename: "{uninstallexe}"

[Run]
; Ensure machine data dirs exist with LocalService modify rights (config + logs only under ExoGPUAgent).
Filename: "{cmd}"; Parameters: "/C mkdir ""{commonappdata}\ExoGridTech\ExoGPUAgent\logs"" 2>nul & icacls ""{commonappdata}\ExoGridTech\ExoGPUAgent"" /grant ""NT AUTHORITY\LOCAL SERVICE:(OI)(CI)M"" /T"; Flags: runhidden waituntilterminated
; Fail install early if 127.0.0.1:port is taken by something other than a healthy ExoGPUAgent.
Filename: "{cmd}"; Parameters: "/C powershell -NoProfile -ExecutionPolicy Bypass -File ""{app}\scripts\Check-Port.ps1"""; StatusMsg: "Checking localhost port availability..."; Flags: runhidden waituntilterminated
Filename: "{app}\{#MyAppExeName}"; Parameters: "service install --exe ""{app}\{#MyAppExeName}"""; StatusMsg: "Registering Windows service (ExoGPUAgent only)..."; Flags: runhidden waituntilterminated
Filename: "{app}\{#MyAppExeName}"; Parameters: "service start"; StatusMsg: "Starting ExoGPUAgent service..."; Flags: runhidden waituntilterminated
Filename: "{cmd}"; Parameters: "/C powershell -NoProfile -ExecutionPolicy Bypass -File ""{app}\scripts\Verify-Health.ps1"""; StatusMsg: "Verifying /health on configured localhost port..."; Flags: runhidden waituntilterminated

[UninstallRun]
Filename: "{app}\{#MyAppExeName}"; Parameters: "service stop"; Flags: runhidden waituntilterminated; RunOnceId: StopSvc
Filename: "{app}\{#MyAppExeName}"; Parameters: "service uninstall"; Flags: runhidden waituntilterminated; RunOnceId: DelSvc

[Code]
var
  NvmlPage: TOutputMsgWizardPage;
  HostPage: TInputQueryWizardPage;
  GuiPage: TInputQueryWizardPage;
  KeepData: Boolean;
  IsFreshInstall: Boolean;

function NvmlPresent(): Boolean;
begin
  Result := FileExists(ExpandConstant('{sys}\nvml.dll')) or
            FileExists(ExpandConstant('{sys}\DriverStore\FileRepository\*nvml.dll'));
  { FileExists does not glob; also probe Program Files NVIDIA }
  if not Result then
    Result := DirExists('C:\Program Files\NVIDIA Corporation\NVSMI') or
              FileExists(ExpandConstant('{sys}\nvcuda.dll'));
end;

procedure InitializeWizard;
begin
  NvmlPage := CreateOutputMsgPage(wpWelcome,
    'NVIDIA / NVML check',
    'ExoGPUAgent needs an NVIDIA driver with NVML for live GPU metrics.',
    'This installer does NOT install NVIDIA drivers, CUDA, or Tailscale.'#13#10#13#10 +
    'If NVML is missing, the service can still install and listen on localhost,'#13#10 +
    'but /health and /gpu will return HTTP 503 until a driver is installed.');

  HostPage := CreateInputQueryPage(wpSelectDir,
    'Basic configuration',
    'Localhost-only agent.',
    'Configuration is stored in %ProgramData%\ExoGridTech\ExoGPUAgent\config.toml'#13#10 +
    'Existing files are never overwritten on upgrade.');
  HostPage.Add('Host name (optional):', False);
  HostPage.Add('HTTP port (default 12001):', False);
  HostPage.Values[0] := GetComputerNameString;
  HostPage.Values[1] := '12001';

  { Fresh-install only: optional Mini GUI. Hidden on upgrades (config already exists). }
  GuiPage := CreateInputQueryPage(HostPage.ID,
    'Local Mini GUI (optional)',
    'Fresh installs only — upgrades never change GUI settings.',
    'Optional localhost admin UI on 127.0.0.1:12010 (never Tailscale/LAN).'#13#10 +
    'Default is disabled. If enabling: password min 12 chars + confirmation.'#13#10 +
    'Leave Enable as no (or blank passwords) to skip.');
  GuiPage.Add('Enable Mini GUI? (yes/no, default no):', False);
  GuiPage.Add('Admin password (min 12 chars):', True);
  GuiPage.Add('Confirm admin password:', True);
  GuiPage.Values[0] := 'no';
  GuiPage.Values[1] := '';
  GuiPage.Values[2] := '';
end;

function ShouldSkipPage(PageID: Integer): Boolean;
var
  Cfg: String;
begin
  Result := False;
  if PageID = GuiPage.ID then
  begin
    Cfg := ExpandConstant('{commonappdata}\ExoGridTech\ExoGPUAgent\config.toml');
    { Skip GUI page on upgrade / reinstall when config already exists. }
    Result := FileExists(Cfg);
  end;
end;

function NextButtonClick(CurPageID: Integer): Boolean;
var
  Port: Integer;
  EnableGui, Pass, Confirm: String;
begin
  Result := True;
  if CurPageID = HostPage.ID then
  begin
    Port := StrToIntDef(Trim(HostPage.Values[1]), -1);
    if (Port < 1) or (Port > 65535) then
    begin
      MsgBox('Port must be a number between 1 and 65535.', mbError, MB_OK);
      Result := False;
    end;
  end;
  if CurPageID = GuiPage.ID then
  begin
    EnableGui := LowerCase(Trim(GuiPage.Values[0]));
    Pass := GuiPage.Values[1];
    Confirm := GuiPage.Values[2];
    if (EnableGui = 'yes') or (EnableGui = 'y') or (EnableGui = 'true') then
    begin
      if Length(Pass) < 12 then
      begin
        MsgBox('Admin password must be at least 12 characters, or set Enable to no.', mbError, MB_OK);
        Result := False;
      end
      else if Pass <> Confirm then
      begin
        MsgBox('Password and confirmation do not match.', mbError, MB_OK);
        Result := False;
      end;
    end;
  end;
end;

procedure WipeAndDeleteFile(const Path: String);
begin
  { Best-effort cleanup only — not a forensic secure erase. }
  if FileExists(Path) then
  begin
    SaveStringToFile(Path, 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx', False);
    DeleteFile(Path);
  end;
end;

procedure RestrictPassFileAcl(const Path: String);
var
  RC: Integer;
begin
  { Create-time ACL: drop inheritance; current user + SYSTEM only (SID S-1-5-18). }
  Exec(ExpandConstant('{sys}\icacls.exe'),
    '"' + Path + '" /inheritance:r /grant:r "' + GetUserNameString +
    '":F /grant:r *S-1-5-18:F',
    '', SW_HIDE, ewWaitUntilTerminated, RC);
end;

procedure WriteGuiConfig(const Cfg, HostName, Port, GuiEnabled: String);
begin
  { Blank line before [gui]: attach CRLF to an empty string — never start a line with #digits
    (ISCC preprocessor would treat that as a directive). }
  SaveStringToFile(Cfg,
    'host_name = "' + HostName + '"'#13#10 +
    'bind = "127.0.0.1"'#13#10 +
    'port = ' + Port + #13#10 +
    'access_mode = "localhost"'#13#10 +
    'autostart = true'#13#10 +
    ''#13#10 +
    '[gui]'#13#10 +
    'enabled = ' + GuiEnabled + #13#10 +
    'bind = "127.0.0.1"'#13#10 +
    'port = 12010'#13#10 +
    'poll_interval_secs = 5'#13#10, False);
end;

procedure WriteConfigIfAbsent;
var
  Dir, Cfg, HostName, Port, EnableGui, Pass, Confirm, Auth, PassFile, Ps1: String;
  ResultCode: Integer;
  WantGui: Boolean;
begin
  Dir := ExpandConstant('{commonappdata}\ExoGridTech\ExoGPUAgent');
  ForceDirectories(Dir);
  ForceDirectories(Dir + '\logs');
  Cfg := Dir + '\config.toml';
  IsFreshInstall := not FileExists(Cfg);
  { Upgrades: never overwrite config/auth; never re-enable GUI. }
  if not IsFreshInstall then
    Exit;

  HostName := Trim(HostPage.Values[0]);
  Port := Trim(HostPage.Values[1]);
  if Port = '' then Port := '12001';

  EnableGui := LowerCase(Trim(GuiPage.Values[0]));
  Pass := GuiPage.Values[1];
  Confirm := GuiPage.Values[2];
  WantGui := ((EnableGui = 'yes') or (EnableGui = 'y') or (EnableGui = 'true')) and
    (Length(Pass) >= 12) and (Pass = Confirm);

  { Always write gui.enabled=false first; flip to true only after secure bootstrap. }
  WriteGuiConfig(Cfg, HostName, Port, 'false');

  if not WantGui then
  begin
    Pass := '';
    Confirm := '';
    GuiPage.Values[1] := '';
    GuiPage.Values[2] := '';
    Exit;
  end;

  Auth := Dir + '\admin_auth.toml';
  if FileExists(Auth) then
  begin
    { Unexpected on fresh install — leave disabled rather than overwrite password. }
    MsgBox('admin_auth.toml already exists; Mini GUI left disabled. Use: exogpuagent gui setup',
      mbInformation, MB_OK);
    Pass := '';
    Confirm := '';
    GuiPage.Values[1] := '';
    GuiPage.Values[2] := '';
    Exit;
  end;

  // Temp under Setup tmp dir only — never ProgramData. Password never on argv.
  // (Do not put ExpandConstant names like tmp inside Pascal brace comments.)
  PassFile := ExpandConstant('{tmp}\ega_gui_pass.tmp');
  Ps1 := ExpandConstant('{app}\scripts\Bootstrap-GuiAuth.ps1');
  try
    { Empty file → restrictive ACL → then write secret (narrow window). }
    SaveStringToFile(PassFile, '', False);
    RestrictPassFileAcl(PassFile);
    SaveStringToFile(PassFile, Pass + #13#10, False);
    Pass := '';
    Confirm := '';
    GuiPage.Values[1] := '';
    GuiPage.Values[2] := '';
    if not Exec(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
      '-NoProfile -ExecutionPolicy Bypass -File "' + Ps1 +
      '" -ExePath "' + ExpandConstant('{app}\{#MyAppExeName}') +
      '" -PassFile "' + PassFile + '"',
      '', SW_HIDE, ewWaitUntilTerminated, ResultCode) or (ResultCode <> 0) then
    begin
      WipeAndDeleteFile(PassFile);
      MsgBox('WARNING: Mini GUI credential bootstrap failed (exit ' + IntToStr(ResultCode) + ').' + #13#10 +
        'Telemetry on 127.0.0.1 still works. GUI left disabled.'#13#10 +
        'Later: Start Menu → ExoGPUAgent Mini GUI Setup   or   exogpuagent gui setup',
        mbInformation, MB_OK);
      Exit;
    end;
    WipeAndDeleteFile(PassFile);
    if FileExists(Auth) then
      WriteGuiConfig(Cfg, HostName, Port, 'true')
    else
      MsgBox('WARNING: bootstrap reported success but admin_auth.toml is missing. GUI left disabled.',
        mbInformation, MB_OK);
  except
    WipeAndDeleteFile(PassFile);
    Pass := '';
    Confirm := '';
    GuiPage.Values[1] := '';
    GuiPage.Values[2] := '';
    MsgBox('WARNING: Mini GUI setup failed unexpectedly. GUI left disabled. Run: exogpuagent gui setup',
      mbInformation, MB_OK);
  end;
end;

procedure DeinitializeSetup;
begin
  { Cancel / early exit / success: ensure Setup-temp pass file is gone. }
  WipeAndDeleteFile(ExpandConstant('{tmp}\ega_gui_pass.tmp'));
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssInstall then
  begin
    if not NvmlPresent() then
      MsgBox('WARNING: NVIDIA/NVML was not clearly detected.'#13#10#13#10 +
        'The service will install and listen on 127.0.0.1 only, but /health and /gpu may return 503 until a driver is installed.'#13#10 +
        'ExoGPUAgent does not install NVIDIA drivers, CUDA, or Tailscale.',
        mbInformation, MB_OK);
  end;
  if CurStep = ssPostInstall then
    WriteConfigIfAbsent;
end;

function InitializeUninstall(): Boolean;
begin
  KeepData := MsgBox(
    'Keep ExoGPUAgent configuration and logs under ProgramData?'#13#10#13#10 +
    'Yes = keep config/logs'#13#10 +
    'No = delete config/logs',
    mbConfirmation, MB_YESNO) = IDYES;
  Result := True;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if (CurUninstallStep = usPostUninstall) and (not KeepData) then
  begin
    DelTree(ExpandConstant('{commonappdata}\ExoGridTech\ExoGPUAgent'), True, True, True);
  end;
end;
