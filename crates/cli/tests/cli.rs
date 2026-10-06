//! `pcr` end to end against `fixtures/`.

use assert_cmd::Command;
use serde_json::Value;
use std::path::PathBuf;

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
