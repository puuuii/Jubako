use iced::Element;
use iced::widget::{Space, button, column, horizontal_space, row, text, text_input};
use iced::Length;

use super::super::{Dialog, Jubako, Message};

impl Jubako {
    pub(super) fn view_dialog<'a>(&self, dialog: &'a Dialog) -> Element<'a, Message> {
        match dialog {
            Dialog::RenameFolder { current_name, .. } => {
                folder_name_dialog("Rename Folder", current_name, "Save", Message::RenameFolder)
            }
            Dialog::NewFolder { parent_id, name } => {
                let title = if parent_id.is_some() {
                    "New Subfolder"
                } else {
                    "New Folder"
                };
                folder_name_dialog(title, name, "Create", Message::CreateFolder)
            }
        }
    }
}

fn folder_name_dialog<'a>(
    title: &'a str,
    value: &'a str,
    confirm_label: &'a str,
    on_confirm: Message,
) -> Element<'a, Message> {
    column![
        text(title).size(16),
        Space::new(Length::Fill, 8),
        text_input("Folder name...", value)
            .on_input(Message::DialogInputChanged)
            .on_submit(on_confirm.clone())
            .padding(8)
            .size(14),
        Space::new(Length::Fill, 12),
        row![
            horizontal_space(),
            button(text("Cancel").size(13))
                .on_press(Message::CloseDialog)
                .style(button::secondary)
                .padding([6, 16]),
            button(text(confirm_label).size(13))
                .on_press(on_confirm)
                .style(button::primary)
                .padding([6, 16]),
        ]
        .spacing(8),
    ]
    .spacing(4)
    .into()
}
