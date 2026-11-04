; Instalador do Conver (Inno Setup 6). Gerado pelo build.ps1, que passa /DVersao=x.y.z e /DOrigem=<pasta Conver>.

#ifndef Versao
  #define Versao "1.0.0"
#endif
#ifndef Origem
  #define Origem "..\Conver"
#endif
#ifndef Repo
  #define Repo "https://github.com/JhonyGoncalves/conver"
#endif

[Setup]
AppId={{6F1C2B4E-7A3D-4C8E-9B21-C0A5E3D7F812}
AppName=Conver
AppVersion={#Versao}
AppVerName=Conver {#Versao}
AppPublisher=JhonyGoncalves
AppPublisherURL={#Repo}
AppSupportURL={#Repo}/issues
AppUpdatesURL={#Repo}/releases
; instala só para o usuário atual: não pede administrador e o Conver consegue atualizar o yt-dlp sozinho
PrivilegesRequired=lowest
DefaultDirName={localappdata}\Programs\Conver
DefaultGroupName=Conver
DisableProgramGroupPage=yes
UninstallDisplayIcon={app}\Conver.exe
UninstallDisplayName=Conver
LicenseFile=..\LICENSE
SetupIconFile=..\assets\conver.ico
WizardStyle=modern
OutputDir=..\dist
OutputBaseFilename=Conver-{#Versao}-instalador
Compression=lzma2/ultra64
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=yes

[Languages]
Name: "pt"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"

[Tasks]
Name: "desktopicon"; Description: "Criar atalho na Área de Trabalho"; GroupDescription: "Atalhos:"
Name: "contextmenu"; Description: "Adicionar ""Abrir com Conver"" ao menu do botão direito"; GroupDescription: "Integração com o Windows:"

[Files]
Source: "{#Origem}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs; Excludes: "portatil.txt,dados\*"

[Icons]
Name: "{group}\Conver"; Filename: "{app}\Conver.exe"
Name: "{group}\Desinstalar o Conver"; Filename: "{uninstallexe}"
Name: "{autodesktop}\Conver"; Filename: "{app}\Conver.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\Conver.exe"; Parameters: "--registrar-menu"; Flags: runhidden; Tasks: contextmenu
Filename: "{app}\Conver.exe"; Description: "Abrir o Conver agora"; Flags: nowait postinstall skipifsilent
; atualização feita de dentro do Conver (instalador silencioso com /RELANCAR=1): reabre o app no fim
Filename: "{app}\Conver.exe"; Flags: nowait; Check: Relancar

[Code]
function Relancar: Boolean;
begin
  Result := ExpandConstant('{param:RELANCAR|0}') = '1';
end;

[UninstallRun]
Filename: "{app}\Conver.exe"; Parameters: "--remover-menu"; Flags: runhidden; RunOnceId: "RemoverMenu"

[UninstallDelete]
; o yt-dlp pode ter sido atualizado pelo próprio Conver
Type: files; Name: "{app}\bin\yt-dlp.exe.antigo"
Type: files; Name: "{app}\bin\yt-dlp.exe.novo"
