# Architecture

Jubako は Rust 言語を使用して構築され、バックエンドとフロントエンドが分離されたアーキテクチャを採用します。

## フロントエンド (UI)
- **Iced**: 純粋な Rust 製 GUI ライブラリ。Elm アーキテクチャに基づいた、シンプルで型安全な UI 構築。

## バックエンド (Core Logic)
- **Clipboard Monitoring**: `arboard` に加え、Windows では `AddClipboardFormatListener`（`WM_CLIPBOARDUPDATE`）、macOS では `NSPasteboard.changeCount` 監視を使用。
- **Global Hotkey**: `global-hotkey` ライブラリを使用し、バックグラウンドでのホットキー監視を実装。
- **Window / Cursor Integration**: Windows API（`windows` crate）および macOS CoreGraphics / AppKit（`core-graphics`, `objc2-app-kit`）を `platform.rs` に隔離。

## データ永続化 (SQLite)
永続化アイテムは以下のスキーマで管理。
- **Folders**: `id`, `parent_id` (NULL = Root), `name`, `created_at`
- **Items**: `id`, `folder_id` (NULL = History), `content_type` (text, image), `content_data` (Text), `content_blob` (BLOB), `label` (Display name), `created_at`, `is_favorite`

## Iced による実装のポイント
- **Application**: `iced::application` を使用し、非同期でホットキーとクリップボードを監視。
- **Window Setting**: `iced::window::Settings` で `decorations: false`, `level: AlwaysOnTop`, `visible: false`, `exit_on_close_request: false` を設定。
- **Message Type**: ホットキーが押された際、`ToggleWindow` メッセージを送信し、ウィンドウの表示・非表示を切り替える。

## データの流れ
1. クリップボードの変更を監視。
2. 変更があったら履歴データベースに保存。
3. `Win + Alt + V`（Windows） / `⌘ + ⌥ + V`（macOS）で UI を表示し、データベースから履歴・永続化アイテムを読み込み。
4. アイテム選択時、クリップボードを上書きし、ウィンドウを隠した上で `Ctrl + V`（Windows） / `⌘ + V`（macOS）を擬似送信。
