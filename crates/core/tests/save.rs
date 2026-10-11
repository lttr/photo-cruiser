//! Save, revert and the sidecar, on temp copies of the fixtures (never on `fixtures/`).

use photo_cruiser_core::meta::{self, JpegSettings};
use photo_cruiser_core::store::{self, PhotoEdits};
use photo_cruiser_core::{EditStack, Error, Op, Rect, decode, embedded_preview, probe};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").join(name)
}

/// A temp folder with copies of `names` (paths relative to `fixtures/`), flattened.
fn copies(names: &[&str]) -> (tempfile::TempDir, Vec<PathBuf>) {
    let dir = tempfile::tempdir().unwrap();
    let paths = names
        .iter()
        .map(|n| {
            let to = dir.path().join(Path::new(n).file_name().unwrap());
            std::fs::copy(fixture(n), &to).unwrap();
            to
        })
        .collect();
    (dir, paths)
}

fn crop(x: f32, y: f32, w: f32, h: f32) -> EditStack {
    EditStack::default().with(Op::Crop(Rect::new(x, y, w, h)))
}

fn backup(photo: &Path) -> PathBuf {
    photo.parent().unwrap().join(".photo-cruiser/originals").join(photo.file_name().unwrap())
}

fn size(p: &Path) -> (u32, u32) {
    let p = probe(p).unwrap();
    (p.width, p.height)
}

#[test]
fn save_crops_keeps_backup_and_reverts() {
    let (_dir, p) = copies(&["img2.jpg"]);
    let photo = &p[0];
    let input = std::fs::read(photo).unwrap();
    let mtime = std::fs::metadata(photo).unwrap().modified().unwrap();

    let r = store::save(photo, &crop(20.0, 10.0, 200.0, 150.0)).unwrap();
    assert_eq!((r.action, r.path.as_path(), r.width, r.height), ("saved", photo.as_path(), 200, 150));
    assert_eq!(size(photo), (200, 150));
    assert_eq!(std::fs::read(backup(photo)).unwrap(), input, "backup is the original, byte for byte");
    assert_eq!(std::fs::metadata(photo).unwrap().modified().unwrap(), mtime, "mtime kept");
    let edits = store::load(photo).unwrap().unwrap();
    assert!(!edits.dirty());
    assert_eq!(store::original(photo), backup(photo));
    assert_eq!(store::backup(photo), Some(backup(photo)));

    // Not dirty: nothing happens.
    assert_eq!(store::save(photo, &crop(20.0, 10.0, 200.0, 150.0)).unwrap().action, "unchanged");

    // A wider crop renders from the backup, not from the saved (smaller) file.
    store::save(photo, &crop(0.0, 0.0, 300.0, 220.0)).unwrap();
    assert_eq!(size(photo), (300, 220));
    assert_eq!(std::fs::read(backup(photo)).unwrap(), input);

    let r = store::revert(photo).unwrap();
    assert_eq!(r.action, "reverted");
    assert_eq!(std::fs::read(photo).unwrap(), input, "revert is byte-identical");
    assert!(!backup(photo).exists());
    assert_eq!(store::load(photo).unwrap(), None, "sidecar removed");
}

#[test]
fn store_and_load_sidecar() {
    let (dir, p) = copies(&["img1.png"]);
    let photo = &p[0];
    assert_eq!(store::load(photo).unwrap(), None);
    let edits = PhotoEdits::new(crop(1.0, 2.0, 30.0, 40.0));
    store::store(photo, &edits).unwrap();
    let path = dir.path().join(".photo-cruiser/edits/img1.png.json");
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!((json["version"].as_u64(), json["ops"][0]["w"].as_f64()), (Some(1), Some(30.0)));
    assert_eq!(store::load(photo).unwrap(), Some(edits));
    // An empty stack, never saved: the sidecar goes away.
    store::store(photo, &PhotoEdits::default()).unwrap();
    assert!(!path.exists());
}

#[test]
fn exif_rotated_photo_is_turned_once() {
    let (_dir, p) = copies(&["previews/thumb-exif6.jpg"]);
    let photo = &p[0];
    assert_eq!(probe(photo).unwrap().orientation, 6);
    assert!(embedded_preview(photo).unwrap().is_some());
    store::save(photo, &crop(0.0, 0.0, 240.0, 300.0)).unwrap();
    let after = probe(photo).unwrap();
    assert_eq!((after.width, after.height, after.orientation), (240, 300, 1));
    assert!(embedded_preview(photo).unwrap().is_none(), "IFD1 thumbnail dropped");
    // The red marker (top-left of the stored image) is top-right once turned 90° CW.
    let img = decode(photo).unwrap().image;
    let px = img.get_pixel(230, 10).0;
    assert!(px[0] > 180 && px[1] < 80, "{px:?}");
}

#[test]
fn jpeg_keeps_fine_tables_sampling_density_and_icc() {
    let (_dir, p) = copies(&["save/q98.jpg", "save/q75.jpg"]);
    let settings = |p: &Path| meta::jpeg_settings(&std::fs::read(p).unwrap()).unwrap();
    let before = settings(&p[0]);
    assert_eq!((before.sampling, before.density), ((2, 2), Some((1, 300, 300))));
    let icc = decode::metadata(&p[0]).unwrap().icc;
    assert!(icc.is_some());

    let r = store::save(&p[0], &crop(10.0, 10.0, 100.0, 80.0)).unwrap();
    assert_eq!(r.quality, "original tables");
    let after = settings(&p[0]);
    assert_eq!(after, before, "same DQT tables, sampling and density");
    assert_eq!(decode::metadata(&p[0]).unwrap().icc, icc);

    let r = store::save(&p[1], &crop(10.0, 10.0, 100.0, 80.0)).unwrap();
    assert_eq!(r.quality, "raised to 95");
    let floor = meta::ijg_tables(95);
    let JpegSettings { tables: Some(t), .. } = settings(&p[1]) else { panic!("no tables") };
    for (t, f) in t.iter().zip(floor) {
        assert!(t.iter().zip(f).all(|(a, b)| *a <= b), "{t:?}");
    }
}

#[test]
fn subsampled_crop_snaps_to_even_pixels() {
    let (_dir, p) = copies(&["save/q98.jpg"]);
    let original = decode(&p[0]).unwrap().image;
    store::save(&p[0], &crop(41.0, 51.0, 100.0, 80.0)).unwrap();
    let saved = decode(&p[0]).unwrap().image;
    assert_eq!(saved.dimensions(), (100, 80));
    let diff = |x0: u32, y0: u32| -> u64 {
        saved
            .enumerate_pixels()
            .map(|(x, y, p)| {
                let o = original.get_pixel(x + x0, y + y0);
                (0..3).map(|c| p.0[c].abs_diff(o.0[c]) as u64).sum::<u64>()
            })
            .sum()
    };
    assert!(diff(40, 50) < diff(41, 51), "{} vs {}", diff(40, 50), diff(41, 51));
}

#[test]
fn heic_becomes_jpeg() {
    let (dir, p) = copies(&["img5.HEIC"]);
    let r = store::save(&p[0], &crop(0.0, 0.0, 100.0, 100.0)).unwrap();
    let jpg = dir.path().join("img5.jpg");
    assert_eq!(
        (r.path.as_path(), r.format.as_str(), r.quality.as_str()),
        (jpg.as_path(), "jpeg", "default 95")
    );
    assert!(!p[0].exists());
    assert!(backup(&p[0]).exists());
    assert_eq!(size(&jpg), (100, 100));
    let edits = store::load(&jpg).unwrap().expect("sidecar found under the original's name");
    assert!(!edits.dirty());
    assert_eq!(store::original(&jpg), backup(&p[0]));
    // Re-save the converted photo: still one JPEG, rendered from the HEIC backup.
    store::save(&jpg, &crop(0.0, 0.0, 300.0, 200.0)).unwrap();
    assert_eq!(size(&jpg), (300, 200));
}

#[test]
fn conversion_refuses_a_taken_name() {
    let (dir, p) = copies(&["img5.HEIC"]);
    std::fs::copy(fixture("img2.jpg"), dir.path().join("img5.jpg")).unwrap();
    let err = store::save(&p[0], &crop(0.0, 0.0, 100.0, 100.0)).unwrap_err();
    assert!(matches!(err, Error::NameTaken { .. }), "{err:?}");
    assert!(p[0].exists() && !backup(&p[0]).exists());
}

#[test]
fn webp_keeps_lossy_or_lossless() {
    let (_dir, p) = copies(&["img9.webp", "save/lossless.webp"]);
    let info = |p: &Path| meta::webp_info(&std::fs::read(p).unwrap()).unwrap();
    assert!(!info(&p[0]).lossless && info(&p[1]).lossless);
    for photo in &p {
        let was = info(photo);
        let r = store::save(photo, &crop(10.0, 10.0, 100.0, 80.0)).unwrap();
        assert_eq!((r.format.as_str(), size(photo)), ("webp", (100, 80)));
        assert_eq!(info(photo), was);
    }
}

#[test]
fn refuses_animations_and_16_bit() {
    let (_dir, p) = copies(&["save/anim.gif", "save/gray16.png"]);
    let err = store::save(&p[0], &crop(0.0, 0.0, 10.0, 10.0)).unwrap_err();
    assert!(matches!(err, Error::Animated { .. }), "{err:?}");
    let err = store::save(&p[1], &crop(0.0, 0.0, 10.0, 10.0)).unwrap_err();
    assert!(matches!(err, Error::HighBitDepth { .. }), "{err:?}");
    for photo in &p {
        assert!(photo.exists() && !backup(photo).exists());
    }
    let tmp: Vec<_> = std::fs::read_dir(p[0].parent().unwrap()).unwrap().flatten().collect();
    assert!(tmp.iter().all(|e| !e.file_name().to_string_lossy().ends_with(".pc-tmp")), "temp removed");
}

#[test]
fn read_only_folder() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, p) = copies(&["img2.jpg"]);
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
    let err = store::save(&p[0], &crop(0.0, 0.0, 10.0, 10.0)).unwrap_err();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(err, Error::ReadOnly { .. }), "{err:?}");
    assert_eq!(err.kind(), "read_only");
}

#[test]
fn outside_change_makes_edits_stale() {
    let (_dir, p) = copies(&["img2.jpg"]);
    let photo = &p[0];
    store::save(photo, &crop(0.0, 0.0, 100.0, 100.0)).unwrap();
    let first_backup = std::fs::read(backup(photo)).unwrap();
    // Another program writes the photo.
    std::fs::copy(fixture("img1.png"), photo).unwrap();
    let err = store::load(photo).unwrap_err();
    assert!(matches!(err, Error::StaleEdits { .. }), "{err:?}");
    assert_eq!(store::load(photo).unwrap(), None, "dropped: the file is the original now");
    assert_eq!(store::original(photo), *photo);
    // The next save keeps the old backup aside.
    let changed = std::fs::read(photo).unwrap();
    store::save(photo, &crop(0.0, 0.0, 50.0, 50.0)).unwrap();
    let aside = PathBuf::from(format!("{}~1", backup(photo).display()));
    assert_eq!(std::fs::read(aside).unwrap(), first_backup);
    assert_eq!(std::fs::read(backup(photo)).unwrap(), changed);
}
