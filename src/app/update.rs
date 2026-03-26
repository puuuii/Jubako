use iced::Task;

use super::{clipboard, Dialog, Jubako, Message, ViewMode};

impl Jubako {
    pub(super) fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Noop => Task::none(),
            Message::SelectView(view) => self.handle_select_view(view),
            Message::ClipboardUpdated => self.handle_clipboard_updated(),
            Message::ToggleWindow => self.handle_toggle_window(),
            Message::HideWindow => self.hide_window(),
            Message::WindowFocusLost => self.handle_window_focus_lost(),
            Message::PasteItem(content) => clipboard::set_text_and_simulate_paste(self, content),
            Message::PasteImageItem(item_id) => self.handle_paste_image_item(item_id),
            Message::DeleteItem(id) => self.handle_delete_item(id),
            Message::StartDragItem(id) => self.handle_start_drag_item(id),
            Message::DragOverFolder(folder_id) => self.handle_drag_over_folder(folder_id),
            Message::DragOverHistory => self.handle_drag_over_history(),
            Message::DropOnFolder(folder_id) => self.handle_drop_on_folder(folder_id),
            Message::DropOnHistory => self.handle_drop_on_history(),
            Message::CancelDrag => self.handle_cancel_drag(),
            Message::OpenNewFolder(parent_id) => self.handle_open_new_folder(parent_id),
            Message::CreateFolder => self.handle_create_folder(),
            Message::DeleteFolder(id) => self.handle_delete_folder(id),
            Message::OpenRenameFolder(id, current_name) => {
                self.handle_open_rename_folder(id, current_name)
            }
            Message::RenameFolder => self.handle_rename_folder(),
            Message::DialogInputChanged(value) => self.handle_dialog_input_changed(value),
            Message::CloseDialog => self.handle_close_dialog(),
            Message::ToggleFolderActions(id) => self.handle_toggle_folder_actions(id),
        }
    }

    fn handle_select_view(&mut self, view: ViewMode) -> Task<Message> {
        self.current_view = view;
        self.expanded_folder_actions = None;
        self.load_items();
        Task::none()
    }

    fn handle_clipboard_updated(&mut self) -> Task<Message> {
        if clipboard::poll_clipboard(self) {
            self.load_items();
        }

        Task::none()
    }

    fn handle_paste_image_item(&mut self, item_id: i64) -> Task<Message> {
        let Some(item) = self.items.iter().find(|item| item.id == item_id) else {
            return Task::none();
        };
        let Some((width, height)) = clipboard::parse_image_description(&item.content_data) else {
            return Task::none();
        };
        let Ok(Some(blob)) = self.db.get_item_blob(item_id) else {
            return Task::none();
        };

        clipboard::set_image_and_simulate_paste(self, width, height, blob)
    }

    fn handle_toggle_window(&mut self) -> Task<Message> {
        if self.is_visible {
            self.hide_window()
        } else {
            self.show_window()
        }
    }

    fn handle_window_focus_lost(&mut self) -> Task<Message> {
        if self.is_visible && self.dialog.is_none() {
            self.hide_window()
        } else {
            Task::none()
        }
    }

    fn handle_delete_item(&mut self, id: i64) -> Task<Message> {
        let _ = self.db.delete_item(id);
        self.refresh_data();
        Task::none()
    }

    fn handle_start_drag_item(&mut self, id: i64) -> Task<Message> {
        self.dragging_item_id = Some(id);
        self.drag_over_folder = None;
        Task::none()
    }

    fn handle_drag_over_folder(&mut self, folder_id: i64) -> Task<Message> {
        if self.dragging_item_id.is_some() {
            self.drag_over_folder = Some(Some(folder_id));
        }

        Task::none()
    }

    fn handle_drag_over_history(&mut self) -> Task<Message> {
        if self.dragging_item_id.is_some() {
            self.drag_over_folder = Some(None);
        }

        Task::none()
    }

    fn handle_drop_on_folder(&mut self, folder_id: i64) -> Task<Message> {
        if let Some(item_id) = self.dragging_item_id.take() {
            let _ = self.db.move_item_to_folder(item_id, Some(folder_id));
            self.refresh_data();
        }

        self.drag_over_folder = None;
        Task::none()
    }

    fn handle_drop_on_history(&mut self) -> Task<Message> {
        if let Some(item_id) = self.dragging_item_id.take() {
            let _ = self.db.move_item_to_folder(item_id, None);
            self.refresh_data();
        }

        self.drag_over_folder = None;
        Task::none()
    }

    fn handle_cancel_drag(&mut self) -> Task<Message> {
        self.dragging_item_id = None;
        self.drag_over_folder = None;
        Task::none()
    }

    fn handle_open_new_folder(&mut self, parent_id: Option<i64>) -> Task<Message> {
        self.dialog = Some(Dialog::NewFolder {
            parent_id,
            name: String::new(),
        });
        self.expanded_folder_actions = None;
        Task::none()
    }

    fn handle_create_folder(&mut self) -> Task<Message> {
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

    fn handle_delete_folder(&mut self, id: i64) -> Task<Message> {
        let _ = self.db.delete_folder(id);
        self.expanded_folder_actions = None;

        if self.current_view == ViewMode::Folder(id) {
            self.current_view = ViewMode::History;
        }

        self.refresh_data();
        Task::none()
    }

    fn handle_open_rename_folder(&mut self, id: i64, current_name: String) -> Task<Message> {
        self.dialog = Some(Dialog::RenameFolder {
            folder_id: id,
            current_name,
        });
        self.expanded_folder_actions = None;
        Task::none()
    }

    fn handle_rename_folder(&mut self) -> Task<Message> {
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

    fn handle_dialog_input_changed(&mut self, value: String) -> Task<Message> {
        match &mut self.dialog {
            Some(Dialog::RenameFolder {
                ref mut current_name,
                ..
            }) => *current_name = value,
            Some(Dialog::NewFolder { ref mut name, .. }) => *name = value,
            _ => {}
        }

        Task::none()
    }

    fn handle_close_dialog(&mut self) -> Task<Message> {
        self.dialog = None;
        Task::none()
    }

    fn handle_toggle_folder_actions(&mut self, id: i64) -> Task<Message> {
        if self.expanded_folder_actions == Some(id) {
            self.expanded_folder_actions = None;
        } else {
            self.expanded_folder_actions = Some(id);
        }

        Task::none()
    }
}
