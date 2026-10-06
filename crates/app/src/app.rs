//! The iced program: boot, update, title. Behaviour lives in [`State`]; this glues it to
//! the runtime (decode threads, dialogs, script mode).

use crate::action::Action;
use crate::message::Message;
use crate::script::{self, Runner, ScriptConfig};
use crate::state::{DEFAULT_MAX_SIDE, Effect, Loaded, State, decode_picture};
use crate::{theme, view};
use iced::{Task, window};
use std::path::PathBuf;

pub const APP_ID: &str = "com.lukastrumm.PhotoCruiser";
pub const WINDOW_SIZE: iced::Size = iced::Size::new(1100.0, 850.0);

#[derive(Clone, Debug)]
pub struct Config {
    pub folder: Option<PathBuf>,
    /// Longest side of the displayed image (GPU texture).
    pub max_side: u32,
    pub script: Option<ScriptConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Config { folder: None, max_side: DEFAULT_MAX_SIDE, script: None }
    }
}

/// The iced application (also driven headlessly by the tests).
pub fn application(cfg: Config) -> iced::Application<impl iced::Program<State = App, Message = Message>> {
    iced::application(move || App::new(cfg.clone()), App::update, view::view)
        .title(App::title)
        .theme(|_: &App| theme::theme())
        .default_font(theme::get().font)
        .window(window_settings())
        // MSAA (iced's default) makes the canvas' 50% black dim ~67% in the real wgpu
        // window (white → 85 instead of 128); iced_test's headless renderer has no MSAA,
        // so only script-mode shots show it. The overlay is axis-aligned: no AA needed.
        .antialiasing(false)
}

fn window_settings() -> window::Settings {
    window::Settings {
        size: WINDOW_SIZE,
        #[cfg(target_os = "linux")]
        platform_specific: window::settings::PlatformSpecific {
            application_id: APP_ID.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

pub struct App {
    pub state: State,
    pub script: Option<Runner>,
}

impl App {
    pub fn new(cfg: Config) -> (Self, Task<Message>) {
        let mut app = App { state: State::new(cfg.max_side), script: cfg.script.map(Runner::new) };
        if let Some(folder) = cfg.folder {
            app.state.apply(&Action::Open { path: folder });
        }
        let start = if app.script.is_some() { script::start() } else { Task::none() };
        let load = app.load_next();
        (app, Task::batch([load, start]))
    }

    pub fn title(&self) -> String {
        self.state.title()
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let task = match message {
            Message::Action(action) => match self.state.apply(&action) {
                Effect::Quit => return iced::exit(),
                Effect::None => Task::none(),
            },
            Message::OpenDialog => {
                let dialog = rfd::AsyncFileDialog::new().set_title("Open photo folder").pick_folder();
                return Task::perform(dialog, |h| Message::FolderPicked(h.map(|h| h.path().into())));
            }
            Message::FolderPicked(Some(path)) => {
                self.state.apply(&Action::Open { path });
                Task::none()
            }
            Message::FolderPicked(None) => Task::none(),
            Message::Decoded(job, result) => {
                self.state.finish(job, result);
                Task::none()
            }
            Message::Crop(kind, p, tol) => {
                self.state.crop_mouse(kind, p, tol);
                Task::none()
            }
            Message::Script(event) => script::update(self, event),
        };
        Task::batch([task, self.load_next()])
    }

    /// Start the next decode (current first, then ±1), if none is in flight.
    fn load_next(&mut self) -> Task<Message> {
        let Some((job, path)) = self.state.next_job() else { return Task::none() };
        let max_side = self.state.max_side;
        Task::perform(decode_async(path, max_side), move |r| Message::Decoded(job, r))
    }
}

/// Decode on a fresh OS thread so neither the UI nor the executor block on it.
async fn decode_async(path: PathBuf, max_side: u32) -> Loaded {
    let (tx, rx) = iced::futures::channel::oneshot::channel();
    std::thread::spawn(move || tx.send(decode_picture(&path, max_side)));
    rx.await.unwrap_or_else(|_| Err("decoder thread panicked".into()))
}
