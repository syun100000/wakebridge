# WakeBridge Agent Instructions

このファイルは、WakeBridgeの開発・保守を行うエージェント向けの必須ルールです。

## 最重要: コミット時のドキュメント更新

コミット単位で、必ず次の2つのドキュメントを作成または更新すること。

- `docs/DEVELOPMENT.ja.md`: 開発者向けの構成、実装判断、変更点、検証方法
- `docs/OPERATIONS.ja.md`: 管理者・運用者向けの操作、配置、Service、UI、復旧手順

コード変更、UI変更、設定変更、Service変更、セキュリティ修正、ドキュメント変更のいずれでも例外を作らない。変更が小さい場合も、両方に変更概要または検証記録を追記する。両ドキュメントを含まないコミットを作成してはいけない。

コミット前チェック:

1. `git status -sb`で対象変更を確認する。
2. 開発ドキュメントに実装変更と検証結果を追記する。
3. 操作ドキュメントに利用者が必要とする手順・影響・復旧方法を追記する。
4. `cargo fmt --all -- --check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test --all-targets`、`cargo build --release`を実行する。
5. 秘密情報、実IP、実MAC、Credential、DB、master keyが差分・履歴にないことを確認する。

## 実装ルール

- UIの利用者向け表示は日本語を標準とする。技術用語は必要な場合だけ英語を併記する。
- パスワード、SSH Credential、Session、master keyをログ、監査詳細、エラーメッセージへ出力しない。
- パスワードはArgon2idでハッシュ化し、現在パスワード確認、CSRF検証、12文字以上の新パスワード検証を維持する。
- SSHホスト鍵検証を無効化しない。任意SSHコマンドAPIを追加しない。
- Wake処理はProvider traitを経由し、Yamaha RTXの固定`wol send`形式だけを使用する。
- Windows固有処理は`src/service.rs`などへ隔離し、Web・認証・Providerの移植性を保つ。
- 実機設定、VPN、NAT、PPPoE、DHCP、重要PCの電源操作は、対象と影響を確認してから行う。
- データ削除、Service削除、GitHub公開範囲変更は、対象を明示し、ユーザーの依頼範囲内でのみ実行する。

## 配置の基準

- 運用バイナリ: `C:\Program Files\WakeBridge\wakebridge.exe`
- 運用データ: `C:\ProgramData\WakeBridge\`
- 初期Listen: `127.0.0.1:8787`
- Service名: `WakeBridge`
