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
fn export_keeps_exif() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.jpg");
    pcr().args(["export", &fixture("img3-exif6.jpg"), "-o", out.to_str().unwrap()]).assert().success();
    let v = run_json(&["info", out.to_str().unwrap(), "--json"], 0);
    // EXIF is there and says upright: the pixels are turned already.
    assert_eq!(
        (v["width"].as_u64(), v["height"].as_u64(), v["orientation"].as_u64()),
        (Some(240), Some(320), Some(1))
    );
    assert!(image::open(&out).is_ok());
    let exif = photo_cruiser_core::decode::metadata(&out).unwrap().exif;
    assert!(exif.is_some(), "EXIF kept");
}

#[test]
fn export_tree_json_and_partial_failure() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in");
    std::fs::create_dir_all(input.join("sub")).unwrap();
    std::fs::copy(fixtures().join("img2.jpg"), input.join("img2.jpg")).unwrap();
    std::fs::copy(fixtures().join("img5.HEIC"), input.join("sub/img5.HEIC")).unwrap();
    std::fs::copy(fixtures().join("img1.png"), input.join("sub/img1.png")).unwrap();
    let (i, out) = (input.to_str().unwrap(), dir.path().join("out"));
    pcr().args(["star", &format!("{i}/img2.jpg"), &format!("{i}/sub/img5.HEIC")]).assert().success();

    std::fs::write(input.join("sub/clip.MOV"), b"x").unwrap();
    let o = out.to_str().unwrap();
    let res =
        pcr().args(["export-tree", i, "-o", o, "--starred", "--json"]).assert().code(0).get_output().clone();
    let v: Value = serde_json::from_slice(&res.stdout).unwrap();
    assert_eq!((v["count"].as_u64(), v["failed"].as_u64()), (Some(2), Some(0)));
    assert!(v["unreadable"][0].as_str().unwrap().ends_with("sub/clip.MOV"));
    let stderr = String::from_utf8_lossy(&res.stderr);
    assert!(stderr.contains("1 file not exported, pcr can't read them: 1 .mov"), "{stderr}");
    assert!(out.join("img2.jpg").is_file() && out.join("sub/img5.jpg").is_file());
    assert!(!out.join("sub/img1.jpg").exists(), "not starred");
    let f =
        v["files"].as_array().unwrap().iter().find(|f| f["output"].as_str().unwrap().ends_with("img5.jpg"));
    assert_eq!(f.unwrap()["action"], "encoded");

    // Not empty now.
    let v = run_json(&["export-tree", i, "-o", out.to_str().unwrap(), "--json"], 2);
    assert_eq!(v["error"]["kind"], "not_empty");

    // A broken photo: the rest is exported, exit 4, the report says which failed.
    std::fs::copy(fixtures().join("img12-broken.jpg"), input.join("img12-broken.jpg")).unwrap();
    let out2 = dir.path().join("out2");
    let v = run_json(&["export-tree", i, "-o", out2.to_str().unwrap(), "--max-side", "100", "--json"], 4);
    assert_eq!((v["count"].as_u64(), v["failed"].as_u64()), (Some(4), Some(1)));
    assert!(out2.join("sub/img1.jpg").is_file());
    let png =
        v["files"].as_array().unwrap().iter().find(|f| f["source"].as_str().unwrap().ends_with("img1.png"));
    let png = png.unwrap();
    assert_eq!(
        (png["width"].as_u64(), png["height"].as_u64(), png["resized"].as_bool()),
        (Some(100), Some(75), Some(true))
    );
    assert_eq!(png["sampling"], "4:4:4");
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

#[test]
fn warmth_render_and_save() {
    let (dir, p) = temp_copies(&["adjust/warm-cast.jpg"]);
    let photo = &p[0];
    let out = dir.path().join("out.png").to_string_lossy().into_owned();
    // The grey wall, top left.
    let pixel = |path: &str| image::open(path).unwrap().into_rgba8().get_pixel(40, 10).0;
    let spread = |p: [u8; 4]| p[..3].iter().max().unwrap() - p[..3].iter().min().unwrap();
    // No value: grey-world auto, cooler for a warm cast.
    let v = json_in(&dir, &["render", photo, "--adjust", "warmth", "-o", &out, "--json"], 0);
    let auto = v["ops"][0]["value"].as_f64().unwrap();
    assert!(auto < -0.5 && v["ops"][0]["tool"] == "warmth", "{v}");
    let balanced = pixel(&out);
    json_in(&dir, &["render", photo, "-o", &out, "--json"], 0);
    let original = pixel(&out);
    assert!(spread(balanced) <= 8 && spread(original) > 20, "{balanced:?} vs {original:?}");
    json_in(&dir, &["render", photo, "--adjust", "warmth=1", "-o", &out, "--json"], 0);
    let warm = pixel(&out);
    assert!(warm[0] > original[0] && warm[2] < original[2], "warmer: {warm:?} vs {original:?}");
    let v = json_in(&dir, &["render", photo, "--adjust", "warmth=0", "-o", &out, "--json"], 0);
    assert_eq!(v["ops"].as_array().unwrap().len(), 0, "0 is no step");

    let edits = dir.path().join("e.json");
    std::fs::write(
        &edits,
        json!({"version": 1, "ops": [{"op": "adjust", "tool": "warmth", "value": auto}]}).to_string(),
    )
    .unwrap();
    let edits = edits.to_string_lossy().into_owned();
    let v = json_in(&dir, &["save", photo, "--edits", &edits, "--json"], 0);
    assert_eq!(v["action"], "saved");
    let v = json_in(&dir, &["edits", photo, "--json"], 0);
    assert_eq!((&v["ops"][0]["tool"], &v["dirty"]), (&json!("warmth"), &json!(false)));
    let saved = image::open(photo).unwrap().into_rgba8().get_pixel(40, 10).0;
    assert!(saved.iter().zip(balanced).all(|(s, a)| s.abs_diff(a) <= 6), "{saved:?} vs {balanced:?}");
}

#[test]
fn shadows_highlights_render_and_save() {
    let (dir, p) = temp_copies(&["adjust/window.jpg"]);
    let photo = &p[0];
    let out = dir.path().join("out.png").to_string_lossy().into_owned();
    // The dark wall left of the window.
    let pixel = |path: &str| image::open(path).unwrap().into_rgba8().get_pixel(160, 100).0;
    // No value: 0.45. "tones" works too.
    let v = json_in(&dir, &["render", photo, "--adjust", "tones", "-o", &out, "--json"], 0);
    assert_eq!(v["ops"][0], json!({"op": "adjust", "tool": "shadows_highlights", "value": 0.45}));
    let lifted = pixel(&out);
    json_in(&dir, &["render", photo, "-o", &out, "--json"], 0);
    let original = pixel(&out);
    assert!(lifted[0] > original[0] + 10, "lifted: {lifted:?} vs {original:?}");
    let v = json_in(&dir, &["render", photo, "--adjust", "shadows_highlights=0", "-o", &out, "--json"], 0);
    assert_eq!(v["ops"].as_array().unwrap().len(), 0, "0 is no step");

    let edits = dir.path().join("e.json");
    std::fs::write(
        &edits,
        json!({"version": 1, "ops": [{"op": "adjust", "tool": "shadows_highlights", "value": 0.45}]})
            .to_string(),
    )
    .unwrap();
    let edits = edits.to_string_lossy().into_owned();
    json_in(&dir, &["render", photo, "--edits", &edits, "-o", &out, "--json"], 0);
    assert_eq!(pixel(&out), lifted);
    let v = json_in(&dir, &["save", photo, "--edits", &edits, "--json"], 0);
    assert_eq!(v["action"], "saved");
    let v = json_in(&dir, &["edits", photo, "--json"], 0);
    assert_eq!((&v["ops"][0]["tool"], &v["dirty"]), (&json!("shadows_highlights"), &json!(false)));
    let saved = image::open(photo).unwrap().into_rgba8().get_pixel(160, 100).0;
    assert!(saved.iter().zip(lifted).all(|(s, a)| s.abs_diff(a) <= 6), "{saved:?} vs {lifted:?}");
}

#[test]
fn local_contrast_render_and_save() {
    let (dir, p) = temp_copies(&["adjust/window.jpg"]);
    let photo = &p[0];
    let out = dir.path().join("out.png").to_string_lossy().into_owned();
    // The spread of the wall's grain, left of the window.
    let spread = |path: &str| {
        let img = image::open(path).unwrap().into_rgba8();
        let v: Vec<u8> = (140..180).map(|x| img.get_pixel(x, 100).0[1]).collect();
        v.iter().max().unwrap() - v.iter().min().unwrap()
    };
    // No value: 1.25. "clarity" works too.
    let v = json_in(&dir, &["render", photo, "--adjust", "clarity=2", "-o", &out, "--json"], 0);
    assert_eq!(v["ops"][0], json!({"op": "adjust", "tool": "local_contrast", "value": 2.0}));
    let boosted = spread(&out);
    let v = json_in(&dir, &["render", photo, "--adjust", "local_contrast", "-o", &out, "--json"], 0);
    assert_eq!(v["ops"][0]["value"], json!(1.25));
    json_in(&dir, &["render", photo, "-o", &out, "--json"], 0);
    let original = spread(&out);
    assert!(boosted > original + 2, "detail: {boosted} vs {original}");
    let v = json_in(&dir, &["render", photo, "--adjust", "local_contrast=1", "-o", &out, "--json"], 0);
    assert_eq!(v["ops"].as_array().unwrap().len(), 0, "100% is no step");

    let edits = dir.path().join("e.json");
    std::fs::write(
        &edits,
        json!({"version": 1, "ops": [{"op": "adjust", "tool": "local_contrast", "value": 2.0}]}).to_string(),
    )
    .unwrap();
    let edits = edits.to_string_lossy().into_owned();
    json_in(&dir, &["render", photo, "--edits", &edits, "-o", &out, "--json"], 0);
    assert_eq!(spread(&out), boosted);
    let v = json_in(&dir, &["save", photo, "--edits", &edits, "--json"], 0);
    assert_eq!(v["action"], "saved");
    let v = json_in(&dir, &["edits", photo, "--json"], 0);
    assert_eq!((&v["ops"][0]["tool"], &v["dirty"]), (&json!("local_contrast"), &json!(false)));
    assert!(spread(photo).abs_diff(boosted) <= 4, "saved: {} vs {boosted}", spread(photo));
}

#[test]
fn contrast_render_and_save() {
    let (dir, p) = temp_copies(&["adjust/window.jpg"]);
    let photo = &p[0];
    let out = dir.path().join("out.png").to_string_lossy().into_owned();
    // The dark wall left of the window, and the bright sky in it.
    let green = |path: &str, x: u32, y: u32| image::open(path).unwrap().into_rgba8().get_pixel(x, y).0[1];
    let tones = |path: &str| (green(path, 150, 100), green(path, 220, 150));
    json_in(&dir, &["render", photo, "-o", &out, "--json"], 0);
    let (wall, window) = tones(&out);
    // No value: 1.2.
    let v = json_in(&dir, &["render", photo, "--adjust", "contrast", "-o", &out, "--json"], 0);
    assert_eq!(v["ops"][0], json!({"op": "adjust", "tool": "contrast", "value": 1.2}));
    let v = json_in(&dir, &["render", photo, "--adjust", "contrast=2", "-o", &out, "--json"], 0);
    assert_eq!(v["ops"][0]["value"], json!(2.0));
    let strong = tones(&out);
    assert!(strong.0 + 3 < wall && strong.1 > window + 3, "{strong:?} vs {:?}", (wall, window));
    let v = json_in(&dir, &["render", photo, "--adjust", "contrast=1", "-o", &out, "--json"], 0);
    assert_eq!(v["ops"].as_array().unwrap().len(), 0, "1 is no step");

    let edits = dir.path().join("e.json");
    std::fs::write(
        &edits,
        json!({"version": 1, "ops": [{"op": "adjust", "tool": "contrast", "value": 2.0}]}).to_string(),
    )
    .unwrap();
    let edits = edits.to_string_lossy().into_owned();
    json_in(&dir, &["render", photo, "--edits", &edits, "-o", &out, "--json"], 0);
    assert_eq!(tones(&out), strong);
    let v = json_in(&dir, &["save", photo, "--edits", &edits, "--json"], 0);
    assert_eq!(v["action"], "saved");
    let v = json_in(&dir, &["edits", photo, "--json"], 0);
    assert_eq!((&v["ops"][0]["tool"], &v["dirty"]), (&json!("contrast"), &json!(false)));
    let saved = tones(photo);
    assert!(
        saved.0.abs_diff(strong.0) <= 4 && saved.1.abs_diff(strong.1) <= 4,
        "saved: {saved:?} vs {strong:?}"
    );
}

#[test]
fn sharpen_render_and_save() {
    let (dir, p) = temp_copies(&["adjust/window.jpg"]);
    let photo = &p[0];
    let out = dir.path().join("out.png").to_string_lossy().into_owned();
    // The window bar's middle, against the sky (x 248..252, y 45).
    let bar = |path: &str| image::open(path).unwrap().into_rgba8().get_pixel(250, 45).0[0];
    let v = json_in(&dir, &["render", photo, "--adjust", "sharpen", "-o", &out, "--json"], 0);
    assert_eq!(v["ops"][0], json!({"op": "adjust", "tool": "sharpen", "value": 0.5}));
    json_in(&dir, &["render", photo, "--adjust", "sharpen=1", "-o", &out, "--json"], 0);
    let sharpened = bar(&out);
    json_in(&dir, &["render", photo, "-o", &out, "--json"], 0);
    let original = bar(&out);
    assert!(sharpened + 3 < original, "bar: {sharpened} vs {original}");
    let v = json_in(&dir, &["render", photo, "--adjust", "sharpen=0", "-o", &out, "--json"], 0);
    assert_eq!(v["ops"].as_array().unwrap().len(), 0, "0 is no step");

    let edits = dir.path().join("e.json");
    std::fs::write(
        &edits,
        json!({"version": 1, "ops": [{"op": "adjust", "tool": "sharpen", "value": 1.0}]}).to_string(),
    )
    .unwrap();
    let edits = edits.to_string_lossy().into_owned();
    json_in(&dir, &["render", photo, "--edits", &edits, "-o", &out, "--json"], 0);
    assert_eq!(bar(&out), sharpened);
    let v = json_in(&dir, &["save", photo, "--edits", &edits, "--json"], 0);
    assert_eq!(v["action"], "saved");
    let v = json_in(&dir, &["edits", photo, "--json"], 0);
    assert_eq!((&v["ops"][0]["tool"], &v["dirty"]), (&json!("sharpen"), &json!(false)));
    assert!(bar(photo).abs_diff(sharpened) <= 6, "saved: {} vs {sharpened}", bar(photo));
}

#[test]
fn dehaze_render_and_save() {
    let (dir, p) = temp_copies(&["adjust/woods-haze.jpg"]);
    let photo = &p[0];
    let out = dir.path().join("out.png").to_string_lossy().into_owned();
    // The middle ridge's green, in haze.
    let ridge = |path: &str| image::open(path).unwrap().into_rgba8().get_pixel(160, 150).0[1];
    json_in(&dir, &["render", photo, "-o", &out, "--json"], 0);
    let hazy = ridge(&out);
    // No value: 0.2. "haze_removal" works too.
    let v = json_in(&dir, &["render", photo, "--adjust", "dehaze", "-o", &out, "--json"], 0);
    assert_eq!(v["ops"][0], json!({"op": "adjust", "tool": "dehaze", "value": 0.2}));
    let clear = ridge(&out);
    assert!(clear + 2 < hazy, "out of the haze: {clear} vs {hazy}");
    let v = json_in(&dir, &["render", photo, "--adjust", "haze_removal=0", "-o", &out, "--json"], 0);
    assert_eq!(v["ops"].as_array().unwrap().len(), 0, "0 is no step");

    let edits = dir.path().join("e.json");
    std::fs::write(
        &edits,
        json!({"version": 1, "ops": [{"op": "adjust", "tool": "dehaze", "value": 0.2}]}).to_string(),
    )
    .unwrap();
    let edits = edits.to_string_lossy().into_owned();
    json_in(&dir, &["render", photo, "--edits", &edits, "-o", &out, "--json"], 0);
    assert_eq!(ridge(&out), clear);
    let v = json_in(&dir, &["save", photo, "--edits", &edits, "--json"], 0);
    assert_eq!(v["action"], "saved");
    let v = json_in(&dir, &["edits", photo, "--json"], 0);
    assert_eq!((&v["ops"][0]["tool"], &v["dirty"]), (&json!("dehaze"), &json!(false)));
    assert!(ridge(photo).abs_diff(clear) <= 3, "saved: {} vs {clear}", ridge(photo));
}
