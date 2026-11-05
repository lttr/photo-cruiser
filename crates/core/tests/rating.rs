//! The star, on temp copies of the fixtures (never on `fixtures/`).

use photo_cruiser_core::meta::{self, Head};
use photo_cruiser_core::rating::{self, sidecar};
use photo_cruiser_core::store;
use photo_cruiser_core::{EditStack, Op, Rect, decode, probe};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").join(name)
}

fn copies(names: &[&str]) -> (tempfile::TempDir, Vec<PathBuf>) {
    let dir = tempfile::tempdir().unwrap();
    let paths = names
        .iter()
        .map(|n| {
            let to = dir.path().join(n);
            std::fs::copy(fixture(n), &to).unwrap();
            to
        })
        .collect();
    (dir, paths)
}

/// The JPEG's bytes from its frame header on (the image data).
fn jpeg_image_data(b: &[u8]) -> &[u8] {
    let sof = (2..b.len()).find(|&i| b[i] == 0xFF && matches!(b[i + 1], 0xC0 | 0xC2));
    &b[sof.unwrap()..]
}

#[test]
fn jpeg_star_goes_into_the_file_and_back() {
    let (_dir, p) = copies(&["img2.jpg", "img3-exif6.jpg"]);
    for photo in &p {
        let before = std::fs::read(photo).unwrap();
        let mtime = std::fs::metadata(photo).unwrap().modified().unwrap();
        let pixels = decode(photo).unwrap().image;
        assert_eq!(rating::read(photo).unwrap(), 0);

        assert!(rating::star(photo, true).unwrap());
        assert_eq!(rating::read(photo).unwrap(), 1);
        assert!(!rating::star(photo, true).unwrap(), "starred already: nothing written");
        let after = std::fs::read(photo).unwrap();
        assert_eq!(jpeg_image_data(&after), jpeg_image_data(&before), "image data byte for byte");
        assert_eq!(decode(photo).unwrap().image, pixels);
        assert_eq!(
            probe(photo).unwrap(),
            probe(&fixture(photo.file_name().unwrap().to_str().unwrap())).unwrap()
        );
        assert_eq!(std::fs::metadata(photo).unwrap().modified().unwrap(), mtime, "mtime kept");
        assert!(!sidecar(photo).exists());
        let Head::Done(h) = meta::jpeg_head(&after) else { panic!() };
        let x = std::str::from_utf8(h.xmp.unwrap()).unwrap();
        assert!(x.contains(r#"xmp:Rating="1""#), "{x}");
        let Head::Done(old) = meta::jpeg_head(&before) else { panic!() };
        assert_eq!(h.exif, old.exif, "EXIF kept");

        // Unstarring keeps the packet (and its size), with rating 0.
        assert!(rating::star(photo, false).unwrap());
        assert_eq!(rating::read(photo).unwrap(), 0);
        assert_eq!(std::fs::read(photo).unwrap().len(), after.len());
    }
}

#[test]
fn webp_gets_an_extended_header() {
    let (_dir, p) = copies(&["img9.webp"]);
    let photo = &p[0];
    let pixels = decode(photo).unwrap().image;
    rating::star(photo, true).unwrap();
    assert_eq!(rating::read(photo).unwrap(), 1);
    let b = std::fs::read(photo).unwrap();
    assert!(meta::webp_xmp(&b).is_some_and(|x| x.windows(13).any(|w| w == br#"xmp:Rating="1"#)));
    assert_eq!(decode(photo).unwrap().image, pixels);
    assert_eq!(u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize, b.len() - 8);
    rating::star(photo, false).unwrap();
    assert_eq!(rating::read(photo).unwrap(), 0);
    assert!(!sidecar(photo).exists());
}

#[test]
fn other_formats_get_a_sidecar() {
    let (_dir, p) = copies(&["img5.HEIC", "img1.png"]);
    for photo in &p {
        let before = std::fs::read(photo).unwrap();
        assert!(!rating::star(photo, false).unwrap(), "unstarring an unrated photo writes nothing");
        assert!(!sidecar(photo).exists());
        rating::star(photo, true).unwrap();
        assert_eq!(std::fs::read(photo).unwrap(), before, "the photo itself is untouched");
        let text = std::fs::read_to_string(sidecar(photo)).unwrap();
        assert!(text.contains(r#"xmp:Rating="1""#), "{text}");
        assert_eq!(rating::read(photo).unwrap(), 1);
        rating::star(photo, false).unwrap();
        assert_eq!(rating::read(photo).unwrap(), 0);
    }
}

#[test]
fn a_darktable_sidecar_is_patched_not_replaced() {
    let (_dir, p) = copies(&["img5.HEIC"]);
    let photo = &p[0];
    let dt = r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="XMP Core 4.4.0-Exiv2">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:darktable="http://darktable.sf.net/"
    xmp:Rating="3"
    darktable:history_end="2">
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
"#;
    std::fs::write(sidecar(photo), dt).unwrap();
    assert_eq!(rating::read(photo).unwrap(), 3, "3 stars from elsewhere count as starred");
    assert!(rating::is_starred(3));
    rating::star(photo, false).unwrap();
    assert_eq!(
        std::fs::read_to_string(sidecar(photo)).unwrap(),
        dt.replace(r#"Rating="3""#, r#"Rating="0""#)
    );
}

#[test]
fn a_sidecar_counts_for_a_jpeg_until_it_is_starred_here() {
    let (_dir, p) = copies(&["img2.jpg"]);
    let photo = &p[0];
    std::fs::write(sidecar(photo), r#"<x:xmpmeta><rdf:RDF><rdf:Description xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmp:Rating="2"/></rdf:RDF></x:xmpmeta>"#).unwrap();
    assert_eq!(rating::read(photo).unwrap(), 2);
    rating::star(photo, false).unwrap();
    assert_eq!(rating::read(photo).unwrap(), 0, "the photo's own packet wins");
}

#[test]
fn saved_edits_survive_a_star_and_saves_keep_it() {
    let (_dir, p) = copies(&["img2.jpg", "img5.HEIC"]);
    let crop = EditStack::default().with(Op::Crop(Rect::new(0.0, 0.0, 100.0, 80.0)));
    for photo in &p {
        let saved = store::save(photo, &crop).unwrap().path;
        rating::star(&saved, true).unwrap();
        // Not stale: the edits are still there.
        let edits = store::load(&saved).unwrap().unwrap();
        assert_eq!(edits.stack, crop);
        // A new save renders from the original, which has no star: the star is carried over.
        let wider = EditStack::default().with(Op::Crop(Rect::new(0.0, 0.0, 120.0, 90.0)));
        store::save(&saved, &wider).unwrap();
        assert_eq!(rating::read(&saved).unwrap(), 1, "{}", saved.display());
        // So is revert, to the original (HEIC: back to its sidecar).
        let reverted = store::revert(&saved).unwrap().path;
        assert_eq!(&reverted, photo);
        assert_eq!(rating::read(photo).unwrap(), 1, "{}", photo.display());
        rating::star(photo, false).unwrap();
    }
}
