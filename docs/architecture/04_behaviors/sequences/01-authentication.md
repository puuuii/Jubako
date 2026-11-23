# 認証とセッショントラスト

## 目的

Jubako が Windows / macOS ユーザーセッション上でどのように信頼され、起動されるかを示します。なお、アプリ独自ログインは存在しません。

## 前提条件

- デスクトップセッションにユーザーがログイン済みであること。
- Windows: `jubako.exe` が配置され、必要に応じて `--no-autostart` なしで起動しスタートアップ登録が有効であること。
- macOS: `Jubako.app`（`LSUIElement = true`）および LaunchAgent（`~/Library/LaunchAgents/com.jubako.app.plist`）が構成され、アクセシビリティ権限が付与されていること。

## シーケンス

```mermaid
sequenceDiagram
    participant User
    participant OSSession as OS Desktop Session
    participant Jubako
    participant Autostart as HKCU Run (Win) / LaunchAgent (Mac)
    participant Hotkey as Hotkey Service

    User->>OSSession: サインイン / ログイン
    OSSession->>Jubako: プロセス起動（スタートアップ / LaunchAgent / 手動）

    alt Windows Release ビルドかつ autostart 有効
        Jubako->>Autostart: ensure_startup_registration()
    else Debug ビルド・--no-autostart・macOS
        Jubako-->>Jubako: 自動登録をスキップ（macOS は LaunchAgent 管理）
    end

    Jubako->>Hotkey: Register Win+Alt+V / Cmd+Option+V

    loop 起動中
        User->>Hotkey: ホットキーを押下
        Hotkey->>Jubako: HotKeyPressed イベント
        Jubako->>User: ツールウィンドウを表示/非表示
    end
```

## 異常系

- Windows でレジストリ書き込み失敗時はエラーログを出し、スタートアップ永続化なしで継続します。
- ホットキー登録失敗時は現行実装（`expect`）で起動時 panic となり、ポップアップは利用できません。
- Windows でバックグラウンド分離起動に失敗した場合はフォアグラウンド動作へフォールバックします。
- macOS でアクセシビリティ権限が未許可の場合、ウィンドウ表示は動作しますが自動ペースト（`osascript`）が失敗します。

## メモ

- 認証はアプリ内 ID 基盤ではなく、OS アカウント/セッション制御に依存します。
- セキュリティ水準は端末アカウント運用と OS ロック・TCC（アクセシビリティ）ポリシーに左右されます。
