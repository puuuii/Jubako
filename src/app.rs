use arboard::Clipboard;
use enigo::{Enigo, Settings};
use global_hotkey::{
    GlobalHotKeyManager, HotKeyState,
    hotkey::{Code, HotKey, Modifiers},
};
use iced::widget::image;
use iced::{Event, Point, Size, Subscription, Task, Theme, event, mouse, window};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
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
    clipboard_state: Arc<Mutex<clipboard::ClipboardState>>,
    clipboard_ui_rx: Arc<Mutex<Option<mpsc::Receiver<()>>>>,
    current_view: ViewMode,
    items: Vec<Item>,
    folders: Vec<Folder>,
    #[allow(dead_code)]
    hotkey_manager: GlobalHotKeyManager,
    #[allow(dead_code)]
    hotkey_id: u32,
    window_id: Option<window::Id>,
    is_visible: bool,
    is_visible_flag: Arc<AtomicBool>,
    dialog: Option<Dialog>,
    enigo: Arc<Mutex<Enigo>>,
    expanded_folder_actions: Option<i64>,
    dragging_item_id: Option<i64>,
    drag_over_folder: Option<Option<i64>>,
    image_handle_cache: HashMap<i64, image::Handle>,
}

pub fn run() -> iced::Result {
    platform::ensure_startup_registration();

    iced::daemon("Jubako", Jubako::update, Jubako::view)
        .subscription(Jubako::subscription)
        .theme(|_, _| Theme::Dark)
        .run_with(Jubako::new)
}

impl Jubako {
    fn new() -> (Self, Task<Message>) {
        let db = Arc::new(Db::new().expect("Failed to initialize DB"));

        let clipboard = Arc::new(Mutex::new(
            Clipboard::new().expect("Failed to initialize clipboard"),
        ));
        let clipboard_state = Arc::new(Mutex::new(clipboard::ClipboardState::default()));

        let manager = GlobalHotKeyManager::new().expect("Failed to init hotkey manager");
        let hotkey = HotKey::new(Some(Modifiers::SUPER | Modifiers::ALT), Code::KeyV);
        let hotkey_id = hotkey.id();
        manager.register(hotkey).expect("Failed to register hotkey");

        let enigo = Arc::new(Mutex::new(
            Enigo::new(&Settings::default()).expect("Failed to init enigo"),
        ));

        if let Err(error) = db.clear_history() {
            eprintln!("Failed to reset history on startup: {}", error);
        }

        clipboard::capture_initial_clipboard(&db, &clipboard, &clipboard_state);

        let is_visible_flag = Arc::new(AtomicBool::new(false));
        let (ui_tx, ui_rx) = mpsc::channel();

        let monitor_db = Arc::clone(&db);
        let monitor_clipboard = Arc::clone(&clipboard);
        let monitor_state = Arc::clone(&clipboard_state);
        let monitor_visible = Arc::clone(&is_visible_flag);

        let _ = std::thread::Builder::new()
            .name("jubako-clipboard-monitor".to_string())
            .spawn(move || loop {
                if platform::wait_for_next_clipboard_update() {
                    let changed = clipboard::poll_clipboard_background(
                        &monitor_db,
                        &monitor_clipboard,
                        &monitor_state,
                    );
                    if changed && monitor_visible.load(Ordering::Relaxed) {
                        let _ = ui_tx.send(());
                    }
                } else {
                    std::thread::sleep(Duration::from_secs(2));
                }
            });

        let mut app = Self {
            db,
            clipboard,
            clipboard_state,
            clipboard_ui_rx: Arc::new(Mutex::new(Some(ui_rx))),
            current_view: ViewMode::History,
            items: Vec::new(),
            folders: Vec::new(),
            hotkey_manager: manager,
            hotkey_id,
            window_id: None,
            is_visible: false,
            is_visible_flag,
            dialog: None,
            enigo,
            expanded_folder_actions: None,
            dragging_item_id: None,
            drag_over_folder: None,
            image_handle_cache: HashMap::new(),
        };

        app.load_folders();

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

            if let Ok(Some((thumb_blob, tw, th))) = self.db.get_item_thumbnail(item_id) {
                if tw > 0 && th > 0 {
                    let thumb_rgba = clipboard::decompress_rgba(&thumb_blob, tw, th);
                    self.image_handle_cache.insert(
                        item_id,
                        image::Handle::from_rgba(tw as u32, th as u32, thumb_rgba),
                    );
                    continue;
                }
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

            let raw_rgba = clipboard::decompress_rgba(&blob, width, height);
            let (tw, th, thumb_rgba) = clipboard::downscale_rgba(
                &raw_rgba,
                width,
                height,
                clipboard::THUMBNAIL_MAX_WIDTH,
                clipboard::THUMBNAIL_MAX_HEIGHT,
            );

            self.image_handle_cache.insert(
                item_id,
                image::Handle::from_rgba(tw as u32, th as u32, thumb_rgba),
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
        const WINDOW_WIDTH: f32 = 640.0;
        const WINDOW_HEIGHT: f32 = 520.0;

        self.is_visible = true;
        self.is_visible_flag.store(true, Ordering::Relaxed);
        self.reset_transient_state();
        clipboard::poll_clipboard_background(&self.db, &self.clipboard, &self.clipboard_state);
        platform::notify_clipboard_activity();
        self.refresh_data();

        let cursor_pos = platform::get_cursor_position();
        let monitor_rect = platform::get_monitor_rect_at_cursor();
        let target_monitor_scale = platform::get_monitor_scale_factor_at_cursor()
            .filter(|scale| *scale > 0.0)
            .unwrap_or(1.0);

        let adjusted_logical = cursor_pos.map(|pos| {
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

            Point::new(
                adjusted_physical.x / target_monitor_scale,
                adjusted_physical.y / target_monitor_scale,
            )
        });

        if let Some(id) = self.window_id {
            let mut tasks = Vec::new();
            if let Some(pos) = adjusted_logical {
                tasks.push(window::move_to(id, pos));
            }
            tasks.push(window::change_mode(id, window::Mode::Windowed));
            tasks.push(window::gain_focus(id));
            Task::batch(tasks)
        } else {
            let position = adjusted_logical
                .map(window::Position::Specific)
                .unwrap_or(window::Position::Centered);

            let (id, open_task) = window::open(window::Settings {
                size: Size::new(WINDOW_WIDTH, WINDOW_HEIGHT),
                position,
                visible: true,
                decorations: false,
                level: window::Level::AlwaysOnTop,
                exit_on_close_request: false,
                ..window::Settings::default()
            });
            self.window_id = Some(id);

            Task::batch(vec![
                open_task.map(|_| Message::Noop),
                window::gain_focus(id),
            ])
        }
    }

    pub(super) fn hide_window(&mut self) -> Task<Message> {
        self.is_visible = false;
        self.is_visible_flag.store(false, Ordering::Relaxed);
        self.reset_transient_state();
        self.items.clear();
        platform::hide_application();

        if let Some(id) = self.window_id.take() {
            window::close(id)
        } else {
            Task::none()
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        let hotkey_sub = Subscription::run(|| {
            iced::futures::stream::unfold((), |_| async {
                loop {
                    let result = tokio::task::spawn_blocking(|| {
                        global_hotkey::GlobalHotKeyEvent::receiver().recv()
                    })
                    .await;

                    match result {
                        Ok(Ok(event)) if event.state == HotKeyState::Pressed => {
                            return Some((Message::ToggleWindow, ()));
                        }
                        Ok(Ok(_)) => continue,
                        _ => {
                            tokio::time::sleep(Duration::from_secs(2)).await;
                        }
                    }
                }
            })
        });

        let ui_rx = self
            .clipboard_ui_rx
            .lock()
            .ok()
            .and_then(|mut guard| guard.take());

        let clipboard_sub = if let Some(rx) = ui_rx {
            Subscription::run_with_id(
                "clipboard-ui-updates",
                iced::futures::stream::unfold(Arc::new(Mutex::new(rx)), |rx| async move {
                    let rx_clone = Arc::clone(&rx);
                    let received = tokio::task::spawn_blocking(move || {
                        rx_clone.lock().ok().and_then(|guard| guard.recv().ok())
                    })
                    .await
                    .ok()
                    .flatten()
                    .is_some();

                    if received {
                        Some((Message::ClipboardUpdated, rx))
                    } else {
                        None
                    }
                }),
            )
        } else {
            Subscription::none()
        };

        let window_sub = if self.is_visible {
            event::listen_with(|event, _status, _id| match event {
                Event::Window(window::Event::Unfocused) => Some(Message::WindowFocusLost),
                Event::Window(window::Event::CloseRequested) => Some(Message::HideWindow),
                _ => None,
            })
        } else {
            Subscription::none()
        };

        let drag_sub = if self.is_visible && self.dragging_item_id.is_some() {
            event::listen_with(|event, _status, _id| match event {
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    Some(Message::CancelDrag)
                }
                _ => None,
            })
        } else {
            Subscription::none()
        };

        Subscription::batch(vec![hotkey_sub, clipboard_sub, window_sub, drag_sub])
    }
}
