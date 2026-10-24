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

[Icons]
Name: "{group}\{#MyAppName} Doctor"; Filename: "{app}\{#MyAppExeName}"; Parameters: "doctor"
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
    'Enable a localhost admin UI on 127.0.0.1:12010 after install.'#13#10 +
    'Leave password blank to skip GUI (default). Min 12 characters if enabled.');
  GuiPage.Add('Enable Mini GUI? (yes/no, default no):', False);
  GuiPage.Add('Admin password (min 12 chars, blank = skip GUI):', True);
  GuiPage.Values[0] := 'no';
  GuiPage.Values[1] := '';
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
  EnableGui, Pass: String;
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
    if (EnableGui = 'yes') or (EnableGui = 'y') or (EnableGui = 'true') then
    begin
      if Length(Pass) < 12 then
      begin
        MsgBox('Admin password must be at least 12 characters, or set Enable to no.', mbError, MB_OK);
        Result := False;
      end;
    end;
  end;
end;

procedure WriteConfigIfAbsent;
var
  Dir, Cfg, HostName, Port, EnableGui, Pass, Auth, PassFile: String;
  ResultCode: Integer;
  GuiEnabled: String;
begin
  Dir := ExpandConstant('{commonappdata}\ExoGridTech\ExoGPUAgent');
  ForceDirectories(Dir);
  ForceDirectories(Dir + '\logs');
  Cfg := Dir + '\config.toml';
  IsFreshInstall := not FileExists(Cfg);
  if not IsFreshInstall then
    Exit;

  HostName := Trim(HostPage.Values[0]);
  Port := Trim(HostPage.Values[1]);
  if Port = '' then Port := '12001';

  EnableGui := LowerCase(Trim(GuiPage.Values[0]));
  Pass := GuiPage.Values[1];
  GuiEnabled := 'false';
  if ((EnableGui = 'yes') or (EnableGui = 'y') or (EnableGui = 'true')) and (Length(Pass) >= 12) then
    GuiEnabled := 'true';

  SaveStringToFile(Cfg,
    'host_name = "' + HostName + '"'#13#10 +
    'bind = "127.0.0.1"'#13#10 +
    'port = ' + Port + #13#10 +
    'access_mode = "localhost"'#13#10 +
    'autostart = true'#13#10 +
    #13#10 +
    '[gui]'#13#10 +
    'enabled = ' + GuiEnabled + #13#10 +
    'bind = "127.0.0.1"'#13#10 +
    'port = 12010'#13#10 +
    'poll_interval_secs = 5'#13#10, False);

  { Bootstrap admin_auth.toml only on fresh install when GUI enabled.
    Prefer --password-stdin so the password is not visible on the process command line. }
  if GuiEnabled = 'true' then
  begin
    Auth := Dir + '\admin_auth.toml';
    if not FileExists(Auth) then
    begin
      PassFile := Dir + '\.gui_init_pass.tmp';
      SaveStringToFile(PassFile, Pass + #13#10, False);
      if not Exec(ExpandConstant('{cmd}'),
        '/C type "' + PassFile + '" | "' + ExpandConstant('{app}\{#MyAppExeName}') +
        '" gui init --machine --password-stdin',
        '', SW_HIDE, ewWaitUntilTerminated, ResultCode) or (ResultCode <> 0) then
        MsgBox('WARNING: Mini GUI password bootstrap failed (exit ' + IntToStr(ResultCode) + ').' + #13#10 +
          'Telemetry still works. Run later: exogpuagent gui init --machine --password-stdin',
          mbInformation, MB_OK);
      DeleteFile(PassFile);
    end;
  end;
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
