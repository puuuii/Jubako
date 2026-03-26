use arboard::Clipboard;
use enigo::{Enigo, Settings};
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyManager, HotKeyState,
};
use iced::widget::image;
use iced::{event, mouse, window, Event, Point, Size, Subscription, Task, Theme};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::db::{Db, Folder, Item};

mod clipboard;
mod platform;
mod update;
mod view;

#[derive(Debug, Clone, PartialEq)]
enum ViewMode {
    History,
    Folder(i64),
}

#[derive(Debug, Clone)]
enum Dialog {
    RenameFolder {
        folder_id: i64,
        current_name: String,
    },
    NewFolder {
        parent_id: Option<i64>,
        name: String,
    },
}

#[derive(Debug, Clone)]
enum Message {
    SearchInputChanged(String),
    SelectView(ViewMode),
    ClipboardUpdated,
    ToggleWindow,
    HideWindow,
    WindowFocusLost,
    PasteItem(String),
    PasteImageItem(i64),
    DeleteItem(i64),
    StartDragItem(i64),
    DragOverFolder(i64),
    DragOverHistory,
    DropOnFolder(i64),
    DropOnHistory,
    CancelDrag,
    OpenNewFolder(Option<i64>),
    CreateFolder,
    DeleteFolder(i64),
    OpenRenameFolder(i64, String),
    RenameFolder,
    DialogInputChanged(String),
    CloseDialog,
    ToggleFolderActions(i64),
    Noop,
}

struct Jubako {
    db: Arc<Db>,
    clipboard: Arc<Mutex<Clipboard>>,
    last_clipboard_content: String,
    last_clipboard_image_hash: u64,
    current_view: ViewMode,
    items: Vec<Item>,
    folders: Vec<Folder>,
    search_query: String,
    #[allow(dead_code)]
    hotkey_manager: GlobalHotKeyManager,
    #[allow(dead_code)]
    hotkey_id: u32,
    is_visible: bool,
    dialog: Option<Dialog>,
    enigo: Arc<Mutex<Enigo>>,
    expanded_folder_actions: Option<i64>,
    dragging_item_id: Option<i64>,
    drag_over_folder: Option<Option<i64>>,
    image_handle_cache: HashMap<i64, image::Handle>,
}

pub fn run() -> iced::Result {
    platform::ensure_startup_registration();

    iced::application("Jubako", Jubako::update, Jubako::view)
        .subscription(Jubako::subscription)
        .theme(|_| Theme::Dark)
        .window(window::Settings {
            size: Size::new(540.0, 540.0),
            position: window::Position::Centered,
            visible: false,
            decorations: false,
            level: window::Level::AlwaysOnTop,
            exit_on_close_request: false,
            ..window::Settings::default()
        })
        .run_with(Jubako::new)
}

impl Jubako {
    fn new() -> (Self, Task<Message>) {
        let db = Arc::new(Db::new().expect("Failed to initialize DB"));
        let clipboard = Arc::new(Mutex::new(
            Clipboard::new().expect("Failed to initialize clipboard"),
        ));

        let manager = GlobalHotKeyManager::new().expect("Failed to init hotkey manager");
        let hotkey = HotKey::new(Some(Modifiers::SUPER | Modifiers::ALT), Code::KeyV);
        let hotkey_id = hotkey.id();
        manager.register(hotkey).expect("Failed to register hotkey");

        let enigo = Arc::new(Mutex::new(
            Enigo::new(&Settings::default()).expect("Failed to init enigo"),
        ));

        let mut app = Self {
            db,
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
            expanded_folder_actions: None,
            dragging_item_id: None,
            drag_over_folder: None,
            image_handle_cache: HashMap::new(),
        };

        if let Err(error) = app.db.clear_history() {
            eprintln!("Failed to reset history on startup: {}", error);
        }

        app.load_folders();
        clipboard::capture_initial_clipboard(&mut app);
        app.load_items();

        let task = Task::perform(
            async {
                tokio::time::sleep(Duration::from_millis(200)).await;
            },
            |_| {
                platform::apply_tool_window_style();
                Message::Noop
            },
        );

        (app, task)
    }

    fn load_folders(&mut self) {
        if let Ok(folders) = self.db.get_folders() {
            self.folders = folders;
        }
    }

    fn load_items(&mut self) {
        self.items = match &self.current_view {
            ViewMode::History => self.db.get_history(200).unwrap_or_default(),
            ViewMode::Folder(folder_id) => {
                self.db.get_items_in_folder(*folder_id).unwrap_or_default()
            }
        };
        self.refresh_image_thumbnail_cache();
    }

    fn refresh_image_thumbnail_cache(&mut self) {
        const THUMBNAIL_CACHE_SIZE: usize = 12;

        let candidate_ids: Vec<i64> = self
            .items
            .iter()
            .filter(|item| item.content_type == "image")
            .take(THUMBNAIL_CACHE_SIZE)
            .map(|item| item.id)
            .collect();
        let keep: HashSet<i64> = candidate_ids.iter().copied().collect();

        self.image_handle_cache
            .retain(|item_id, _| keep.contains(item_id));

        for item_id in candidate_ids {
            if self.image_handle_cache.contains_key(&item_id) {
                continue;
            }

            let Some(item) = self.items.iter().find(|item| item.id == item_id) else {
                continue;
            };
            let Some((width, height)) = clipboard::parse_image_description(&item.content_data)
            else {
                continue;
            };
            let Ok(Some(blob)) = self.db.get_item_blob(item_id) else {
                continue;
            };

            self.image_handle_cache.insert(
                item_id,
                image::Handle::from_rgba(width as u32, height as u32, blob),
            );
        }
    }

    fn refresh_data(&mut self) {
        self.load_folders();
        self.load_items();
    }

    fn reset_transient_state(&mut self) {
        self.expanded_folder_actions = None;
        self.dragging_item_id = None;
        self.drag_over_folder = None;
        self.dialog = None;
    }

    fn show_window(&mut self) -> Task<Message> {
        const WINDOW_WIDTH: f32 = 540.0;
        const WINDOW_HEIGHT: f32 = 540.0;

        self.is_visible = true;
        self.search_query.clear();
        self.reset_transient_state();
        self.refresh_data();

        let cursor_pos = platform::get_cursor_position();
        let monitor_rect = platform::get_monitor_rect_at_cursor();
        let target_monitor_scale = platform::get_monitor_scale_factor_at_cursor()
            .filter(|scale| *scale > 0.0)
            .unwrap_or(1.0);

        window::get_latest().and_then(move |id| {
            window::get_scale_factor(id).then(move |window_scale| {
                let mut tasks: Vec<Task<Message>> = Vec::new();

                if let Some(pos) = cursor_pos {
                    let adjusted_physical = if let Some((origin, size)) = monitor_rect {
                        let mid_x = origin.x + size.width / 2.0;
                        let mid_y = origin.y + size.height / 2.0;
                        let window_width_physical = WINDOW_WIDTH * target_monitor_scale;
                        let window_height_physical = WINDOW_HEIGHT * target_monitor_scale;

                        let x = if pos.x >= mid_x {
                            pos.x - window_width_physical
                        } else {
                            pos.x
                        };

                        let y = if pos.y >= mid_y {
                            pos.y - window_height_physical
                        } else {
                            pos.y
                        };

                        Point::new(x, y)
                    } else {
                        pos
                    };

                    let move_scale = if window_scale > 0.0 {
                        window_scale
                    } else {
                        1.0
                    };
                    let adjusted_logical = Point::new(
                        adjusted_physical.x / move_scale,
                        adjusted_physical.y / move_scale,
                    );

                    tasks.push(window::move_to(id, adjusted_logical));
                }

                tasks.push(window::change_mode(id, window::Mode::Windowed));
                tasks.push(window::gain_focus(id));

                Task::batch(tasks)
            })
        })
    }

    fn hide_window(&mut self) -> Task<Message> {
        self.is_visible = false;
        self.reset_transient_state();

        window::get_latest().and_then(|id| window::change_mode(id, window::Mode::Hidden))
    }

    fn subscription(&self) -> Subscription<Message> {
        let hotkey_sub = Subscription::run(|| {
            iced::futures::stream::unfold((), |_| async {
                let result = tokio::task::spawn_blocking(|| {
                    global_hotkey::GlobalHotKeyEvent::receiver().recv()
                })
                .await;

                match result {
                    Ok(Ok(event)) if event.state == HotKeyState::Pressed => {
                        Some((Message::ToggleWindow, ()))
                    }
                    Ok(Ok(_)) => Some((Message::Noop, ())),
                    _ => {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        Some((Message::Noop, ()))
                    }
                }
            })
        });

        let clipboard_sub = Subscription::run(|| {
            iced::futures::stream::unfold((), |_| async {
                let updated = tokio::task::spawn_blocking(|| {
                    platform::wait_for_clipboard_update(Duration::from_secs(1))
                })
                .await
                .unwrap_or(false);

                if updated {
                    Some((Message::ClipboardUpdated, ()))
                } else {
                    Some((Message::Noop, ()))
                }
            })
        });

        let focus_sub = event::listen_with(|event, _status, _id| match event {
            Event::Window(window::Event::Unfocused) => Some(Message::WindowFocusLost),
            Event::Window(window::Event::CloseRequested) => Some(Message::HideWindow),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                Some(Message::CancelDrag)
            }
            _ => None,
        });

        Subscription::batch(vec![hotkey_sub, clipboard_sub, focus_sub])
    }
}
