//! Tests.
//!
//! - `update_*`: the [`State`] model driven by [`Action`]s directly, decodes run
//!   synchronously. No iced runtime.
//! - The rest: headless end-to-end tests with `iced_test`. The real program
//!   (boot/update/view/title/theme) runs in an `Emulator`, which executes Tasks on the
//!   app's executor, so decoding is real, on threads. Mouse input (click "Crop" by text,
//!   drags) goes through `Emulator::run` with `Instruction`s: raw iced mouse events
//!   dispatched through the real widget tree. Keys: iced_test has no arrow keys in its
//!   instructions, so they are tapped on a `Simulator` built on the emulator's current
//!   view; the messages the canvas publishes are fed to `Emulator::update`.
//!   Not covered: winit (OS events → iced events) and the window surface — that is what
//!   script mode (`just shot`) is for.

use crate::action::Action;
use crate::app::{self, Config};
use crate::message::Message;
use crate::state::{Effect, Mode, State, decode_picture};
use iced::futures::channel::mpsc;
use iced::keyboard::{Key, key::Named};
use iced::mouse::Button;
use iced::{Point, Program, Rectangle, Size};
use iced_test::emulator::{self, Emulator};
use iced_test::instruction::{Interaction, Mouse, Target};
use iced_test::selector::{self, Candidate};
use iced_test::{Instruction, Simulator};
use photo_cruiser_core::geom::{PixelRect, fit_rect};
use photo_cruiser_core::{Folder, Rect};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const SIZE: Size = Size::new(1000.0, 750.0);
/// The headless wgpu backend caps textures at 2048 px.
const TEST_MAX_SIDE: u32 = 2048;
const TIMEOUT: Duration = Duration::from_secs(20);

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").canonicalize().unwrap()
}

fn names() -> Vec<String> {
    Folder::open(fixtures()).unwrap().entries.into_iter().map(|e| e.name).collect()
}

// ---------------------------------------------------------------------------------------
// Pure update tests

/// Run decodes synchronously until the loader is idle.
fn settle(s: &mut State) {
    while let Some((job, path)) = s.next_job() {
        let r = decode_picture(&path, s.max_side);
        s.finish(job, r);
    }
    assert!(s.is_idle());
}

fn opened(path: &Path) -> State {
    let mut s = State::new(TEST_MAX_SIDE);
    assert_eq!(s.apply(&Action::Open { path: path.into() }), Effect::None);
    settle(&mut s);
    s
}

#[test]
fn update_browse_and_goto_bounds() {
    let mut s = opened(&fixtures());
    assert_eq!((s.len(), s.index, s.position()), (9, 0, "1 / 9".into()));
    assert_eq!(s.status(), "img1.png — 1 / 9");
    assert_eq!(s.title(), "Photo Cruiser — fixtures");
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
fn update_set_crop_clamps_and_modes() {
    let mut s = opened(&fixtures()); // img1.png, 320×240
    s.apply(&Action::SetCrop(Rect::new(300.0, 200.0, 100.0, 100.0)));
    assert_eq!(s.mode, Mode::Crop, "set_crop turns crop mode on");
    assert_eq!(s.crop_pixels(), Some(PixelRect { x: 300, y: 200, w: 20, h: 40 }));
    assert_eq!(s.status(), "img1.png — 1 / 9 — crop 20×40 @ 300,200");
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
fn update_open_resets() {
    let mut s = opened(&fixtures());
    s.apply(&Action::Goto { index: 3 });
    s.apply(&Action::SetCrop(Rect::new(1.0, 1.0, 10.0, 10.0)));
    s.apply(&Action::Open { path: fixtures().join("subdir") });
    settle(&mut s);
    assert_eq!((s.len(), s.index, s.mode, s.crop_pixels()), (1, 0, Mode::Browse, None));
    assert_eq!(s.title(), "Photo Cruiser — subdir");
    assert_eq!(s.status(), "nested.jpg — 1 / 1");

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

fn harness() -> Harness<impl Program<Message = Message, State = app::App>> {
    let cfg = Config { folder: Some(fixtures()), max_side: TEST_MAX_SIDE, script: None };
    Harness::new(app::application(cfg))
}

impl<P: Program<Message = Message> + 'static> Harness<P> {
    fn new(prog: P) -> Self {
        let (tx, rx) = mpsc::channel(100);
        let emu = Emulator::new(tx, &prog, emulator::Mode::Zen, SIZE);
        let mut h = Harness { prog, emu, rx };
        h.until_ready(); // boot task: decode current and neighbours
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
        let want = format!("{name} — {} / 9", i + 1);
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
    assert_eq!(Program::title(&h.prog, &state, window), "Photo Cruiser — fixtures");
}

#[test]
fn clamping_at_both_ends() {
    let mut h = harness();
    h.until("first", |h| h.status() == "img1.png — 1 / 9" && h.bounds("photo").is_some());
    h.keys(Key::Named(Named::ArrowLeft), 3);
    assert_eq!(h.status(), "img1.png — 1 / 9");
    // 30 × → in one burst (like a held key): clamps at 9, stale decodes are dropped.
    h.keys(Key::Named(Named::ArrowRight), 30);
    assert_eq!(h.status(), "img12-broken.jpg — 9 / 9");
    h.until("broken shown", |h| h.has_text("decode failed"));
    h.keys(Key::Named(Named::ArrowLeft), 1);
    h.until("img11", |h| h.status() == "img11.bmp — 8 / 9" && h.bounds("photo").is_some());
}

#[test]
fn crop_draw_move_resize_dim_and_esc() {
    let mut h = harness();
    h.until("img1 shown", |h| h.bounds("photo").is_some());
    // Toolbar button, clicked by its text.
    let target = Some(Target::Text("Crop".into()));
    h.run(Interaction::Mouse(Mouse::Click { button: Button::Left, target }));
    assert!(h.status().contains("crop: drag"), "{}", h.status());
    let before = h.snapshot("nocrop");

    let (w, hh) = (320.0, 240.0); // img1.png
    let fit = h.fit((320, 240));
    let at = |fx: f32, fy: f32| Point::new(fit.x + fx * fit.w, fit.y + fy * fit.h);
    h.drag(at(0.2, 0.2), at(0.6, 0.6)); // draw 0.2..0.6
    assert_close(parse_crop(&h.status()), [0.4 * w, 0.4 * hh, 0.2 * w, 0.2 * hh]);
    h.drag(at(0.4, 0.4), at(0.5, 0.45)); // move by +0.1, +0.05
    assert_close(parse_crop(&h.status()), [0.4 * w, 0.4 * hh, 0.3 * w, 0.25 * hh]);
    h.drag(at(0.7, 0.65), at(0.8, 0.85)); // resize the bottom-right corner
    assert_close(parse_crop(&h.status()), [0.5 * w, 0.6 * hh, 0.3 * w, 0.25 * hh]);

    // 50% dim outside the selection (sRGB blending via `web-colors`), untouched inside.
    let after = h.snapshot("crop");
    for (fx, fy) in [(0.1, 0.1), (0.9, 0.5), (0.5, 0.95)] {
        let ratio = mean(&after, at(fx, fy)) / mean(&before, at(fx, fy));
        assert!((ratio - 0.5).abs() < 0.04, "outside at ({fx},{fy}) not dimmed to ~50%: {ratio}");
    }
    let inside = mean(&after, at(0.55, 0.7)) / mean(&before, at(0.55, 0.7));
    assert!((inside - 1.0).abs() < 0.02, "inside changed: {inside}");

    // Esc leaves crop mode.
    h.keys(Key::Named(Named::Escape), 1);
    assert_eq!(h.status(), "img1.png — 1 / 9");
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
    // Arrow keys drop the selection.
    h.keys(Key::Named(Named::ArrowLeft), 1);
    assert!(h.status().ends_with("(Esc to leave)"), "{}", h.status());
}
