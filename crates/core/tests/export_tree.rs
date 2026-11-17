//! `export_tree` on temp copies of the fixtures (never on `fixtures/`).

use photo_cruiser_core::decode::{self, Metadata};
use photo_cruiser_core::export::{self, Encoding};
use photo_cruiser_core::export_tree::{FileReport, Options, Report, export_tree};
use photo_cruiser_core::meta;
use photo_cruiser_core::render::{self, RenderOpts};
use photo_cruiser_core::{EditStack, Error, Op, Rect, probe, rating, store};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").join(name)
}

/// A temp folder with `(fixture, relative target)` copies.
fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (from, to) in files {
        let to = dir.path().join("in").join(to);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(fixture(from), &to).unwrap();
    }
    dir
}

fn run(dir: &Path, opts: &Options) -> Report {
    export_tree(&dir.join("in"), &dir.join("out"), opts).unwrap()
}

fn file<'a>(r: &'a Report, rel: &str) -> &'a FileReport {
    r.files.iter().find(|f| f.source.ends_with(rel)).unwrap_or_else(|| panic!("{rel} not in {:#?}", r.files))
}

/// Every file under `dir`, relative, sorted.
fn listing(dir: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(root, &p, out);
            } else {
                out.push(p.strip_prefix(root).unwrap().to_string_lossy().into_owned());
            }
        }
    }
    let mut out = vec![];
    walk(dir, dir, &mut out);
    out.sort();
    out
}

#[test]
fn mirrors_the_tree_converts_and_skips_hidden() {
    let dir = tree(&[
        ("img2.jpg", "img2.jpg"),
        ("img1.png", "a/img1.png"),
        ("img5.HEIC", "a/b/img5.HEIC"),
        ("img9.webp", "a/b/img9.webp"),
        ("img2.jpg", ".photo-cruiser/x.jpg"),
        ("img2.jpg", ".hidden/y.jpg"),
        ("notes.txt", "empty/notes.txt"),
    ]);
    let r = run(dir.path(), &Options::default());
    assert_eq!(
        listing(&dir.path().join("out")),
        ["a/b/img5.jpg", "a/b/img9.jpg", "a/img1.jpg", "img2.jpg"],
        "hidden folders and non-photos left out, no empty folders"
    );
    assert_eq!((r.count, r.failed), (4, 0));
    assert_eq!(file(&r, "a/img1.png").reason, "converted");
    for f in &r.files {
        assert_eq!(probe(&f.output).unwrap().format, decode::Format::Jpeg, "{:?}", f.output);
    }
    assert_eq!(probe(&dir.path().join("out/a/b/img5.jpg")).unwrap().width, 320);
}

#[test]
fn output_must_be_new_or_empty() {
    let dir = tree(&[("img2.jpg", "img2.jpg")]);
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();
    std::fs::write(out.join("old.txt"), "x").unwrap();
    let e = export_tree(&dir.path().join("in"), &out, &Options::default()).unwrap_err();
    assert!(matches!(e, Error::NotEmpty { .. }), "{e}");
    assert_eq!(listing(&out), ["old.txt"], "nothing written");

    // Exporting a folder into itself is refused too, and so is a file as the output.
    let e = export_tree(&dir.path().join("in"), &dir.path().join("in"), &Options::default()).unwrap_err();
    assert!(matches!(e, Error::NotEmpty { .. }), "{e}");
    let e = export_tree(&dir.path().join("in"), &out.join("old.txt"), &Options::default()).unwrap_err();
    assert!(matches!(e, Error::NotEmpty { .. }), "{e}");

    // An empty folder is fine; a missing one is created; a missing parent is an error.
    std::fs::create_dir(dir.path().join("empty")).unwrap();
    export_tree(&dir.path().join("in"), &dir.path().join("empty"), &Options::default()).unwrap();
    export_tree(&dir.path().join("in"), &dir.path().join("new"), &Options::default()).unwrap();
    assert!(dir.path().join("new/img2.jpg").is_file());
    let e =
        export_tree(&dir.path().join("in"), &dir.path().join("no/such"), &Options::default()).unwrap_err();
    assert_eq!(e.kind(), "io");
}

#[test]
fn starred_only_and_the_star_carries_over() {
    let dir = tree(&[("img2.jpg", "img2.jpg"), ("img1.png", "img1.png"), ("img5.HEIC", "img5.HEIC")]);
    let input = dir.path().join("in");
    rating::star(&input.join("img1.png"), true).unwrap();
    rating::star(&input.join("img5.HEIC"), true).unwrap();
    let r = run(dir.path(), &Options { starred: true, ..Options::default() });
    assert_eq!(listing(&dir.path().join("out")), ["img1.jpg", "img5.jpg"]);
    for f in &r.files {
        assert!(rating::is_starred(rating::read(&f.output).unwrap()), "{:?}", f.output);
        assert!(!rating::sidecar(&f.output).exists(), "star inside the JPEG, no sidecar");
    }
}

#[test]
fn name_clash_jpeg_keeps_its_name() {
    let dir = tree(&[("img5.HEIC", "p.HEIC"), ("img2.jpg", "p.JPG"), ("img1.png", "p.png")]);
    let r = run(dir.path(), &Options::default());
    assert_eq!(listing(&dir.path().join("out")), ["p (2).jpg", "p (3).jpg", "p.JPG"]);
    assert!(!file(&r, "p.JPG").renamed);
    assert!(file(&r, "p.HEIC").output.ends_with("p (2).jpg"));
    assert!(file(&r, "p.HEIC").renamed);
    assert!(file(&r, "p.png").output.ends_with("p (3).jpg"));
}

#[test]
fn edited_photos_render_once_from_the_original() {
    let dir = tree(&[("img2.jpg", "saved.jpg"), ("img1.png", "dirty.png")]);
    let input = dir.path().join("in");
    let crop = |w| EditStack::default().with(Op::Crop(Rect::new(10.0, 20.0, w, 100.0)));
    // Saved: the file in the folder is already cropped; the export starts from the backup
    // with the current (wider) stack, not from the saved file.
    let saved = input.join("saved.jpg");
    store::save(&saved, &crop(100.0)).unwrap();
    store::store_stack(&saved, &crop(200.0)).unwrap();
    // Dirty, never saved: only the sidecar has the edits.
    store::store_stack(&input.join("dirty.png"), &crop(150.0)).unwrap();

    let r = run(dir.path(), &Options::default());
    let out = file(&r, "saved.jpg");
    assert_eq!((out.action, out.reason), ("encoded", "edited"));
    let p = probe(&out.output).unwrap();
    assert_eq!((p.width, p.height), (200, 100));
    let p = probe(&file(&r, "dirty.png").output).unwrap();
    assert_eq!((p.width, p.height), (150, 100));

    // Pixels: the export is render(original, stack), up to JPEG loss.
    let original = decode::decode(&store::original(&saved)).unwrap();
    let want = render::render(&original, &crop(200.0), RenderOpts::default()).unwrap();
    let got = image::open(&out.output).unwrap().into_rgba8();
    let diff: f64 = want.pixels().zip(got.pixels()).map(|(a, b)| a.0[0].abs_diff(b.0[0]) as f64).sum::<f64>()
        / (200.0 * 100.0);
    assert!(diff < 3.0, "mean diff {diff}");
}

#[test]
fn unedited_jpeg_copied_unless_re_encoding_saves_enough() {
    let dir = tree(&[("save/q98.jpg", "q98.jpg"), ("save/q75.jpg", "q75.jpg")]);
    let r = run(dir.path(), &Options::default());
    let q98 = file(&r, "q98.jpg");
    assert_eq!((q98.action, q98.reason), ("encoded", "smaller"));
    assert!((q98.bytes as f64) <= q98.source_bytes as f64 * 0.8, "{} vs {}", q98.bytes, q98.source_bytes);
    let q75 = file(&r, "q75.jpg");
    assert_eq!((q75.action, q75.reason), ("copied", "original"));
    assert_eq!(std::fs::read(&q75.output).unwrap(), std::fs::read(&q75.source).unwrap(), "byte for byte");
}

#[test]
fn metadata_and_mtime_carry_over() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in");
    std::fs::create_dir(&input).unwrap();
    // A JPEG with an ICC profile and EXIF orientation 6, from the rotated fixture.
    let src = fixture("img3-exif6.jpg");
    let exif = decode::metadata(&src).unwrap().exif.unwrap();
    let raw = image::open(&src).unwrap(); // `image` doesn't apply orientation itself
    let photo = input.join("rot.jpg");
    let icc = b"fake icc profile".to_vec();
    let encoding = Encoding::Jpeg { settings: Box::default(), quality: 100 };
    export::write(&raw.to_rgba8(), &photo, &encoding, &Metadata { icc: Some(icc.clone()), exif: Some(exif) })
        .unwrap();
    let mtime = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
    std::fs::File::options().write(true).open(&photo).unwrap().set_modified(mtime).unwrap();

    // Force the encode: every unedited JPEG is re-encoded with a negative saving.
    let r = run(dir.path(), &Options { min_saving: -100.0, ..Options::default() });
    let out = &file(&r, "rot.jpg").output;
    assert_eq!(file(&r, "rot.jpg").action, "encoded");
    let p = probe(out).unwrap();
    assert_eq!((p.width, p.height, p.orientation), (240, 320, 1), "pixels upright, EXIF says so");
    let meta = decode::metadata(out).unwrap();
    assert_eq!(meta.icc, Some(icc));
    assert_eq!(meta::exif_orientation(&meta.exif.unwrap()), Some(1));
    assert_eq!(std::fs::metadata(out).unwrap().modified().unwrap(), mtime);
}

#[test]
fn a_broken_photo_fails_alone() {
    let dir = tree(&[("img12-broken.jpg", "img12-broken.jpg"), ("img2.jpg", "img2.jpg")]);
    let r = run(dir.path(), &Options::default());
    assert_eq!((r.count, r.failed, r.encoded + r.copied), (2, 1, 1));
    let broken = file(&r, "img12-broken.jpg");
    assert_eq!(broken.action, "failed");
    assert_eq!(broken.error.as_ref().unwrap().kind, "decode");
    assert_eq!(listing(&dir.path().join("out")), ["img2.jpg"], "no half-written or temp files");
}

#[test]
fn bigger_than_max_side_is_scaled_down() {
    let dir = tree(&[("save/q75.jpg", "q75.jpg"), ("img1.png", "img1.png")]);
    let r = run(dir.path(), &Options { max_side: Some(160), ..Options::default() });
    // Copied before (re-encoding saves too little); too big now, so encoded at the new size.
    let q75 = file(&r, "q75.jpg");
    let p = probe(&q75.output).unwrap();
    assert_eq!((q75.action, q75.reason, q75.resized), ("encoded", "resized", true));
    assert_eq!(p.width.max(p.height), 160);
    assert_eq!((q75.width, q75.height), (p.width, p.height));
    let png = file(&r, "img1.png");
    assert_eq!((png.reason, png.resized, png.width, png.height), ("converted", true, 160, 120));

    // Small enough: untouched.
    let dir = tree(&[("img1.png", "img1.png")]);
    let r = run(dir.path(), &Options { max_side: Some(320), ..Options::default() });
    let png = file(&r, "img1.png");
    assert_eq!((png.resized, png.width, png.height), (false, 320, 240));
}

#[test]
fn chroma_sampling_follows_the_original() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in");
    std::fs::create_dir(&input).unwrap();
    let img = image::open(fixture("img2.jpg")).unwrap().to_rgba8();
    for (name, sampling) in [("s444.jpg", (1, 1)), ("s422.jpg", (2, 1)), ("s420.jpg", (2, 2))] {
        let settings = meta::JpegSettings { sampling, ..Default::default() };
        let encoding = Encoding::Jpeg { settings: Box::new(settings), quality: 100 };
        export::write(&img, &input.join(name), &encoding, &Metadata::default()).unwrap();
        assert_eq!(decode::chroma_sampling(&input.join(name)).unwrap(), sampling, "{name}");
    }
    std::fs::copy(fixture("img1.png"), input.join("img1.png")).unwrap();
    std::fs::copy(fixture("img5.HEIC"), input.join("img5.HEIC")).unwrap();

    let r = run(dir.path(), &Options { min_saving: -100.0, ..Options::default() });
    for (name, sampling, label) in [
        ("s444.jpg", (1, 1), "4:4:4"),
        ("s422.jpg", (2, 1), "4:2:2"),
        ("s420.jpg", (2, 2), "4:2:0"),
        ("img1.png", (1, 1), "4:4:4"),
    ] {
        let f = file(&r, name);
        assert_eq!((f.action, f.sampling), ("encoded", label), "{name}");
        let written = decode::jpeg_settings(&f.output).unwrap().unwrap().sampling;
        assert_eq!(written, sampling, "{name}");
    }
    // HEIC: whatever libheif says the photo has, the JPEG has.
    let heic = file(&r, "img5.HEIC");
    let want = decode::chroma_sampling(&input.join("img5.HEIC")).unwrap();
    assert_eq!(decode::jpeg_settings(&heic.output).unwrap().unwrap().sampling, want);
}

#[test]
fn files_pcr_cant_read_are_listed() {
    let dir = tree(&[("img2.jpg", "img2.jpg"), ("notes.txt", "a/notes.txt"), ("img2.jpg", ".hidden/y.jpg")]);
    let input = dir.path().join("in");
    for name in ["clip.MOV", "a/scan.tif", "img2.jpg.xmp", "IMG_1.AAE", ".DS_Store", ".hidden/v.mp4"] {
        std::fs::write(input.join(name), b"x").unwrap();
    }
    let r = run(dir.path(), &Options::default());
    let rel: Vec<_> = r
        .unreadable
        .iter()
        .map(|p| p.strip_prefix(&r.source).unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(rel, ["clip.MOV", "a/notes.txt", "a/scan.tif"], "sidecars and hidden files aren't listed");
    assert_eq!((r.count, r.failed), (1, 0), "a warning, not a failure");
}

#[test]
fn a_star_that_cant_be_written_fails_the_photo() {
    // A copied JPEG whose XMP isn't UTF-8 can't take the star (kept in a sidecar here).
    let dir = tree(&[("save/q75.jpg", "q75.jpg")]);
    let photo = dir.path().join("in/q75.jpg");
    let bytes = std::fs::read(&photo).unwrap();
    let payload = [b"http://ns.adobe.com/xap/1.0/\0".as_slice(), &[0xff, 0xfe, 0x80, 0x81]].concat();
    let mut app1 = vec![0xff, 0xe1];
    app1.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    app1.extend_from_slice(&payload);
    std::fs::write(&photo, [&bytes[..2], &app1, &bytes[2..]].concat()).unwrap();
    let xmp = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmp:Rating="1"/></rdf:RDF></x:xmpmeta>"#;
    std::fs::write(rating::sidecar(&photo), xmp).unwrap();
    assert!(rating::is_starred(rating::read(&photo).unwrap()));

    let r = run(dir.path(), &Options { starred: true, ..Options::default() });
    assert_eq!((r.count, r.failed), (1, 1));
    assert_eq!(file(&r, "q75.jpg").error.as_ref().unwrap().kind, "decode");
    assert_eq!(listing(&dir.path().join("out")), Vec::<String>::new(), "nothing left behind");
}
