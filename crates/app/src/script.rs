//! Script mode: drive the real window with a list of [`Action`]s, then write evidence.
//!
//! `photo-cruiser <dir> [--script steps.json] [--shot out.png] [--dump-state state.json]`
//!
//! 1. The window opens normally (boot opens the folder and starts decoding).
//! 2. Each step is applied once the loader is idle (current ± 1 decoded).
//!    A `quit` step ends the script early (evidence is still written).
//! 3. Once all steps ran and the loader is idle, the current image is explicitly
//!    allocated on the GPU (`image::allocate`; big images are otherwise uploaded
//!    asynchronously and may be missing from a frame), then a short settle delay lets
//!    the window redraw.
//! 4. `window::screenshot` → PNG (`--shot`), then the state JSON (`--dump-state`).
//! 5. Exit 0. Exit 1 on failure to write the PNG/JSON or to find the window; the
//!    caller (`main`) also exits 2 on usage / script parse errors and 3 on timeout.
//!    Rejected actions and decode errors of the current image do not fail the run:
//!    they are listed in the state's `errors`.

use crate::action::Action;
use crate::app::App;
use crate::message::Message;
use crate::state::WindowSize;
use iced::{Task, window};
use iced_runtime::image::{self, Allocation};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Duration;

/// Process exit code decided by script mode; read by `main` after the event loop ends.
pub static EXIT_CODE: AtomicI32 = AtomicI32::new(0);

/// Wait between "allocated on the GPU" and the screenshot, for the redraw.
const SETTLE: Duration = Duration::from_millis(300);
/// Poll interval while waiting for decodes.
const POLL: Duration = Duration::from_millis(10);

#[derive(Clone, Debug, Default)]
pub struct ScriptConfig {
    pub steps: Vec<Action>,
    pub shot: Option<PathBuf>,
    pub dump: Option<PathBuf>,
}

#[derive(Debug)]
pub struct Runner {
    cfg: ScriptConfig,
    next: usize,
    /// Keeps the current image resident on the GPU until the screenshot is taken.
    allocation: Option<Allocation>,
    done: bool,
}

impl Runner {
    pub fn new(cfg: ScriptConfig) -> Self {
        Runner { cfg, next: 0, allocation: None, done: false }
    }
}

#[derive(Clone, Debug)]
pub enum Event {
    Tick,
    Allocated(Option<Result<Allocation, image::Error>>),
    Capture,
    Window(Option<window::Id>),
    Shot(window::Screenshot),
    Size(iced::Size),
}

pub fn start() -> Task<Message> {
    after(Duration::from_millis(50), Event::Tick)
}

/// Emit `event` after `delay` (timer on an OS thread; no async runtime needed).
fn after(delay: Duration, event: Event) -> Task<Message> {
    let (tx, rx) = iced::futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        std::thread::sleep(delay);
        let _ = tx.send(());
    });
    Task::perform(rx, move |_| Message::Script(event.clone()))
}

pub fn update(app: &mut App, event: Event) -> Task<Message> {
    let Some(runner) = app.script.as_mut() else { return Task::none() };
    if runner.done {
        return Task::none();
    }
    match event {
        Event::Tick if !app.state.is_idle() => after(POLL, Event::Tick),
        Event::Tick if runner.next < runner.cfg.steps.len() => {
            let step = runner.cfg.steps[runner.next].clone();
            runner.next += 1;
            eprintln!("script: step {}: {}", runner.next, serde_json::to_string(&step).unwrap_or_default());
            if step == Action::Quit {
                runner.next = runner.cfg.steps.len();
            } else {
                app.state.apply(&step);
            }
            // `App::update` batches the decode this may need; the tick waits for it.
            after(POLL, Event::Tick)
        }
        Event::Tick => match app.state.picture() {
            Some(p) => image::allocate(p.handle.clone()).map(|r| Message::Script(Event::Allocated(Some(r)))),
            None => Task::done(Message::Script(Event::Allocated(None))),
        },
        Event::Allocated(result) => {
            match result {
                Some(Ok(a)) => runner.allocation = Some(a),
                Some(Err(e)) => app.state.errors.push(format!("GPU upload failed: {e}")),
                None => {}
            }
            after(SETTLE, Event::Capture)
        }
        Event::Capture => window::latest().map(|id| Message::Script(Event::Window(id))),
        Event::Window(None) => after(Duration::from_millis(50), Event::Capture),
        Event::Window(Some(id)) if runner.cfg.shot.is_some() => {
            window::screenshot(id).map(|s| Message::Script(Event::Shot(s)))
        }
        Event::Window(Some(id)) => window::size(id).map(|s| Message::Script(Event::Size(s))),
        Event::Shot(shot) => {
            let window = WindowSize {
                width: shot.size.width as f32 / shot.scale_factor,
                height: shot.size.height as f32 / shot.scale_factor,
            };
            let path = runner.cfg.shot.clone().unwrap_or_default();
            let saved = ::image::RgbaImage::from_raw(shot.size.width, shot.size.height, shot.rgba.to_vec())
                .ok_or_else(|| "screenshot buffer has the wrong size".to_string())
                .and_then(|img| img.save(&path).map_err(|e| e.to_string()));
            match saved {
                Ok(()) => {
                    eprintln!(
                        "script: screenshot {}×{} → {}",
                        shot.size.width,
                        shot.size.height,
                        path.display()
                    );
                    finish(app, Some(window))
                }
                Err(e) => fail(app, format!("cannot write screenshot {}: {e}", path.display())),
            }
        }
        Event::Size(size) => finish(app, Some(WindowSize { width: size.width, height: size.height })),
    }
}

fn finish(app: &mut App, window: Option<WindowSize>) -> Task<Message> {
    let Some(runner) = app.script.as_mut() else { return iced::exit() };
    runner.done = true;
    runner.allocation = None;
    let cfg = runner.cfg.clone();
    if let Some(path) = &cfg.dump {
        let dump = app.state.dump(window, cfg.shot.as_deref());
        let json = serde_json::to_string_pretty(&dump).expect("state serializes");
        if let Err(e) = std::fs::write(path, json + "\n") {
            return fail(app, format!("cannot write state {}: {e}", path.display()));
        }
    }
    iced::exit()
}

fn fail(app: &mut App, msg: String) -> Task<Message> {
    eprintln!("photo-cruiser: {msg}");
    EXIT_CODE.store(1, Ordering::SeqCst);
    if let Some(r) = app.script.as_mut() {
        r.done = true;
    }
    iced::exit()
}
