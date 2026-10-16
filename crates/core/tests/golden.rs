//! Golden images: `render` output compared to committed references in `tests/golden/`.
//!
//! Tolerance: dimensions must match exactly; every channel of every pixel may differ by at
//! most `MAX_DIFF` (decoder versions round differently), and the mean absolute channel
//! difference must stay under `MAX_MEAN`. On failure the actual image is written to
//! `$PC_ARTIFACTS/golden/` (default `/tmp/photo-cruiser/golden/`) next to a summary.
//!
//! Regenerate the references only on purpose, after checking the new output:
//! `just bless-golden` (sets `PC_BLESS_GOLDEN=1`). A plain test run never writes them.

use image::RgbaImage;
use photo_cruiser_core::render::{RenderOpts, render};
use photo_cruiser_core::{EditStack, Op, Rect, Tool, decode};
use std::path::{Path, PathBuf};

const MAX_DIFF: u8 = 4;
const MAX_MEAN: f64 = 0.5;

struct Case {
    name: &'static str,
    fixture: &'static str,
    crop: Option<Rect>,
    max_side: Option<u32>,
    /// A tool at its opening value for the cropped photo.
    adjust: Option<Tool>,
}

const CASES: &[Case] = &[
    Case { name: "png-full", fixture: "img1.png", crop: None, max_side: None, adjust: None },
    Case {
        name: "jpeg-crop",
        fixture: "img2.jpg",
        crop: Some(Rect::new(40.0, 30.0, 200.0, 150.0)),
        max_side: None,
        adjust: None,
    },
    Case {
        name: "jpeg-exif6-crop",
        fixture: "img3-exif6.jpg",
        crop: Some(Rect::new(20.0, 100.0, 180.0, 120.0)),
        max_side: None,
        adjust: None,
    },
    Case {
        name: "heic-crop-clamped-max-side",
        fixture: "img5.HEIC",
        crop: Some(Rect::new(200.0, -40.0, 300.0, 200.0)),
        max_side: Some(64),
        adjust: None,
    },
    Case {
        name: "jpeg-crop-auto-exposure",
        fixture: "img2.jpg",
        crop: Some(Rect::new(40.0, 30.0, 200.0, 150.0)),
        max_side: None,
        adjust: Some(Tool::Exposure),
    },
    Case { name: "avif-max-side", fixture: "img4.AVIF", crop: None, max_side: Some(100), adjust: None },
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn artifacts() -> PathBuf {
    std::env::var_os("PC_ARTIFACTS")
        .map_or_else(|| PathBuf::from("/tmp/photo-cruiser"), PathBuf::from)
        .join("golden")
}

fn render_case(c: &Case) -> RgbaImage {
    let src = decode(&root().join("../../fixtures").join(c.fixture)).unwrap();
    let mut edits = c.crop.map_or_else(EditStack::default, |r| EditStack::default().with(Op::Crop(r)));
    if let Some(tool) = c.adjust {
        let value = tool.opening(&render(&src, &edits, RenderOpts::default()).unwrap());
        edits.set_adjust(tool, Some(value));
    }
    render(&src, &edits, RenderOpts { max_side: c.max_side }).unwrap()
}

/// `None` if `actual` is within tolerance of `expected`, else a human-readable summary.
fn compare(expected: &RgbaImage, actual: &RgbaImage) -> Option<String> {
    if expected.dimensions() != actual.dimensions() {
        return Some(format!("size {:?}, expected {:?}", actual.dimensions(), expected.dimensions()));
    }
    let (mut worst, mut worst_at, mut sum, mut over) = (0u8, (0, 0), 0u64, 0usize);
    for (x, y, e) in expected.enumerate_pixels() {
        let a = actual.get_pixel(x, y);
        let d = e.0.iter().zip(a.0).map(|(e, a)| e.abs_diff(a)).max().unwrap();
        sum += e.0.iter().zip(a.0).map(|(e, a)| u64::from(e.abs_diff(a))).sum::<u64>();
        over += usize::from(d > MAX_DIFF);
        if d > worst {
            (worst, worst_at) = (d, (x, y));
        }
    }
    let mean = sum as f64 / (expected.len() as f64);
    (worst > MAX_DIFF || mean > MAX_MEAN).then(|| {
        let (x, y) = worst_at;
        format!(
            "max channel diff {worst} at ({x},{y}): expected {:?}, got {:?}; mean diff {mean:.3} \
             (limits {MAX_DIFF} / {MAX_MEAN}); {over} of {} pixels over the limit",
            expected.get_pixel(x, y).0,
            actual.get_pixel(x, y).0,
            expected.width() * expected.height(),
        )
    })
}

#[test]
fn render_matches_golden_images() {
    let bless = std::env::var_os("PC_BLESS_GOLDEN").is_some();
    let dir = root().join("tests/golden");
    let mut failures = Vec::new();
    for c in CASES {
        let actual = render_case(c);
        let reference = dir.join(format!("{}.png", c.name));
        if bless {
            std::fs::create_dir_all(&dir).unwrap();
            actual.save(&reference).unwrap();
            eprintln!("blessed {}", reference.display());
            continue;
        }
        let expected = image::open(&reference)
            .unwrap_or_else(|e| panic!("{}: {e} (create it with `just bless-golden`)", reference.display()))
            .into_rgba8();
        if let Some(summary) = compare(&expected, &actual) {
            std::fs::create_dir_all(artifacts()).unwrap();
            let out = artifacts().join(format!("{}.actual.png", c.name));
            actual.save(&out).unwrap();
            failures.push(format!(
                "{}: {summary}\n  expected {}\n  actual   {}",
                c.name,
                reference.display(),
                out.display()
            ));
        }
    }
    assert!(failures.is_empty(), "{} golden mismatch(es):\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn compare_reports_differences() {
    let a = RgbaImage::from_pixel(4, 4, image::Rgba([100, 100, 100, 255]));
    assert_eq!(compare(&a, &a), None);
    let mut b = a.clone();
    b.put_pixel(1, 2, image::Rgba([100, 100, 100 + MAX_DIFF, 255]));
    assert_eq!(compare(&a, &b), None, "within tolerance");
    b.put_pixel(3, 0, image::Rgba([120, 100, 100, 255]));
    let msg = compare(&a, &b).unwrap();
    assert!(msg.contains("max channel diff 20 at (3,0)") && msg.contains("1 of 16 pixels"), "{msg}");
    assert!(compare(&a, &RgbaImage::new(4, 5)).unwrap().contains("size (4, 5)"));
}
