//! Layout: toolbar, image area (image + crop overlay stacked), status line. Full screen
//! shows only the image area.
//! Widget ids used by tests: `photo` (the current photo, sharp), `preview` (its embedded
//! preview, or still the previous photo while it decodes), `message` (error / empty),
//! `body` (the image area).

use crate::action::Action;
use crate::app::App;
use crate::crop_overlay::Overlay;
use crate::message::Message;
use crate::state::{Display, Mode};
use crate::theme;
use iced::widget::{button, canvas, column, container, image, row, space, stack, text};
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
        (Some(_), Some(Err(e))) => text(e.clone()).size(20).into(),
        (Some(_), _) => match &s.shown {
            Some(shown) => {
                let img = |h: &image::Handle, opacity: f32| {
                    image(h.clone())
                        .content_fit(ContentFit::Contain)
                        .width(Fill)
                        .height(Fill)
                        .opacity(opacity)
                };
                match s.fade_progress() {
                    Some((from, t)) => stack![img(from, 1.0 - t), img(&shown.handle, t)].into(),
                    None => img(&shown.handle, 1.0).into(),
                }
            }
            // First image of a folder still decoding: nothing to show yet.
            None => space().into(),
        },
    };
    let id = match s.display() {
        Display::Sharp => "photo",
        Display::Preview | Display::Previous => "preview",
        Display::None => "message",
    };
    // The overlay is always present: it also handles the keyboard shortcuts.
    let overlay = canvas(Overlay {
        active: s.mode == Mode::Crop,
        selection: s.crop.selection,
        image: s.picture().map(|p| p.source_size()),
    })
    .width(Fill)
    .height(Fill);
    let bg = if s.fullscreen { Color::BLACK } else { BG };
    let body = container(stack![container(content).center(Fill).id(id), overlay])
        .id("body")
        .style(move |_| container::Style::default().background(bg).color(Color::WHITE));
    if s.fullscreen {
        return body.into();
    }

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
