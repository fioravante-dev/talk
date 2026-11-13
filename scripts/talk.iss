; O instalador do talk no Windows (Inno Setup 6). Não chame direto: o
; scripts\package-windows.ps1 compila, junta as DLLs do CUDA e passa a versão.
;
; Instala por usuário, sem pedir administrador, em %LOCALAPPDATA%\Programs\talk
; (o mesmo lugar do install-windows.ps1), liga o início no login e baixa os
; modelos para %LOCALAPPDATA%\talk\models se ainda não estiverem lá.
; Desinstalar deixa config, histórico e modelos.

#ifndef Version
  #error "passe /DVersion=x.y.z (o package-windows.ps1 faz isso)"
#endif
#ifndef Dist
  #define Dist "..\target\dist"
#endif

[Setup]
AppId={{6C1E0F7A-3B2D-4E59-9A0B-7D4C2E1F5A88}
AppName=talk
AppVersion={#Version}
AppPublisher=talk
AppPublisherURL=https://github.com/fioravante-dev/talk
DefaultDirName={localappdata}\Programs\talk
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir={#Dist}
OutputBaseFilename=talk-setup-{#Version}
Compression=lzma2/ultra64
SolidCompression=yes
LZMAUseSeparateProcess=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
UninstallDisplayIcon={app}\talk-ui.exe
UninstallDisplayName=talk
CloseApplications=no
WizardStyle=modern

[Languages]
Name: "pt"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "model"; Description: "Baixar o modelo de transcrição (Whisper large-v3 q5, 1 GB)"; Check: not ModelPresent
Name: "autostart"; Description: "Abrir o talk ao entrar no Windows"

[Files]
Source: "{#Dist}\talk-rs.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Dist}\talk-start.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Dist}\talk-ui.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Dist}\talk-pill.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Dist}\cublas*64_*.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\talk"; Filename: "{app}\talk-start.exe"; Parameters: "--open"; IconFilename: "{app}\talk-ui.exe"

[Registry]
; O mesmo valor que o `talk-rs autostart on` escreve.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "talk"; ValueData: """{app}\talk-start.exe"""; Tasks: autostart; Flags: uninsdeletevalue

[Run]
Filename: "{app}\talk-start.exe"; Parameters: "--open"; Description: "Abrir o talk agora"; Flags: nowait postinstall

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM talk-rs.exe /IM talk-ui.exe /IM talk-pill.exe"; Flags: runhidden; RunOnceId: "stop"

[UninstallDelete]
Type: filesandordirs; Name: "{app}\talk-ui.exe.WebView2"
Type: filesandordirs; Name: "{app}\talk-pill.exe.WebView2"

[Code]
const
  HF = 'https://huggingface.co/';
  MODEL = 'ggml-large-v3-q5_0.bin';
  VAD = 'ggml-silero-v5.1.2.bin';

var
  Downloads: TDownloadWizardPage;

function ModelsDir: String;
begin
  Result := ExpandConstant('{localappdata}\talk\models');
end;

function ModelPresent: Boolean;
begin
  Result := FileExists(ModelsDir + '\' + MODEL);
end;

procedure InitializeWizard;
begin
  Downloads := CreateDownloadPage(SetupMessage(msgWizardPreparing), 'Baixando os modelos do Hugging Face...', nil);
end;

{ O Windows não deixa sobrescrever um .exe em uso: o talk sai antes da cópia. }
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Code: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM talk-rs.exe /IM talk-ui.exe /IM talk-pill.exe', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Sleep(500);
  DeleteFile(ExpandConstant('{localappdata}\talk\talk-rs.sock'));
  DeleteFile(ExpandConstant('{localappdata}\talk\talk-ui.sock'));
  Result := '';
end;

{ Os modelos baixam para a pasta temporária e só então vão para o lugar:
  um download interrompido não deixa um modelo pela metade. }
function NextButtonClick(CurPageID: Integer): Boolean;
var
  Want: Boolean;
begin
  Result := True;
  if CurPageID <> wpReady then Exit;
  Want := WizardIsTaskSelected('model');
  if FileExists(ModelsDir + '\' + VAD) and not Want then Exit;
  Downloads.Clear;
  if not FileExists(ModelsDir + '\' + VAD) then
    Downloads.Add(HF + 'ggml-org/whisper-vad/resolve/main/' + VAD, VAD, '');
  if Want then
    Downloads.Add(HF + 'ggerganov/whisper.cpp/resolve/main/' + MODEL, MODEL, '');
  Downloads.Show;
  try
    try
      Downloads.Download;
      ForceDirectories(ModelsDir);
      if FileExists(ExpandConstant('{tmp}\' + VAD)) then
        FileCopy(ExpandConstant('{tmp}\' + VAD), ModelsDir + '\' + VAD, False);
      if Want then
        FileCopy(ExpandConstant('{tmp}\' + MODEL), ModelsDir + '\' + MODEL, False);
    except
      if Downloads.AbortedByUser then
        Log('download cancelado')
      else
        SuppressibleMsgBox('Não deu para baixar os modelos: ' + GetExceptionMessage + #13#10#13#10 +
          'O talk instala assim mesmo; baixe depois como diz o README.', mbError, MB_OK, IDOK);
    end;
  finally
    Downloads.Hide;
  end;
end;

{ O início no login pode ter sido ligado depois, pelo `talk-rs autostart on`. }
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RegDeleteValue(HKEY_CURRENT_USER, 'Software\Microsoft\Windows\CurrentVersion\Run', 'talk');
end;
