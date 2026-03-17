use arboard::{Clipboard, ImageData};
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
};
use iced::widget::{
    button, column, container, horizontal_rule, horizontal_space, image, row, scrollable, text,
    text_input, Column, Row, Space,
};
use iced::{event, window, Color, Element, Event, Length, Point, Size, Subscription, Task, Theme};
use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::Duration;

mod db;
use db::{Db, Folder, Item};

// ─────────────────────────────── ViewMode ───────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum ViewMode {
    History,
    Folder(i64),
}

// ─────────────────────────────── Dialog ─────────────────────────────────

#[derive(Debug, Clone)]
enum Dialog {
    RenameItem {
        item_id: i64,
        current_label: String,
    },
    RenameFolder {
        folder_id: i64,
        current_name: String,
    },
    NewFolder {
        parent_id: Option<i64>,
        name: String,
    },
    MoveToFolder {
        item_id: i64,
    },
}

// ─────────────────────────────── Message ────────────────────────────────

#[derive(Debug, Clone)]
enum Message {
    // Search
    SearchInputChanged(String),
    // Navigation
    SelectView(ViewMode),
    // Clipboard polling
    Tick,
    // Window
    ToggleWindow,
    HideWindow,
    WindowFocusLost,
    // Item actions
    PasteItem(String),
    PasteImageItem {
        width: usize,
        height: usize,
        rgba: Vec<u8>,
    },
    DeleteItem(i64),
    OpenRenameItem(i64, Option<String>),
    OpenMoveItem(i64),
    MoveItemToFolder(i64, Option<i64>),
    RenameItem,
    // Folder actions
    OpenNewFolder(Option<i64>),
    CreateFolder,
    DeleteFolder(i64),
    OpenRenameFolder(i64, String),
    RenameFolder,
    // Dialog
    DialogInputChanged(String),
    CloseDialog,
    // Action panel toggle
    ToggleItemActions(i64),
    ToggleFolderActions(i64),
    // No-op
    Noop,
}

// ──────────────────────────── Application State ─────────────────────────

struct Jubako {
    db: Arc<Db>,
    clipboard: Arc<Mutex<Clipboard>>,
    last_clipboard_content: String,
    last_clipboard_image_hash: u64,
    // Current view
    current_view: ViewMode,
    items: Vec<Item>,
    folders: Vec<Folder>,
    search_query: String,
    // Hotkey — these fields must stay alive so the hotkey registration persists
    #[allow(dead_code)]
    hotkey_manager: GlobalHotKeyManager,
    #[allow(dead_code)]
    hotkey_id: u32,
    is_visible: bool,
    // Dialog state
    dialog: Option<Dialog>,
    // For paste simulation
    enigo: Arc<Mutex<Enigo>>,
    // Which item currently has its action panel expanded
    expanded_item_actions: Option<i64>,
    // Which folder currently has its action panel expanded
    expanded_folder_actions: Option<i64>,
}

// ────────────────────────── Helper: get cursor position ─────────────────

#[cfg(target_os = "windows")]
fn get_cursor_position() -> Option<Point> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let mut point = POINT { x: 0, y: 0 };
    unsafe {
        if GetCursorPos(&mut point).is_ok() {
            Some(Point::new(point.x as f32, point.y as f32))
        } else {
            None
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn get_cursor_position() -> Option<Point> {
    None
}

// ────────────────────── Helper: set tool window style ───────────────────

/// After the window is created we flip on WS_EX_TOOLWINDOW so that the
/// window never appears in the taskbar.
#[cfg(target_os = "windows")]
fn apply_tool_window_style() {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowLongW, SetWindowLongW, GWL_EXSTYLE, WS_EX_TOOLWINDOW,
    };
    unsafe {
        let hwnd = GetForegroundWindow();
        if !hwnd.is_invalid() {
            let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
            SetWindowLongW(hwnd, GWL_EXSTYLE, ex_style | WS_EX_TOOLWINDOW.0 as i32);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn apply_tool_window_style() {}

// ────────────────────────── Helper: truncate ────────────────────────────

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max).collect();
        format!("{}...", truncated)
    }
}

// ────────────────────── Helper: hash image data ─────────────────────────

/// Compute a fast hash of image RGBA bytes for duplicate detection.
fn hash_image_data(img: &ImageData<'_>) -> u64 {
    let mut hasher = DefaultHasher::new();
    img.width.hash(&mut hasher);
    img.height.hash(&mut hasher);
    img.bytes.hash(&mut hasher);
    hasher.finish()
}

/// Parse the image description string (format: "WxH:HEX_HASH") back into
/// width and height.
fn parse_image_description(desc: &str) -> Option<(usize, usize)> {
    // Format: "WIDTHxHEIGHT:HASH"
    let dim_part = desc.split(':').next()?;
    let mut parts = dim_part.split('x');
    let w: usize = parts.next()?.parse().ok()?;
    let h: usize = parts.next()?.parse().ok()?;
    Some((w, h))
}

// ────────────────────────────── Entry point ─────────────────────────────

pub fn main() -> iced::Result {
    iced::application("Jubako", Jubako::update, Jubako::view)
        .subscription(Jubako::subscription)
        .theme(|_| Theme::Dark)
        .window(window::Settings {
            size: Size::new(800.0, 600.0),
            position: window::Position::Centered,
            visible: false,
            decorations: false,
            level: window::Level::AlwaysOnTop,
            exit_on_close_request: false,
            ..window::Settings::default()
        })
        .run_with(Jubako::new)
}

// ──────────────────────────── Implementation ────────────────────────────

impl Jubako {
    fn new() -> (Self, Task<Message>) {
        let db = Db::new().expect("Failed to initialize DB");
        let db = Arc::new(db);

        let clipboard = Clipboard::new().expect("Failed to initialize clipboard");
        let clipboard = Arc::new(Mutex::new(clipboard));

        let manager = GlobalHotKeyManager::new().expect("Failed to init hotkey manager");
        let hotkey = HotKey::new(Some(Modifiers::SUPER | Modifiers::ALT), Code::KeyV);
        let hotkey_id = hotkey.id();
        manager.register(hotkey).expect("Failed to register hotkey");

        let enigo = Enigo::new(&Settings::default()).expect("Failed to init enigo");
        let enigo = Arc::new(Mutex::new(enigo));

        let mut app = Self {
            db: db.clone(),
            clipboard,
            last_clipboard_content: String::new(),
            last_clipboard_image_hash: 0,
            current_view: ViewMode::History,
            items: Vec::new(),
            folders: Vec::new(),
            search_query: String::new(),
            hotkey_manager: manager,
            hotkey_id,
            is_visible: false,
            dialog: None,
            enigo,
            expanded_item_actions: None,
            expanded_folder_actions: None,
        };

        // Load initial data
        app.load_folders();

        // Capture whatever is already on the clipboard and save it to DB
        // so that content copied before the app started is also shown in the UI.
        if let Ok(mut cb) = app.clipboard.lock() {
            // Try text first
            if let Ok(txt) = cb.get_text() {
                if !txt.is_empty() {
                    app.last_clipboard_content = txt.clone();
                    match app.db.check_duplicate(&txt) {
                        Ok(Some(_existing_id)) => { /* already stored */ }
                        _ => {
                            let _ = app.db.insert_item(&txt, "text");
                        }
                    }
                }
            }
            // Try image
            if let Ok(img) = cb.get_image() {
                let hash = hash_image_data(&img);
                app.last_clipboard_image_hash = hash;
                let desc = format!("{}x{}:{:016x}", img.width, img.height, hash);
                match app.db.check_image_duplicate(&desc) {
                    Ok(Some(_existing_id)) => { /* already stored */ }
                    _ => {
                        let _ = app.db.insert_image_item(&desc, &img.bytes);
                    }
                }
            }
        }

        app.load_items();

        // Apply tool-window style shortly after launch
        let task = Task::perform(
            async {
                tokio::time::sleep(Duration::from_millis(200)).await;
            },
            |_| {
                apply_tool_window_style();
                Message::Noop
            },
        );

        (app, task)
    }

    // ────────────────── Data helpers ──────────────────

    fn load_folders(&mut self) {
        if let Ok(folders) = self.db.get_folders() {
            self.folders = folders;
        }
    }

    fn load_items(&mut self) {
        self.items = match &self.current_view {
            ViewMode::History => self.db.get_history(200).unwrap_or_default(),
            ViewMode::Folder(fid) => self.db.get_items_in_folder(*fid).unwrap_or_default(),
        };
    }

    fn refresh_data(&mut self) {
        self.load_folders();
        self.load_items();
    }

    fn displayed_items(&self) -> Vec<&Item> {
        if self.search_query.is_empty() {
            self.items.iter().collect()
        } else {
            // Use DB search if query is present
            // Since search_items returns owned Vec, we use self.items filtered
            let q = self.search_query.to_lowercase();
            self.items
                .iter()
                .filter(|item| {
                    item.content_data.to_lowercase().contains(&q)
                        || item
                            .label
                            .as_ref()
                            .map(|l| l.to_lowercase().contains(&q))
                            .unwrap_or(false)
                })
                .collect()
        }
    }

    // ────────────────── Show / Hide window ──────────────────

    fn show_window(&mut self) -> Task<Message> {
        self.is_visible = true;
        self.search_query.clear();
        self.expanded_item_actions = None;
        self.expanded_folder_actions = None;
        self.dialog = None;
        self.refresh_data();

        let cursor_pos = get_cursor_position();

        window::get_latest().and_then(move |id| {
            let mut tasks: Vec<Task<Message>> = Vec::new();

            if let Some(pos) = cursor_pos {
                tasks.push(window::move_to(id, pos));
            }
            tasks.push(window::change_mode(id, window::Mode::Windowed));
            tasks.push(window::gain_focus(id));

            Task::batch(tasks)
        })
    }

    fn hide_window(&mut self) -> Task<Message> {
        self.is_visible = false;
        self.expanded_item_actions = None;
        self.expanded_folder_actions = None;
        self.dialog = None;

        window::get_latest().and_then(|id| window::change_mode(id, window::Mode::Hidden))
    }

    // ────────────────── Update ──────────────────

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Noop => Task::none(),

            // ── Search ──
            Message::SearchInputChanged(query) => {
                self.search_query = query.clone();
                // When searching, load from DB search for wider results
                if !query.is_empty() {
                    if let Ok(results) = self.db.search_items(&query, 200) {
                        self.items = results;
                    }
                } else {
                    self.load_items();
                }
                Task::none()
            }

            // ── Navigation ──
            Message::SelectView(view) => {
                self.current_view = view;
                self.search_query.clear();
                self.expanded_item_actions = None;
                self.expanded_folder_actions = None;
                self.load_items();
                Task::none()
            }

            // ── Clipboard polling ──
            Message::Tick => {
                let mut should_refresh = false;
                {
                    if let Ok(mut cb) = self.clipboard.lock() {
                        // Check for text content
                        let mut text_changed = false;
                        if let Ok(txt) = cb.get_text() {
                            if !txt.is_empty() && txt != self.last_clipboard_content {
                                self.last_clipboard_content = txt.clone();
                                text_changed = true;
                                // Duplicate check
                                match self.db.check_duplicate(&txt) {
                                    Ok(Some(_existing_id)) => {
                                        // Already in history, skip
                                    }
                                    _ => {
                                        if let Err(e) = self.db.insert_item(&txt, "text") {
                                            eprintln!("Failed to save item: {}", e);
                                        }
                                    }
                                }
                                // Refresh if we're viewing history
                                if self.current_view == ViewMode::History
                                    && self.search_query.is_empty()
                                {
                                    should_refresh = true;
                                }
                            }
                        }

                        // Check for image content (only if text didn't change,
                        // to avoid capturing both text and image from the same copy)
                        if !text_changed {
                            if let Ok(img) = cb.get_image() {
                                let hash = hash_image_data(&img);
                                if hash != self.last_clipboard_image_hash {
                                    self.last_clipboard_image_hash = hash;
                                    let desc =
                                        format!("{}x{}:{:016x}", img.width, img.height, hash);
                                    match self.db.check_image_duplicate(&desc) {
                                        Ok(Some(_existing_id)) => { /* already stored */ }
                                        _ => {
                                            if let Err(e) =
                                                self.db.insert_image_item(&desc, &img.bytes)
                                            {
                                                eprintln!("Failed to save image item: {}", e);
                                            }
                                        }
                                    }
                                    if self.current_view == ViewMode::History
                                        && self.search_query.is_empty()
                                    {
                                        should_refresh = true;
                                    }
                                }
                            }
                        }
                    }
                }
                if should_refresh {
                    self.load_items();
                }
                Task::none()
            }

            // ── Window ──
            Message::ToggleWindow => {
                if self.is_visible {
                    self.hide_window()
                } else {
                    self.show_window()
                }
            }

            Message::HideWindow => self.hide_window(),

            Message::WindowFocusLost => {
                // Don't hide if a dialog is open (user may be interacting with it)
                if self.is_visible && self.dialog.is_none() {
                    self.hide_window()
                } else {
                    Task::none()
                }
            }

            // ── Item: paste ──
            Message::PasteItem(content) => {
                // Set clipboard
                if let Ok(mut cb) = self.clipboard.lock() {
                    let _ = cb.set_text(content.clone());
                    self.last_clipboard_content = content;
                }

                // Hide window first, then schedule the paste simulation
                self.is_visible = false;
                self.dialog = None;
                self.expanded_item_actions = None;

                let enigo = self.enigo.clone();

                window::get_latest()
                    .and_then(move |id| window::change_mode::<Message>(id, window::Mode::Hidden))
                    .chain(Task::perform(
                        async move {
                            tokio::time::sleep(Duration::from_millis(150)).await;
                            if let Ok(mut e) = enigo.lock() {
                                let _ = e.key(Key::Control, Direction::Press);
                                let _ = e.key(Key::Unicode('v'), Direction::Click);
                                let _ = e.key(Key::Control, Direction::Release);
                            }
                        },
                        |_| Message::Noop,
                    ))
            }

            Message::PasteImageItem {
                width,
                height,
                rgba,
            } => {
                // Set image to clipboard
                if let Ok(mut cb) = self.clipboard.lock() {
                    let img = ImageData {
                        width,
                        height,
                        bytes: Cow::Owned(rgba),
                    };
                    let hash = hash_image_data(&img);
                    let _ = cb.set_image(img);
                    self.last_clipboard_image_hash = hash;
                }

                // Hide window first, then schedule the paste simulation
                self.is_visible = false;
                self.dialog = None;
                self.expanded_item_actions = None;

                let enigo = self.enigo.clone();

                window::get_latest()
                    .and_then(move |id| window::change_mode::<Message>(id, window::Mode::Hidden))
                    .chain(Task::perform(
                        async move {
                            tokio::time::sleep(Duration::from_millis(150)).await;
                            if let Ok(mut e) = enigo.lock() {
                                let _ = e.key(Key::Control, Direction::Press);
                                let _ = e.key(Key::Unicode('v'), Direction::Click);
                                let _ = e.key(Key::Control, Direction::Release);
                            }
                        },
                        |_| Message::Noop,
                    ))
            }

            // ── Item: delete ──
            Message::DeleteItem(id) => {
                let _ = self.db.delete_item(id);
                self.expanded_item_actions = None;
                self.refresh_data();
                Task::none()
            }

            // ── Item: rename dialog ──
            Message::OpenRenameItem(id, current) => {
                self.dialog = Some(Dialog::RenameItem {
                    item_id: id,
                    current_label: current.unwrap_or_default(),
                });
                self.expanded_item_actions = None;
                Task::none()
            }

            Message::RenameItem => {
                if let Some(Dialog::RenameItem {
                    item_id,
                    ref current_label,
                }) = self.dialog
                {
                    let label = current_label.clone();
                    if !label.is_empty() {
                        let _ = self.db.rename_item(item_id, &label);
                    }
                }
                self.dialog = None;
                self.refresh_data();
                Task::none()
            }

            // ── Item: move dialog ──
            Message::OpenMoveItem(id) => {
                self.dialog = Some(Dialog::MoveToFolder { item_id: id });
                self.expanded_item_actions = None;
                Task::none()
            }

            Message::MoveItemToFolder(item_id, folder_id) => {
                let _ = self.db.move_item_to_folder(item_id, folder_id);
                self.dialog = None;
                self.refresh_data();
                Task::none()
            }

            // ── Folder: new folder dialog ──
            Message::OpenNewFolder(parent_id) => {
                self.dialog = Some(Dialog::NewFolder {
                    parent_id,
                    name: String::new(),
                });
                self.expanded_folder_actions = None;
                Task::none()
            }

            Message::CreateFolder => {
                if let Some(Dialog::NewFolder {
                    parent_id,
                    ref name,
                }) = self.dialog
                {
                    if !name.trim().is_empty() {
                        let _ = self.db.create_folder(name.trim(), parent_id);
                    }
                }
                self.dialog = None;
                self.refresh_data();
                Task::none()
            }

            // ── Folder: delete ──
            Message::DeleteFolder(id) => {
                let _ = self.db.delete_folder(id);
                self.expanded_folder_actions = None;
                // If we were viewing that folder, go back to History
                if self.current_view == ViewMode::Folder(id) {
                    self.current_view = ViewMode::History;
                }
                self.refresh_data();
                Task::none()
            }

            // ── Folder: rename dialog ──
            Message::OpenRenameFolder(id, current_name) => {
                self.dialog = Some(Dialog::RenameFolder {
                    folder_id: id,
                    current_name,
                });
                self.expanded_folder_actions = None;
                Task::none()
            }

            Message::RenameFolder => {
                if let Some(Dialog::RenameFolder {
                    folder_id,
                    ref current_name,
                }) = self.dialog
                {
                    if !current_name.trim().is_empty() {
                        let _ = self.db.rename_folder(folder_id, current_name.trim());
                    }
                }
                self.dialog = None;
                self.refresh_data();
                Task::none()
            }

            // ── Dialog ──
            Message::DialogInputChanged(value) => {
                match &mut self.dialog {
                    Some(Dialog::RenameItem {
                        ref mut current_label,
                        ..
                    }) => *current_label = value,
                    Some(Dialog::RenameFolder {
                        ref mut current_name,
                        ..
                    }) => *current_name = value,
                    Some(Dialog::NewFolder { ref mut name, .. }) => *name = value,
                    _ => {}
                }
                Task::none()
            }

            Message::CloseDialog => {
                self.dialog = None;
                Task::none()
            }

            // ── Action panel toggles ──
            Message::ToggleItemActions(id) => {
                if self.expanded_item_actions == Some(id) {
                    self.expanded_item_actions = None;
                } else {
                    self.expanded_item_actions = Some(id);
                }
                self.expanded_folder_actions = None;
                Task::none()
            }

            Message::ToggleFolderActions(id) => {
                if self.expanded_folder_actions == Some(id) {
                    self.expanded_folder_actions = None;
                } else {
                    self.expanded_folder_actions = Some(id);
                }
                self.expanded_item_actions = None;
                Task::none()
            }
        }
    }

    // ────────────────── View ──────────────────

    fn view(&self) -> Element<'_, Message> {
        // ── Search bar ──
        let search = container(
            text_input("Search items...", &self.search_query)
                .on_input(Message::SearchInputChanged)
                .padding(8)
                .size(16),
        )
        .padding(8)
        .width(Length::Fill);

        // ── Sidebar ──
        let sidebar = self.view_sidebar();

        // ── Main pane ──
        let main_pane = self.view_main_pane();

        // ── Two-column body ──
        let body = row![
            container(sidebar)
                .width(Length::FillPortion(1))
                .height(Length::Fill)
                .style(|_theme: &Theme| {
                    container::Style {
                        border: iced::Border {
                            color: Color::from_rgb(0.3, 0.3, 0.3),
                            width: 1.0,
                            radius: 0.0.into(),
                        },
                        ..container::Style::default()
                    }
                }),
            container(main_pane)
                .width(Length::FillPortion(3))
                .height(Length::Fill),
        ]
        .spacing(0)
        .height(Length::Fill);

        // ── Full layout ──
        let layout: Element<'_, Message> =
            column![search, body].spacing(0).height(Length::Fill).into();

        // ── Dialog overlay ──
        if let Some(ref dlg) = self.dialog {
            let dialog_content = self.view_dialog(dlg);
            // Since iced 0.13 doesn't have a stack/overlay widget, we replace
            // the entire view with a dimmed background + centered dialog when open.
            let dimmed_bg = container(
                column![
                    Space::new(Length::Fill, Length::FillPortion(1)),
                    container(container(dialog_content).padding(20).max_width(450).style(
                        |_theme: &Theme| container::Style {
                            background: Some(iced::Background::Color(Color::from_rgb(
                                0.15, 0.15, 0.18,
                            ))),
                            border: iced::Border {
                                color: Color::from_rgb(0.4, 0.4, 0.5),
                                width: 2.0,
                                radius: 8.0.into(),
                            },
                            ..container::Style::default()
                        }
                    ),)
                    .width(Length::Fill)
                    .center_x(Length::Fill),
                    Space::new(Length::Fill, Length::FillPortion(1)),
                ]
                .align_x(iced::Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(iced::Background::Color(Color::from_rgba(
                    0.0, 0.0, 0.0, 0.5,
                ))),
                ..container::Style::default()
            });

            dimmed_bg.into()
        } else {
            container(layout)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_theme: &Theme| container::Style {
                    background: Some(iced::Background::Color(Color::from_rgb(0.1, 0.1, 0.12))),
                    ..container::Style::default()
                })
                .into()
        }
    }

    // ────────────────── Sidebar view ──────────────────

    fn view_sidebar(&self) -> Element<'_, Message> {
        let mut col = Column::new().spacing(2).padding(8);

        // ── "History" entry ──
        let history_selected = self.current_view == ViewMode::History;
        let history_style = if history_selected {
            button::primary
        } else {
            button::text
        };

        col = col.push(
            button(
                text("\u{1F4CB} History")
                    .shaping(text::Shaping::Advanced)
                    .size(14),
            )
            .on_press(Message::SelectView(ViewMode::History))
            .style(history_style)
            .width(Length::Fill),
        );

        col = col.push(horizontal_rule(1));

        // ── "New Root Folder" button ──
        col = col.push(
            button(
                text("+ New Folder")
                    .size(12)
                    .color(Color::from_rgb(0.5, 0.8, 0.5)),
            )
            .on_press(Message::OpenNewFolder(None))
            .style(button::text)
            .width(Length::Fill),
        );

        col = col.push(Space::new(Length::Fill, 4));

        // ── Folder tree ──
        let root_folders: Vec<&Folder> = self
            .folders
            .iter()
            .filter(|f| f.parent_id.is_none())
            .collect();

        for folder in &root_folders {
            col = col.push(self.view_folder_tree(folder, 0));
        }

        scrollable(col).height(Length::Fill).into()
    }

    fn view_folder_tree(&self, folder: &Folder, depth: u16) -> Element<'_, Message> {
        let indent = (depth as f32) * 16.0;
        let is_selected = self.current_view == ViewMode::Folder(folder.id);
        let folder_id = folder.id;

        let btn_style = if is_selected {
            button::primary
        } else {
            button::text
        };

        let folder_icon = "\u{1F4C1}";
        let label_text = format!("{} {}", folder_icon, folder.name);

        let folder_btn = button(text(label_text).shaping(text::Shaping::Advanced).size(13))
            .on_press(Message::SelectView(ViewMode::Folder(folder_id)))
            .style(btn_style)
            .width(Length::Fill);

        // Action buttons for folder
        let actions_btn = button(text("\u{22EE}").size(14).shaping(text::Shaping::Advanced))
            .on_press(Message::ToggleFolderActions(folder_id))
            .style(button::text)
            .padding(2);

        let folder_row = Row::new()
            .push(Space::new(indent, 0))
            .push(folder_btn)
            .push(actions_btn)
            .align_y(iced::Alignment::Center);

        let mut col = Column::new().spacing(1);
        col = col.push(folder_row);

        // Expanded action panel for this folder
        if self.expanded_folder_actions == Some(folder_id) {
            let panel = container(
                row![
                    button(text("+").size(11))
                        .on_press(Message::OpenNewFolder(Some(folder_id)))
                        .style(button::secondary)
                        .padding([2, 6]),
                    button(text("\u{270F}").size(11).shaping(text::Shaping::Advanced))
                        .on_press(Message::OpenRenameFolder(folder_id, folder.name.clone()))
                        .style(button::secondary)
                        .padding([2, 6]),
                    button(text("\u{1F5D1}").size(11).shaping(text::Shaping::Advanced))
                        .on_press(Message::DeleteFolder(folder_id))
                        .style(button::danger)
                        .padding([2, 6]),
                ]
                .spacing(4),
            )
            .padding(iced::Padding {
                top: 2.0,
                right: 0.0,
                bottom: 2.0,
                left: indent + 20.0,
            });

            col = col.push(panel);
        }

        // Child folders
        let children: Vec<&Folder> = self
            .folders
            .iter()
            .filter(|f| f.parent_id == Some(folder_id))
            .collect();

        for child in children {
            col = col.push(self.view_folder_tree(child, depth + 1));
        }

        col.into()
    }

    // ────────────────── Main pane view ──────────────────

    fn view_main_pane(&self) -> Element<'_, Message> {
        let header_text = match &self.current_view {
            ViewMode::History => "History".to_string(),
            ViewMode::Folder(fid) => {
                let name = self
                    .folders
                    .iter()
                    .find(|f| f.id == *fid)
                    .map(|f| f.name.clone())
                    .unwrap_or_else(|| "Folder".to_string());
                format!("\u{1F4C1} {}", name)
            }
        };

        let header = container(
            text(header_text)
                .size(16)
                .shaping(text::Shaping::Advanced)
                .color(Color::from_rgb(0.7, 0.7, 0.8)),
        )
        .padding([8, 12]);

        let items = self.displayed_items();

        let mut items_col = Column::new().spacing(4).padding([4, 8]);

        if items.is_empty() {
            items_col = items_col.push(
                container(
                    text("No items")
                        .size(14)
                        .color(Color::from_rgb(0.5, 0.5, 0.5)),
                )
                .padding(20)
                .center_x(Length::Fill),
            );
        } else {
            for item in &items {
                items_col = items_col.push(self.view_item(item));
            }
        }

        let content = scrollable(items_col).height(Length::Fill);

        column![header, horizontal_rule(1), content]
            .spacing(0)
            .height(Length::Fill)
            .into()
    }

    fn view_item(&self, item: &Item) -> Element<'_, Message> {
        let item_id = item.id;
        let is_image = item.content_type == "image";

        let timestamp = item.created_at.format("%m/%d %H:%M").to_string();

        let fav_icon = if item.is_favorite { "\u{2605}" } else { "" };

        let press_message: Message = if is_image {
            if let Some(ref blob) = item.content_blob {
                if let Some((w, h)) = parse_image_description(&item.content_data) {
                    Message::PasteImageItem {
                        width: w,
                        height: h,
                        rgba: blob.clone(),
                    }
                } else {
                    Message::Noop
                }
            } else {
                Message::Noop
            }
        } else {
            Message::PasteItem(item.content_data.clone())
        };

        // Build the inner content depending on type
        let content_btn = if is_image {
            let dims = parse_image_description(&item.content_data);
            let dim_label = dims
                .map(|(w, h)| format!("\u{1F5BC} Image ({}x{})", w, h))
                .unwrap_or_else(|| "\u{1F5BC} Image".to_string());

            let display_label = item
                .label
                .as_ref()
                .map(|l| truncate_str(l, 60))
                .unwrap_or(dim_label);

            // Build a thumbnail from RGBA data if available
            let mut content_col = Column::new().spacing(2);

            if let (Some(ref blob), Some((w, h))) = (&item.content_blob, dims) {
                let handle = image::Handle::from_rgba(w as u32, h as u32, blob.clone());
                content_col = content_col.push(
                    container(
                        image(handle)
                            .content_fit(iced::ContentFit::ScaleDown)
                            .width(Length::Fixed(120.0))
                            .height(Length::Fixed(80.0)),
                    )
                    .padding(2),
                );
            }

            content_col = content_col.push(
                row![
                    text(display_label)
                        .size(13)
                        .shaping(text::Shaping::Advanced)
                        .width(Length::Fill),
                    text(fav_icon)
                        .size(13)
                        .shaping(text::Shaping::Advanced)
                        .color(Color::from_rgb(1.0, 0.85, 0.0)),
                ]
                .spacing(4),
            );
            content_col = content_col.push(
                text(timestamp)
                    .size(10)
                    .color(Color::from_rgb(0.5, 0.5, 0.5)),
            );

            button(content_col)
                .on_press(press_message)
                .style(button::text)
                .width(Length::Fill)
                .padding([6, 8])
        } else {
            let display_text = item
                .label
                .as_ref()
                .map(|l| truncate_str(l, 80))
                .unwrap_or_else(|| truncate_str(&item.content_data, 80));

            // Replace newlines for display
            let display_text = display_text.replace('\n', " \u{21B5} ");

            button(
                column![
                    row![
                        text(display_text)
                            .size(13)
                            .shaping(text::Shaping::Advanced)
                            .width(Length::Fill),
                        text(fav_icon)
                            .size(13)
                            .shaping(text::Shaping::Advanced)
                            .color(Color::from_rgb(1.0, 0.85, 0.0)),
                    ]
                    .spacing(4),
                    text(timestamp)
                        .size(10)
                        .color(Color::from_rgb(0.5, 0.5, 0.5)),
                ]
                .spacing(2),
            )
            .on_press(press_message)
            .style(button::text)
            .width(Length::Fill)
            .padding([6, 8])
        };

        let action_btn = button(text("\u{22EE}").size(16).shaping(text::Shaping::Advanced))
            .on_press(Message::ToggleItemActions(item_id))
            .style(button::text)
            .padding([4, 6]);

        let mut col = Column::new().spacing(0);

        col = col.push(
            row![content_btn, action_btn]
                .spacing(0)
                .align_y(iced::Alignment::Center),
        );

        // Expanded action panel
        if self.expanded_item_actions == Some(item_id) {
            let panel = container(
                row![
                    button(text("Move").size(11))
                        .on_press(Message::OpenMoveItem(item_id))
                        .style(button::secondary)
                        .padding([3, 8]),
                    button(text("Rename").size(11))
                        .on_press(Message::OpenRenameItem(item_id, item.label.clone()))
                        .style(button::secondary)
                        .padding([3, 8]),
                    button(text("Delete").size(11))
                        .on_press(Message::DeleteItem(item_id))
                        .style(button::danger)
                        .padding([3, 8]),
                ]
                .spacing(6),
            )
            .padding(iced::Padding {
                top: 2.0,
                right: 8.0,
                bottom: 6.0,
                left: 8.0,
            });

            col = col.push(panel);
        }

        container(col)
            .width(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(iced::Background::Color(Color::from_rgb(0.14, 0.14, 0.16))),
                border: iced::Border {
                    color: Color::from_rgb(0.2, 0.2, 0.22),
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..container::Style::default()
            })
            .into()
    }

    // ────────────────── Dialog view ──────────────────

    fn view_dialog(&self, dialog: &Dialog) -> Element<'_, Message> {
        match dialog {
            Dialog::RenameItem {
                item_id: _,
                current_label,
            } => column![
                text("Rename Item").size(16),
                Space::new(Length::Fill, 8),
                text_input("Enter label...", current_label)
                    .on_input(Message::DialogInputChanged)
                    .on_submit(Message::RenameItem)
                    .padding(8)
                    .size(14),
                Space::new(Length::Fill, 12),
                row![
                    horizontal_space(),
                    button(text("Cancel").size(13))
                        .on_press(Message::CloseDialog)
                        .style(button::secondary)
                        .padding([6, 16]),
                    button(text("Save").size(13))
                        .on_press(Message::RenameItem)
                        .style(button::primary)
                        .padding([6, 16]),
                ]
                .spacing(8),
            ]
            .spacing(4)
            .into(),

            Dialog::RenameFolder {
                folder_id: _,
                current_name,
            } => column![
                text("Rename Folder").size(16),
                Space::new(Length::Fill, 8),
                text_input("Folder name...", current_name)
                    .on_input(Message::DialogInputChanged)
                    .on_submit(Message::RenameFolder)
                    .padding(8)
                    .size(14),
                Space::new(Length::Fill, 12),
                row![
                    horizontal_space(),
                    button(text("Cancel").size(13))
                        .on_press(Message::CloseDialog)
                        .style(button::secondary)
                        .padding([6, 16]),
                    button(text("Save").size(13))
                        .on_press(Message::RenameFolder)
                        .style(button::primary)
                        .padding([6, 16]),
                ]
                .spacing(8),
            ]
            .spacing(4)
            .into(),

            Dialog::NewFolder { parent_id, name } => {
                let title = if parent_id.is_some() {
                    "New Subfolder"
                } else {
                    "New Folder"
                };
                column![
                    text(title).size(16),
                    Space::new(Length::Fill, 8),
                    text_input("Folder name...", name)
                        .on_input(Message::DialogInputChanged)
                        .on_submit(Message::CreateFolder)
                        .padding(8)
                        .size(14),
                    Space::new(Length::Fill, 12),
                    row![
                        horizontal_space(),
                        button(text("Cancel").size(13))
                            .on_press(Message::CloseDialog)
                            .style(button::secondary)
                            .padding([6, 16]),
                        button(text("Create").size(13))
                            .on_press(Message::CreateFolder)
                            .style(button::primary)
                            .padding([6, 16]),
                    ]
                    .spacing(8),
                ]
                .spacing(4)
                .into()
            }

            Dialog::MoveToFolder { item_id } => {
                let item_id = *item_id;

                let mut folder_list = Column::new().spacing(4);

                // Option to move back to History (no folder)
                folder_list = folder_list.push(
                    button(
                        text("\u{1F4CB} History (no folder)")
                            .size(13)
                            .shaping(text::Shaping::Advanced),
                    )
                    .on_press(Message::MoveItemToFolder(item_id, None))
                    .style(button::text)
                    .width(Length::Fill),
                );

                for folder in &self.folders {
                    let indent = if folder.parent_id.is_some() {
                        "    "
                    } else {
                        ""
                    };
                    let label = format!("{}\u{1F4C1} {}", indent, folder.name);
                    folder_list = folder_list.push(
                        button(text(label).size(13).shaping(text::Shaping::Advanced))
                            .on_press(Message::MoveItemToFolder(item_id, Some(folder.id)))
                            .style(button::text)
                            .width(Length::Fill),
                    );
                }

                column![
                    text("Move to Folder").size(16),
                    Space::new(Length::Fill, 8),
                    scrollable(folder_list).height(Length::Fixed(300.0)),
                    Space::new(Length::Fill, 12),
                    row![
                        horizontal_space(),
                        button(text("Cancel").size(13))
                            .on_press(Message::CloseDialog)
                            .style(button::secondary)
                            .padding([6, 16]),
                    ]
                    .spacing(8),
                ]
                .spacing(4)
                .into()
            }
        }
    }

    // ────────────────── Subscription ──────────────────

    fn subscription(&self) -> Subscription<Message> {
        // 1) Clipboard poll tick every 1 second
        let tick = iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick);

        // 2) Hotkey subscription
        let hotkey_sub = Subscription::run(|| {
            iced::futures::stream::unfold((), |_| async {
                let result =
                    tokio::task::spawn_blocking(|| GlobalHotKeyEvent::receiver().recv()).await;

                match result {
                    Ok(Ok(event)) if event.state == HotKeyState::Pressed => {
                        Some((Message::ToggleWindow, ()))
                    }
                    Ok(Ok(_released)) => {
                        // Ignore Released events
                        Some((Message::Noop, ()))
                    }
                    _ => {
                        // If the channel is disconnected, sleep to avoid busy loop
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        Some((Message::Noop, ()))
                    }
                }
            })
        });

        // 3) Window event subscription for focus loss
        let focus_sub = event::listen_with(|event, _status, _id| match event {
            Event::Window(window::Event::Unfocused) => Some(Message::WindowFocusLost),
            Event::Window(window::Event::CloseRequested) => Some(Message::HideWindow),
            _ => None,
        });

        Subscription::batch(vec![tick, hotkey_sub, focus_sub])
    }
}
