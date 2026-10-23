; ExoGPUAgent Windows installer (Inno Setup 6)
; Phase 2B — localhost only, native SCM service, no NVIDIA/Tailscale installers.

#define MyAppName "ExoGPUAgent"
#define MyAppVersion "0.2.0"
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
  KeepData: Boolean;

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
    'Localhost-only agent (Phase 2B).',
    'Configuration is stored in %ProgramData%\ExoGridTech\ExoGPUAgent\config.toml'#13#10 +
    'Existing files are never overwritten on upgrade.');
  HostPage.Add('Host name (optional):', False);
  HostPage.Add('HTTP port (default 12001):', False);
  HostPage.Values[0] := GetComputerNameString;
  HostPage.Values[1] := '12001';
end;

function NextButtonClick(CurPageID: Integer): Boolean;
var
  Port: Integer;
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
end;

procedure WriteConfigIfAbsent;
var
  Dir, Cfg, HostName, Port: String;
begin
  Dir := ExpandConstant('{commonappdata}\ExoGridTech\ExoGPUAgent');
  ForceDirectories(Dir);
  ForceDirectories(Dir + '\logs');
  Cfg := Dir + '\config.toml';
  if not FileExists(Cfg) then
  begin
    HostName := Trim(HostPage.Values[0]);
    Port := Trim(HostPage.Values[1]);
    if Port = '' then Port := '12001';
    SaveStringToFile(Cfg,
      'host_name = "' + HostName + '"'#13#10 +
      'bind = "127.0.0.1"'#13#10 +
      'port = ' + Port + #13#10 +
      'access_mode = "localhost"'#13#10 +
      'autostart = true'#13#10, False);
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
