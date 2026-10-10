//! The iced program: boot, update, title. Behaviour lives in [`State`]; this glues it to
//! the runtime (decode threads, dialogs, script mode).

use crate::action::Action;
use crate::message::Message;
use crate::script::{self, Runner, ScriptConfig};
use crate::state::{DEFAULT_MAX_SIDE, DEFAULT_TRANSITION, Effect, Loaded, State, decode_job, thumb_job};
use crate::{theme, view};
use iced::widget::operation::AbsoluteOffset;
use iced::{Subscription, Task, window};
use std::path::PathBuf;
use std::time::Duration;

pub const APP_ID: &str = "com.lukastrumm.PhotoCruiser";
pub const WINDOW_SIZE: iced::Size = iced::Size::new(1100.0, 850.0);

#[derive(Clone, Debug)]
pub struct Config {
    pub folder: Option<PathBuf>,
    /// Longest side of the displayed image (GPU texture).
    pub max_side: u32,
    /// Cross-fade between images; zero disables it.
    pub transition: Duration,
    pub script: Option<ScriptConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Config { folder: None, max_side: DEFAULT_MAX_SIDE, transition: DEFAULT_TRANSITION, script: None }
    }
}

/// The iced application (also driven headlessly by the tests).
pub fn application(cfg: Config) -> iced::Application<impl iced::Program<State = App, Message = Message>> {
    iced::application(move || App::new(cfg.clone()), App::update, view::view)
        .title(App::title)
        .subscription(App::subscription)
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
    /// `PC_RESUME_FILE` (set by `just dev`): the current photo's path is written here, so a
    /// restarted app can reopen it.
    resume: Option<PathBuf>,
    resumed: Option<PathBuf>,
}

impl App {
    pub fn new(cfg: Config) -> (Self, Task<Message>) {
        let mut app = App {
            state: State::new(cfg.max_side, cfg.transition),
            script: cfg.script.map(Runner::new),
            resume: std::env::var_os("PC_RESUME_FILE").map(PathBuf::from),
            resumed: None,
        };
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

    /// Redraw every frame while a fade runs; watch resizes for full-screen changes.
    pub fn subscription(&self) -> Subscription<Message> {
        let frames = match self.state.fade_progress() {
            Some(_) => window::frames().map(|_| Message::Frame),
            None => Subscription::none(),
        };
        Subscription::batch([frames, window::resize_events().map(|(id, _)| Message::Resized(id))])
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let task = match message {
            Message::Action(action) => self.apply(&action),
            Message::OpenDialog => {
                (self.state.menu, self.state.help) = (false, false);
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
            Message::Thumb(job, result) => {
                self.state.finish_thumb(job, result);
                Task::none()
            }
            Message::Body(size) => {
                self.state.body = Some(size);
                Task::none()
            }
            Message::GridScrolled(y) => {
                self.state.grid_offset = y;
                Task::none()
            }
            Message::Frame => Task::none(),
            // The window manager may change the mode itself; every resize re-reads it.
            Message::Resized(id) => {
                return window::mode(id).map(|m| Message::WindowMode(m == window::Mode::Fullscreen));
            }
            Message::WindowMode(on) => {
                self.state.fullscreen = on;
                Task::none()
            }
            Message::Crop(kind, p, tol) => {
                self.state.crop_mouse(kind, p, tol);
                Task::none()
            }
            Message::Script(event) => script::update(self, event),
        };
        self.write_resume();
        let scroll = match self.state.scroll.take() {
            Some(y) => iced::widget::operation::scroll_to("grid", AbsoluteOffset { x: None, y: Some(y) }),
            None => Task::none(),
        };
        Task::batch([task, scroll, self.load_next()])
    }

    fn write_resume(&mut self) {
        let (Some(file), Some(current)) = (&self.resume, self.state.current_path()) else { return };
        if self.resumed.as_deref() != Some(current) {
            let _ = std::fs::write(file, current.as_os_str().as_encoded_bytes());
            self.resumed = Some(current.to_path_buf());
        }
    }

    /// Apply an action and carry out its [`Effect`] (decodes are scheduled by `update`).
    pub fn apply(&mut self, action: &Action) -> Task<Message> {
        match self.state.apply(action) {
            Effect::Quit => iced::exit(),
            Effect::Fullscreen(on) => set_fullscreen(on),
            Effect::None => Task::none(),
        }
    }

    /// Start decodes for every free loader slot (preview and current first, then neighbours),
    /// and thumbnails for every free thumbnail slot.
    fn load_next(&mut self) -> Task<Message> {
        let max_side = self.state.max_side;
        let mut tasks: Vec<_> = std::iter::from_fn(|| self.state.next_job())
            .map(|(job, path)| {
                let decode = move || decode_job(&path, job.kind, max_side);
                Task::perform(on_thread(decode), move |r| Message::Decoded(job, r))
            })
            .collect();
        tasks.extend(std::iter::from_fn(|| self.state.next_thumb_job()).map(|(job, path)| {
            Task::perform(on_thread(move || thumb_job(&path)), move |r| Message::Thumb(job, r))
        }));
        Task::batch(tasks)
    }
}

fn set_fullscreen(on: bool) -> Task<Message> {
    let mode = if on { window::Mode::Fullscreen } else { window::Mode::Windowed };
    window::latest().and_then(move |id| window::set_mode(id, mode))
}

/// Decode on a fresh OS thread so neither the UI nor the executor block on it.
async fn on_thread(decode: impl FnOnce() -> Loaded + Send + 'static) -> Loaded {
    let (tx, rx) = iced::futures::channel::oneshot::channel();
    std::thread::spawn(move || tx.send(decode()));
    rx.await.unwrap_or_else(|_| Err("decoder thread panicked".into()))
}
