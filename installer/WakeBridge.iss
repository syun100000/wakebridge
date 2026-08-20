#ifndef MyAppVersion
#define MyAppVersion "0.1.0"
#endif

#ifndef MyAppSource
#define MyAppSource "payload\wakebridge.exe"
#endif

#ifndef MyOutputDir
#define MyOutputDir "..\dist"
#endif

#define MyAppName "WakeBridge"
#define MyAppPublisher "WakeBridge contributors"
#define MyAppURL "https://github.com/syun100000/wakebridge"
#define MyAppExeName "wakebridge.exe"

[Setup]
AppId={{A9E9A7AC-4D94-4F25-8C6B-0D501A0F5C40}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}/releases
DefaultDirName={autopf}\WakeBridge
DisableDirPage=yes
DisableProgramGroupPage=yes
DefaultGroupName={#MyAppName}
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#MyOutputDir}
OutputBaseFilename=WakeBridge-Setup-{#MyAppVersion}-x64
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
Uninstallable=yes
UninstallDisplayName={#MyAppName}
UninstallDisplayIcon={app}\{#MyAppExeName}
AppReadmeFile={app}\docs\README.ja.md
AllowNoIcons=yes
ChangesAssociations=no
AlwaysRestart=no
CloseApplications=no

[Languages]
Name: "japanese"; MessagesFile: "compiler:Languages\Japanese.isl"

[Dirs]
Name: "{commonappdata}\WakeBridge"

[Files]
Source: "{#MyAppSource}"; DestDir: "{app}"; DestName: "{#MyAppExeName}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}\docs"; DestName: "README.md"; Flags: ignoreversion
Source: "..\README.ja.md"; DestDir: "{app}\docs"; DestName: "README.ja.md"; Flags: ignoreversion
Source: "..\README.en.md"; DestDir: "{app}\docs"; DestName: "README.en.md"; Flags: ignoreversion
Source: "..\docs\OPERATIONS.ja.md"; DestDir: "{app}\docs"; DestName: "OPERATIONS.ja.md"; Flags: ignoreversion
Source: "..\docs\DEVELOPMENT.ja.md"; DestDir: "{app}\docs"; DestName: "DEVELOPMENT.ja.md"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE"; Flags: ignoreversion
Source: "..\LICENSE-MIT"; DestDir: "{app}"; DestName: "LICENSE-MIT"; Flags: ignoreversion
Source: "..\LICENSE-APACHE"; DestDir: "{app}"; DestName: "LICENSE-APACHE"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#MyAppName}\操作ドキュメント"; Filename: "{app}\docs\OPERATIONS.ja.md"
Name: "{autoprograms}\{#MyAppName}\開発ドキュメント"; Filename: "{app}\docs\DEVELOPMENT.ja.md"
Name: "{autoprograms}\{#MyAppName}\ローカル操作画面"; Filename: "{sys}\rundll32.exe"; Parameters: "url.dll,FileProtocolHandler http://127.0.0.1:8787"

[Run]
Filename: "{app}\{#MyAppExeName}"; Parameters: "service install"; StatusMsg: "WakeBridge Serviceを登録しています..."; Flags: runhidden waituntilterminated
Filename: "{app}\{#MyAppExeName}"; Parameters: "service start"; StatusMsg: "WakeBridge Serviceを起動しています..."; Flags: runhidden waituntilterminated

[UninstallDelete]
Type: filesandordirs; Name: "{commonappdata}\WakeBridge"; Check: ShouldDeleteData

[Code]
var
  DeleteDataRequested: Boolean;

function ServiceExists(): Boolean;
var
  ResultCode: Integer;
begin
  if not Exec(ExpandConstant('{sys}\sc.exe'), 'query WakeBridge', '', SW_HIDE,
    ewWaitUntilTerminated, ResultCode) then
  begin
    Result := True;
    Exit;
  end;
  if ResultCode = 0 then
    Result := True
  else if ResultCode = 1060 then
    Result := False
  else
    Result := True;
end;

function WakeBridgeExecutableExists(): Boolean;
begin
  Result := FileExists(ExpandConstant('{app}\{#MyAppExeName}'));
end;

function RunWakeBridge(const Parameters: String): Boolean;
var
  ResultCode: Integer;
begin
  Result := Exec(ExpandConstant('{app}\{#MyAppExeName}'), Parameters, '', SW_HIDE,
    ewWaitUntilTerminated, ResultCode) and (ResultCode = 0);
end;

function WaitForServiceAbsent(): Boolean;
var
  ResultCode: Integer;
  Attempts: Integer;
begin
  Result := False;
  for Attempts := 1 to 120 do
  begin
    if not Exec(ExpandConstant('{sys}\sc.exe'), 'query WakeBridge', '', SW_HIDE,
      ewWaitUntilTerminated, ResultCode) then
    begin
      Exit;
    end;
    if ResultCode = 1060 then
    begin
      Result := True;
      Exit;
    end;
    if ResultCode <> 0 then
      Exit;
    Sleep(250);
  end;
end;

function UninstallWakeBridgeService(): Boolean;
begin
  Result := True;
  if not ServiceExists() then
    Exit;

  if not WakeBridgeExecutableExists() then
  begin
    MsgBox(
      'WakeBridge Serviceは登録されていますが、運用バイナリが見つかりません。'#13#10#13#10 +
      'Serviceを手動で確認してからアンインストールを再実行してください。',
      mbError, MB_OK);
    Result := False;
    Exit;
  end;

  if not RunWakeBridge('service uninstall') then
  begin
    MsgBox(
      'WakeBridge Serviceを停止・削除できませんでした。'#13#10#13#10 +
      'Serviceが使用中でないことと、管理者権限で実行していることを確認してから再実行してください。',
      mbError, MB_OK);
    Result := False;
    Exit;
  end;

  if not WaitForServiceAbsent() then
  begin
    MsgBox(
      'WakeBridge Serviceの削除完了を確認できませんでした。'#13#10#13#10 +
      'データは削除せず、Serviceの状態を確認してから再実行してください。',
      mbError, MB_OK);
    Result := False;
  end;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  if ServiceExists() then
  begin
    if not WakeBridgeExecutableExists() then
    begin
      Result := '既存のWakeBridge Serviceは見つかりましたが、運用バイナリが見つかりません。' + #13#10 +
        '手動でServiceを確認してから再実行してください。';
      Exit;
    end;
    if not RunWakeBridge('service uninstall') then
    begin
      Result := '既存のWakeBridge Serviceを停止・削除できませんでした。' + #13#10 +
        'Serviceを使用中の処理がないことを確認してから再実行してください。';
      Exit;
    end;
    if not WaitForServiceAbsent() then
    begin
      Result := '既存のWakeBridge Serviceが削除完了になりませんでした。';
    end;
  end;
end;

function HasCommandLineParam(const Parameter: String): Boolean;
var
  Index: Integer;
begin
  Result := False;
  for Index := 1 to ParamCount do
  begin
    if CompareText(ParamStr(Index), Parameter) = 0 then
    begin
      Result := True;
      Exit;
    end;
  end;
end;

function InitializeUninstall(): Boolean;
begin
  DeleteDataRequested := HasCommandLineParam('/DELETE_DATA');
  if not WizardSilent then
  begin
    if DeleteDataRequested then
    begin
      DeleteDataRequested := MsgBox(
        'WakeBridgeのデータも削除します。'#13#10#13#10 +
        'SQLite、ユーザー、パスワードハッシュ、SSH Credential、master keyが削除されます。'#13#10 +
        'この操作は元に戻せません。続行しますか？',
        mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = idYes;
    end
    else
    begin
      DeleteDataRequested := MsgBox(
        'C:\ProgramData\WakeBridge\のデータも削除しますか？'#13#10#13#10 +
        '「いいえ」を選ぶと、設定・ユーザー・パスワード・Credentialを保持します。',
        mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = idYes;
    end;
  end;
  { Service cleanup must finish before Inno removes the application files. }
  Result := UninstallWakeBridgeService();
end;

function ShouldDeleteData(): Boolean;
begin
  Result := DeleteDataRequested and (not ServiceExists());
end;
