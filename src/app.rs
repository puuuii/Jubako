use arboard::Clipboard;
use global_hotkey::{
    GlobalHotKeyManager, HotKeyState,
    hotkey::{Code, HotKey, Modifiers},
};
use iced::widget::image;
use iced::{Event, Point, Size, Subscription, Task, Theme, event, mouse, window};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use crate::db::{ContentType, Db, Folder, Item};
use crate::image_codec;

mod clipboard;
pub(crate) mod platform;
mod theme;
mod update;
mod view;

#[derive(Debug, Clone, PartialEq, Eq)]
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
    WindowFocused,
    WindowFocusLost,
    PasteItem(i64),
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

#[derive(Default)]
struct WindowState {
    id: Option<window::Id>,
    is_visible: bool,
    has_focused: bool,
    shown_at: Option<Instant>,
}

#[derive(Default)]
struct DragState {
    dragging_item_id: Option<i64>,
    over_target: Option<Option<i64>>,
}

impl DragState {
    fn is_dragging(&self) -> bool {
        self.dragging_item_id.is_some()
    }

    fn clear(&mut self) {
        self.dragging_item_id = None;
        self.over_target = None;
    }
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
    window: WindowState,
    is_visible_flag: Arc<AtomicBool>,
    dialog: Option<Dialog>,
    expanded_folder_actions: Option<i64>,
    drag: DragState,
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

        if let Err(error) = db.clear_history() {
            eprintln!("Failed to reset history on startup: {error}");
        }

        clipboard::capture_initial_clipboard(&db, &clipboard, &clipboard_state);

        let is_visible_flag = Arc::new(AtomicBool::new(false));
        let (ui_tx, ui_rx) = mpsc::channel();
        spawn_clipboard_monitor(
            Arc::clone(&db),
            Arc::clone(&clipboard),
            Arc::clone(&clipboard_state),
            Arc::clone(&is_visible_flag),
            ui_tx,
        );

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
            window: WindowState::default(),
            is_visible_flag,
            dialog: None,
            expanded_folder_actions: None,
            drag: DragState::default(),
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
            .filter(|item| item.content_type == ContentType::Image)
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
                    let thumb_rgba = image_codec::decompress_rgba(&thumb_blob, tw, th);
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
            let Some((width, height)) =
                image_codec::parse_image_description(&item.content_data)
            else {
                continue;
            };
            let Ok(Some(blob)) = self.db.get_item_blob(item_id) else {
                continue;
            };

            let raw_rgba = image_codec::decompress_rgba(&blob, width, height);
            let (tw, th, thumb_rgba) = image_codec::downscale_rgba(
                &raw_rgba,
                width,
                height,
                image_codec::THUMBNAIL_MAX_WIDTH,
                image_codec::THUMBNAIL_MAX_HEIGHT,
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
        self.drag.clear();
        self.dialog = None;
    }

    fn show_window(&mut self) -> Task<Message> {
        const WINDOW_WIDTH: f32 = 640.0;
        const WINDOW_HEIGHT: f32 = 520.0;

        self.window.is_visible = true;
        self.is_visible_flag.store(true, Ordering::Relaxed);
        self.window.has_focused = false;
        self.window.shown_at = Some(Instant::now());
        self.reset_transient_state();

        clipboard::poll_clipboard_background(&self.db, &self.clipboard, &self.clipboard_state);
        platform::notify_clipboard_activity();
        self.refresh_data();
        platform::unhide_application();

        let adjusted_logical = compute_window_position(WINDOW_WIDTH, WINDOW_HEIGHT);

        if let Some(id) = self.window.id {
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
            self.window.id = Some(id);

            open_task.then(window::gain_focus)
        }
    }

    pub(super) fn hide_window(&mut self) -> Task<Message> {
        self.window.is_visible = false;
        self.is_visible_flag.store(false, Ordering::Relaxed);
        self.window.has_focused = false;
        self.window.shown_at = None;
        self.reset_transient_state();
        self.items.clear();
        platform::hide_application();

        if let Some(id) = self.window.id.take() {
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

        let window_sub = if self.window.is_visible {
            event::listen_with(|event, _status, _id| match event {
                Event::Window(window::Event::Focused) => Some(Message::WindowFocused),
                Event::Window(window::Event::Unfocused) => Some(Message::WindowFocusLost),
                Event::Window(window::Event::CloseRequested) => Some(Message::HideWindow),
                _ => None,
            })
        } else {
            Subscription::none()
        };

        let drag_sub = if self.window.is_visible && self.drag.is_dragging() {
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

fn spawn_clipboard_monitor(
    db: Arc<Db>,
    clipboard: Arc<Mutex<Clipboard>>,
    state: Arc<Mutex<clipboard::ClipboardState>>,
    visible_flag: Arc<AtomicBool>,
    ui_tx: mpsc::Sender<()>,
) {
    let _ = std::thread::Builder::new()
        .name("jubako-clipboard-monitor".to_string())
        .spawn(move || loop {
            if platform::wait_for_next_clipboard_update() {
                let changed = clipboard::poll_clipboard_background(&db, &clipboard, &state);
                if changed && visible_flag.load(Ordering::Relaxed) {
                    let _ = ui_tx.send(());
                }
            } else {
                std::thread::sleep(Duration::from_secs(2));
            }
        });
}

fn compute_window_position(window_width: f32, window_height: f32) -> Option<Point> {
    let cursor_pos = platform::get_cursor_position()?;
    let monitor_rect = platform::get_monitor_rect_at_cursor();
    let scale = platform::get_monitor_scale_factor_at_cursor()
        .filter(|s| *s > 0.0)
        .unwrap_or(1.0);

    let physical = if let Some((origin, size)) = monitor_rect {
        let mid_x = origin.x + size.width / 2.0;
        let mid_y = origin.y + size.height / 2.0;
        let w_phys = window_width * scale;
        let h_phys = window_height * scale;

        let x = if cursor_pos.x >= mid_x {
            cursor_pos.x - w_phys
        } else {
            cursor_pos.x
        };
        let y = if cursor_pos.y >= mid_y {
            cursor_pos.y - h_phys
        } else {
            cursor_pos.y
        };
        Point::new(x, y)
    } else {
        cursor_pos
    };

    Some(Point::new(physical.x / scale, physical.y / scale))
}
