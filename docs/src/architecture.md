# Architecture

Jubako は Rust 言語を使用して構築され、バックエンドとフロントエンドが分離されたアーキテクチャを採用します。

## フロントエンド (UI)
- **Iced**: 純粋な Rust 製 GUI ライブラリ。Elm アーキテクチャに基づいた、シンプルで型安全な UI 構築。

## バックエンド (Core Logic)
- **Clipboard Monitoring**: `arboard` や Windows API (`AddClipboardFormatListener`) を使用。
- **Global Hotkey**: `global-hotkey` ライブラリを使用し、バックグラウンドでのホットキー監視を実装。
## データ永続化 (SQLite / JSON)
永続化アイテムは以下のスキーマ（または相当の構造）で管理。
- **Folders**: `id`, `parent_id` (NULL = Root), `name`, `order`
- **Items**: `id`, `folder_id` (NULL = History), `content_type` (text, image), `content_data` (BLOB/Text), `label` (Display name), `created_at`

## Iced による実装のポイント
- **Application Trait**: `iced::Application` を使用し、非同期でホットキーを監視し、イベントを受信。
- **Window Setting**: `iced::window::Settings` で `resizable: false`, `decorations: false`, `always_on_top: true` を設定。
- **Message Type**: ホットキーが押された際、`Show` メッセージを送信し、ウィンドウを表示する。

## データの流れ
1. クリップボードの変更を監視。
2. 変更があったら履歴データベースに保存。
3. `Win + Alt + V` で UI を表示し、データベースから履歴・永続化アイテムを読み込み。
4. アイテム選択時、クリップボードを上書きし、オプションで `Ctrl + V` をエミュレート。
