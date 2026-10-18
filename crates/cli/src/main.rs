//! `pcr`: command-line front end for Photo Cruiser.
//!
//! `--json` output is pretty-printed JSON on stdout, one document per invocation; field
//! names are stable. Errors always go to stderr as a human message; with `--json` stdout
//! additionally gets `{"error": {"kind": ..., "message": ...}}`.
//!
//! Exit codes: 0 ok, 1 usage, 2 I/O (missing files, write/encode failures, unsupported
//! output extension, a read-only folder or a taken name on save, missing `photo-cruiser`
//! binary), 3 decode (corrupt or unsupported input image, invalid edits JSON, a crop that
//! is empty after clamping to the image, an animated or 16-bit image on save).

mod skill;

use clap::{CommandFactory, Parser, Subcommand};
use photo_cruiser_core::adjust;
use photo_cruiser_core::export::{self, OutFormat};
use photo_cruiser_core::render::{self, RenderOpts};
use photo_cruiser_core::store::{self, PhotoEdits};
use photo_cruiser_core::{EditStack, Error, Folder, Op, Probe, Rect, Tool};
use serde::Serialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

const AFTER_HELP: &str = "\
--json prints pretty JSON to stdout; on failure {\"error\": {\"kind\", \"message\"}}.
Exit codes: 0 ok, 1 usage, 2 I/O (incl. encode, unsupported output format, read-only
folder, name taken), 3 decode (incl. unsupported input, invalid edits JSON, crop outside
the image, animated or 16-bit image).";

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
    /// Render a photo with edits to PNG or JPEG (chosen by the output extension). Starts
    /// from the original (the backup, if the photo was saved) with the photo's own edits.
    Render {
        file: PathBuf,
        /// Edit stack JSON (`{"version": 1, "ops": [...]}`), instead of the photo's own edits.
        #[arg(long, conflicts_with = "no_edits")]
        edits: Option<PathBuf>,
        /// Ignore the photo's own edits.
        #[arg(long)]
        no_edits: bool,
        /// Crop `x,y,w,h` in source pixels, appended after the edits. Clamped to the
        /// image; a crop fully outside it is an `empty_crop` error.
        #[arg(long, value_parser = parse_crop)]
        crop: Option<Rect>,
        /// A one-slider tool `TOOL[=VALUE]` (e.g. `exposure=0.5`), set after the edits and
        /// `--crop`. Without a value: the value the app opens the tool at (auto, measured
        /// on the cropped photo). Repeatable. Tools: levels (auto levels strength, 0..2,
        /// opens at 1), exposure (EV, -2..2), colour (× darktable's "basic colorfulness:
        /// standard", 0..2, opens at 1).
        #[arg(long, value_name = "TOOL[=VALUE]", value_parser = parse_adjust)]
        adjust: Vec<(Tool, Option<f32>)>,
        /// Downscale the result so its longer side is at most N.
        #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
        max_side: Option<u32>,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Export a photo at full resolution with its edits applied, to a new file. Starts from
    /// the original (the backup, if the photo was saved).
    Export {
        file: PathBuf,
        /// Edit stack JSON, instead of the photo's own edits.
        #[arg(long, conflicts_with = "no_edits")]
        edits: Option<PathBuf>,
        /// Ignore the photo's own edits.
        #[arg(long)]
        no_edits: bool,
        #[arg(short, long)]
        output: PathBuf,
        /// JPEG quality 1-100.
        #[arg(long, default_value_t = 90, value_parser = clap::value_parser!(u8).range(1..=100))]
        quality: u8,
        #[arg(long)]
        json: bool,
    },
    /// A photo's edits: the current stack, the saved one, dirty, and the backup.
    Edits {
        file: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Render the photo's edits (or `--edits`, which become its edits) into the photo file.
    /// The original is kept in `.photo-cruiser/originals/`.
    Save {
        file: PathBuf,
        #[arg(long)]
        edits: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Restore the original photo and drop its edits.
    Revert {
        file: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Launch the GUI (`photo-cruiser` next to this binary); further args are passed through.
    App {
        dir: PathBuf,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Print a Claude Code skill (SKILL.md) that teaches an agent to use pcr: the files,
    /// the edits format and every command.
    Skill {
        /// Write `<DIR>/SKILL.md` instead (e.g. `~/.claude/skills/pcr`), creating DIR.
        #[arg(short, long, value_name = "DIR")]
        output: Option<PathBuf>,
        #[arg(long)]
        json: bool,
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
            Error::Io { .. }
            | Error::Encode { .. }
            | Error::UnsupportedOutput { .. }
            | Error::ReadOnly { .. }
            | Error::NameTaken { .. } => 2,
            Error::Unsupported { .. }
            | Error::Decode { .. }
            | Error::Edits(_)
            | Error::EmptyCrop(_)
            | Error::Animated { .. }
            | Error::HighBitDepth { .. }
            | Error::StaleEdits { .. } => 3,
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
        | Cmd::Export { json, .. }
        | Cmd::Edits { json, .. }
        | Cmd::Save { json, .. }
        | Cmd::Revert { json, .. }
        | Cmd::Skill { json, .. } => *json,
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
        Cmd::Render { file, edits, no_edits, crop, adjust, max_side, output, json } => {
            let mut stack = stack_for(&file, edits.as_deref(), no_edits)?;
            if let Some(r) = crop {
                stack.ops.push(Op::Crop(r));
            }
            write_rendered(&file, &stack, &adjust, max_side, &output, 90, json)?
        }
        Cmd::Export { file, edits, no_edits, output, quality, json } => {
            let stack = stack_for(&file, edits.as_deref(), no_edits)?;
            write_rendered(&file, &stack, &[], None, &output, quality, json)?
        }
        Cmd::Edits { file, json } => show_edits(&file, json)?,
        Cmd::Save { file, edits, json } => {
            let stack = match edits {
                Some(path) => {
                    let stack = read_edits(&path)?;
                    store::store_stack(&file, &stack)?;
                    stack
                }
                None => load_edits(&file)?.map(|e| e.stack).unwrap_or_default(),
            };
            print_report(&store::save(&file, &stack)?, json);
        }
        Cmd::Revert { file, json } => print_report(&store::revert(&file)?, json),
        Cmd::App { dir, args } => return app(&dir, &args),
        Cmd::Skill { output, json } => write_skill(output.as_deref(), json)?,
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
    adjust: &[(Tool, Option<f32>)],
    max_side: Option<u32>,
    output: &Path,
    quality: u8,
    json: bool,
) -> Result<(), Fail> {
    // Check the output extension before the (possibly slow) decode.
    let format = OutFormat::from_path(output, quality)?;
    let src = photo_cruiser_core::decode(&store::original(file))?;
    let mut edits = edits.clone();
    for (tool, value) in adjust {
        let value = match value {
            Some(v) => *v,
            None => adjust::opening(&src, &edits, *tool)?,
        };
        edits.set_adjust(*tool, Some(value));
    }
    let img = render::render(&src, &edits, RenderOpts { max_side })?;
    export::save(&img, output, format)?;
    let (width, height) = img.dimensions();
    if json {
        print_json(&RenderJson { path: output, width, height, ops: &edits.ops });
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
    /// The edits rendered (with `--crop` and `--adjust` values filled in).
    ops: &'a [Op],
}

fn read_edits(path: &Path) -> Result<EditStack, Fail> {
    Ok(EditStack::load(path)?)
}

/// The photo's sidecar. Stale edits (the photo changed elsewhere) are a warning.
fn load_edits(file: &Path) -> Result<Option<PhotoEdits>, Fail> {
    match store::load(file) {
        Err(e @ Error::StaleEdits { .. }) => {
            eprintln!("pcr: warning: {e}");
            Ok(None)
        }
        res => Ok(res?),
    }
}

/// `--edits`, else (unless `--no-edits`) the photo's own.
fn stack_for(file: &Path, edits: Option<&Path>, no_edits: bool) -> Result<EditStack, Fail> {
    match edits {
        Some(path) => read_edits(path),
        None if no_edits => Ok(EditStack::default()),
        None => Ok(load_edits(file)?.map(|e| e.stack).unwrap_or_default()),
    }
}

#[derive(Serialize)]
struct EditsJson<'a> {
    path: &'a Path,
    ops: &'a [Op],
    /// `null` if never saved.
    saved: Option<&'a [Op]>,
    dirty: bool,
    backup: Option<PathBuf>,
}

fn show_edits(file: &Path, json: bool) -> Result<(), Fail> {
    if !file.is_file() {
        return Err(Error::io(file, std::io::ErrorKind::NotFound.into()).into());
    }
    let edits = load_edits(file)?.unwrap_or_default();
    let out = EditsJson {
        path: file,
        ops: &edits.stack.ops,
        saved: edits.saved.as_ref().map(|s| s.ops.as_slice()),
        dirty: edits.dirty(),
        backup: store::backup(file),
    };
    if json {
        print_json(&out);
        return Ok(());
    }
    let ops = |ops: &[Op]| serde_json::to_string(ops).expect("ops serialise");
    println!("ops: {}", ops(out.ops));
    println!("saved: {}", out.saved.map_or("never".into(), ops));
    println!("dirty: {}", out.dirty);
    println!("backup: {}", out.backup.map_or("none".into(), |b| b.display().to_string()));
    Ok(())
}

fn print_report(r: &store::SaveReport, json: bool) {
    if json {
        print_json(r);
    } else if r.action == "saved" {
        println!(
            "{} {}x{} {} ({}, {} bytes)",
            r.path.display(),
            r.width,
            r.height,
            r.format,
            r.quality,
            r.bytes
        );
    } else {
        println!("{} {}", r.path.display(), r.action);
    }
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

fn write_skill(dir: Option<&Path>, json: bool) -> Result<(), Fail> {
    let text = skill::markdown(&mut Cli::command());
    let Some(dir) = dir else {
        if json {
            print_json(&json!({"skill": text}));
        } else {
            print!("{text}");
        }
        return Ok(());
    };
    let path = dir.join("SKILL.md");
    std::fs::create_dir_all(dir)
        .and_then(|()| std::fs::write(&path, &text))
        .map_err(|e| Fail::from(Error::io(&path, e)))?;
    if json {
        print_json(&json!({"path": path, "bytes": text.len()}));
    } else {
        println!("{}", path.display());
    }
    Ok(())
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

/// `exposure` or `exposure=0.5`.
fn parse_adjust(s: &str) -> Result<(Tool, Option<f32>), String> {
    let (name, value) = match s.split_once('=') {
        Some((n, v)) => (n, Some(v.trim().parse::<f32>().map_err(|e| format!("{v:?}: {e}"))?)),
        None => (s, None),
    };
    match value {
        Some(v) if !v.is_finite() => Err(format!("{v}: not a number")),
        _ => Ok((name.trim().parse()?, value)),
    }
}

fn error_json(f: &Fail) -> Value {
    json!({"error": ErrorJson { kind: f.kind, message: f.message.clone() }})
}

fn print_json(v: &impl Serialize) {
    println!("{}", serde_json::to_string_pretty(v).expect("JSON value serialises"));
}
