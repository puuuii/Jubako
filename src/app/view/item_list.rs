use iced::widget::{
    Column, button, column, container, horizontal_rule, image, mouse_area, row, scrollable, text,
};
use iced::{ContentFit, Element, Length, mouse};

use crate::db::{ContentType, Item};
use crate::image_codec;

use super::super::{Jubako, Message, ViewMode, theme};

impl Jubako {
    pub(super) fn view_main_pane(&self) -> Element<'_, Message> {
        let header_text = match &self.current_view {
            ViewMode::History => "History".to_string(),
            ViewMode::Folder(folder_id) => {
                let name = self
                    .folders
                    .iter()
                    .find(|folder| folder.id == *folder_id)
                    .map(|folder| folder.name.as_str())
                    .unwrap_or("Folder");
                format!("\u{1F4C1} {name}")
            }
        };

        let header = container(
            text(header_text)
                .size(16)
                .shaping(text::Shaping::Advanced)
                .color(theme::COLOR_HEADER_TEXT),
        )
        .padding([8, 12]);

        let mut items_column = Column::new().spacing(4).padding([4, 8]);

        if self.items.is_empty() {
            items_column = items_column.push(
                container(text("No items").size(14).color(theme::COLOR_MUTED_TEXT))
                    .padding(20)
                    .center_x(Length::Fill),
            );
        } else {
            for item in &self.items {
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

    fn view_item<'a>(&'a self, item: &'a Item) -> Element<'a, Message> {
        let favorite_icon = if item.is_favorite { "\u{2605}" } else { "" };

        let content_button = match item.content_type {
            ContentType::Image => self.view_image_item_button(item, favorite_icon),
            ContentType::Text => view_text_item_button(item, favorite_icon),
        };

        let is_being_dragged = self.drag.dragging_item_id == Some(item.id);

        let drag_handle: Element<'_, Message> = mouse_area(
            container(
                text("\u{2807}")
                    .size(14)
                    .shaping(text::Shaping::Advanced)
                    .color(theme::COLOR_DRAG_HANDLE),
            )
            .padding([6, 4]),
        )
        .on_press(Message::StartDragItem(item.id))
        .interaction(if is_being_dragged {
            mouse::Interaction::Grabbing
        } else {
            mouse::Interaction::Grab
        })
        .into();

        let delete_button = button(
            text("\u{00D7}")
                .size(16)
                .shaping(text::Shaping::Advanced)
                .color(theme::COLOR_DELETE_ICON),
        )
        .on_press(Message::DeleteItem(item.id))
        .style(button::text)
        .padding([4, 6]);

        container(
            row![drag_handle, content_button, delete_button]
                .spacing(0)
                .align_y(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .style(move |_| theme::item_card_style(is_being_dragged))
        .into()
    }

    fn view_image_item_button<'a>(
        &'a self,
        item: &'a Item,
        favorite_icon: &'a str,
    ) -> Element<'a, Message> {
        let dimensions = image_codec::parse_image_description(&item.content_data);
        let press_message = if dimensions.is_some() {
            Message::PasteItem(item.id)
        } else {
            Message::Noop
        };

        let default_label = dimensions
            .map(|(width, height)| format!("\u{1F5BC} Image ({width}x{height})"))
            .unwrap_or_else(|| "\u{1F5BC} Image".to_string());

        let display_label = item
            .label
            .as_deref()
            .map(|label| truncate_chars(label, 60))
            .unwrap_or(default_label);

        let mut content = Column::new().spacing(2);

        if let (Some(_), Some(handle)) = (dimensions, self.image_handle_cache.get(&item.id)) {
            content = content.push(
                container(
                    image(handle.clone())
                        .content_fit(ContentFit::ScaleDown)
                        .width(Length::Fixed(120.0))
                        .height(Length::Fixed(80.0)),
                )
                .padding(2),
            );
        }

        content = content.push(label_row(display_label, favorite_icon));

        button(content)
            .on_press(press_message)
            .style(button::text)
            .width(Length::Fill)
            .padding([6, 8])
            .into()
    }
}

fn view_text_item_button<'a>(item: &'a Item, favorite_icon: &'a str) -> Element<'a, Message> {
    let source = item.label.as_deref().unwrap_or(&item.content_data);
    let display_text = truncate_chars(source, 80).replace('\n', " \u{21B5} ");

    button(column![label_row(display_text, favorite_icon)].spacing(2))
        .on_press(Message::PasteItem(item.id))
        .style(button::text)
        .width(Length::Fill)
        .padding([6, 8])
        .into()
}

fn label_row<'a>(label: String, favorite_icon: &'a str) -> Element<'a, Message> {
    row![
        text(label)
            .size(13)
            .shaping(text::Shaping::Advanced)
            .width(Length::Fill),
        text(favorite_icon)
            .size(13)
            .shaping(text::Shaping::Advanced)
            .color(theme::COLOR_FAVORITE_STAR),
    ]
    .spacing(4)
    .into()
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    match value.char_indices().nth(max_chars) {
        Some((idx, _)) => format!("{}...", &value[..idx]),
        None => value.to_string(),
    }
}
