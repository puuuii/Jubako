use iced::widget::{
    button, column, container, horizontal_rule, horizontal_space, image, mouse_area, row,
    scrollable, text, text_input, Column, Row, Space,
};
use iced::{Color, Element, Length, Theme};

use crate::db::{Folder, Item};

use super::{clipboard, Dialog, Jubako, Message, ViewMode};

impl Jubako {
    pub(super) fn view(&self) -> Element<'_, Message> {
        let body = row![
            container(self.view_sidebar())
                .width(Length::FillPortion(1))
                .height(Length::Fill)
                .style(|_theme: &Theme| container::Style {
                    border: iced::Border {
                        color: Color::from_rgb(0.3, 0.3, 0.3),
                        width: 1.0,
                        radius: 0.0.into(),
                    },
                    ..container::Style::default()
                }),
            container(self.view_main_pane())
                .width(Length::FillPortion(3))
                .height(Length::Fill),
        ]
        .spacing(0)
        .height(Length::Fill);

        let layout: Element<'_, Message> = column![body].spacing(0).height(Length::Fill).into();

        if let Some(dialog) = &self.dialog {
            let dialog_content = self.view_dialog(dialog);
            container(
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
                    ))
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
            })
            .into()
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

    fn view_sidebar(&self) -> Element<'_, Message> {
        let history_style = if self.current_view == ViewMode::History {
            button::primary
        } else {
            button::text
        };

        let is_history_drop_target =
            self.dragging_item_id.is_some() && self.drag_over_folder == Some(None);

        let history_button = button(
            text("\u{1F4CB} History")
                .shaping(text::Shaping::Advanced)
                .size(14),
        )
        .on_press(Message::SelectView(ViewMode::History))
        .style(history_style)
        .width(Length::Fill);

        let history_entry: Element<'_, Message> = if self.dragging_item_id.is_some() {
            mouse_area(container(history_button).style(move |_theme: &Theme| {
                if is_history_drop_target {
                    container::Style {
                        background: Some(iced::Background::Color(Color::from_rgb(0.2, 0.3, 0.5))),
                        border: iced::Border {
                            color: Color::from_rgb(0.3, 0.5, 0.8),
                            width: 2.0,
                            radius: 4.0.into(),
                        },
                        ..container::Style::default()
                    }
                } else {
                    container::Style::default()
                }
            }))
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
                .color(Color::from_rgb(0.5, 0.8, 0.5)),
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
            self.dragging_item_id.is_some() && self.drag_over_folder == Some(Some(folder_id));

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

        let folder_row: Element<'_, Message> = if self.dragging_item_id.is_some() {
            mouse_area(container(content).style(move |_theme: &Theme| {
                if is_drop_target {
                    container::Style {
                        background: Some(iced::Background::Color(Color::from_rgb(0.2, 0.3, 0.5))),
                        border: iced::Border {
                            color: Color::from_rgb(0.3, 0.5, 0.8),
                            width: 2.0,
                            radius: 4.0.into(),
                        },
                        ..container::Style::default()
                    }
                } else {
                    container::Style::default()
                }
            }))
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
                .padding(iced::Padding {
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

    fn view_main_pane(&self) -> Element<'_, Message> {
        let header_text = match &self.current_view {
            ViewMode::History => "History".to_string(),
            ViewMode::Folder(folder_id) => {
                let name = self
                    .folders
                    .iter()
                    .find(|folder| folder.id == *folder_id)
                    .map(|folder| folder.name.clone())
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

        let mut items_column = Column::new().spacing(4).padding([4, 8]);
        let items = self.displayed_items();

        if items.is_empty() {
            items_column = items_column.push(
                container(
                    text("No items")
                        .size(14)
                        .color(Color::from_rgb(0.5, 0.5, 0.5)),
                )
                .padding(20)
                .center_x(Length::Fill),
            );
        } else {
            for item in items {
                items_column = items_column.push(self.view_item(item));
            }
        }

        column![
            header,
            horizontal_rule(1),
            scrollable(items_column).height(Length::Fill)
        ]
        .spacing(0)
        .height(Length::Fill)
        .into()
    }

    fn view_item(&self, item: &Item) -> Element<'_, Message> {
        let is_image = item.content_type == "image";
        let favorite_icon = if item.is_favorite { "\u{2605}" } else { "" };

        let press_message = if is_image {
            if clipboard::parse_image_description(&item.content_data).is_some() {
                Message::PasteImageItem(item.id)
            } else {
                Message::Noop
            }
        } else {
            Message::PasteItem(item.content_data.clone())
        };

        let content_button = if is_image {
            let dimensions = clipboard::parse_image_description(&item.content_data);
            let default_label = dimensions
                .map(|(width, height)| format!("\u{1F5BC} Image ({}x{})", width, height))
                .unwrap_or_else(|| "\u{1F5BC} Image".to_string());

            let display_label = item
                .label
                .as_ref()
                .map(|label| truncate_str(label, 60))
                .unwrap_or(default_label);

            let mut content = Column::new().spacing(2);

            if let (Some(_), Some(handle)) = (dimensions, self.image_handle_cache.get(&item.id)) {
                content = content.push(
                    container(
                        image(handle.clone())
                            .content_fit(iced::ContentFit::ScaleDown)
                            .width(Length::Fixed(120.0))
                            .height(Length::Fixed(80.0)),
                    )
                    .padding(2),
                );
            }

            content = content.push(
                row![
                    text(display_label)
                        .size(13)
                        .shaping(text::Shaping::Advanced)
                        .width(Length::Fill),
                    text(favorite_icon)
                        .size(13)
                        .shaping(text::Shaping::Advanced)
                        .color(Color::from_rgb(1.0, 0.85, 0.0)),
                ]
                .spacing(4),
            );

            button(content)
                .on_press(press_message)
                .style(button::text)
                .width(Length::Fill)
                .padding([6, 8])
        } else {
            let display_text = item
                .label
                .as_ref()
                .map(|label| truncate_str(label, 80))
                .unwrap_or_else(|| truncate_str(&item.content_data, 80))
                .replace('\n', " \u{21B5} ");

            button(
                column![
                    row![
                        text(display_text)
                            .size(13)
                            .shaping(text::Shaping::Advanced)
                            .width(Length::Fill),
                        text(favorite_icon)
                            .size(13)
                            .shaping(text::Shaping::Advanced)
                            .color(Color::from_rgb(1.0, 0.85, 0.0)),
                    ]
                    .spacing(4),
                ]
                .spacing(2),
            )
            .on_press(press_message)
            .style(button::text)
            .width(Length::Fill)
            .padding([6, 8])
        };

        let drag_handle: Element<'_, Message> = mouse_area(
            container(
                text("\u{2807}")
                    .size(14)
                    .shaping(text::Shaping::Advanced)
                    .color(Color::from_rgb(0.45, 0.45, 0.45)),
            )
            .padding([6, 4]),
        )
        .on_press(Message::StartDragItem(item.id))
        .interaction(if self.dragging_item_id == Some(item.id) {
            iced::mouse::Interaction::Grabbing
        } else {
            iced::mouse::Interaction::Grab
        })
        .into();

        let delete_button = button(
            text("\u{00D7}")
                .size(16)
                .shaping(text::Shaping::Advanced)
                .color(Color::from_rgb(0.6, 0.6, 0.6)),
        )
        .on_press(Message::DeleteItem(item.id))
        .style(button::text)
        .padding([4, 6]);

        let is_being_dragged = self.dragging_item_id == Some(item.id);

        container(
            row![drag_handle, content_button, delete_button]
                .spacing(0)
                .align_y(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .style(move |_theme: &Theme| container::Style {
            background: Some(iced::Background::Color(if is_being_dragged {
                Color::from_rgb(0.2, 0.2, 0.28)
            } else {
                Color::from_rgb(0.14, 0.14, 0.16)
            })),
            border: iced::Border {
                color: if is_being_dragged {
                    Color::from_rgb(0.3, 0.5, 0.8)
                } else {
                    Color::from_rgb(0.2, 0.2, 0.22)
                },
                width: 1.0,
                radius: 4.0.into(),
            },
            ..container::Style::default()
        })
        .into()
    }

    fn view_dialog(&self, dialog: &Dialog) -> Element<'_, Message> {
        match dialog {
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
        }
    }

    fn displayed_items(&self) -> Vec<&Item> {
        self.items.iter().collect()
    }
}

fn truncate_str(value: &str, max: usize) -> String {
    if value.len() <= max {
        value.to_string()
    } else {
        let truncated: String = value.chars().take(max).collect();
        format!("{}...", truncated)
    }
}
