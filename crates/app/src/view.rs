//! Layout: image area (the grid of thumbnails, or one image with the crop overlay
//! stacked over it), then a footer with the status line and the menu button. The menu
//! opens upwards from the button, over the image. Full screen shows only the image area.
//! Widget ids used by tests: `photo` (the current photo, sharp), `preview` (its embedded
//! preview, or still the previous photo while it decodes), `message` (error / empty),
//! `grid` (the thumbnails' scrollable), `selected` (the current photo's grid cell),
//! `body` (the image area), `menu-button`, `menu`, `menu-focus` (its focused item),
//! `help` (shortcuts panel), `about`, `aspects` (the crop's aspect buttons in the footer),
//! `aspect-panel` and `aspect-input` (the custom aspect field), `apply` (the crop's
//! Apply button), `failed` (the dot on a grid cell whose save failed), `palette`,
//! `palette-input` and `palette-focus` (the command palette, its field, its focused row), `tool`,
//! `tool-slider`, `tool-auto` and `tool-neutral` (the open tool's panel in the footer, its
//! slider, its Auto and No change buttons).

use crate::action::Action;
use crate::adjust_panel::Slider;
use crate::app::App;
use crate::crop_overlay::{Modes, Overlay, SHORTCUTS};
use crate::grid::LABEL;
use crate::menu;
use crate::message::Message;
use crate::palette;
use crate::state::{Display, Mode, State, ToolPanel, View, tilde};
use crate::theme;
use iced::alignment::Horizontal;
use iced::widget::{
    button, canvas, column, container, image, mouse_area, opaque, row, scrollable, space, stack, text,
    text_input,
};
use iced::{Center, Color, ContentFit, Element, Fill};
use photo_cruiser_core::crop::Aspect;

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
            // The open tool's live preview.
            Some(_) if s.preview.as_ref().is_some_and(|p| p.index == s.index) => {
                let handle = s.preview.as_ref().map(|p| p.handle.clone()).expect("checked");
                image(handle).content_fit(ContentFit::Contain).width(Fill).height(Fill).into()
            }
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
            input: s.aspect_input.is_some(),
            palette: s.palette.is_some(),
            crop: s.mode == Mode::Crop,
            tool: s.tool.is_some(),
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

    let mut footer = row![text(s.status()).size(13).width(Fill)].align_y(Center);
    if s.mode == Mode::Crop && !grid {
        footer = footer.push(aspects(s));
        let apply = button(text("Apply").size(13))
            .padding([3, 10])
            .style(theme::flat(true))
            .on_press_maybe(s.crop.selection.map(|_| Action::ApplyCrop.into()));
        footer = footer.push(container(apply).padding([0, 8]).id("apply"));
    }
    if let (Some(p), false) = (&s.tool, grid) {
        footer = footer.push(tool_panel(s, p));
    }
    let footer = footer.push(
        container(
            button(hamburger())
                .padding([7, 8])
                .style(theme::flat(s.menu))
                .on_press(Action::ToggleMenu.into()),
        )
        .id("menu-button"),
    );
    let main = column![
        body,
        container(footer).padding(iced::Padding::from([2, 4]).left(12)).width(Fill).style(theme::bar)
    ];
    if let Some(p) = &s.palette {
        return stack![main, command_palette(s, p)].into();
    }
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
    if let Some(input) = &s.aspect_input {
        let parsed = input.parse::<Aspect>().ok().filter(|a| *a != Aspect::Original);
        let field = text_input("e.g. 7:5 or 2.35", input)
            .id("aspect-input")
            .on_input(Message::AspectInput)
            .on_submit_maybe(parsed.map(|aspect| Action::SetAspect { aspect }.into()))
            .width(180);
        let hint = match parsed {
            _ if input.trim().is_empty() => "Width:height, Enter to use it",
            Some(_) => "Enter to use it",
            None => "Not a ratio: try 7:5 or 2.35",
        };
        let panel = container(
            column![text("Custom aspect ratio").size(18), field, text(hint).size(12).style(theme::dim)]
                .spacing(12),
        )
        .padding(24)
        .style(theme::menu)
        .id("aspect-panel");
        let layer = container(opaque(panel)).center(Fill);
        return stack![main, mouse_area(layer).on_press(Action::ToggleCustomAspect.into())].into();
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

/// The palette's layer: the field and the rows matching it, near the top of the window;
/// a click outside closes it. The [`palette::Keys`] layer on top takes its keys.
fn command_palette<'a>(s: &'a State, p: &'a palette::Palette) -> Element<'a, Message> {
    let field = text_input("Type an action", &p.query)
        .id("palette-input")
        .on_input(|query| Action::PaletteQuery { query }.into())
        .padding([6, 10]);
    let items = palette::filtered(s);
    let total = items.len();
    let rows = items.into_iter().enumerate().skip(p.top).take(palette::VISIBLE).map(|(i, c)| {
        let content = row![text(c.label).width(Fill), text(c.keys).size(12).style(theme::dim)]
            .spacing(24)
            .align_y(Center);
        let focused = i == p.focus;
        let button =
            button(content).padding([6, 12]).width(Fill).style(theme::flat(focused)).on_press(c.message);
        let row = container(mouse_area(button).on_enter(Action::PaletteFocus { index: i }.into()));
        if focused { row.id("palette-focus").into() } else { row.into() }
    });
    let mut list = column(rows);
    if total == 0 {
        list = list.push(container(text("No matching action").style(theme::dim)).padding([6, 12]));
    }
    let hidden = total.saturating_sub(p.top + palette::VISIBLE);
    if hidden > 0 {
        list =
            list.push(container(text(format!("{hidden} more")).size(12).style(theme::dim)).padding([4, 12]));
    }
    let panel =
        container(column![field, list].spacing(4).width(440)).padding(6).style(theme::menu).id("palette");
    let layer = container(opaque(panel)).align_x(Center).width(Fill).height(Fill).padding([80, 0]);
    let keys = canvas(palette::Keys).width(Fill).height(Fill);
    stack![mouse_area(layer).on_press(Action::ClosePalette.into()), keys].into()
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
        let failed = s.save_failed.contains_key(&folder.entries[i].path);
        let content: Element<_> = match failed {
            true => {
                let dot = container(space()).width(12).height(12).style(theme::failed).id("failed");
                stack![content, container(dot).padding(6).width(Fill).align_right(Fill)].into()
            }
            false => content,
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

/// The crop's aspect choices: the presets, a custom ratio, and portrait / landscape
/// (for a fixed, non-square ratio).
fn aspects(s: &State) -> Element<'_, Message> {
    let current = s.crop.aspect;
    let choice = |label: String, active: bool, message: Message| -> Element<'_, Message> {
        button(text(label).size(13)).padding([3, 8]).style(theme::flat(active)).on_press(message).into()
    };
    let mut buttons: Vec<_> = Aspect::PRESETS
        .into_iter()
        .map(|a| {
            let label = match a {
                Aspect::Original => "Original".into(),
                Aspect::Free => "Free".into(),
                _ => a.to_string(),
            };
            choice(label, a == current, Action::SetAspect { aspect: a }.into())
        })
        .collect();
    let custom = if current.is_preset() { "Custom…".into() } else { format!("{current}…") };
    buttons.push(choice(custom, !current.is_preset(), Action::ToggleCustomAspect.into()));
    if let Some(r) = s.crop_ratio().filter(|r| (r - 1.0).abs() > 1e-3) {
        let label = if r > 1.0 { "Landscape" } else { "Portrait" };
        buttons.push(text("·").style(theme::dim).into());
        buttons.push(choice(label.into(), false, Action::SwapAspect.into()));
    }
    container(row(buttons).spacing(2).align_y(Center)).padding([0, 8]).id("aspects").into()
}

/// The open tool: its name, slider, value and a Done button.
fn tool_panel<'a>(s: &'a State, p: &'a ToolPanel) -> Element<'a, Message> {
    let value = p.value(&s.stack());
    let slider: Element<_> = match p.opening {
        Some(opening) => {
            let slider = canvas(Slider { tool: p.tool, value, opening: Some(opening) }).width(260).height(24);
            container(slider).id("tool-slider").into()
        }
        None => text("loading…").size(13).style(theme::dim).width(260).into(),
    };
    let done = button(text("Done").size(13))
        .padding([3, 10])
        .style(theme::flat(true))
        .on_press(Action::CloseTool.into());
    // The two resets, each greyed out while the slider is already there.
    let reset = |label: &'static str, to: Option<f32>, action: Action, id: &'static str| {
        let b = button(text(label).size(13))
            .padding([3, 8])
            .style(theme::flat(false))
            .on_press_maybe(to.filter(|v| *v != value).map(|_| action.into()));
        container(b).id(id)
    };
    let auto = reset("Auto", p.opening, Action::AutoAdjust, "tool-auto");
    let neutral =
        reset("No change", p.opening.map(|_| p.tool.neutral()), Action::NeutralAdjust, "tool-neutral");
    let content = row![
        text(p.tool.label()).size(13),
        slider,
        text(p.tool.format(value)).size(13).width(64),
        row![auto, neutral].spacing(2),
        done,
    ]
    .spacing(10)
    .align_y(Center);
    container(content).padding([0, 8]).id("tool").into()
}

/// Three bars: the menu icon (drawn, so it doesn't depend on the font's glyphs).
fn hamburger<'a>() -> Element<'a, Message> {
    let bar = || container(space()).width(14).height(2).style(theme::icon);
    column![bar(), bar(), bar()].spacing(3).into()
}
