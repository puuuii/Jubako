use iced::widget::{
    Column, Row, Space, button, container, horizontal_rule, mouse_area, row, scrollable, text,
};
use iced::{Element, Length, Padding};

use crate::db::Folder;

use super::super::{Jubako, Message, ViewMode, theme};

impl Jubako {
    pub(super) fn view_sidebar(&self) -> Element<'_, Message> {
        let history_style = if self.current_view == ViewMode::History {
            button::primary
        } else {
            button::text
        };

        let is_history_drop_target =
            self.drag.is_dragging() && self.drag.over_target == Some(None);

        let history_button = button(
            text("\u{1F4CB} History")
                .shaping(text::Shaping::Advanced)
                .size(14),
        )
        .on_press(Message::SelectView(ViewMode::History))
        .style(history_style)
        .width(Length::Fill);

        let history_entry: Element<'_, Message> = if self.drag.is_dragging() {
            mouse_area(
                container(history_button)
                    .style(move |_| theme::drop_target_style(is_history_drop_target)),
            )
            .on_enter(Message::DragOverHistory)
            .on_release(Message::DropOnHistory)
            .into()
        } else {
            history_button.into()
        };

        let new_folder_button = button(
            text("\u{2795}")
                .shaping(text::Shaping::Advanced)
                .size(14)
                .color(theme::COLOR_NEW_FOLDER_ICON),
        )
        .on_press(Message::OpenNewFolder(None))
        .style(button::text)
        .padding([4, 8]);

        let history_row = Row::new()
            .spacing(4)
            .align_y(iced::Alignment::Center)
            .push(container(history_entry).width(Length::Fill))
            .push(new_folder_button);

        let mut column = Column::new()
            .spacing(2)
            .padding(8)
            .push(history_row)
            .push(horizontal_rule(1))
            .push(Space::new(Length::Fill, 4));

        for folder in self
            .folders
            .iter()
            .filter(|folder| folder.parent_id.is_none())
        {
            column = column.push(self.view_folder_tree(folder, 0));
        }

        scrollable(column).height(Length::Fill).into()
    }

    fn view_folder_tree(&self, folder: &Folder, depth: u16) -> Element<'_, Message> {
        let indent = (depth as f32) * 16.0;
        let folder_id = folder.id;
        let is_drop_target =
            self.drag.is_dragging() && self.drag.over_target == Some(Some(folder_id));

        let button_style = if self.current_view == ViewMode::Folder(folder_id) {
            button::primary
        } else {
            button::text
        };

        let folder_button = button(
            text(format!("\u{1F4C1} {}", folder.name))
                .shaping(text::Shaping::Advanced)
                .size(13),
        )
        .on_press(Message::SelectView(ViewMode::Folder(folder_id)))
        .style(button_style)
        .width(Length::Fill);

        let actions_button = button(text("\u{22EE}").size(14).shaping(text::Shaping::Advanced))
            .on_press(Message::ToggleFolderActions(folder_id))
            .style(button::text)
            .padding(2);

        let content = Row::new()
            .push(Space::new(indent, 0))
            .push(folder_button)
            .push(actions_button)
            .align_y(iced::Alignment::Center);

        let folder_row: Element<'_, Message> = if self.drag.is_dragging() {
            mouse_area(
                container(content).style(move |_| theme::drop_target_style(is_drop_target)),
            )
            .on_enter(Message::DragOverFolder(folder_id))
            .on_release(Message::DropOnFolder(folder_id))
            .into()
        } else {
            content.into()
        };

        let mut column = Column::new().spacing(1).push(folder_row);

        if self.expanded_folder_actions == Some(folder_id) {
            column = column.push(
                container(
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
                .padding(Padding {
                    top: 2.0,
                    right: 0.0,
                    bottom: 2.0,
                    left: indent + 20.0,
                }),
            );
        }

        for child in self
            .folders
            .iter()
            .filter(|child| child.parent_id == Some(folder_id))
        {
            column = column.push(self.view_folder_tree(child, depth + 1));
        }

        column.into()
    }
}
