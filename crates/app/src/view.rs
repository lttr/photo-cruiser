//! Layout: image area (the grid of thumbnails, or one image with the crop overlay
//! stacked over it), then a footer with the status line and the menu button. The menu
//! opens upwards from the button, over the image. Full screen shows only the image area.
//! Widget ids used by tests: `photo` (the current photo, sharp), `preview` (its embedded
//! preview, or still the previous photo while it decodes), `message` (error / empty),
//! `grid` (the thumbnails' scrollable), `selected` (the current photo's grid cell),
//! `body` (the image area), `menu-button`, `menu`, `menu-focus` (its focused item),
//! `help` (shortcuts panel), `about`.

use crate::action::Action;
use crate::app::App;
use crate::crop_overlay::{Modes, Overlay, SHORTCUTS};
use crate::grid::LABEL;
use crate::menu;
use crate::message::Message;
use crate::state::{Display, Mode, State, View, tilde};
use crate::theme;
use iced::alignment::Horizontal;
use iced::widget::{
    button, canvas, column, container, image, mouse_area, opaque, row, scrollable, space, stack, text,
};
use iced::{Center, Color, ContentFit, Element, Fill};

const BG: Color = Color::from_rgb(0x11 as f32 / 255.0, 0x11 as f32 / 255.0, 0x11 as f32 / 255.0);

pub fn view(app: &App) -> Element<'_, Message> {
    let s = &app.state;
    let current = s.current();
    let content: Element<_> = match (&s.folder, current) {
        (None, _) => text("Open a folder with photos (Ctrl+O)").size(20).into(),
        (Some(_), _) if s.folder_error.is_some() => {
            text(s.folder_error.clone().unwrap_or_default()).size(20).into()
        }
        (Some(f), _) if f.is_empty() => text(format!("No images in {}", tilde(&f.path))).size(20).into(),
        (Some(_), _) if s.view == View::Grid => grid(s),
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
        _ if s.view == View::Grid => "grid-area",
        Display::Sharp => "photo",
        Display::Preview | Display::Previous => "preview",
        Display::None => "message",
    };
    // The overlay is always present: it also handles the keyboard shortcuts.
    let grid = s.view == View::Grid;
    let overlay = canvas(Overlay {
        modes: Modes {
            menu: s.menu,
            help: s.help,
            about: s.about,
            crop: s.mode == Mode::Crop,
            fullscreen: s.fullscreen,
            grid,
        },
        selection: s.crop.selection,
        image: s.picture().map(|p| p.source_size()),
        body: s.body,
    })
    .width(Fill)
    .height(Fill);
    let bg = if s.fullscreen { Color::BLACK } else { BG };
    let content = container(content).center(Fill).id(id);
    // In the grid the overlay goes underneath: it only needs the keys, the cells the mouse.
    let layers = if grid { stack![overlay, content] } else { stack![content, overlay] };
    let body = container(layers)
        .id("body")
        .style(move |_| container::Style::default().background(bg).color(Color::WHITE));
    if s.fullscreen {
        return body.into();
    }

    let footer = row![
        text(s.status()).size(13).width(Fill),
        container(
            button(hamburger())
                .padding([7, 8])
                .style(theme::flat(s.menu))
                .on_press(Action::ToggleMenu.into())
        )
        .id("menu-button"),
    ]
    .align_y(Center);
    let main = column![
        body,
        container(footer).padding(iced::Padding::from([2, 4]).left(12)).width(Fill).style(theme::bar)
    ];
    if s.help {
        let rows = SHORTCUTS.iter().map(|(keys, what)| {
            row![text(*keys).width(150).style(theme::dim), text(*what)].spacing(16).into()
        });
        let panel =
            container(column![text("Keyboard shortcuts").size(18), column(rows).spacing(8)].spacing(16))
                .padding(24)
                .style(theme::menu)
                .id("help");
        let layer = container(opaque(panel)).center(Fill);
        return stack![main, mouse_area(layer).on_press(Action::ToggleHelp.into())].into();
    }
    if s.about {
        let panel = container(
            column![
                image(app.icon.clone()).width(96).height(96),
                text("Photo Cruiser").size(22),
                text(format!("Version {}", env!("CARGO_PKG_VERSION"))).style(theme::dim),
                text("Fast photo browser and editor"),
            ]
            .spacing(8)
            .align_x(Center),
        )
        .padding([24, 40])
        .style(theme::menu)
        .id("about");
        let layer = container(opaque(panel)).center(Fill);
        return stack![main, mouse_area(layer).on_press(Action::ToggleAbout.into())].into();
    }
    if !s.menu {
        return main.into();
    }

    let item = |i: usize, item: menu::Item| -> Element<'_, Message> {
        let content = row![text(item.label(s)).width(Fill), text(item.keys(s)).size(12).style(theme::dim)]
            .spacing(24)
            .align_y(Center);
        let focused = s.menu_focus == Some(i);
        let button = button(content)
            .padding([6, 12])
            .width(Fill)
            .style(theme::flat(focused))
            .on_press(item.message(s));
        let item = container(mouse_area(button).on_enter(Action::MenuFocus { index: i }.into()));
        if focused { item.id("menu-focus").into() } else { item.into() }
    };
    let menu = container(column(menu::ITEMS.into_iter().enumerate().map(|(i, it)| item(i, it))).width(220))
        .padding(4)
        .style(theme::menu)
        .id("menu");
    // A click anywhere outside the menu closes it (and goes no further).
    let layer = container(opaque(menu)).align_right(Fill).align_bottom(Fill).padding([42, 6]);
    stack![main, mouse_area(layer).on_press(Action::CloseMenu.into())].into()
}

/// Every photo of the folder as a thumbnail, in the rows of [`State::grid`], the current
/// one highlighted. A click opens the photo. Square cells frame the thumbnail with padding
/// (the selection shows there) and put the name under it; unpadded (justified) cells
/// fill with the photo and draw the selection and the name over it.
fn grid<'a>(s: &'a State) -> Element<'a, Message> {
    let Some(folder) = &s.folder else { return space().into() };
    let g = s.grid();
    let m = g.margins;
    let below = s.layout.label_below();
    let label = if s.names && below { LABEL } else { 0.0 };
    let cell = |i: usize, w: f32, h: f32| -> Element<'a, Message> {
        let (image_w, image_h) = (w - 2.0 * m.cell, h - 2.0 * m.cell - label);
        let fit = if below { ContentFit::Contain } else { ContentFit::Cover };
        let thumb: Element<_> = match s.thumbs.get(i) {
            Some(Ok(p)) => image(p.handle.clone()).content_fit(fit).width(Fill).height(Fill).into(),
            Some(Err(_)) => text("Cannot decode").size(12).style(theme::dim).into(),
            None => space().into(),
        };
        let selected = i == s.index;
        let name = || text(folder.entries[i].name.as_str()).size(12).wrapping(text::Wrapping::None);
        let thumb = container(thumb).center_x(image_w).center_y(image_h);
        let content: Element<_> = if below {
            let mut content = column![thumb];
            if s.names {
                content = content
                    .push(container(name()).center_x(image_w).height(LABEL).align_bottom(LABEL).clip(true));
            }
            content.into()
        } else {
            let mut layers = stack![thumb];
            if s.names {
                let strip = container(name()).padding([2, 6]).width(Fill).clip(true).style(theme::scrim);
                layers = layers.push(container(strip).height(Fill).align_bottom(Fill));
            }
            if selected {
                layers = layers.push(container(space()).width(Fill).height(Fill).style(theme::frame));
            }
            layers.into()
        };
        let cell = button(content)
            .padding(m.cell)
            .width(w)
            .height(h)
            .style(theme::cell(selected, below))
            .on_press(Action::ShowPhoto { index: Some(i) }.into());
        let cell = container(cell);
        if selected { cell.id("selected").into() } else { cell.into() }
    };
    let rows = g
        .rows
        .into_iter()
        .map(|r| row(r.items.zip(r.widths).map(|(i, w)| cell(i, w, r.h))).spacing(m.gap).into());
    // Square cells are centred; justified rows start on the left (the scrollbar's on the right).
    let side = if below { Horizontal::Center } else { Horizontal::Left };
    scrollable(container(column(rows).spacing(m.gap)).padding(m.pad).width(Fill).align_x(side))
        .id("grid")
        .on_scroll(|v| Message::GridScrolled(v.absolute_offset().y))
        .width(Fill)
        .height(Fill)
        .into()
}

/// Three bars: the menu icon (drawn, so it doesn't depend on the font's glyphs).
fn hamburger<'a>() -> Element<'a, Message> {
    let bar = || container(space()).width(14).height(2).style(theme::icon);
    column![bar(), bar(), bar()].spacing(3).into()
}
