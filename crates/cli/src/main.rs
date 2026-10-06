//! `pcr`: command-line front end for Photo Cruiser.
//!
//! `--json` output is pretty-printed JSON on stdout, one document per invocation; field
//! names are stable. Errors always go to stderr as a human message; with `--json` stdout
//! additionally gets `{"error": {"kind": ..., "message": ...}}`.
//!
//! Exit codes: 0 ok, 1 usage, 2 I/O (missing files, write/encode failures, unsupported
//! output extension, missing `photo-cruiser` binary), 3 decode (corrupt or unsupported
//! input image, invalid edits JSON, a crop that is empty after clamping to the image).

use clap::{Parser, Subcommand};
use photo_cruiser_core::export::{self, OutFormat};
use photo_cruiser_core::render::{self, RenderOpts};
use photo_cruiser_core::{EditStack, Error, Folder, Op, Probe, Rect};
use serde::Serialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

const AFTER_HELP: &str = "\
--json prints pretty JSON to stdout; on failure {\"error\": {\"kind\", \"message\"}}.
Exit codes: 0 ok, 1 usage, 2 I/O (incl. encode, unsupported output format),
3 decode (incl. unsupported input, invalid edits JSON, crop outside the image).";

#[derive(Parser)]
#[command(name = "pcr", version, about = "Photo Cruiser command line", after_help = AFTER_HELP)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List the photos of a folder in display order, with 0-based indices.
    List {
        dir: PathBuf,
        /// Also read format, size and orientation of each entry (errors are per entry).
        #[arg(long)]
        probe: bool,
        #[arg(long)]
        json: bool,
    },
    /// Format, displayed size, EXIF orientation and full decode time of a photo.
    Info {
        file: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Render a photo with edits to PNG or JPEG (chosen by the output extension).
    Render {
        file: PathBuf,
        /// Edit stack JSON (`{"version": 1, "ops": [...]}`).
        #[arg(long)]
        edits: Option<PathBuf>,
        /// Crop `x,y,w,h` in source pixels, appended after the edits. Clamped to the
        /// image; a crop fully outside it is an `empty_crop` error.
        #[arg(long, value_parser = parse_crop)]
        crop: Option<Rect>,
        /// Downscale the result so its longer side is at most N.
        #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
        max_side: Option<u32>,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Export a photo at full resolution with an edit stack applied.
    Export {
        file: PathBuf,
        #[arg(long)]
        edits: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        /// JPEG quality 1-100.
        #[arg(long, default_value_t = 90, value_parser = clap::value_parser!(u8).range(1..=100))]
        quality: u8,
        #[arg(long)]
        json: bool,
    },
    /// Launch the GUI (`photo-cruiser` next to this binary); further args are passed through.
    App {
        dir: PathBuf,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

/// A failure with its exit code and machine-readable kind.
#[derive(Debug)]
struct Fail {
    code: u8,
    kind: &'static str,
    message: String,
}

impl Fail {
    fn new(code: u8, kind: &'static str, message: impl ToString) -> Self {
        Fail { code, kind, message: message.to_string().trim().to_string() }
    }
}

impl From<Error> for Fail {
    fn from(e: Error) -> Self {
        let code = match e {
            Error::Io { .. } | Error::Encode { .. } | Error::UnsupportedOutput { .. } => 2,
            Error::Unsupported { .. } | Error::Decode { .. } | Error::Edits(_) | Error::EmptyCrop(_) => 3,
        };
        Fail::new(code, e.kind(), e)
    }
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) if !e.use_stderr() => {
            // --help / --version
            let _ = e.print();
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            let _ = e.print();
            if std::env::args().any(|a| a == "--json") {
                let text = e.to_string();
                let msg = text.lines().next().unwrap_or_default().trim_start_matches("error: ");
                print_json(&error_json(&Fail::new(1, "usage", msg)));
            }
            return ExitCode::from(1);
        }
    };
    let json = match &cli.cmd {
        Cmd::List { json, .. }
        | Cmd::Info { json, .. }
        | Cmd::Render { json, .. }
        | Cmd::Export { json, .. } => *json,
        Cmd::App { .. } => false,
    };
    match run(cli.cmd) {
        Ok(code) => code,
        Err(f) => {
            eprintln!("pcr: {}", f.message);
            if json {
                print_json(&error_json(&f));
            }
            ExitCode::from(f.code)
        }
    }
}

fn run(cmd: Cmd) -> Result<ExitCode, Fail> {
    match cmd {
        Cmd::List { dir, probe, json } => list(&dir, probe, json)?,
        Cmd::Info { file, json } => info(&file, json)?,
        Cmd::Render { file, edits, crop, max_side, output, json } => {
            let mut stack = edits.as_deref().map(read_edits).transpose()?.unwrap_or_default();
            if let Some(r) = crop {
                stack.ops.push(Op::Crop(r));
            }
            write_rendered(&file, &stack, max_side, &output, 90, json)?
        }
        Cmd::Export { file, edits, output, quality, json } => {
            write_rendered(&file, &read_edits(&edits)?, None, &output, quality, json)?
        }
        Cmd::App { dir, args } => return app(&dir, &args),
    }
    Ok(ExitCode::SUCCESS)
}

#[derive(Serialize)]
struct ListJson<'a> {
    folder: &'a Path,
    count: usize,
    entries: &'a [ListEntry<'a>],
}

/// One `list` entry; `probe` fields are flattened in when `--probe` is given.
#[derive(Serialize)]
struct ListEntry<'a> {
    index: usize,
    name: &'a str,
    path: &'a Path,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    probe: Option<ProbeResult>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum ProbeResult {
    Ok(Probe),
    Err { error: ErrorJson },
}

#[derive(Serialize)]
struct ErrorJson {
    kind: &'static str,
    message: String,
}

#[derive(Serialize)]
struct InfoJson<'a> {
    path: &'a Path,
    #[serde(flatten)]
    probe: Probe,
    decode_ms: f64,
}

fn list(dir: &Path, probe: bool, json: bool) -> Result<(), Fail> {
    let folder = Folder::open(dir)?;
    let entries: Vec<ListEntry> = folder
        .entries
        .iter()
        .enumerate()
        .map(|(index, e)| ListEntry {
            index,
            name: &e.name,
            path: &e.path,
            probe: probe.then(|| match photo_cruiser_core::probe(&e.path) {
                Ok(p) => ProbeResult::Ok(p),
                Err(err) => {
                    ProbeResult::Err { error: ErrorJson { kind: err.kind(), message: err.to_string() } }
                }
            }),
        })
        .collect();
    if json {
        print_json(&ListJson { folder: &folder.path, count: entries.len(), entries: &entries });
        return Ok(());
    }
    for e in &entries {
        match &e.probe {
            None => println!("{}\t{}", e.index, e.name),
            Some(ProbeResult::Ok(p)) => println!(
                "{}\t{}\t{}\t{}x{}\torientation {}",
                e.index, e.name, p.format, p.width, p.height, p.orientation
            ),
            Some(ProbeResult::Err { error }) => println!("{}\t{}\terror: {}", e.index, e.name, error.message),
        }
    }
    Ok(())
}

fn info(file: &Path, json: bool) -> Result<(), Fail> {
    let probe = photo_cruiser_core::probe(file)?;
    let t = Instant::now();
    photo_cruiser_core::decode(file)?;
    let decode_ms = (t.elapsed().as_secs_f64() * 1e5).round() / 100.0;
    if json {
        print_json(&InfoJson { path: file, probe, decode_ms });
    } else {
        println!("path: {}", file.display());
        println!("format: {}", probe.format);
        println!("size: {}x{}", probe.width, probe.height);
        println!("orientation: {}", probe.orientation);
        println!("decode: {decode_ms} ms");
    }
    Ok(())
}

fn write_rendered(
    file: &Path,
    edits: &EditStack,
    max_side: Option<u32>,
    output: &Path,
    quality: u8,
    json: bool,
) -> Result<(), Fail> {
    // Check the output extension before the (possibly slow) decode.
    let format = OutFormat::from_path(output, quality)?;
    let src = photo_cruiser_core::decode(file)?;
    let img = render::render(&src, edits, RenderOpts { max_side })?;
    export::save(&img, output, format)?;
    let (width, height) = img.dimensions();
    if json {
        print_json(&RenderJson { path: output, width, height });
    } else {
        println!("{} {width}x{height}", output.display());
    }
    Ok(())
}

#[derive(Serialize)]
struct RenderJson<'a> {
    path: &'a Path,
    width: u32,
    height: u32,
}

fn read_edits(path: &Path) -> Result<EditStack, Fail> {
    Ok(EditStack::load(path)?)
}

/// Run the GUI binary that sits next to `pcr`, forwarding args and exit code.
fn app(dir: &Path, args: &[String]) -> Result<ExitCode, Fail> {
    let exe = std::env::current_exe().map_err(|e| Fail::new(2, "io", e))?;
    let gui = exe.with_file_name(format!("photo-cruiser{}", std::env::consts::EXE_SUFFIX));
    if !gui.is_file() {
        return Err(Fail::new(
            2,
            "io",
            format!("{} not found; build it with `cargo build -p photo-cruiser-app`", gui.display()),
        ));
    }
    let status = std::process::Command::new(&gui)
        .arg(dir)
        .args(args)
        .status()
        .map_err(|e| Fail::new(2, "io", format!("{}: {e}", gui.display())))?;
    Ok(ExitCode::from(status.code().map_or(1, |c| c.clamp(0, 255) as u8)))
}

/// `x,y,w,h` in source pixels; `w` and `h` must be positive.
fn parse_crop(s: &str) -> Result<Rect, String> {
    let v: Vec<f32> =
        s.split(',').map(|p| p.trim().parse::<f32>()).collect::<Result<_, _>>().map_err(|e| e.to_string())?;
    match v[..] {
        [x, y, w, h] if v.iter().all(|n| n.is_finite()) && w > 0.0 && h > 0.0 => Ok(Rect::new(x, y, w, h)),
        _ => Err("expected x,y,w,h with w,h > 0".into()),
    }
}

fn error_json(f: &Fail) -> Value {
    json!({"error": ErrorJson { kind: f.kind, message: f.message.clone() }})
}

fn print_json(v: &impl Serialize) {
    println!("{}", serde_json::to_string_pretty(v).expect("JSON value serialises"));
}
