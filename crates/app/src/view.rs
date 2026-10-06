//! Layout: toolbar, image area (image + crop overlay stacked), status line.
//! Widget ids used by tests: `photo` (image shown), `message` (error / empty / loading),
//! `body` (the image area).

use crate::action::Action;
use crate::app::App;
use crate::crop_overlay::Overlay;
use crate::message::Message;
use crate::state::Mode;
use crate::theme;
use iced::widget::{button, canvas, column, container, image, row, stack, text};
use iced::{Color, ContentFit, Element, Fill};

const BG: Color = Color::from_rgb(0x11 as f32 / 255.0, 0x11 as f32 / 255.0, 0x11 as f32 / 255.0);

pub fn view(app: &App) -> Element<'_, Message> {
    let s = &app.state;
    let current = s.current();
    let content: Element<_> = match (&s.folder, current) {
        (None, _) => text("Open a folder with photos (Ctrl+O)").size(20).into(),
        (Some(_), _) if s.folder_error.is_some() => {
            text(s.folder_error.clone().unwrap_or_default()).size(20).into()
        }
        (Some(f), _) if f.is_empty() => text(format!("No images in {}", f.path.display())).size(20).into(),
        (Some(_), Some(Ok(p))) => {
            image(p.handle.clone()).content_fit(ContentFit::Contain).width(Fill).height(Fill).into()
        }
        (Some(_), Some(Err(e))) => text(e.clone()).size(20).into(),
        (Some(_), None) => text("Loading…").into(),
    };
    let id = if matches!(current, Some(Ok(_))) { "photo" } else { "message" };
    // The overlay is always present: it also handles the keyboard shortcuts.
    let overlay = canvas(Overlay {
        active: s.mode == Mode::Crop,
        selection: s.crop.selection,
        image: s.picture().map(|p| p.source_size()),
    })
    .width(Fill)
    .height(Fill);
    let body = container(stack![container(content).center(Fill).id(id), overlay])
        .id("body")
        .style(|_| container::Style::default().background(BG).color(Color::WHITE));

    let tool = |label, suggested, msg| {
        button(text(label)).padding([6, 16]).style(theme::button(suggested)).on_press(msg)
    };
    let toolbar = row![
        tool("Open folder…", false, Message::OpenDialog),
        tool("Crop", s.mode == Mode::Crop, Action::ToggleCrop.into()),
    ]
    .spacing(8);
    column![
        container(toolbar).padding([6, 10]).width(Fill).style(theme::bar),
        body,
        container(text(s.status()).size(13)).padding([4, 12]).width(Fill).style(theme::bar),
    ]
    .into()
}
