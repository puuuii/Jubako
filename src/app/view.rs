use iced::widget::{Space, column, container, row};
use iced::{Element, Length, window};

use super::{Jubako, Message, theme};

mod dialog;
mod item_list;
mod sidebar;

impl Jubako {
    pub(super) fn view(&self, _window: window::Id) -> Element<'_, Message> {
        if !self.window.is_visible {
            return Space::new(0, 0).into();
        }

        let body = row![
            container(self.view_sidebar())
                .width(Length::FillPortion(1))
                .height(Length::Fill)
                .style(theme::sidebar_container_style),
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
                    container(
                        container(dialog_content)
                            .padding(20)
                            .max_width(450)
                            .style(theme::dialog_card_style),
                    )
                    .width(Length::Fill)
                    .center_x(Length::Fill),
                    Space::new(Length::Fill, Length::FillPortion(1)),
                ]
                .align_x(iced::Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::dialog_backdrop_style)
            .into()
        } else {
            container(layout)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(theme::app_background_style)
                .into()
        }
    }
}
