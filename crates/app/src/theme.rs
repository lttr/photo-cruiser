//! COSMIC-like look for plain iced: colours, corner radii and font are read once at
//! startup from the user's COSMIC config (`~/.config/cosmic/...`, RON files) and turned
//! into an `iced::Theme` plus a few style functions. Falls back to built-in dark values
//! when the files are missing (other desktops / OSes). No live reload.

use iced::widget::{button as btn, container};
use iced::{Background, Border, Color, Font, Theme, theme::Palette};
use serde::Deserialize;
use std::path::PathBuf;
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
    on: Rgba,
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
        on: c(0.0, 0.0, 0.0, 1.0),
    },
    button: Component {
        base: c(0.62, 0.62, 0.62, 0.25),
        hover: c(0.388, 0.388, 0.388, 0.4),
        pressed: c(0.167, 0.167, 0.167, 0.625),
        on: c(0.753, 0.753, 0.753, 1.0),
    },
    radius_xl: 160.0,
    font: Font::DEFAULT,
};

fn color(c: Rgba) -> Color {
    Color::from_rgba(c.red, c.green, c.blue, c.alpha)
}

/// Read `~/.config/cosmic/<id>/v1/<key>` as RON.
fn read<T: for<'de> Deserialize<'de>>(id: &str, key: &str) -> Option<T> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
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

/// Pill-shaped COSMIC button: `suggested` uses the accent colour, otherwise the
/// translucent "standard" button colours.
pub fn button(suggested: bool) -> impl Fn(&Theme, btn::Status) -> btn::Style {
    move |_, status| {
        let t = get();
        let c = if suggested { t.accent } else { t.button };
        let bg = match status {
            btn::Status::Hovered => c.hover,
            btn::Status::Pressed => c.pressed,
            _ => c.base,
        };
        btn::Style {
            background: Some(Background::Color(color(bg))),
            text_color: color(c.on),
            border: Border::default().rounded(t.radius_xl),
            ..btn::Style::default()
        }
    }
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
