//! Tests.
//!
//! - `update_*`: the [`State`] model driven by [`Action`]s directly, decodes run
//!   synchronously. No iced runtime.
//! - The rest: headless end-to-end tests with `iced_test`. The real program
//!   (boot/update/view/title/theme) runs in an `Emulator`, which executes Tasks on the
//!   app's executor, so decoding is real, on threads. Mouse input (menu clicks,
//!   drags) goes through `Emulator::run` with `Instruction`s: raw iced mouse events
//!   dispatched through the real widget tree. Keys: iced_test has no arrow keys in its
//!   instructions, so they are tapped on a `Simulator` built on the emulator's current
//!   view; the messages the canvas publishes are fed to `Emulator::update`.
//!   Not covered: winit (OS events → iced events) and the window surface — that is what
//!   script mode (`just shot`) is for.

use crate::action::Action;
use crate::app::{self, Config};
use crate::crop_overlay::Modes;
use crate::grid::Layout;
use crate::message::Message;
use crate::settings::Settings;
use crate::state::{Display, Effect, Mode, State, View, decode_job, io_job, size_job, thumb_job};
use iced::futures::channel::mpsc;
use iced::keyboard::{Key, key::Named};
use iced::mouse::Button;
use iced::{Point, Program, Rectangle, Size};
use iced_test::emulator::{self, Emulator};
use iced_test::instruction::{self, Interaction, Keyboard, Mouse, Target};
use iced_test::selector::{self, Candidate};
use iced_test::{Instruction, Simulator};
use photo_cruiser_core::geom::{PixelRect, fit_rect};
use photo_cruiser_core::loader::Kind;
use photo_cruiser_core::{Folder, Rect};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const SIZE: Size = Size::new(1000.0, 750.0);
/// The headless wgpu backend caps textures at 2048 px.
const TEST_MAX_SIDE: u32 = 2048;
const TIMEOUT: Duration = Duration::from_secs(20);

/// A copy of `fixtures/` for this test (each test runs on its own thread): leaving an
/// edited photo saves it, and nothing may ever be saved into `fixtures/`.
fn fixtures() -> PathBuf {
    thread_local! {
        static COPY: tempfile::TempDir = {
            let dir = tempfile::tempdir().unwrap();
            copy_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures"), dir.path());
            dir
        };
    }
    COPY.with(|d| d.path().canonicalize().unwrap())
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &target);
        } else {
            std::fs::copy(e.path(), target).unwrap();
        }
    }
}

fn names() -> Vec<String> {
    Folder::open(fixtures()).unwrap().entries.into_iter().map(|e| e.name).collect()
}

// ---------------------------------------------------------------------------------------
// Pure update tests

/// Run decodes (and grid thumbnails, size probes) synchronously until the loader is idle.
fn settle(s: &mut State) {
    while let Some((job, paths)) = s.next_size_job() {
        s.finish_sizes(&job, &size_job(&paths));
    }
    loop {
        while let Some(io) = s.next_io() {
            s.finish_io(io_job(io));
        }
        while let Some(load) = s.next_job() {
            let r = decode_job(&load, s.max_side);
            s.finish(load.job, r);
        }
        while let Some((job, path)) = s.next_thumb_job() {
            s.finish_thumb(job, thumb_job(&path, job.side));
        }
        if s.is_idle() {
            break;
        }
    }
}

/// No fades: they would make `is_idle` time-dependent. File names on (most checks read
/// them from the status line).
fn opened(path: &Path) -> State {
    let mut s = State::new(TEST_MAX_SIDE, Duration::ZERO);
    s.names = true;
    assert_eq!(s.apply(&Action::Open { path: path.into() }), Effect::None);
    settle(&mut s);
    s
}

#[test]
fn update_browse_and_goto_bounds() {
    let mut s = opened(&fixtures());
    assert_eq!((s.len(), s.index, s.position()), (9, 0, "1 / 9".into()));
    assert_eq!(s.status(), "img1.png · 1 / 9");
    assert_eq!(s.title(), format!("PhotoCruiser: {}", crate::state::tilde(&fixtures())));
    s.apply(&Action::Prev);
    assert_eq!(s.index, 0);
    s.apply(&Action::Goto { index: 8 });
    settle(&mut s);
    assert_eq!(s.current_name(), Some("img12-broken.jpg"));
    assert!(s.current().unwrap().as_ref().is_err_and(|e| e.contains("decode")), "{:?}", s.current());
    s.apply(&Action::Next); // clamps
    assert_eq!(s.index, 8);
    assert!(s.errors.is_empty());
    s.apply(&Action::Goto { index: 9 }); // out of range: rejected, logged
    assert_eq!(s.index, 8);
    assert_eq!(s.errors.len(), 1, "{:?}", s.errors);
    // The broken current image is reported in the dump, not as a crash.
    let dump = s.dump(None, None);
    assert_eq!(dump.errors.len(), 2);
    assert_eq!(s.apply(&Action::Quit), Effect::Quit);
}

#[test]
fn update_crop_aspect() {
    use photo_cruiser_core::crop::Aspect;
    let mut s = opened(&fixtures()); // img1.png, 320×240
    assert_eq!(
        (s.crop.aspect, s.crop_ratio()),
        (Aspect::Original, Some(4.0 / 3.0)),
        "the photo's by default"
    );
    s.apply(&Action::SetCrop(Rect::new(0.0, 0.0, 200.0, 200.0)));
    s.apply(&Action::SetAspect { aspect: Aspect::Ratio { w: 16.0, h: 9.0 } });
    let px = s.crop_pixels().unwrap();
    assert!((px.w as f32 / px.h as f32 - 16.0 / 9.0).abs() < 0.02, "{px}");
    s.apply(&Action::SwapAspect);
    assert_eq!(s.crop_ratio(), Some(9.0 / 16.0));
    s.apply(&Action::ToggleCustomAspect);
    assert_eq!(s.aspect_input.as_deref(), Some(""), "field opens empty for a preset");
    s.apply(&Action::Next);
    assert_eq!(s.aspect_input, None, "any other action closes it");
    let v = serde_json::to_value(s.dump(None, None)).unwrap();
    assert_eq!((v["aspect"].as_str(), v["aspect_swapped"].as_bool()), (Some("16:9"), Some(true)));
    s.apply(&Action::NextAspect);
    assert_eq!(s.crop.aspect, Aspect::Original, "a preset after the last one wraps");
    // The ratio follows each photo: img3 is portrait.
    s.apply(&Action::Goto { index: 2 });
    settle(&mut s);
    assert_eq!(s.crop_ratio(), Some(4.0 / 3.0), "swapped: portrait photo, landscape crop");
}

#[test]
fn update_set_crop_clamps_and_modes() {
    let mut s = opened(&fixtures()); // img1.png, 320×240
    s.apply(&Action::SetCrop(Rect::new(300.0, 200.0, 100.0, 100.0)));
    assert_eq!(s.mode, Mode::Crop, "set_crop turns crop mode on");
    assert_eq!(s.crop_pixels(), Some(PixelRect { x: 300, y: 200, w: 20, h: 40 }));
    assert_eq!(s.status(), "img1.png · 1 / 9 · crop 20×40 @ 300,200");
    s.apply(&Action::ClearCrop);
    assert_eq!((s.mode, s.crop_pixels()), (Mode::Crop, None));
    s.apply(&Action::SetCrop(Rect::new(500.0, 0.0, 10.0, 10.0))); // fully outside
    assert_eq!(s.crop_pixels(), None);
    assert_eq!(s.errors.len(), 1);
    s.apply(&Action::SetCrop(Rect::new(10.0, 10.0, 50.0, 50.0)));
    s.apply(&Action::Next); // navigation drops the selection, keeps the mode
    assert_eq!((s.mode, s.crop_pixels()), (Mode::Crop, None));
    s.apply(&Action::ToggleCrop);
    assert_eq!(s.mode, Mode::Browse);
    s.apply(&Action::SetCrop(Rect::new(0.0, 0.0, 5.0, 5.0)));
    s.apply(&Action::ExitCrop);
    assert_eq!((s.mode, s.crop_pixels()), (Mode::Browse, None));

    // EXIF-rotated: crop space is the displayed (oriented) 240×320.
    s.apply(&Action::Goto { index: 2 });
    settle(&mut s);
    s.apply(&Action::SetCrop(Rect::new(0.0, 0.0, 1000.0, 1000.0)));
    assert_eq!(s.crop_pixels(), Some(PixelRect { x: 0, y: 0, w: 240, h: 320 }));

    // Broken image: no crop possible.
    s.apply(&Action::Goto { index: 8 });
    settle(&mut s);
    let before = s.errors.len();
    s.apply(&Action::SetCrop(Rect::new(0.0, 0.0, 10.0, 10.0)));
    assert_eq!(s.errors.len(), before + 1);
}

#[test]
fn update_fullscreen_toggle_and_exit() {
    let mut s = opened(&fixtures());
    assert_eq!(s.apply(&Action::ExitFullscreen), Effect::None, "already windowed");
    assert_eq!(s.apply(&Action::ToggleFullscreen), Effect::Fullscreen(true));
    s.apply(&Action::SetCrop(Rect::new(10.0, 10.0, 50.0, 50.0)));
    s.apply(&Action::Next);
    s.apply(&Action::Open { path: fixtures() });
    assert!(s.fullscreen, "browsing, crop and open keep full screen");
    assert_eq!(s.apply(&Action::ExitFullscreen), Effect::Fullscreen(false));
    assert!(!s.fullscreen);
    assert_eq!(s.apply(&Action::ToggleFullscreen), Effect::Fullscreen(true));
    assert_eq!(s.apply(&Action::ToggleFullscreen), Effect::Fullscreen(false));
}

#[test]
fn update_open_resets() {
    let mut s = opened(&fixtures());
    s.apply(&Action::Goto { index: 3 });
    s.apply(&Action::SetCrop(Rect::new(1.0, 1.0, 10.0, 10.0)));
    s.apply(&Action::Open { path: fixtures().join("subdir") });
    settle(&mut s);
    assert_eq!((s.len(), s.index, s.mode, s.crop_pixels()), (1, 0, Mode::Browse, None));
    assert_eq!(s.title(), format!("PhotoCruiser: {}", crate::state::tilde(&fixtures().join("subdir"))));
    assert_eq!(s.status(), "nested.jpg · 1 / 1");

    // A file path opens its folder at that file.
    s.apply(&Action::Open { path: fixtures().join("img9.webp") });
    settle(&mut s);
    assert_eq!((s.len(), s.current_name()), (9, Some("img9.webp")));

    // Empty and missing folders: messages, no panic.
    let empty = std::env::temp_dir().join(format!("photo-cruiser-empty-{}", std::process::id()));
    std::fs::create_dir_all(&empty).unwrap();
    s.apply(&Action::Open { path: empty.clone() });
    settle(&mut s);
    assert_eq!((s.len(), s.status(), s.position()), (0, String::new(), "0 / 0".into()));
    std::fs::remove_dir(&empty).unwrap();
    s.apply(&Action::Open { path: "/nonexistent/photo-cruiser".into() });
    assert!(s.folder_error.is_some());
    assert!(s.errors.last().unwrap().contains("Cannot open folder"));
}

#[test]
fn update_previous_photo_stays_until_decoded() {
    let mut s = opened(&fixtures());
    assert_eq!(s.display(), Display::Sharp);
    // Neighbour: preloaded, shown at once.
    s.apply(&Action::Next);
    assert_eq!((s.display(), s.shown.as_ref().unwrap().index), (Display::Sharp, 1));
    // Far jump: the previous photo stays (no blank, no "Loading…") until it's decoded.
    s.apply(&Action::Goto { index: 6 });
    assert_eq!((s.display(), s.shown.as_ref().unwrap().index), (Display::Previous, 1));
    assert!(s.dump(None, None).loading);
    settle(&mut s);
    assert_eq!((s.display(), s.shown.as_ref().unwrap().index), (Display::Sharp, 6));
    // A broken photo shows its error instead.
    s.apply(&Action::Goto { index: 8 });
    settle(&mut s);
    assert_eq!(s.display(), Display::None);
}

#[test]
fn update_embedded_preview_then_sharp() {
    let dir = fixtures().join("previews"); // letterbox.jpg, rounded.jpg, thumb-exif6.jpg, thumb.HEIC
    let mut s = opened(&dir);
    for (index, preview_size) in [(2, (240, 320)), (3, (320, 240))] {
        s.apply(&Action::Open { path: dir.clone() });
        s.apply(&Action::Goto { index });
        // The preview job comes first and gets its own slot.
        let load = s.next_job().unwrap();
        let (job, path) = (load.job, load.path.clone());
        assert_eq!((job.index, job.kind), (index, Kind::Preview));
        s.finish(job, decode_job(&load, s.max_side));
        assert_eq!(s.display(), Display::Preview, "{path:?}");
        assert!(s.picture().is_none(), "no crop on a preview");
        assert_eq!(s.loader.preview(index).unwrap().as_ref().unwrap().source, preview_size);
        settle(&mut s);
        assert_eq!(s.display(), Display::Sharp);
        assert_eq!(s.picture().unwrap().source, preview_size);
    }
    // Aspect mismatch: no preview, the previous photo stays instead.
    s.apply(&Action::Goto { index: 0 });
    let load = s.next_job().unwrap();
    assert_eq!(load.job.kind, Kind::Preview);
    s.finish(load.job, decode_job(&load, s.max_side));
    assert_eq!(s.display(), Display::Previous);
}

#[test]
fn update_fades_except_when_flipping_quickly() {
    let mut s = opened(&fixtures());
    s.transition = Duration::from_millis(100);
    s.apply(&Action::Next); // preloaded: fades in at once
    assert!(s.fade_progress().is_some());
    assert!(!s.is_idle(), "script mode waits for the fade");
    std::thread::sleep(Duration::from_millis(120));
    assert!(s.fade_progress().is_none());
    settle(&mut s);
    // Two switches in quick succession (held key): the second one is instant.
    s.apply(&Action::Prev);
    s.apply(&Action::Next);
    assert_eq!(s.display(), Display::Sharp);
    assert!(s.fade.is_none());
    // Zero transition: never.
    std::thread::sleep(Duration::from_millis(250));
    s.transition = Duration::ZERO;
    s.apply(&Action::Prev);
    assert!(s.fade.is_none());
}

#[test]
fn update_grid_view() {
    let mut s = opened(&fixtures());
    assert_eq!(s.view, View::Grid, "a folder opens in the grid");
    // Visible thumbnails are loaded; the broken file's is an error.
    assert!((0..8).all(|i| s.thumbs.get(i).is_some_and(|t| t.is_ok())));
    assert!(s.thumbs.get(8).unwrap().is_err());
    assert_eq!(s.dump(None, None).view, View::Grid);

    s.body = Some(Size::new(850.0, 700.0)); // 4 columns: rows 0–3, 4–7, 8
    assert_eq!(s.grid().rows[0].items.len(), 4);
    s.apply(&Action::Down);
    s.apply(&Action::Next);
    assert_eq!(s.index, 5);
    s.apply(&Action::Down); // into the short last row
    assert_eq!(s.index, 8);
    s.apply(&Action::Down); // no row below
    assert_eq!(s.index, 8);
    s.apply(&Action::Up);
    s.apply(&Action::Up);
    assert_eq!(s.index, 0);
    s.apply(&Action::Up);
    assert_eq!(s.index, 0);

    // Enter opens the selection; row moves do nothing there; Esc goes back.
    s.apply(&Action::Goto { index: 2 });
    s.apply(&Action::ShowPhoto { index: None });
    assert_eq!((s.view, s.index), (View::Photo, 2));
    s.apply(&Action::Down);
    assert_eq!(s.index, 2);
    s.apply(&Action::ShowGrid);
    assert_eq!((s.view, s.index), (View::Grid, 2));
    s.apply(&Action::ShowPhoto { index: Some(6) });
    assert_eq!((s.view, s.index), (View::Photo, 6));
    s.apply(&Action::ShowGrid);
    s.apply(&Action::ShowPhoto { index: Some(9) });
    assert_eq!((s.view, s.index, s.errors.len()), (View::Grid, 6, 1));

    // Crop from the grid crops the current photo; back in the grid, crop mode is off.
    s.apply(&Action::ToggleCrop);
    assert_eq!((s.view, s.mode), (View::Photo, Mode::Crop));
    s.apply(&Action::ShowGrid);
    assert_eq!((s.view, s.mode, s.crop_pixels()), (View::Grid, Mode::Browse, None));

    // No thumbnails load while a single photo shows.
    s.apply(&Action::Open { path: fixtures() });
    s.apply(&Action::ShowPhoto { index: None });
    assert!(s.next_thumb_job().is_none());
    // A file opens in the photo view.
    s.apply(&Action::Open { path: fixtures().join("img2.jpg") });
    assert_eq!((s.view, s.index), (View::Photo, 1));

    // Moving past the visible rows asks the grid to scroll; the newly visible thumbnails load.
    s.apply(&Action::Open { path: fixtures() });
    settle(&mut s);
    s.body = Some(Size::new(850.0, 260.0)); // about one row
    assert_eq!(s.scroll.take(), Some(0.0), "opening scrolls to the top");
    s.apply(&Action::Down);
    s.apply(&Action::Down);
    let y = s.scroll.take().expect("scrolls");
    assert_eq!(s.grid_offset, y);
    assert!(s.visible().contains(&8), "{:?}", s.visible());
    s.apply(&Action::Up);
    s.apply(&Action::Up);
    assert_eq!(s.scroll.take(), Some(0.0));
}

#[test]
fn dump_json_shape() {
    let mut s = opened(&fixtures());
    s.apply(&Action::SetCrop(Rect::new(40.4, 30.0, 200.0, 150.0)));
    let v = serde_json::to_value(s.dump(None, Some(Path::new("/tmp/x.png")))).unwrap();
    assert_eq!(v["count"], 9);
    assert_eq!(v["index"], 0);
    assert_eq!(v["position"], "1 / 9");
    assert_eq!(v["mode"], "crop");
    assert_eq!(v["crop"], serde_json::json!({"x": 40, "y": 30, "w": 200, "h": 150}));
    assert_eq!(v["loading"], false);
    assert_eq!(v["display"], "sharp");
    assert_eq!(v["shot"], "/tmp/x.png");
    assert!(v["file"].as_str().unwrap().ends_with("img1.png"));
    assert!(v["window"].is_null());
}

// ---------------------------------------------------------------------------------------
// iced_test end-to-end

struct Harness<P: Program<Message = Message> + 'static> {
    prog: P,
    emu: Emulator<P>,
    rx: mpsc::Receiver<emulator::Event<P>>,
}

/// Opened on the fixtures, in the grid, file names on (`N`).
fn grid_harness() -> Harness<impl Program<Message = Message, State = app::App>> {
    let mut h = bare_harness();
    h.keys(Key::Character("n".into()), 1);
    h
}

/// Opened on the fixtures, in the grid, as at start.
fn bare_harness() -> Harness<impl Program<Message = Message, State = app::App>> {
    let cfg = Config {
        folder: Some(fixtures()),
        max_side: TEST_MAX_SIDE,
        transition: Duration::ZERO,
        script: None,
        settings: None,
    };
    Harness::new(app::application(cfg))
}

/// Opened on the fixtures, showing the first photo (Enter in the grid).
fn harness() -> Harness<impl Program<Message = Message, State = app::App>> {
    let mut h = grid_harness();
    h.keys(Key::Named(Named::Enter), 1);
    h
}

impl<P: Program<Message = Message> + 'static> Harness<P> {
    fn new(prog: P) -> Self {
        let (tx, rx) = mpsc::channel(100);
        let emu = Emulator::new(tx, &prog, emulator::Mode::Zen, SIZE);
        let mut h = Harness { prog, emu, rx };
        h.until_ready(); // boot task: decode current and neighbours
        // What the overlay reports on its first redraw in the real window.
        let body = h.bounds("body").unwrap().size();
        h.emu.update(&h.prog, Message::Body(body));
        h
    }

    /// Process emulator events; returns true when a `Ready` arrived.
    fn pump(&mut self) -> bool {
        let mut ready = false;
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                emulator::Event::Action(a) => self.emu.perform(&self.prog, a),
                emulator::Event::Ready => ready = true,
                emulator::Event::Failed(i) => panic!("instruction failed: {i}"),
            }
        }
        ready
    }

    fn until_ready(&mut self) {
        let start = Instant::now();
        while !self.pump() {
            assert!(start.elapsed() < TIMEOUT, "timeout waiting for Ready");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Pump until `cond` holds (checked on the current view).
    fn until(&mut self, what: &str, cond: impl Fn(&mut Self) -> bool) {
        let start = Instant::now();
        while !cond(self) {
            assert!(start.elapsed() < TIMEOUT, "timeout waiting for {what}; status {:?}", self.status());
            std::thread::sleep(Duration::from_millis(5));
            self.pump();
        }
    }

    fn sim(&self) -> Simulator<'_, Message, P::Theme, P::Renderer> {
        let settings = iced::Settings { default_font: crate::theme::get().font, ..Default::default() };
        Simulator::with_size(settings, SIZE, self.emu.view(&self.prog))
    }

    /// The status bar text (`name — i / N ...`).
    fn status(&self) -> String {
        let pick = |c: Candidate<'_>| match c {
            Candidate::Text { content, .. } if content.contains(" / ") => Some(content.to_owned()),
            _ => None,
        };
        self.sim().find(pick).unwrap_or_default()
    }

    fn has_text(&self, needle: &str) -> bool {
        let pick = |c: Candidate<'_>| match c {
            Candidate::Text { content, .. } if content.contains(needle) => Some(()),
            _ => None,
        };
        self.sim().find(pick).is_ok()
    }

    /// Centre of the first text widget showing exactly `needle`.
    fn text_center(&self, needle: &str) -> Point {
        let pick = |c: Candidate<'_>| match c {
            Candidate::Text { content, bounds, .. } if content == needle => Some(bounds.center()),
            _ => None,
        };
        self.sim().find(pick).unwrap_or_else(|_| panic!("no text {needle:?}"))
    }

    fn click_menu_button(&mut self) {
        let target = Some(Target::Point(self.bounds("menu-button").expect("menu button").center()));
        self.run(Interaction::Mouse(Mouse::Click { button: Button::Left, target }));
    }

    fn bounds(&self, id: &'static str) -> Option<Rectangle> {
        self.sim().find(selector::id(id)).ok().map(|t| t.bounds())
    }

    /// Tap `key` `times` times through the widget tree, then apply the published messages.
    fn keys(&mut self, key: Key, times: usize) {
        let messages: Vec<Message> = {
            let mut sim = self.sim();
            for _ in 0..times {
                sim.tap_key(key.clone());
            }
            sim.into_messages().collect()
        };
        assert_eq!(messages.len(), times, "key {key:?} produced {messages:?}");
        for m in messages {
            self.emu.update(&self.prog, m);
        }
    }

    fn run(&mut self, i: Interaction) {
        self.emu.run(&self.prog, Instruction::Interact(i));
        self.until_ready();
    }

    /// Left-button drag from `a` to `b` (window coordinates) via the emulator.
    fn drag(&mut self, a: Point, b: Point) {
        let mid = Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
        self.run(Interaction::Mouse(Mouse::Press { button: Button::Left, target: Some(Target::Point(a)) }));
        self.run(Interaction::Mouse(Mouse::Move(Target::Point(mid))));
        self.run(Interaction::Mouse(Mouse::Move(Target::Point(b))));
        self.run(Interaction::Mouse(Mouse::Release { button: Button::Left, target: Some(Target::Point(b)) }));
    }

    /// Where the current image (source size `img`) is drawn, in window coordinates.
    fn fit(&self, img: (u32, u32)) -> Rect {
        let b = self.bounds("body").unwrap();
        fit_rect(Rect::new(b.x, b.y, b.width, b.height), img.into())
    }

    /// Render the current view with `Simulator::snapshot` and return its pixels.
    /// Not `Emulator::screenshot`: it draws a single frame (big images upload
    /// asynchronously in iced_wgpu, so they'd be missing) and it consumes the emulator's
    /// UI cache. The snapshot is at scale factor 2; pixels are only reachable through
    /// `Snapshot::matches_image`, which writes `<stem>-<renderer>.png` if missing.
    fn snapshot(&self, name: &str) -> image::RgbaImage {
        let theme = self.emu.theme(&self.prog).unwrap();
        let mut sim = self.sim();
        sim.snapshot(&theme).unwrap(); // queues the texture upload
        std::thread::sleep(Duration::from_millis(300));
        let snap = sim.snapshot(&theme).unwrap();
        let dir = std::env::temp_dir().join(format!("photo-cruiser-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(snap.matches_image(dir.join("shot.png")).unwrap());
        let file = std::fs::read_dir(&dir).unwrap().next().unwrap().unwrap().path();
        let img = image::open(&file).unwrap().into_rgba8();
        std::fs::remove_dir_all(&dir).unwrap();
        img
    }
}

/// Parse `crop W×H @ X,Y` out of the status line.
fn parse_crop(status: &str) -> [f32; 4] {
    let s = status.split("crop ").nth(1).unwrap_or_else(|| panic!("no crop in {status:?}"));
    let n: Vec<f32> = s.split(['×', ' ', '@', ',']).filter_map(|t| t.parse().ok()).collect();
    [n[0], n[1], n[2], n[3]]
}

fn assert_close(got: [f32; 4], want: [f32; 4]) {
    assert!(got.iter().zip(want).all(|(a, b)| (a - b).abs() <= 2.0), "got {got:?}, want ~{want:?}");
}

/// Mean of R, G, B over a 10×10 patch around `p` (logical px; snapshots are at scale 2).
fn mean(img: &image::RgbaImage, p: Point) -> f32 {
    let (cx, cy) = (p.x as u32 * 2, p.y as u32 * 2);
    let px: Vec<f32> = (cx - 5..cx + 5)
        .flat_map(|x| (cy - 5..cy + 5).map(move |y| (x, y)))
        .flat_map(|(x, y)| img.get_pixel(x, y).0[..3].to_vec())
        .map(f32::from)
        .collect();
    px.iter().sum::<f32>() / px.len() as f32
}

#[test]
fn browse_all_fixtures() {
    let names = names();
    assert_eq!(names.len(), 9);
    let mut h = harness();
    for (i, name) in names.iter().enumerate() {
        if i > 0 {
            h.keys(Key::Named(Named::ArrowRight), 1);
        }
        let want = format!("{name} · {} / 9", i + 1);
        if name.contains("broken") {
            // Shows its error (in place of the image), not "Loading…".
            h.until(&want, |h| h.status() == want && h.has_text("decode failed"));
            assert!(h.bounds("message").is_some());
            assert!(h.bounds("photo").is_none());
        } else {
            h.until(&want, |h| h.status() == want && h.bounds("photo").is_some());
        }
    }
    let (state, window) = h.emu.into_state();
    assert_eq!(
        Program::title(&h.prog, &state, window),
        format!("PhotoCruiser: {}", crate::state::tilde(&fixtures()))
    );
}

#[test]
fn clamping_at_both_ends() {
    let mut h = harness();
    h.until("first", |h| h.status() == "img1.png · 1 / 9" && h.bounds("photo").is_some());
    h.keys(Key::Named(Named::ArrowLeft), 3);
    assert_eq!(h.status(), "img1.png · 1 / 9");
    // 30 × → in one burst (like a held key): clamps at 9, stale decodes are dropped.
    h.keys(Key::Named(Named::ArrowRight), 30);
    assert_eq!(h.status(), "img12-broken.jpg · 9 / 9");
    h.until("broken shown", |h| h.has_text("decode failed"));
    h.keys(Key::Named(Named::ArrowLeft), 1);
    h.until("img11", |h| h.status() == "img11.bmp · 8 / 9" && h.bounds("photo").is_some());
}

#[test]
fn crop_draw_move_resize_dim_and_esc() {
    let mut h = harness();
    h.until("img1 shown", |h| h.bounds("photo").is_some());
    // Through the menu: open it, pick "Crop" by its text.
    h.click_menu_button();
    assert!(h.bounds("menu").is_some(), "menu open");
    let target = Some(Target::Text("Crop".into()));
    h.run(Interaction::Mouse(Mouse::Click { button: Button::Left, target }));
    assert!(h.status().contains("crop: drag"), "{}", h.status());
    assert!(h.bounds("menu").is_none(), "picking an item closes the menu");
    let before = h.snapshot("nocrop");

    let (w, hh) = (320.0, 240.0); // img1.png
    let fit = h.fit((320, 240));
    let at = |fx: f32, fy: f32| Point::new(fit.x + fx * fit.w, fit.y + fy * fit.h);
    h.drag(at(0.2, 0.2), at(0.6, 0.6)); // draw 0.2..0.6
    assert_close(parse_crop(&h.status()), [0.4 * w, 0.4 * hh, 0.2 * w, 0.2 * hh]);
    h.drag(at(0.4, 0.4), at(0.5, 0.45)); // move by +0.1, +0.05
    assert_close(parse_crop(&h.status()), [0.4 * w, 0.4 * hh, 0.3 * w, 0.25 * hh]);
    // Resize the bottom-right corner: keeps the photo's 4:3, the larger side wins.
    h.drag(at(0.7, 0.65), at(0.8, 0.85));
    assert_close(parse_crop(&h.status()), [0.6 * w, 0.6 * hh, 0.3 * w, 0.25 * hh]);

    // 50% dim outside the selection (sRGB blending via `web-colors`), untouched inside.
    let after = h.snapshot("crop");
    for (fx, fy) in [(0.1, 0.1), (0.95, 0.5), (0.5, 0.95)] {
        let ratio = mean(&after, at(fx, fy)) / mean(&before, at(fx, fy));
        assert!((ratio - 0.5).abs() < 0.04, "outside at ({fx},{fy}) not dimmed to ~50%: {ratio}");
    }
    let inside = mean(&after, at(0.55, 0.7)) / mean(&before, at(0.55, 0.7));
    assert!((inside - 1.0).abs() < 0.02, "inside changed: {inside}");

    // Free (a footer button): the corner follows the cursor.
    h.run(Interaction::Mouse(Mouse::Click {
        button: Button::Left,
        target: Some(Target::Text("Free".into())),
    }));
    h.drag(at(0.9, 0.85), at(0.8, 0.95));
    assert_close(parse_crop(&h.status()), [0.5 * w, 0.7 * hh, 0.3 * w, 0.25 * hh]);

    // Esc leaves crop mode.
    h.keys(Key::Named(Named::Escape), 1);
    assert_eq!(h.status(), "img1.png · 1 / 9");
}

#[test]
fn crop_aspect_presets_custom_and_swap() {
    let mut h = harness();
    h.until("img1 shown", |h| h.bounds("photo").is_some());
    assert!(h.bounds("aspects").is_none(), "aspect buttons only while cropping");
    h.keys(Key::Character("c".into()), 1);
    assert!(h.bounds("aspects").is_some() && h.has_text("Landscape"));
    let fit = h.fit((320, 240));
    let at = |fx: f32, fy: f32| Point::new(fit.x + fx * fit.w, fit.y + fy * fit.h);
    h.drag(at(0.1, 0.1), at(0.6, 0.3));
    assert_close(parse_crop(&h.status()), [160.0, 120.0, 32.0, 24.0]);

    // `A` twice: original → free → 1:1, the selection reshaped around its centre.
    h.keys(Key::Character("a".into()), 2);
    let [cw, ch, ..] = parse_crop(&h.status());
    assert!((cw - ch).abs() <= 1.0, "{}", h.status());
    assert!(!h.has_text("Landscape"), "no orientation for a square");

    // Custom 2:1, typed into the field; `X` turns it portrait.
    let target = Some(Target::Text("Custom…".into()));
    h.run(Interaction::Mouse(Mouse::Click { button: Button::Left, target }));
    assert!(h.bounds("aspect-input").is_some());
    h.run(Interaction::Keyboard(Keyboard::Typewrite("2:1".into())));
    h.run(Interaction::Keyboard(Keyboard::Type(instruction::Key::Enter)));
    assert!(h.bounds("aspect-panel").is_none() && h.has_text("2:1…"), "{}", h.status());
    let [cw, ch, ..] = parse_crop(&h.status());
    assert!((cw / ch - 2.0).abs() < 0.05, "{}", h.status());
    h.keys(Key::Character("x".into()), 1);
    let [cw, ch, ..] = parse_crop(&h.status());
    assert!((ch / cw - 2.0).abs() < 0.05, "{}", h.status());
    assert!(h.has_text("Portrait"));

    // Esc closes the field without changing anything.
    h.run(Interaction::Mouse(Mouse::Click {
        button: Button::Left,
        target: Some(Target::Text("2:1…".into())),
    }));
    h.keys(Key::Named(Named::Escape), 1);
    assert!(h.bounds("aspect-panel").is_none());
    assert!(h.status().contains("crop "), "still cropping: {}", h.status());
}

#[test]
fn crop_drag_past_edge_is_clamped() {
    let mut h = harness();
    h.keys(Key::Named(Named::ArrowRight), 2); // img3-exif6.jpg: displayed 240×320
    h.until("img3 shown", |h| h.status().starts_with("img3-exif6.jpg") && h.bounds("photo").is_some());
    h.keys(Key::Character("c".into()), 1);
    let fit = h.fit((240, 320));
    h.drag(
        Point::new(fit.x + fit.w / 2.0, fit.y + fit.h / 2.0),
        Point::new(SIZE.width - 1.0, SIZE.height - 1.0),
    );
    assert_close(parse_crop(&h.status()), [120.0, 160.0, 120.0, 160.0]);
    // Arrow keys apply the selection; the next photo has none.
    h.keys(Key::Named(Named::ArrowLeft), 1);
    assert!(h.status().contains("(Esc to leave)"), "{}", h.status());
}

#[test]
fn fullscreen_hides_bars_and_keys() {
    let mut h = harness();
    h.until("img1 shown", |h| h.bounds("photo").is_some());
    let windowed = h.bounds("body").unwrap();
    assert!(windowed.height < SIZE.height && h.bounds("menu-button").is_some());

    h.keys(Key::Character("f".into()), 1);
    assert_eq!(h.bounds("body").unwrap(), Rectangle::with_size(SIZE), "image area fills the window");
    assert!(h.bounds("menu-button").is_none() && h.status().is_empty(), "footer hidden");
    h.keys(Key::Named(Named::ArrowRight), 1); // keys still work
    h.keys(Key::Named(Named::F11), 1);
    assert_eq!(h.status(), "img2.jpg · 2 / 9");

    h.keys(Key::Named(Named::F11), 1);
    assert!(h.bounds("menu-button").is_none());

    // Crop in full screen: Esc leaves crop mode first, then full screen.
    h.keys(Key::Character("c".into()), 1);
    let fit = h.fit((320, 240)); // img2.jpg
    h.drag(Point::new(fit.x, fit.y), Point::new(fit.x + fit.w / 2.0, fit.y + fit.h / 2.0));
    h.keys(Key::Named(Named::F11), 1);
    assert_close(parse_crop(&h.status()), [160.0, 120.0, 0.0, 0.0]);
    h.keys(Key::Named(Named::F11), 1);
    h.keys(Key::Named(Named::Escape), 1);
    assert!(h.bounds("menu-button").is_none(), "first Esc only leaves crop mode");
    h.keys(Key::Named(Named::Escape), 1);
    assert_eq!(h.status(), "img2.jpg · 2 / 9");
}

#[test]
fn menu_hidden_by_default_and_closes() {
    let mut h = harness();
    h.until("img1 shown", |h| h.bounds("photo").is_some());
    assert!(h.bounds("menu").is_none() && !h.has_text("Open folder"), "no menu or toolbar by default");
    let full = h.bounds("body").unwrap();
    assert!(full.height > SIZE.height - 40.0, "image area takes all but the footer: {full:?}");

    h.click_menu_button();
    assert!(h.has_text("Open folder") && h.has_text("Full screen"));
    h.keys(Key::Named(Named::Escape), 1);
    assert!(h.bounds("menu").is_none(), "Esc closes it");

    h.click_menu_button();
    h.click_menu_button();
    assert!(h.bounds("menu").is_none(), "the button toggles it");
    h.keys(Key::Named(Named::F10), 1);
    assert!(h.bounds("menu").is_some(), "F10 opens it");
    h.keys(Key::Named(Named::F10), 1);
    assert!(h.bounds("menu").is_none(), "F10 closes it");

    h.click_menu_button();
    let target = Some(Target::Point(full.center()));
    h.run(Interaction::Mouse(Mouse::Click { button: Button::Left, target }));
    assert!(h.bounds("menu").is_none(), "a click outside closes it");
    assert_eq!(h.status(), "img1.png · 1 / 9");

    h.click_menu_button();
    h.keys(Key::Named(Named::ArrowRight), 1);
    assert!(h.bounds("menu").is_none(), "any action closes it");
    assert_eq!(h.status(), "img2.jpg · 2 / 9");
}

#[test]
fn shortcuts_panel() {
    let mut h = harness();
    h.until("img1 shown", |h| h.bounds("photo").is_some());
    assert!(h.bounds("help").is_none());
    h.keys(Key::Character("?".into()), 1);
    assert!(h.has_text("Keyboard shortcuts") && h.has_text("Next / previous photo"));
    h.keys(Key::Named(Named::Escape), 1);
    assert!(h.bounds("help").is_none(), "Esc closes it");
    h.keys(Key::Named(Named::F1), 1);
    assert!(h.bounds("help").is_some(), "F1 opens it");
    h.keys(Key::Named(Named::F1), 1);
    assert!(h.bounds("help").is_none(), "F1 closes it");

    h.click_menu_button();
    let target = Some(Target::Text("Keyboard shortcuts".into()));
    h.run(Interaction::Mouse(Mouse::Click { button: Button::Left, target }));
    assert!(h.bounds("help").is_some() && h.bounds("menu").is_none(), "opens from the menu");
    let target = Some(Target::Point(Point::new(5.0, 5.0)));
    h.run(Interaction::Mouse(Mouse::Click { button: Button::Left, target }));
    assert!(h.bounds("help").is_none(), "a click outside closes it");
}

#[test]
fn menu_keyboard() {
    let mut h = harness();
    h.until("img1 shown", |h| h.bounds("photo").is_some());
    h.keys(Key::Named(Named::F10), 1);
    let first = h.bounds("menu-focus").expect("F10 focuses the first item");
    h.keys(Key::Named(Named::ArrowDown), 1);
    let second = h.bounds("menu-focus").unwrap();
    assert!(second.y > first.y, "↓ moves down");
    h.keys(Key::Named(Named::ArrowUp), 2);
    let last = h.bounds("menu-focus").unwrap();
    assert!(last.y > second.y, "↑ from the first item wraps to the last");
    h.keys(Key::Named(Named::ArrowDown), 1);
    assert_eq!(h.bounds("menu-focus").unwrap().y, first.y, "↓ from the last wraps to the first");
    h.keys(Key::Named(Named::End), 1);
    assert_eq!(h.bounds("menu-focus").unwrap().y, last.y, "End");
    h.keys(Key::Named(Named::Home), 1);
    assert_eq!(h.bounds("menu-focus").unwrap().y, first.y, "Home");
    assert_eq!(h.status(), "img1.png · 1 / 9", "the photo doesn't change");

    // Open folder, All photos, Crop: Enter picks it and closes the menu.
    h.keys(Key::Named(Named::ArrowDown), 2);
    h.keys(Key::Named(Named::Enter), 1);
    assert!(h.bounds("menu").is_none() && h.status().contains("crop: drag"), "{}", h.status());

    // Opened with the mouse: nothing focused until ↓ / ↑, or the mouse is over an item.
    h.click_menu_button();
    assert!(h.bounds("menu-focus").is_none());
    h.keys(Key::Named(Named::Enter), 1);
    assert!(h.bounds("menu").is_some(), "Enter with nothing focused does nothing");
    h.keys(Key::Named(Named::ArrowUp), 1);
    h.keys(Key::Named(Named::Space), 1);
    assert!(h.bounds("about").is_some(), "↑ focuses the last item (About), Space picks it");
}

#[test]
fn about_panel() {
    let mut h = harness();
    h.until("img1 shown", |h| h.bounds("photo").is_some());
    let about = || Some(Target::Text("About Photo Cruiser".into()));
    h.click_menu_button();
    h.run(Interaction::Mouse(Mouse::Click { button: Button::Left, target: about() }));
    assert!(h.bounds("about").is_some() && h.bounds("menu").is_none(), "opens from the menu");
    assert!(h.has_text(&format!("Version {}", env!("CARGO_PKG_VERSION"))));
    h.keys(Key::Named(Named::Escape), 1);
    assert!(h.bounds("about").is_none(), "Esc closes it");
    h.click_menu_button();
    h.run(Interaction::Mouse(Mouse::Click { button: Button::Left, target: about() }));
    let target = Some(Target::Point(Point::new(5.0, 5.0)));
    h.run(Interaction::Mouse(Mouse::Click { button: Button::Left, target }));
    assert!(h.bounds("about").is_none(), "a click outside closes it");
}

#[test]
fn grid_click_keys_and_back() {
    let mut h = grid_harness();
    assert!(h.bounds("grid").is_some() && h.bounds("photo").is_none());
    assert_eq!(h.status(), "img1.png · 1 / 9");
    assert!(h.bounds("selected").unwrap().contains(h.text_center("img1.png")));

    // A click on a thumbnail opens that photo.
    let target = Some(Target::Text("img3-exif6.jpg".into()));
    h.run(Interaction::Mouse(Mouse::Click { button: Button::Left, target }));
    h.until("img3 shown", |h| h.status() == "img3-exif6.jpg · 3 / 9" && h.bounds("photo").is_some());
    assert!(h.bounds("grid").is_none());

    // Esc: back to the grid, on the same photo.
    h.keys(Key::Named(Named::Escape), 1);
    assert!(h.bounds("grid").is_some() && h.bounds("photo").is_none());
    assert!(h.bounds("selected").unwrap().contains(h.text_center("img3-exif6.jpg")));

    // 4 columns at this size: ↓ is +4, ↑ is −4; Enter opens the selection.
    h.keys(Key::Named(Named::ArrowDown), 1);
    assert_eq!(h.status(), "img10.gif · 7 / 9");
    h.keys(Key::Named(Named::ArrowRight), 1);
    h.keys(Key::Character("k".into()), 1);
    assert_eq!(h.status(), "img4.AVIF · 4 / 9");
    assert!(h.bounds("selected").unwrap().contains(h.text_center("img4.AVIF")));
    h.keys(Key::Named(Named::Enter), 1);
    h.until("img4 shown", |h| h.bounds("photo").is_some());
    assert_eq!(h.status(), "img4.AVIF · 4 / 9");
}

#[test]
fn names_hidden_by_default_and_toggled_with_n() {
    let mut s = State::new(TEST_MAX_SIDE, Duration::ZERO);
    s.apply(&Action::Open { path: fixtures() });
    assert_eq!((s.names, s.status()), (false, "1 / 9".into()));
    s.apply(&Action::ToggleNames);
    assert_eq!(s.status(), "img1.png · 1 / 9");
    assert!(s.dump(None, None).names);

    let mut h = bare_harness();
    assert_eq!(h.status(), "1 / 9");
    assert!(!h.has_text("img1.png") && !h.has_text("img2.jpg"), "no names in the grid");
    let cell = h.bounds("selected").unwrap();
    h.keys(Key::Character("n".into()), 1);
    assert_eq!(h.status(), "img1.png · 1 / 9");
    assert!(h.has_text("img2.jpg"), "grid labels shown");
    assert!(h.bounds("selected").unwrap().height > cell.height, "cells make room for the name");
    h.keys(Key::Character("n".into()), 1);
    h.keys(Key::Named(Named::Enter), 1);
    h.until("img1 shown", |h| h.bounds("photo").is_some());
    assert_eq!(h.status(), "1 / 9", "photo view: no name either");
    h.keys(Key::Character("n".into()), 1);
    assert_eq!(h.status(), "img1.png · 1 / 9");
}

#[test]
fn layouts_cycled_with_z() {
    let mut s = opened(&fixtures());
    let small = s.thumbs.get(1).unwrap().as_ref().unwrap().handle.clone();
    s.apply(&Action::NextLayout);
    assert_eq!((s.layout, s.dump(None, None).layout), (Layout::Big, Layout::Big));
    // Re-decoded bigger; the small ones stay until then.
    assert!(!s.is_idle());
    assert_eq!(s.thumbs.get(1).unwrap().as_ref().unwrap().handle, small);
    settle(&mut s);
    let big = s.thumbs.get(1).unwrap().as_ref().unwrap().handle.clone();
    assert_ne!(big, small);

    // Justified: same thumbnails, but every size is probed first.
    s.apply(&Action::NextLayout);
    assert_eq!(s.layout, Layout::Justified);
    assert!(!s.is_idle() && s.next_thumb_job().is_none());
    settle(&mut s);
    assert_eq!(s.thumbs.get(1).unwrap().as_ref().unwrap().handle, big);
    assert_eq!((s.sizes.get(2), s.sizes.get(8)), (Some((240, 320)), None), "oriented; broken");
    let g = s.grid();
    let width = |i: usize| {
        let r = &g.rows[g.row_of(i).unwrap()];
        r.widths[i - r.items.start]
    };
    assert!(width(2) < width(1), "the portrait photo is narrower");
    assert_eq!(g.rows.iter().map(|r| r.items.len()).sum::<usize>(), 9);
    s.apply(&Action::Down);
    assert_eq!(s.index, g.rows[1].items.start);
    settle(&mut s);

    // Back to small: the big thumbnails do, nothing to decode.
    s.apply(&Action::NextLayout);
    assert_eq!(s.layout, Layout::Small);
    assert!(s.next_thumb_job().is_none() && s.is_idle());
    s.apply(&Action::SetLayout { layout: Layout::Justified });
    assert_eq!(s.layout, Layout::Justified);

    let mut h = bare_harness();
    let image = |r: Rectangle| (r.width - 12.0, r.height - 12.0);
    let cell = image(h.bounds("selected").unwrap());
    h.keys(Key::Character("z".into()), 1);
    assert_eq!(image(h.bounds("selected").unwrap()), (2.0 * cell.0, 2.0 * cell.1));
    h.keys(Key::Character("z".into()), 1);
    // Once sizes are probed, the selected photo (4:3) and, two to the right, the portrait
    // one are shown at their aspect ratios.
    // No cell padding: the cell is the photo.
    let aspect = |h: &mut Harness<_>| {
        let r = h.bounds("selected").unwrap();
        r.width / r.height
    };
    h.until("sizes probed", |h| (aspect(h) - 4.0 / 3.0).abs() < 0.01);
    assert_eq!(h.bounds("selected").unwrap().position(), Point::new(4.0, 4.0));
    h.keys(Key::Named(Named::ArrowRight), 2);
    assert!((aspect(&mut h) - 3.0 / 4.0).abs() < 0.01);
    h.keys(Key::Character("z".into()), 1);
    h.keys(Key::Named(Named::ArrowLeft), 2);
    assert_eq!(image(h.bounds("selected").unwrap()), cell);
}

#[test]
fn layout_and_names_remembered_in_settings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let with_settings = || {
        let cfg = Config {
            folder: Some(fixtures()),
            max_side: TEST_MAX_SIDE,
            transition: Duration::ZERO,
            script: None,
            settings: Some(path.clone()),
        };
        Harness::new(app::application(cfg))
    };
    let mut h = with_settings();
    let small = h.bounds("selected").unwrap();
    assert!(!path.exists(), "nothing written until something changes");
    h.keys(Key::Character("z".into()), 1);
    h.keys(Key::Character("n".into()), 1);
    assert_eq!(Settings::load(&path), Settings { layout: Layout::Big, names: true });
    // A new run starts big, with names.
    let h = with_settings();
    assert!(h.bounds("selected").unwrap().width > small.width);
    assert!(h.status().starts_with("img1.png · "), "{}", h.status());
}

#[test]
fn update_tilde() {
    use crate::state::tilde;
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    assert_eq!(tilde(&home.join("Pictures/x")), "~/Pictures/x");
    assert_eq!(tilde(&home), "~");
    assert_eq!(tilde(Path::new("/tmp/x")), "/tmp/x");
}

#[test]
fn question_mark_uses_the_shifted_key() {
    use crate::crop_overlay::key_message;
    use iced::keyboard::Modifiers;
    // What winit sends for Shift+/ on a US layout: `key` without Shift, `modified_key` with it.
    let (slash, question) = (Key::Character("/".into()), Key::Character("?".into()));
    let msg = key_message(&slash, &question, Modifiers::SHIFT, Modes::default());
    assert!(matches!(msg, Some(Message::Action(Action::ToggleHelp))), "{msg:?}");
    assert!(key_message(&slash, &slash, Modifiers::empty(), Modes::default()).is_none());
}

#[test]
fn vim_keys() {
    let mut h = harness();
    h.until("img1 shown", |h| h.bounds("photo").is_some());
    h.keys(Key::Character("l".into()), 2);
    assert_eq!(h.status(), "img3-exif6.jpg · 3 / 9");
    h.keys(Key::Character("j".into()), 1);
    h.keys(Key::Character("h".into()), 1);
    h.keys(Key::Character("k".into()), 1);
    assert!(h.status().ends_with("2 / 9"), "{}", h.status());
    h.keys(Key::Character("G".into()), 1);
    assert!(h.status().ends_with("9 / 9"), "{}", h.status());
    h.keys(Key::Character("g".into()), 1);
    assert!(h.status().ends_with("1 / 9"), "{}", h.status());
    h.keys(Key::Named(Named::End), 1);
    assert!(h.status().ends_with("9 / 9"), "{}", h.status());
    h.keys(Key::Named(Named::Home), 1);
    assert!(h.status().ends_with("1 / 9"), "{}", h.status());
}

#[test]
fn shifted_g_is_last() {
    use crate::crop_overlay::key_message;
    use iced::keyboard::Modifiers;
    let (g, big_g) = (Key::Character("g".into()), Key::Character("G".into()));
    let msg = |k, m, mods| key_message(k, m, mods, Modes::default());
    assert!(matches!(msg(&g, &big_g, Modifiers::SHIFT), Some(Message::Action(Action::Last))));
    assert!(matches!(msg(&g, &g, Modifiers::empty()), Some(Message::Action(Action::First))));
    assert!(msg(&g, &g, Modifiers::CTRL).is_none(), "Ctrl+G is not a shortcut");
}

#[test]
fn q_does_not_quit() {
    use crate::crop_overlay::key_message;
    let q = Key::Character("q".into());
    assert!(key_message(&q, &q, iced::keyboard::Modifiers::empty(), Modes::default()).is_none());
}

// ---------------------------------------------------------------------------------------
// Edits: apply, undo, save (on the temp copy of the fixtures)

fn probe_size(p: &Path) -> (u32, u32) {
    let p = photo_cruiser_core::probe(p).unwrap();
    (p.width, p.height)
}

fn crop_ops(x: f32, y: f32, w: f32, h: f32) -> Vec<photo_cruiser_core::Op> {
    vec![photo_cruiser_core::Op::Crop(Rect::new(x, y, w, h))]
}

#[test]
fn update_apply_save_undo_revert() {
    let mut s = opened(&fixtures());
    s.apply(&Action::ShowPhoto { index: Some(1) }); // img2.jpg, 320×240
    settle(&mut s);
    let photo = s.current_path().unwrap().to_path_buf();
    let input = std::fs::read(&photo).unwrap();
    s.apply(&Action::ToggleCrop);
    s.apply(&Action::SetCrop(Rect::new(20.0, 10.0, 200.0, 150.0)));
    assert!(s.stack().is_empty(), "a selection is a draft");
    s.apply(&Action::ApplyCrop);
    assert_eq!((s.mode, s.stack().ops, s.dirty()), (Mode::Browse, crop_ops(20.0, 10.0, 200.0, 150.0), true));
    settle(&mut s);
    assert_eq!(s.picture().unwrap().source, (200, 150), "the cropped photo shows");
    let sidecar = photo.parent().unwrap().join(".photo-cruiser/edits/img2.jpg.json");
    assert!(sidecar.exists(), "edits written at once");
    assert_eq!(probe_size(&photo), (320, 240), "not saved while on the photo");

    // Crop mode shows the whole original, the crop selected.
    s.apply(&Action::ToggleCrop);
    assert_eq!(s.crop.selection, Some(Rect::new(20.0, 10.0, 200.0, 150.0)));
    settle(&mut s);
    assert_eq!(s.picture().unwrap().source, (320, 240));
    s.apply(&Action::ExitCrop);

    // Leaving saves.
    s.apply(&Action::Next);
    assert_eq!(s.saving(), [photo.as_path()]);
    assert!(s.status().ends_with("— saving…"), "{}", s.status());
    settle(&mut s);
    assert_eq!(probe_size(&photo), (200, 150));
    s.apply(&Action::Prev);
    settle(&mut s);
    let v = serde_json::to_value(s.dump(None, None)).unwrap();
    assert_eq!((v["dirty"].as_bool(), v["edits"][0]["w"].as_f64()), (Some(false), Some(200.0)));
    assert_eq!(v["saved"][0]["w"].as_f64(), Some(200.0));
    assert!(v["backup"].as_str().unwrap().ends_with(".photo-cruiser/originals/img2.jpg"));
    assert_eq!(s.picture().unwrap().source, (200, 150), "rendered from the backup, cropped once");

    // Undo works across the save; redo.
    s.apply(&Action::Undo);
    assert_eq!((s.stack().ops.len(), s.dirty()), (0, true));
    s.apply(&Action::Redo);
    assert_eq!((s.stack().ops.len(), s.dirty()), (1, false));

    // Revert: the original, byte for byte, at once.
    s.apply(&Action::Revert);
    settle(&mut s);
    assert_eq!(std::fs::read(&photo).unwrap(), input);
    assert!(!s.dirty() && s.stack().is_empty() && !sidecar.exists());
    assert_eq!(s.picture().unwrap().source, (320, 240));
    assert!(s.errors.is_empty(), "{:?}", s.errors);
}

#[test]
fn update_draft_applied_on_leaving_dropped_by_esc() {
    let mut s = opened(&fixtures());
    s.apply(&Action::ShowPhoto { index: Some(1) });
    settle(&mut s);
    s.apply(&Action::SetCrop(Rect::new(0.0, 0.0, 100.0, 100.0)));
    s.apply(&Action::ExitCrop);
    assert!(s.stack().is_empty() && !s.dirty(), "Esc drops the draft");
    // Draw and press →: applied and saved, crop mode stays for the next photo.
    s.apply(&Action::SetCrop(Rect::new(0.0, 0.0, 100.0, 100.0)));
    let photo = s.current_path().unwrap().to_path_buf();
    s.apply(&Action::Next);
    assert_eq!((s.mode, s.crop.selection), (Mode::Crop, None));
    settle(&mut s);
    assert_eq!(probe_size(&photo), (100, 100));
    // Back in crop mode on it: its crop is the selection; Esc keeps the applied crop.
    s.apply(&Action::Prev);
    assert_eq!(s.crop.selection, Some(Rect::new(0.0, 0.0, 100.0, 100.0)));
    s.apply(&Action::ExitCrop);
    assert_eq!(s.stack().ops, crop_ops(0.0, 0.0, 100.0, 100.0));
}

#[test]
fn update_quit_waits_for_saves() {
    let mut s = opened(&fixtures());
    s.apply(&Action::ShowPhoto { index: Some(1) });
    settle(&mut s);
    s.apply(&Action::SetCrop(Rect::new(0.0, 0.0, 100.0, 100.0)));
    assert_eq!(s.apply(&Action::Quit), Effect::None, "saves first");
    assert!(s.quitting);
    assert_eq!(s.status(), "Saving 1 photo…");
    while let Some(io) = s.next_io() {
        s.finish_io(io_job(io));
    }
    assert!(s.io_idle());
    assert_eq!(probe_size(&fixtures().join("img2.jpg")), (100, 100));
    // Nothing to save: quits at once.
    assert_eq!(s.apply(&Action::Quit), Effect::Quit);
}

#[test]
fn update_saves_edits_left_from_a_crash() {
    let photo = fixtures().join("img2.jpg");
    let stack = photo_cruiser_core::EditStack::default()
        .with(photo_cruiser_core::Op::Crop(Rect::new(0.0, 0.0, 64.0, 48.0)));
    photo_cruiser_core::store::store_stack(&photo, &stack).unwrap();
    let mut s = State::new(TEST_MAX_SIDE, Duration::ZERO);
    s.apply(&Action::Open { path: fixtures() });
    assert_eq!(s.saving(), [photo.as_path()]);
    assert!(s.status().ends_with("— saving 1 photo edited last time"), "{}", s.status());
    settle(&mut s);
    assert_eq!(probe_size(&photo), (64, 48));
    assert!(!s.status().contains("saving"), "{}", s.status());
}

#[test]
fn update_failed_save_stays_dirty() {
    let mut s = opened(&fixtures().join("save")); // anim.gif first
    s.apply(&Action::ShowPhoto { index: Some(0) });
    settle(&mut s);
    assert_eq!(s.current_name(), Some("anim.gif"));
    s.apply(&Action::SetCrop(Rect::new(0.0, 0.0, 10.0, 10.0)));
    s.apply(&Action::Save);
    settle(&mut s);
    assert!(s.dirty());
    assert!(s.status().contains("— not saved: "), "{}", s.status());
    assert!(s.errors.last().unwrap().contains("animated images can't be edited"), "{:?}", s.errors);
    assert!(s.save_failed.contains_key(s.current_path().unwrap()));
}

#[test]
fn update_converted_photo_keeps_its_place() {
    let mut s = opened(&fixtures());
    s.apply(&Action::ShowPhoto { index: Some(4) }); // img5.HEIC
    settle(&mut s);
    s.apply(&Action::SetCrop(Rect::new(0.0, 0.0, 100.0, 100.0)));
    s.apply(&Action::Save);
    settle(&mut s);
    assert_eq!((s.index, s.current_name()), (4, Some("img5.jpg")));
    assert!(!s.dirty());
    assert_eq!(s.picture().unwrap().source, (100, 100));
    s.apply(&Action::Revert);
    settle(&mut s);
    assert_eq!(s.current_name(), Some("img5.HEIC"));
    assert!(s.errors.is_empty(), "{:?}", s.errors);
}

#[test]
fn edit_keys() {
    use crate::crop_overlay::key_message;
    use iced::keyboard::Modifiers;
    let z = Key::Character("z".into());
    let crop = Modes { crop: true, ..Modes::default() };
    let action = |k: &Key, mods, m| match key_message(k, k, mods, m) {
        Some(Message::Action(a)) => a,
        other => panic!("{other:?}"),
    };
    assert_eq!(action(&z, Modifiers::CTRL, Modes::default()), Action::Undo);
    assert_eq!(action(&z, Modifiers::CTRL | Modifiers::SHIFT, Modes::default()), Action::Redo);
    assert_eq!(action(&Key::Character("y".into()), Modifiers::CTRL, Modes::default()), Action::Redo);
    assert_eq!(action(&Key::Character("s".into()), Modifiers::CTRL, Modes::default()), Action::Save);
    assert_eq!(action(&Key::Named(Named::Enter), Modifiers::empty(), crop), Action::ApplyCrop);
    assert_eq!(action(&z, Modifiers::empty(), Modes::default()), Action::NextLayout, "plain Z unchanged");
}

#[test]
fn enter_applies_arrow_saves_failed_dot() {
    let mut h = harness(); // img1.png
    h.keys(Key::Named(Named::ArrowRight), 1); // img2.jpg
    h.until("img2", |h| h.status().starts_with("img2.jpg") && h.bounds("photo").is_some());
    h.keys(Key::Character("c".into()), 1);
    assert!(h.bounds("apply").is_some(), "Apply button in crop mode");
    let fit = h.fit((320, 240));
    h.drag(Point::new(fit.x + 10.0, fit.y + 10.0), Point::new(fit.x + fit.w / 2.0, fit.y + fit.h / 2.0));
    h.keys(Key::Named(Named::Enter), 1);
    assert!(!h.status().contains("crop"), "applied, crop mode left: {}", h.status());
    h.until("cropped photo shown", |h| h.bounds("photo").is_some() && h.bounds("apply").is_none());
    h.keys(Key::Named(Named::ArrowRight), 1);
    let photo = fixtures().join("img2.jpg");
    h.until("saved", |h| !h.status().contains("saving") && probe_size(&photo).0 < 320);
}

#[test]
fn failed_save_dot_in_grid() {
    let cfg = Config {
        folder: Some(fixtures().join("save")), // anim.gif first
        max_side: TEST_MAX_SIDE,
        transition: Duration::ZERO,
        script: None,
        settings: None,
    };
    let mut h = Harness::new(app::application(cfg));
    h.keys(Key::Named(Named::Enter), 1);
    h.until("anim.gif", |h| h.bounds("photo").is_some());
    h.keys(Key::Character("c".into()), 1);
    let fit = h.fit((64, 48));
    h.drag(Point::new(fit.x + 10.0, fit.y + 10.0), Point::new(fit.x + fit.w / 2.0, fit.y + fit.h / 2.0));
    h.keys(Key::Named(Named::Escape), 1); // drops the draft, leaves crop mode
    assert!(h.bounds("failed").is_none());
    h.keys(Key::Character("c".into()), 1);
    h.drag(Point::new(fit.x + 10.0, fit.y + 10.0), Point::new(fit.x + fit.w / 2.0, fit.y + fit.h / 2.0));
    h.emu.update(&h.prog, Action::Save.into()); // Ctrl+S (keys with modifiers: see `edit_keys`)
    h.until("save failed", |h| h.status().contains("not saved: "));
    h.keys(Key::Named(Named::Escape), 1); // back to the grid (Save left crop mode)
    h.until("grid with a dot", |h| h.bounds("failed").is_some());
}

fn palette_labels(s: &State) -> Vec<String> {
    crate::palette::filtered(s).into_iter().map(|c| c.label).collect()
}

#[test]
fn update_palette_lists_what_can_be_done_sorted() {
    let mut s = opened(&fixtures()); // grid, img1.png
    s.apply(&Action::TogglePalette);
    let grid = palette_labels(&s);
    assert!(grid.is_sorted_by_key(|l| l.to_lowercase()), "alphabetical: {grid:?}");
    for l in ["Open photo", "Next photo", "Last photo", "Crop", "Big thumbnails", "Open folder…", "Quit"] {
        assert!(grid.contains(&l.into()), "{l} in {grid:?}");
    }
    for l in ["All photos", "Previous photo", "Small thumbnails", "Undo", "Save", "Apply crop"] {
        assert!(!grid.contains(&l.into()), "{l} not in {grid:?}");
    }

    // Typing filters (every word, any case) and focuses the first row; ↓/↑ wrap.
    s.apply(&Action::PaletteQuery { query: "PHO".into() });
    let want = ["About Photo Cruiser", "Last photo", "Next photo", "Open photo"];
    assert_eq!(palette_labels(&s), want.map(String::from), "no First / Previous on the first photo");
    s.apply(&Action::PaletteQuery { query: "photo n".into() });
    assert_eq!(palette_labels(&s), ["Next photo", "Open photo"].map(String::from));
    s.apply(&Action::PalettePrev);
    assert_eq!(s.palette.as_ref().unwrap().focus, 1);
    s.apply(&Action::PaletteNext);
    assert_eq!(s.palette.as_ref().unwrap().focus, 0);
    s.apply(&Action::PaletteQuery { query: "zzz".into() });
    assert!(palette_labels(&s).is_empty());
    s.apply(&Action::PaletteNext);
    assert_eq!(s.palette.as_ref().unwrap().focus, 0, "nothing to move to");

    // Any other action closes it.
    s.apply(&Action::Next);
    assert!(s.palette.is_none());

    // Photo view, cropping: the crop's actions; after applying: undo, save, reset.
    s.apply(&Action::ShowPhoto { index: Some(1) }); // img2.jpg
    settle(&mut s);
    s.apply(&Action::SetCrop(Rect::new(20.0, 10.0, 200.0, 150.0)));
    s.apply(&Action::TogglePalette);
    let crop = palette_labels(&s);
    for l in ["All photos", "Apply crop", "Cancel crop", "Aspect ratio free", "Aspect ratio 1:1", "Save"] {
        assert!(crop.contains(&l.into()), "{l} in {crop:?}");
    }
    assert!(!crop.contains(&"Aspect ratio original".into()), "the aspect in use");
    assert!(!crop.contains(&"Crop".into()) && !crop.contains(&"Open photo".into()));
    s.apply(&Action::ApplyCrop);
    s.apply(&Action::TogglePalette);
    let edited = palette_labels(&s);
    for l in ["Crop", "Undo", "Save", "Reset edits", "Revert to original"] {
        assert!(edited.contains(&l.into()), "{l} in {edited:?}");
    }
    assert!(!edited.contains(&"Redo".into()));
    s.apply(&Action::TogglePalette);
    assert!(s.palette.is_none(), "Ctrl+K again closes it");
}

#[test]
fn palette_keys() {
    use crate::crop_overlay::key_message;
    use iced::keyboard::Modifiers;
    let k = Key::Character("k".into());
    let m = key_message(&k, &k, Modifiers::CTRL, Modes::default());
    assert!(matches!(m, Some(Message::Action(Action::TogglePalette))), "{m:?}");
    let open = Modes { palette: true, ..Modes::default() };
    assert!(
        key_message(&Key::Character("c".into()), &k, Modifiers::empty(), open).is_none(),
        "keys go to the field"
    );
}

#[test]
fn palette_type_move_and_pick() {
    let mut h = harness(); // img1.png
    h.until("img1 shown", |h| h.bounds("photo").is_some());
    h.emu.update(&h.prog, Action::TogglePalette.into());
    assert!(h.bounds("palette").is_some() && h.has_text("Keyboard shortcuts"));
    // The field's focus comes as a task.
    std::thread::sleep(Duration::from_millis(50));
    h.pump();

    // Typed into the field; ↓ moves the focus, Enter picks it.
    h.run(Interaction::Keyboard(Keyboard::Typewrite("photo".into())));
    assert!(!h.has_text("Keyboard shortcuts") && h.has_text("All photos"), "filtered");
    assert!(h.bounds("palette-focus").unwrap().contains(h.text_center("About Photo Cruiser")));
    h.keys(Key::Named(Named::ArrowDown), 2);
    assert!(h.bounds("palette-focus").unwrap().contains(h.text_center("Last photo")));
    h.keys(Key::Named(Named::Enter), 1);
    assert!(h.bounds("palette").is_none());
    h.until("last photo", |h| h.status().contains("9 / 9"));

    // Esc closes it; so does a click outside; a click on a row picks it.
    h.emu.update(&h.prog, Action::TogglePalette.into());
    h.keys(Key::Named(Named::Escape), 1);
    assert!(h.bounds("palette").is_none() && h.bounds("grid").is_none(), "Esc only closes the palette");
    h.emu.update(&h.prog, Action::TogglePalette.into());
    let target = Some(Target::Point(Point::new(5.0, 700.0)));
    h.run(Interaction::Mouse(Mouse::Click { button: Button::Left, target }));
    assert!(h.bounds("palette").is_none(), "a click outside closes it");
    h.emu.update(&h.prog, Action::TogglePalette.into());
    let target = Some(Target::Text("Keyboard shortcuts".into()));
    h.run(Interaction::Mouse(Mouse::Click { button: Button::Left, target }));
    assert!(h.bounds("palette").is_none() && h.bounds("help").is_some());
}
