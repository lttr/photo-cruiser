//! Every fixture format through the real decoders. Fixtures: `just fixtures`.

use photo_cruiser_core::{Error, Folder, Format, decode, embedded_preview, probe};
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").canonicalize().unwrap()
}

#[test]
fn folder_lists_photos_in_natural_order() {
    let f = Folder::open(fixtures()).unwrap();
    let names: Vec<_> = f.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "img1.png",
            "img2.jpg",
            "img3-exif6.jpg",
            "img4.AVIF",
            "img5.HEIC",
            "img9.webp",
            "img10.gif",
            "img11.bmp",
            "img12-broken.jpg"
        ]
    );
}

#[test]
fn decodes_every_format() {
    let cases = [
        ("img1.png", Format::Png, (320, 240), 1),
        ("img2.jpg", Format::Jpeg, (320, 240), 1),
        ("img3-exif6.jpg", Format::Jpeg, (240, 320), 6),
        ("img4.AVIF", Format::Avif, (320, 240), 1),
        ("img5.HEIC", Format::Heic, (320, 240), 1),
        ("img9.webp", Format::Webp, (160, 120), 1),
        ("img10.gif", Format::Gif, (160, 120), 1),
        ("img11.bmp", Format::Bmp, (64, 48), 1),
    ];
    for (name, format, size, orientation) in cases {
        let path = fixtures().join(name);
        let p = probe(&path).unwrap_or_else(|e| panic!("probe {name}: {e}"));
        assert_eq!((p.format, (p.width, p.height), p.orientation), (format, size, orientation), "{name}");
        let d = decode(&path).unwrap_or_else(|e| panic!("decode {name}: {e}"));
        assert_eq!(d.image.dimensions(), size, "{name}");
        assert_eq!(d.source_size(), size, "{name}");
        // The red marker square is drawn in the stored top-left corner; after EXIF 6
        // (rotate 90° CW) it ends up top-right.
        let (w, _) = size;
        let marker = if orientation == 6 { (w - 5, 5) } else { (5, 5) };
        let px = d.image.get_pixel(marker.0, marker.1).0;
        assert!(px[0] > 180 && px[1] < 80 && px[2] < 80, "{name}: marker pixel {px:?}");
    }
}

#[test]
fn broken_and_missing_files_are_errors() {
    let broken = fixtures().join("img12-broken.jpg");
    assert!(matches!(decode(&broken), Err(Error::Decode { .. })), "{:?}", decode(&broken).map(|_| ()));
    assert!(matches!(probe(&fixtures().join("nope.jpg")), Err(Error::Io { .. })));
    assert!(matches!(probe(&fixtures().join("nope.heic")), Err(Error::Io { .. })));
    assert!(matches!(decode(&fixtures().join("notes.txt")), Err(Error::Unsupported { .. })));
}

#[test]
fn downscale_keeps_source_size() {
    let d = decode(&fixtures().join("img5.HEIC")).unwrap().downscale(100);
    assert_eq!(d.image.dimensions(), (100, 75));
    assert_eq!(d.source_size(), (320, 240));
}

#[test]
fn embedded_previews() {
    let dir = fixtures().join("previews");
    // HEIF thumbnail item.
    let p = embedded_preview(&dir.join("thumb.HEIC")).unwrap().expect("heic thumbnail");
    assert_eq!((p.format, p.image.dimensions(), p.source_size()), (Format::Heic, (80, 60), (320, 240)));
    // EXIF IFD1 thumbnail, oriented like the photo (EXIF 6: marker ends up top-right).
    let p = embedded_preview(&dir.join("thumb-exif6.jpg")).unwrap().expect("exif thumbnail");
    assert_eq!((p.format, p.image.dimensions(), p.source_size()), (Format::Jpeg, (60, 80), (240, 320)));
    let px = p.image.get_pixel(56, 3).0;
    assert!(px[0] > 150 && px[1] < 100 && px[2] < 100, "marker pixel {px:?}");
    // 4:3 thumbnail of a 16:9 photo: rejected.
    assert!(embedded_preview(&dir.join("letterbox.jpg")).unwrap().is_none());
    // No thumbnail, other formats: none. Broken: error.
    for name in ["img2.jpg", "img5.HEIC", "img1.png", "img9.webp"] {
        assert!(embedded_preview(&fixtures().join(name)).unwrap().is_none(), "{name}");
    }
    assert!(embedded_preview(&fixtures().join("img12-broken.jpg")).is_err());
}
