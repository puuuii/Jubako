use iced::widget::container;
use iced::{Background, Border, Color, Theme};

pub(super) const COLOR_FAVORITE_STAR: Color = Color::from_rgb(1.0, 0.85, 0.0);
pub(super) const COLOR_NEW_FOLDER_ICON: Color = Color::from_rgb(0.5, 0.8, 0.5);
pub(super) const COLOR_HEADER_TEXT: Color = Color::from_rgb(0.7, 0.7, 0.8);
pub(super) const COLOR_MUTED_TEXT: Color = Color::from_rgb(0.5, 0.5, 0.5);
pub(super) const COLOR_DRAG_HANDLE: Color = Color::from_rgb(0.45, 0.45, 0.45);
pub(super) const COLOR_DELETE_ICON: Color = Color::from_rgb(0.6, 0.6, 0.6);

pub(super) fn app_background_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgb(0.1, 0.1, 0.12))),
        ..container::Style::default()
    }
}

pub(super) fn sidebar_container_style(_theme: &Theme) -> container::Style {
    container::Style {
        border: Border {
            color: Color::from_rgb(0.3, 0.3, 0.3),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

pub(super) fn drop_target_style(is_drop_target: bool) -> container::Style {
    if is_drop_target {
        container::Style {
            background: Some(Background::Color(Color::from_rgb(0.2, 0.3, 0.5))),
            border: Border {
                color: Color::from_rgb(0.3, 0.5, 0.8),
                width: 2.0,
                radius: 4.0.into(),
            },
            ..container::Style::default()
        }
    } else {
        container::Style::default()
    }
}

pub(super) fn item_card_style(is_being_dragged: bool) -> container::Style {
    container::Style {
        background: Some(Background::Color(if is_being_dragged {
            Color::from_rgb(0.2, 0.2, 0.28)
        } else {
            Color::from_rgb(0.14, 0.14, 0.16)
        })),
        border: Border {
            color: if is_being_dragged {
                Color::from_rgb(0.3, 0.5, 0.8)
            } else {
                Color::from_rgb(0.2, 0.2, 0.22)
            },
            width: 1.0,
            radius: 4.0.into(),
        },
        ..container::Style::default()
    }
}

pub(super) fn dialog_card_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgb(0.15, 0.15, 0.18))),
        border: Border {
            color: Color::from_rgb(0.4, 0.4, 0.5),
            width: 2.0,
            radius: 8.0.into(),
        },
        ..container::Style::default()
    }
}

pub(super) fn dialog_backdrop_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5))),
        ..container::Style::default()
    }
}
