//! `pcr` end to end against `fixtures/`.

use assert_cmd::Command;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn fixture(name: &str) -> String {
    fixtures().join(name).to_string_lossy().into_owned()
}

fn pcr() -> Command {
    Command::cargo_bin("pcr").unwrap()
}

/// Run, assert the exit code, parse stdout as JSON.
fn run_json(args: &[&str], code: i32) -> Value {
    let out = pcr().args(args).assert().code(code).get_output().stdout.clone();
    serde_json::from_slice(&out).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out)))
}

const ORDER: [&str; 9] = [
    "img1.png",
    "img2.jpg",
    "img3-exif6.jpg",
    "img4.AVIF",
    "img5.HEIC",
    "img9.webp",
    "img10.gif",
    "img11.bmp",
    "img12-broken.jpg",
];

#[test]
fn list_text_order() {
    let out = pcr().args(["list", &fixture("")]).assert().success().get_output().stdout.clone();
    let lines: Vec<String> = String::from_utf8(out).unwrap().lines().map(str::to_string).collect();
    let expected: Vec<String> = ORDER.iter().enumerate().map(|(i, n)| format!("{i}\t{n}")).collect();
    assert_eq!(lines, expected);
}

#[test]
fn list_json_schema() {
    let v = run_json(&["list", &fixture(""), "--json"], 0);
    assert_eq!(v["count"], 9);
    assert!(v["folder"].is_string());
    let entries = v["entries"].as_array().unwrap();
    for (i, (e, name)) in entries.iter().zip(ORDER).enumerate() {
        let keys: Vec<&str> = e.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, ["index", "name", "path"]);
        assert_eq!(e["index"], i);
        assert_eq!(e["name"], name);
        assert!(e["path"].as_str().unwrap().ends_with(name));
    }
}

#[test]
fn list_probe() {
    let v = run_json(&["list", &fixture(""), "--json", "--probe"], 0);
    let e = &v["entries"];
    assert_eq!(e[2]["format"], "jpeg");
    assert_eq!(
        (e[2]["width"].as_u64(), e[2]["height"].as_u64(), e[2]["orientation"].as_u64()),
        (Some(240), Some(320), Some(6))
    );
    assert_eq!(e[4]["format"], "heic");
    assert_eq!(e[3]["format"], "avif");
    assert_eq!(e[7]["width"], 64);
    // The broken file gets a per-entry error, the command still succeeds.
    assert_eq!(e[8]["error"]["kind"], "decode");
    assert!(e[8].get("width").is_none());
}

#[test]
fn info_exif6_jpeg() {
    let v = run_json(&["info", &fixture("img3-exif6.jpg"), "--json"], 0);
    assert_eq!(v["format"], "jpeg");
    assert_eq!((v["width"].as_u64(), v["height"].as_u64()), (Some(240), Some(320)));
    assert_eq!(v["orientation"], 6);
    assert!(v["decode_ms"].as_f64().unwrap() >= 0.0);
    assert!(v["path"].as_str().unwrap().ends_with("img3-exif6.jpg"));
}

#[test]
fn info_heic() {
    let v = run_json(&["info", &fixture("img5.HEIC"), "--json"], 0);
    assert_eq!(v["format"], "heic");
    assert_eq!((v["width"].as_u64(), v["height"].as_u64()), (Some(320), Some(240)));
    assert_eq!(v["orientation"], 1);
}

#[test]
fn render_crop_heic_exact() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.png");
    let v = run_json(
        &["render", &fixture("img5.HEIC"), "--crop", "10,20,100,50", "-o", out.to_str().unwrap(), "--json"],
        0,
    );
    assert_eq!((v["width"].as_u64(), v["height"].as_u64()), (Some(100), Some(50)));
    assert_eq!(image::open(&out).unwrap().into_rgba8().dimensions(), (100, 50));
}

#[test]
fn render_crop_outside_image_fails() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.png");
    let o = out.to_str().unwrap();
    let v = run_json(&["render", &fixture("img1.png"), "--crop", "400,10,50,50", "-o", o, "--json"], 3);
    assert_eq!(v["error"]["kind"], "empty_crop");
    assert!(v["error"]["message"].as_str().unwrap().contains("400,10,50,50"), "{v}");
    assert!(!out.exists(), "nothing is written for an empty crop");
    // Partial overlap clamps: 320x240 image, crop starts at 300 → 20 px wide.
    let v = run_json(&["render", &fixture("img1.png"), "--crop", "300,200,50,50", "-o", o, "--json"], 0);
    assert_eq!((v["width"].as_u64(), v["height"].as_u64()), (Some(20), Some(40)));
}

#[test]
fn export_crop_outside_image_fails() {
    let dir = tempfile::tempdir().unwrap();
    let edits = dir.path().join("e.json");
    std::fs::write(&edits, r#"{"version": 1, "ops": [{"op": "crop", "x": -100, "y": 0, "w": 50, "h": 50}]}"#)
        .unwrap();
    let out = dir.path().join("out.jpg");
    let v = run_json(
        &[
            "export",
            &fixture("img2.jpg"),
            "--edits",
            edits.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--json",
        ],
        3,
    );
    assert_eq!(v["error"]["kind"], "empty_crop");
    assert!(!out.exists());
}

#[test]
fn render_max_side() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.jpg");
    pcr()
        .args(["render", &fixture("img3-exif6.jpg"), "--max-side", "160", "-o", out.to_str().unwrap()])
        .assert()
        .success();
    // Oriented 240x320 → longer side 160.
    assert_eq!(image::open(&out).unwrap().into_rgba8().dimensions(), (120, 160));
}

#[test]
fn export_jpeg_with_edits() {
    let dir = tempfile::tempdir().unwrap();
    let edits = dir.path().join("e.json");
    std::fs::write(&edits, r#"{"version": 1, "ops": [{"op": "crop", "x": 40, "y": 0, "w": 200, "h": 120}]}"#)
        .unwrap();
    let out = dir.path().join("out.jpg");
    let v = run_json(
        &[
            "export",
            &fixture("img2.jpg"),
            "--edits",
            edits.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--quality",
            "80",
            "--json",
        ],
        0,
    );
    assert_eq!((v["width"].as_u64(), v["height"].as_u64()), (Some(200), Some(120)));
    let img = image::open(&out).unwrap();
    assert_eq!((img.width(), img.height()), (200, 120));
}

#[test]
fn missing_file_is_io() {
    let v = run_json(&["info", &fixture("nope.jpg"), "--json"], 2);
    assert_eq!(v["error"]["kind"], "io");
    pcr().args(["list", &fixture("no-such-dir")]).assert().code(2);
}

#[test]
fn broken_jpeg_is_decode() {
    let v = run_json(&["info", &fixture("img12-broken.jpg"), "--json"], 3);
    assert_eq!(v["error"]["kind"], "decode");
    assert!(v["error"]["message"].as_str().unwrap().contains("img12-broken.jpg"));
}

#[test]
fn bad_edits_and_output() {
    let dir = tempfile::tempdir().unwrap();
    let edits = dir.path().join("e.json");
    std::fs::write(&edits, r#"{"version": 99}"#).unwrap();
    let out = dir.path().join("out.jpg");
    let v = run_json(
        &[
            "export",
            &fixture("img1.png"),
            "--edits",
            edits.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--json",
        ],
        3,
    );
    assert_eq!(v["error"]["kind"], "edits");
    let v = run_json(
        &["render", &fixture("img1.png"), "-o", dir.path().join("x.tiff").to_str().unwrap(), "--json"],
        2,
    );
    assert_eq!(v["error"]["kind"], "unsupported_output");
}

#[test]
fn usage_errors_exit_1() {
    pcr().assert().code(1);
    pcr().arg("bogus").assert().code(1);
    pcr().args(["render", &fixture("img1.png")]).assert().code(1); // missing -o
    let v = run_json(&["render", &fixture("img1.png"), "--crop", "1,2,3", "-o", "x.png", "--json"], 1);
    assert_eq!(v["error"]["kind"], "usage");
    pcr()
        .args(["export", &fixture("img1.png"), "--edits", "e.json", "-o", "x.jpg", "--quality", "0"])
        .assert()
        .code(1);
    pcr().arg("--help").assert().success();
}

/// A temp folder with a copy of each fixture in `names`, plus a private data dir (for the
/// read-only sidecar fallback and the trash).
fn temp_copies(names: &[&str]) -> (tempfile::TempDir, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("photos")).unwrap();
    let paths = names
        .iter()
        .map(|n| {
            let to = dir.path().join("photos").join(Path::new(n).file_name().unwrap());
            std::fs::copy(fixtures().join(n), &to).unwrap();
            to.to_string_lossy().into_owned()
        })
        .collect();
    (dir, paths)
}

fn pcr_in(dir: &tempfile::TempDir) -> Command {
    let mut c = pcr();
    c.env("XDG_DATA_HOME", dir.path().join("data"));
    c
}

fn json_in(dir: &tempfile::TempDir, args: &[&str], code: i32) -> Value {
    let out = pcr_in(dir).args(args).assert().code(code).get_output().stdout.clone();
    serde_json::from_slice(&out).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out)))
}

#[test]
fn edits_save_revert() {
    let (dir, p) = temp_copies(&["img2.jpg"]);
    let photo = &p[0];
    let input = std::fs::read(photo).unwrap();
    let v = json_in(&dir, &["edits", photo, "--json"], 0);
    assert_eq!(
        (v["ops"].as_array().unwrap().len(), &v["saved"], &v["dirty"]),
        (0, &Value::Null, &Value::Bool(false))
    );

    let edits = dir.path().join("e.json");
    std::fs::write(&edits, r#"{"version":1,"ops":[{"op":"crop","x":10,"y":10,"w":100,"h":80}]}"#).unwrap();
    let v = json_in(&dir, &["save", photo, "--edits", &edits.to_string_lossy(), "--json"], 0);
    assert_eq!(
        (&v["action"], &v["width"], &v["height"], &v["format"]),
        (&json!("saved"), &json!(100), &json!(80), &json!("jpeg"))
    );
    let v = json_in(&dir, &["edits", photo, "--json"], 0);
    assert_eq!(v["dirty"], false);
    assert_eq!(v["saved"][0]["w"], 100.0);
    assert!(v["backup"].as_str().unwrap().ends_with(".photo-cruiser/originals/img2.jpg"));
    assert_eq!(json_in(&dir, &["save", photo, "--json"], 0)["action"], "unchanged");

    // Export uses the sidecar and renders from the backup: cropped once, not twice.
    let out = dir.path().join("out.png").to_string_lossy().into_owned();
    let v = json_in(&dir, &["export", photo, "-o", &out, "--json"], 0);
    assert_eq!((&v["width"], &v["height"]), (&json!(100), &json!(80)));
    let v = json_in(&dir, &["render", photo, "--no-edits", "-o", &out, "--json"], 0);
    assert_eq!((&v["width"], &v["height"]), (&json!(320), &json!(240)), "the original");

    assert_eq!(json_in(&dir, &["revert", photo, "--json"], 0)["action"], "reverted");
    assert_eq!(std::fs::read(photo).unwrap(), input);
    assert_eq!(json_in(&dir, &["edits", photo, "--json"], 0)["backup"], Value::Null);
}

#[test]
fn converted_photo_reverts_to_the_original_name() {
    let (dir, p) = temp_copies(&["img5.HEIC"]);
    let input = std::fs::read(&p[0]).unwrap();
    let edits = dir.path().join("e.json");
    std::fs::write(&edits, r#"{"version":1,"ops":[{"op":"crop","x":0,"y":0,"w":64,"h":64}]}"#).unwrap();
    let v = json_in(&dir, &["save", &p[0], "--edits", &edits.to_string_lossy(), "--json"], 0);
    let jpg = v["path"].as_str().unwrap().to_string();
    assert!(jpg.ends_with("photos/img5.jpg"), "{jpg}");
    let v = json_in(&dir, &["revert", &jpg, "--json"], 0);
    assert!(v["path"].as_str().unwrap().ends_with("photos/img5.HEIC"));
    assert_eq!(std::fs::read(&p[0]).unwrap(), input);
    assert!(!std::path::Path::new(&jpg).exists(), "our JPEG went to the trash");
}

#[test]
fn read_only_folder_keeps_edits_in_the_fallback() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, p) = temp_copies(&["img2.jpg"]);
    let photos = dir.path().join("photos");
    std::fs::set_permissions(&photos, std::fs::Permissions::from_mode(0o555)).unwrap();
    let edits = dir.path().join("e.json");
    std::fs::write(&edits, r#"{"version":1,"ops":[{"op":"crop","x":0,"y":0,"w":64,"h":64}]}"#).unwrap();
    let v = json_in(&dir, &["save", &p[0], "--edits", &edits.to_string_lossy(), "--json"], 2);
    let v2 = json_in(&dir, &["edits", &p[0], "--json"], 0);
    std::fs::set_permissions(&photos, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(v["error"]["kind"], "read_only");
    assert_eq!((&v2["dirty"], &v2["ops"][0]["w"]), (&json!(true), &json!(64.0)));
    let fallback: Vec<_> = std::fs::read_dir(dir.path().join("data/photo-cruiser/edits")).unwrap().collect();
    assert_eq!(fallback.len(), 1);
}

#[test]
fn adjust_render_and_save() {
    let (dir, p) = temp_copies(&["img2.jpg"]);
    let photo = &p[0];
    let out = dir.path().join("out.png").to_string_lossy().into_owned();
    // No value: the app's opening value, measured on the cropped photo.
    let v = json_in(
        &dir,
        &["render", photo, "--crop", "40,30,200,150", "--adjust", "exposure", "-o", &out, "--json"],
        0,
    );
    assert_eq!(v["ops"][1]["op"], "adjust");
    assert_eq!(v["ops"][1]["tool"], "exposure");
    let auto = v["ops"][1]["value"].as_f64().unwrap();
    assert!((-2.0..-0.1).contains(&auto), "white and blue checks come out darker: {auto}");
    let pixel = |path: &str| image::open(path).unwrap().into_rgba8().get_pixel(0, 0).0;
    let auto_px = pixel(&out);

    // The same through edits JSON, then saved into the photo.
    let edits = dir.path().join("e.json");
    let stack = json!({"version": 1, "ops": [{"op": "crop", "x": 40, "y": 30, "w": 200, "h": 150},
        {"op": "adjust", "tool": "exposure", "value": auto}]});
    std::fs::write(&edits, stack.to_string()).unwrap();
    let edits = edits.to_string_lossy().into_owned();
    json_in(&dir, &["render", photo, "--edits", &edits, "-o", &out, "--json"], 0);
    assert_eq!(pixel(&out), auto_px);
    let v = json_in(&dir, &["save", photo, "--edits", &edits, "--json"], 0);
    assert_eq!((&v["action"], &v["width"]), (&json!("saved"), &json!(200)));
    let v = json_in(&dir, &["edits", photo, "--json"], 0);
    assert_eq!((&v["ops"][1]["tool"], &v["dirty"]), (&json!("exposure"), &json!(false)));
    let saved = image::open(photo).unwrap().into_rgba8().get_pixel(0, 0).0;
    assert!(saved.iter().zip(auto_px).all(|(s, a)| s.abs_diff(a) <= 6), "{saved:?} vs {auto_px:?}");

    let v = json_in(&dir, &["render", photo, "--adjust", "blur", "-o", &out, "--json"], 1);
    assert_eq!(v["error"]["kind"], "usage");
}

#[test]
fn levels_render_and_save() {
    let (dir, p) = temp_copies(&["adjust/document.jpg"]);
    let photo = &p[0];
    let out = dir.path().join("out.png").to_string_lossy().into_owned();
    let page = "60,0,260,240";
    // No value: strength 1, the paper (top left of the page) turns white.
    let v = json_in(&dir, &["render", photo, "--crop", page, "--adjust", "levels", "-o", &out, "--json"], 0);
    assert_eq!(v["ops"][1], json!({"op": "adjust", "tool": "levels", "value": 1.0}));
    let pixel = |path: &str| image::open(path).unwrap().into_rgba8().get_pixel(5, 5).0;
    let auto_px = pixel(&out);
    assert!(auto_px[..3].iter().all(|c| *c >= 245), "{auto_px:?}");
    // Strength 0 is no step at all.
    let v =
        json_in(&dir, &["render", photo, "--crop", page, "--adjust", "levels=0", "-o", &out, "--json"], 0);
    assert_eq!(v["ops"].as_array().unwrap().len(), 1);
    assert!(pixel(&out)[2] < 170, "yellowish paper: {:?}", pixel(&out));

    // The same through edits JSON, then saved into the photo.
    let edits = dir.path().join("e.json");
    let stack = json!({"version": 1, "ops": [{"op": "crop", "x": 60, "y": 0, "w": 260, "h": 240},
        {"op": "adjust", "tool": "levels", "value": 1}]});
    std::fs::write(&edits, stack.to_string()).unwrap();
    let edits = edits.to_string_lossy().into_owned();
    json_in(&dir, &["render", photo, "--edits", &edits, "-o", &out, "--json"], 0);
    assert_eq!(pixel(&out), auto_px);
    let v = json_in(&dir, &["save", photo, "--edits", &edits, "--json"], 0);
    assert_eq!((&v["action"], &v["width"]), (&json!("saved"), &json!(260)));
    let v = json_in(&dir, &["edits", photo, "--json"], 0);
    assert_eq!((&v["ops"][1]["tool"], &v["dirty"]), (&json!("levels"), &json!(false)));
    let saved = image::open(photo).unwrap().into_rgba8().get_pixel(5, 5).0;
    assert!(saved.iter().zip(auto_px).all(|(s, a)| s.abs_diff(a) <= 6), "{saved:?} vs {auto_px:?}");
}

#[test]
fn colour_render_and_save() {
    let (dir, p) = temp_copies(&["img3-exif6.jpg"]);
    let photo = &p[0];
    let out = dir.path().join("out.png").to_string_lossy().into_owned();
    let pixel = |path: &str| image::open(path).unwrap().into_rgba8().get_pixel(150, 60).0;
    // No value: 1×. "color" works too.
    let v = json_in(&dir, &["render", photo, "--adjust", "color", "-o", &out, "--json"], 0);
    assert_eq!(v["ops"][0], json!({"op": "adjust", "tool": "colour", "value": 1.0}));
    let one = pixel(&out);
    json_in(&dir, &["render", photo, "-o", &out, "--json"], 0);
    let original = pixel(&out);
    let spread = |p: [u8; 4]| p[..3].iter().max().unwrap() - p[..3].iter().min().unwrap();
    assert!(spread(one) > spread(original), "more colourful: {one:?} vs {original:?}");
    let v = json_in(&dir, &["render", photo, "--adjust", "colour=0", "-o", &out, "--json"], 0);
    assert_eq!(v["ops"].as_array().unwrap().len(), 0, "0× is no step");

    let edits = dir.path().join("e.json");
    std::fs::write(
        &edits,
        json!({"version": 1, "ops": [{"op": "adjust", "tool": "colour", "value": 1}]}).to_string(),
    )
    .unwrap();
    let edits = edits.to_string_lossy().into_owned();
    json_in(&dir, &["render", photo, "--edits", &edits, "-o", &out, "--json"], 0);
    assert_eq!(pixel(&out), one);
    let v = json_in(&dir, &["save", photo, "--edits", &edits, "--json"], 0);
    assert_eq!(v["action"], "saved");
    let v = json_in(&dir, &["edits", photo, "--json"], 0);
    assert_eq!((&v["ops"][0]["tool"], &v["dirty"]), (&json!("colour"), &json!(false)));
    let saved = image::open(photo).unwrap().into_rgba8().get_pixel(150, 60).0;
    assert!(saved.iter().zip(one).all(|(s, a)| s.abs_diff(a) <= 6), "{saved:?} vs {one:?}");
}

#[test]
fn skill_prints_or_writes() {
    let out = pcr().arg("skill").assert().success().get_output().stdout.clone();
    let md = String::from_utf8(out).unwrap();
    assert!(md.starts_with("---\nname: pcr\n") && md.contains("### `pcr save`"), "{md}");
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("skills/pcr");
    let v = run_json(&["skill", "-o", &target.to_string_lossy(), "--json"], 0);
    assert_eq!(v["path"], json!(target.join("SKILL.md")));
    assert_eq!(std::fs::read_to_string(target.join("SKILL.md")).unwrap(), md);
}
