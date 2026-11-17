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
    /// A tool at its opening value for the cropped photo (the whole photo for Straighten).
    adjust: Option<Tool>,
}

/// The page in `adjust/document.jpg`, without the desk.
const PAGE: Rect = Rect::new(60.0, 0.0, 260.0, 240.0);

/// The house in `straighten/horizon.jpg`, as it is before straightening.
const HOUSE: Rect = Rect::new(170.0, 10.0, 130.0, 150.0);

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
    Case {
        name: "document-crop-levels",
        fixture: "adjust/document.jpg",
        crop: Some(PAGE),
        max_side: None,
        adjust: Some(Tool::Levels),
    },
    Case {
        name: "jpeg-exif6-colour",
        fixture: "img3-exif6.jpg",
        crop: None,
        max_side: None,
        adjust: Some(Tool::Colour),
    },
    Case {
        name: "warm-cast-warmth",
        fixture: "adjust/warm-cast.jpg",
        crop: None,
        max_side: None,
        adjust: Some(Tool::Warmth),
    },
    Case {
        name: "window-shadows-highlights",
        fixture: "adjust/window.jpg",
        crop: None,
        max_side: None,
        adjust: Some(Tool::ShadowsHighlights),
    },
    Case {
        name: "window-local-contrast",
        fixture: "adjust/window.jpg",
        crop: None,
        max_side: None,
        adjust: Some(Tool::LocalContrast),
    },
    Case {
        name: "window-contrast",
        fixture: "adjust/window.jpg",
        crop: None,
        max_side: None,
        adjust: Some(Tool::Contrast),
    },
    Case {
        name: "window-sharpen",
        fixture: "adjust/window.jpg",
        crop: None,
        max_side: None,
        adjust: Some(Tool::Sharpen),
    },
    Case {
        name: "woods-haze-dehaze",
        fixture: "adjust/woods-haze.jpg",
        crop: None,
        max_side: None,
        adjust: Some(Tool::Dehaze),
    },
    Case {
        name: "jpeg-crop-vignette",
        fixture: "img2.jpg",
        crop: Some(Rect::new(40.0, 30.0, 200.0, 150.0)),
        max_side: None,
        adjust: Some(Tool::Vignette),
    },
    Case {
        name: "horizon-straighten",
        fixture: "straighten/horizon.jpg",
        crop: None,
        max_side: None,
        adjust: Some(Tool::Straighten),
    },
    Case {
        name: "horizon-straighten-crop",
        fixture: "straighten/horizon.jpg",
        crop: Some(HOUSE),
        max_side: None,
        adjust: Some(Tool::Straighten),
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
        let value = tool.opening(&render(&src, &edits.geometry_for(tool), RenderOpts::default()).unwrap());
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

/// Auto levels on a photo of a document (`adjust/document.jpg`): yellowish paper, grey-brown
/// text, a shadow in a corner, a dark desk left of the page.
#[test]
fn levels_on_a_document() {
    let src = decode(&root().join("../../fixtures/adjust/document.jpg")).unwrap();
    let page = EditStack::default().with(Op::Crop(PAGE));
    let at = |stack: &EditStack, strength: f32| {
        let mut s = stack.clone();
        s.set_adjust(Tool::Levels, Some(strength));
        render(&src, &s, RenderOpts::default()).unwrap()
    };
    let original = at(&page, 0.0);
    assert_eq!(original, render(&src, &page, RenderOpts::default()).unwrap(), "strength 0: unchanged");
    // Paper and text, told apart on the original.
    let paper = |img: &RgbaImage| -> Vec<[u8; 3]> {
        img.enumerate_pixels()
            .filter(|(x, y, _)| {
                original.get_pixel(*x, *y).0[..3].iter().map(|c| u32::from(*c)).sum::<u32>() > 330
            })
            .map(|(_, _, p)| [p.0[0], p.0[1], p.0[2]])
            .collect()
    };
    let white = |img: &RgbaImage| {
        let p = paper(img);
        p.iter().filter(|c| c.iter().all(|c| *c >= 245)).count() as f32 / p.len() as f32
    };
    let (text, corner) = ((22, 27), (5, 5));
    let px = |img: &RgbaImage, (x, y): (u32, u32)| img.get_pixel(x, y).0;
    // About [85, 76, 61] and [205, 192, 150]: grey-brown on yellowish.
    assert!(px(&original, text)[..3].iter().all(|c| (55..=95).contains(c)), "{:?}", px(&original, text));
    assert!(px(&original, corner)[2] < 160 && white(&original) == 0.0);

    let one = at(&page, 1.0);
    assert!(px(&one, corner)[..3].iter().all(|c| *c >= 245), "white paper: {:?}", px(&one, corner));
    assert!(px(&one, text)[..3].iter().all(|c| *c <= 30), "dark text: {:?}", px(&one, text));
    // Strength 2 clips more: much more of the paper turns white.
    let two = at(&page, 2.0);
    assert!(
        white(&two) > 0.4 && white(&one) < 0.2,
        "white paper: {} at 1, {} at 2",
        white(&one),
        white(&two)
    );

    // Measured on the crop: on the whole photo the dark desk is the black point, so the
    // text stays grey.
    let whole = at(&EditStack::default(), 1.0);
    let on_whole = px(&whole, (text.0 + 60, text.1));
    assert!(on_whole[..3].iter().all(|c| *c >= 60), "desk as the black point: {on_whole:?}");
}

/// Warmth on `adjust/warm-cast.jpg`, a neutral scene under warm light: opening at grey-world
/// white balance makes the grey wall and floor grey again.
#[test]
fn warmth_removes_a_warm_cast() {
    let src = decode(&root().join("../../fixtures/adjust/warm-cast.jpg")).unwrap();
    let original = render(&src, &EditStack::default(), RenderOpts::default()).unwrap();
    let value = Tool::Warmth.opening(&original);
    // The cast is linear gains (1, 0.8, 0.55); the warm patches (red, skin) hold it back a little.
    assert!((-0.95..=-0.6).contains(&value), "opens cooler: {value}");
    let mut stack = EditStack::default();
    stack.set_adjust(Tool::Warmth, Some(value));
    let balanced = render(&src, &stack, RenderOpts::default()).unwrap();
    let spread =
        |p: [u8; 4]| i32::from(*p[..3].iter().max().unwrap()) - i32::from(*p[..3].iter().min().unwrap());
    // The wall above the patches, and the floor.
    for (x, y) in [(40, 10), (280, 10), (40, 200), (280, 200)] {
        let (before, after) = (original.get_pixel(x, y).0, balanced.get_pixel(x, y).0);
        assert!(spread(before) > 20, "cast at ({x},{y}): {before:?}");
        assert!(spread(after) <= 8, "grey at ({x},{y}): {after:?} (was {before:?})");
        assert!(after[2] > after[0].saturating_sub(8), "not left warm: {after:?}");
    }
}

/// Shadows & highlights on `adjust/window.jpg`, a dark room with a bright window: the room
/// lifts, the window comes down a little, and the wall next to the window doesn't glow.
#[test]
fn shadows_highlights_on_a_backlit_room() {
    let src = decode(&root().join("../../fixtures/adjust/window.jpg")).unwrap();
    let original = render(&src, &EditStack::default(), RenderOpts::default()).unwrap();
    assert_eq!(Tool::ShadowsHighlights.opening(&original), 0.45);
    let mut stack = EditStack::default();
    stack.set_adjust(Tool::ShadowsHighlights, Some(0.45));
    let out = render(&src, &stack, RenderOpts::default()).unwrap();
    let luma = |img: &RgbaImage, x: u32, y: u32| {
        let p = img.get_pixel(x, y).0;
        (u32::from(p[0]) * 2126 + u32::from(p[1]) * 7152 + u32::from(p[2]) * 722) / 10_000
    };
    let (wall, sky) = ((160, 100), (215, 50));
    assert!(luma(&out, wall.0, wall.1) > luma(&original, wall.0, wall.1) + 15, "room lifted");
    assert!(luma(&out, sky.0, sky.1) + 5 < luma(&original, sky.0, sky.1), "window pulled back");
    // The wall right next to the window is lifted like the wall far from it: no halo.
    let near: u32 = (192..198).map(|x| luma(&out, x, 100)).sum::<u32>() / 6;
    let far: u32 = (150..156).map(|x| luma(&out, x, 100)).sum::<u32>() / 6;
    assert!(near.abs_diff(far) <= 3, "wall near the window {near}, far {far}");
    // Mid-tones barely change: a grey card in the middle of a mid-grey photo.
    let mut grey = image::RgbaImage::from_pixel(64, 64, image::Rgba([119, 119, 119, 255]));
    Tool::ShadowsHighlights.apply(&mut grey, 0.45);
    assert!(grey.get_pixel(32, 32).0[..3].iter().all(|c| c.abs_diff(119) <= 1));
}

/// Local contrast at 200% on `adjust/window.jpg`: the wall's grain gets stronger, the wall
/// next to the bright window doesn't darken, and the sky around the tree crown (a soft
/// edge, about 1 EV) doesn't glow: no halos.
#[test]
fn local_contrast_on_a_backlit_room() {
    let src = decode(&root().join("../../fixtures/adjust/window.jpg")).unwrap();
    let original = render(&src, &EditStack::default(), RenderOpts::default()).unwrap();
    assert_eq!(Tool::LocalContrast.opening(&original), 1.25);
    let mut stack = EditStack::default();
    stack.set_adjust(Tool::LocalContrast, Some(2.0));
    let out = render(&src, &stack, RenderOpts::default()).unwrap();
    let green = |img: &RgbaImage, x: u32| i32::from(img.get_pixel(x, 100).0[1]);
    let spread = |img: &RgbaImage, xs: std::ops::Range<u32>| {
        let v: Vec<i32> = xs.map(|x| green(img, x)).collect();
        v.iter().max().unwrap() - v.iter().min().unwrap()
    };
    assert!(spread(&out, 140..180) > spread(&original, 140..180) + 2, "grain stronger");
    // Up to the window's edge (x 200), the wall stays within the levels of its grain far
    // from it.
    let (lo, hi) =
        (130..170).fold((i32::MAX, i32::MIN), |(lo, hi), x| (lo.min(green(&out, x)), hi.max(green(&out, x))));
    for x in 170..199 {
        assert!(
            (lo - 2..=hi + 2).contains(&green(&out, x)),
            "column {x}: {} outside {lo}..={hi}",
            green(&out, x)
        );
    }
    // The sky left and right of the crown (x 235..290 at y 90), against the original.
    for x in (225..235).chain(291..299) {
        let (now, was) = (out.get_pixel(x, 90).0[0], original.get_pixel(x, 90).0[0]);
        assert!(now <= was.saturating_add(4), "sky at {x}: {now} vs {was}");
    }
}

/// Sharpen at 1 on `adjust/window.jpg`: the dark window bar (x 248..252) against the sky
/// gets darker in the middle and the sky next to it brighter; a downscaled render, whose
/// radius shrinks with it, stays close to the full-size render downscaled.
#[test]
fn sharpen_on_a_backlit_room() {
    let src = decode(&root().join("../../fixtures/adjust/window.jpg")).unwrap();
    let original = render(&src, &EditStack::default(), RenderOpts::default()).unwrap();
    assert_eq!(Tool::Sharpen.opening(&original), 0.5);
    let mut stack = EditStack::default();
    stack.set_adjust(Tool::Sharpen, Some(1.0));
    let out = render(&src, &stack, RenderOpts::default()).unwrap();
    let red = |img: &RgbaImage, x: u32| i32::from(img.get_pixel(x, 45).0[0]);
    assert!(red(&out, 250) < red(&original, 250) - 3, "bar: {} vs {}", red(&out, 250), red(&original, 250));
    assert!(red(&out, 246) > red(&original, 246) + 3, "sky: {} vs {}", red(&out, 246), red(&original, 246));

    let small = decode::decode_max_side(&root().join("../../fixtures/adjust/window.jpg"), Some(160)).unwrap();
    assert_eq!(small.image.width(), 160);
    let preview = render(&small, &stack, RenderOpts::default()).unwrap();
    let saved = render(&src, &stack, RenderOpts { max_side: Some(160) }).unwrap();
    let mean =
        preview.pixels().zip(saved.pixels()).map(|(a, b)| f64::from(a.0[0].abs_diff(b.0[0]))).sum::<f64>()
            / f64::from(preview.width() * preview.height());
    assert!(mean < 1.5, "preview vs save: mean {mean}");
}

/// Dehaze at its opening 0.2 on `adjust/woods-haze.jpg`: every ridge comes out of the haze
/// (darker, further from the airlight), the far ridge the most by its share of haze, and
/// the sky stays. On `adjust/window.jpg`, without haze, it changes little.
#[test]
fn dehaze_on_hazy_hills_and_a_clear_room() {
    let open = |name: &str| {
        let src = decode(&root().join("../../fixtures/adjust").join(name)).unwrap();
        let original = render(&src, &EditStack::default(), RenderOpts::default()).unwrap();
        assert_eq!(Tool::Dehaze.opening(&original), 0.2);
        let mut out = original.clone();
        Tool::Dehaze.apply(&mut out, 0.2);
        (original, out)
    };
    let (original, out) = open("woods-haze.jpg");
    let g = |img: &RgbaImage, y: u32| {
        (0..20).map(|x| f64::from(img.get_pixel(150 + x, y).0[1])).sum::<f64>() / 20.0
    };
    let sky = 15;
    assert!((g(&out, sky) - g(&original, sky)).abs() < 1.0, "sky");
    // Ridges at y 95 (far), 150 (middle) and 215 (near): their distance from the sky, in
    // sRGB levels, grows by a share, the far one's most.
    let gain = |y| (g(&original, sky) - g(&out, y)) / (g(&original, sky) - g(&original, y));
    let (far, mid, near) = (gain(95), gain(150), gain(215));
    assert!(far > mid && mid > near && near > 1.02, "far {far:.3}, middle {mid:.3}, near {near:.3}");

    let (original, out) = open("window.jpg");
    let mean =
        original.pixels().zip(out.pixels()).map(|(a, b)| f64::from(a.0[1].abs_diff(b.0[1]))).sum::<f64>()
            / f64::from(original.width() * original.height());
    assert!(mean < 3.0, "a clear photo changes little: mean {mean:.2} levels");
}

/// The vignette follows the crop: the cropped photo's corners darken, as if the crop were
/// the whole photo, not the corners of the original frame.
#[test]
fn vignette_follows_the_crop() {
    let src = decode(&root().join("../../fixtures/img2.jpg")).unwrap();
    let crop = EditStack::default().with(Op::Crop(Rect::new(40.0, 30.0, 200.0, 150.0)));
    let cropped = render(&src, &crop, RenderOpts::default()).unwrap();
    assert_eq!(Tool::Vignette.opening(&cropped), -0.5);
    let mut stack = crop.clone();
    stack.set_adjust(Tool::Vignette, Some(-0.5));
    let out = render(&src, &stack, RenderOpts::default()).unwrap();
    let mut expected = cropped.clone();
    Tool::Vignette.apply(&mut expected, -0.5);
    assert_eq!(out, expected);
    let sum = |img: &RgbaImage, x: u32, y: u32| {
        img.get_pixel(x, y).0[..3].iter().map(|&c| u32::from(c)).sum::<u32>()
    };
    let (w, h) = out.dimensions();
    for (x, y) in [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)] {
        assert!(sum(&out, x, y) < sum(&cropped, x, y), "corner {x},{y} darker");
    }
    assert_eq!(out.get_pixel(w / 2, h / 2), cropped.get_pixel(w / 2, h / 2), "centre untouched");
}

/// Straighten on `straighten/horizon.jpg`, turned 3.4° clockwise: it opens within 0.5° of
/// −3.4°, and then the horizon is level: as high at the left as at the right.
#[test]
fn straighten_levels_a_tilted_horizon() {
    let src = decode(&root().join("../../fixtures/straighten/horizon.jpg")).unwrap();
    let mut stack = EditStack::default();
    let angle = photo_cruiser_core::adjust::opening(&src, &stack, Tool::Straighten).unwrap();
    assert!((angle + 3.4).abs() <= 0.5, "opens at {angle}");
    // A crop made before keeps the same opening: it is measured on the whole photo.
    stack.set_crop(Some(HOUSE));
    assert_eq!(photo_cruiser_core::adjust::opening(&src, &stack, Tool::Straighten).unwrap(), angle);
    stack.set_crop(None);
    // The horizon: the first row from the top that is sea (blue over red), per column.
    let horizon = |img: &RgbaImage, x: u32| {
        (0..img.height()).find(|&y| {
            let p = img.get_pixel(x, y).0;
            p[0] < 80 && p[2] > 100
        })
    };
    let before = render(&src, &stack, RenderOpts::default()).unwrap();
    let tilt = |img: &RgbaImage| {
        let (l, r) = (horizon(img, 10).unwrap() as f32, horizon(img, img.width() - 11).unwrap() as f32);
        ((r - l) / (img.width() - 21) as f32).atan().to_degrees()
    };
    assert!((tilt(&before) - 3.4).abs() < 0.5, "fixture tilt {}", tilt(&before));
    stack.set_adjust(Tool::Straighten, Some(angle));
    let after = render(&src, &stack, RenderOpts::default()).unwrap();
    assert!(tilt(&after).abs() < 0.5, "after: {}", tilt(&after));
    assert!(after.pixels().all(|p| p.0[3] == 255), "no empty corners");
}
