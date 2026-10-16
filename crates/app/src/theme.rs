//! COSMIC-like look for plain iced: colours, corner radii and font are read once at
//! startup from the user's COSMIC config (`~/.config/cosmic/...`, RON files) and turned
//! into an `iced::Theme` plus a few style functions. Falls back to built-in dark values
//! when the files are missing (other desktops / OSes). No live reload.

use iced::widget::{button as btn, container, text};
use iced::{Background, Border, Color, Font, Shadow, Theme, Vector, theme::Palette};
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Deserialize, Clone, Copy)]
struct Rgba {
    red: f32,
    green: f32,
    blue: f32,
    alpha: f32,
}

/// A COSMIC "component" (accent, button, ...): only the fields used here.
#[derive(Deserialize, Clone, Copy)]
struct Component {
    base: Rgba,
    hover: Rgba,
    pressed: Rgba,
}

/// A COSMIC "container" (background, primary, ...).
#[derive(Deserialize, Clone, Copy)]
struct Layer {
    base: Rgba,
    on: Rgba,
}

#[derive(Deserialize)]
struct Radii {
    radius_xl: [f32; 4],
}

#[derive(Deserialize)]
struct FontCfg {
    family: String,
}

pub struct Cosmic {
    background: Layer,
    accent: Component,
    button: Component,
    radius_xl: f32,
    pub font: Font,
}

const fn c(r: f32, g: f32, b: f32, a: f32) -> Rgba {
    Rgba { red: r, green: g, blue: b, alpha: a }
}

/// COSMIC's default dark theme (approximately), used when no config is found.
const FALLBACK: Cosmic = Cosmic {
    background: Layer { base: c(0.106, 0.106, 0.106, 1.0), on: c(0.908, 0.908, 0.908, 1.0) },
    accent: Component {
        base: c(0.388, 0.816, 0.875, 1.0),
        hover: c(0.35, 0.73, 0.78, 1.0),
        pressed: c(0.22, 0.46, 0.49, 1.0),
    },
    button: Component {
        base: c(0.62, 0.62, 0.62, 0.25),
        hover: c(0.388, 0.388, 0.388, 0.4),
        pressed: c(0.167, 0.167, 0.167, 0.625),
    },
    radius_xl: 160.0,
    font: Font::DEFAULT,
};

fn color(c: Rgba) -> Color {
    Color::from_rgba(c.red, c.green, c.blue, c.alpha)
}

/// Read `~/.config/cosmic/<id>/v1/<key>` as RON.
fn read<T: for<'de> Deserialize<'de>>(id: &str, key: &str) -> Option<T> {
    let base = crate::settings::config_dir()?;
    let text = std::fs::read_to_string(base.join("cosmic").join(id).join("v1").join(key)).ok()?;
    ron::from_str(&text).ok()
}

fn load() -> Cosmic {
    let dark = read::<bool>("com.system76.CosmicTheme.Mode", "is_dark").unwrap_or(true);
    let id = if dark { "com.system76.CosmicTheme.Dark" } else { "com.system76.CosmicTheme.Light" };
    let font = read::<FontCfg>("com.system76.CosmicTk", "interface_font")
        // iced wants a `&'static str`; leaked once at startup.
        .map_or(Font::DEFAULT, |f| Font::with_name(Box::leak(f.family.into_boxed_str())));
    Cosmic {
        background: read(id, "background").unwrap_or(FALLBACK.background),
        accent: read(id, "accent").unwrap_or(FALLBACK.accent),
        button: read(id, "button").unwrap_or(FALLBACK.button),
        radius_xl: read::<Radii>(id, "corner_radii").map_or(FALLBACK.radius_xl, |r| r.radius_xl[0]),
        font,
    }
}

pub fn get() -> &'static Cosmic {
    static COSMIC: OnceLock<Cosmic> = OnceLock::new();
    COSMIC.get_or_init(load)
}

pub fn theme() -> Theme {
    let t = get();
    let palette = Palette {
        background: color(t.background.base),
        text: color(t.background.on),
        primary: color(t.accent.base),
        ..Palette::DARK
    };
    Theme::custom("COSMIC", palette)
}

/// Header / status bars: the COSMIC window background.
pub fn bar(_: &Theme) -> container::Style {
    let t = get();
    container::Style {
        background: Some(color(t.background.base).into()),
        text_color: Some(color(t.background.on)),
        ..container::Style::default()
    }
}

/// Borderless button (menu button and items): background only on hover/press, or while
/// `active` (its menu is open).
pub fn flat(active: bool) -> impl Fn(&Theme, btn::Status) -> btn::Style {
    move |_, status| {
        let t = get();
        let bg = match status {
            btn::Status::Pressed => Some(t.button.pressed),
            btn::Status::Hovered => Some(t.button.hover),
            _ if active => Some(t.button.base),
            _ => None,
        };
        btn::Style {
            background: bg.map(|c| Background::Color(color(c))),
            text_color: match status {
                btn::Status::Disabled => Color { a: 0.4, ..color(t.background.on) },
                _ => color(t.background.on),
            },
            border: Border::default().rounded(8),
            ..btn::Style::default()
        }
    }
}

/// The footer's press-and-hold Original label: a button, pressed while `held`.
pub fn held(held: bool) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let t = get();
        container::Style {
            background: Some(color(if held { t.button.pressed } else { t.button.base }).into()),
            text_color: Some(color(t.background.on)),
            border: Border::default().rounded(8),
            ..container::Style::default()
        }
    }
}

/// A grid cell: an accent border when `selected` (the current photo), a tint on hover.
/// Unpadded (justified) cells are plain: the photo covers them.
pub fn cell(selected: bool, padded: bool) -> impl Fn(&Theme, btn::Status) -> btn::Style {
    move |_, status| {
        let t = get();
        if !padded {
            // Unpadded: the photo is all there is (selection: [`frame`]).
            return btn::Style { text_color: color(t.background.on), ..btn::Style::default() };
        }
        let bg = match status {
            btn::Status::Pressed => Some(t.button.pressed),
            btn::Status::Hovered => Some(t.button.hover),
            _ if selected => Some(t.button.base),
            _ => None,
        };
        let border = if selected { color(t.accent.base) } else { Color::TRANSPARENT };
        btn::Style {
            background: bg.map(|c| Background::Color(color(c))),
            text_color: color(t.background.on),
            border: Border { color: border, width: 2.0, radius: 8.into() },
            ..btn::Style::default()
        }
    }
}

/// The selection, drawn over an unpadded thumbnail.
pub fn frame(_: &Theme) -> container::Style {
    let border = Border { color: color(get().accent.base), width: 3.0, radius: 0.into() };
    container::Style { border, ..container::Style::default() }
}

/// The dot on a grid cell whose photo couldn't be saved.
pub fn failed(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgb8(0xE0, 0x40, 0x40).into()),
        border: Border { color: Color::WHITE, width: 1.5, radius: 6.into() },
        ..container::Style::default()
    }
}

/// Text over a photo: white on a dark translucent strip.
pub fn scrim(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.55).into()),
        text_color: Some(Color::WHITE),
        ..container::Style::default()
    }
}

/// The menu panel: a slightly lighter card over the image.
pub fn menu(_: &Theme) -> container::Style {
    let t = get();
    let (bg, on) = (color(t.background.base), color(t.background.on));
    container::Style {
        background: Some(mix(bg, on, 0.06).into()),
        text_color: Some(on),
        border: Border { color: mix(bg, on, 0.15), width: 1.0, radius: 10.into() },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.4),
            offset: Vector::new(0.0, 4.0),
            blur_radius: 16.0,
        },
        ..container::Style::default()
    }
}

/// Bars of the menu icon.
pub fn icon(_: &Theme) -> container::Style {
    let t = get();
    container::Style {
        background: Some(color(t.background.on).into()),
        border: Border::default().rounded(1),
        ..container::Style::default()
    }
}

/// Secondary text (keyboard shortcuts).
pub fn dim(_: &Theme) -> text::Style {
    let t = get();
    text::Style { color: Some(mix(color(t.background.base), color(t.background.on), 0.55)) }
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::from_rgb(a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t)
}
