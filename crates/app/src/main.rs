//! Photo Cruiser desktop app (iced 0.14).
//!
//! ```text
//! photo-cruiser [DIR] [--script steps.json] [--shot out.png] [--dump-state state.json]
//!               [--max-side N] [--transition MS] [--timeout SECS]
//! ```
//! Any of `--script/--shot/--dump-state` switches to script mode (see [`script`]).
//! Exit codes: 0 ok, 1 runtime failure (window, writing PNG/JSON), 2 usage or bad
//! script, 3 script-mode timeout.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::Ordering;
use std::time::Duration;

mod action;
mod app;
mod crop_overlay;
mod grid;
mod message;
mod script;
mod state;
#[cfg(test)]
mod tests;
mod theme;
mod view;

const USAGE: &str = "usage: photo-cruiser [DIR] [--script steps.json] [--shot out.png] \
[--dump-state state.json] [--max-side N] [--transition MS] [--timeout SECS]";

struct Args {
    cfg: app::Config,
    timeout: Duration,
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut cfg = app::Config::default();
    let mut script_path = None;
    let mut script = script::ScriptConfig::default();
    let mut timeout = Duration::from_secs(30);
    let mut scripted = false;
    while let Some(arg) = args.next() {
        scripted |= matches!(arg.as_str(), "--script" | "--shot" | "--dump-state");
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "-h" | "--help" => return Err(String::new()),
            "--script" => script_path = Some(PathBuf::from(value("--script")?)),
            "--shot" => script.shot = Some(value("--shot")?.into()),
            "--dump-state" => script.dump = Some(value("--dump-state")?.into()),
            "--max-side" => {
                cfg.max_side = value("--max-side")?.parse().map_err(|e| format!("--max-side: {e}"))?;
            }
            "--transition" => {
                let ms: u64 = value("--transition")?.parse().map_err(|e| format!("--transition: {e}"))?;
                cfg.transition = Duration::from_millis(ms);
            }
            "--timeout" => {
                let secs: f64 = value("--timeout")?.parse().map_err(|e| format!("--timeout: {e}"))?;
                timeout = Duration::from_secs_f64(secs);
            }
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            _ if cfg.folder.is_some() => return Err(format!("unexpected argument {arg}")),
            _ => cfg.folder = Some(arg.into()),
        }
    }
    if let Some(path) = script_path {
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        script.steps =
            action::parse_script(&text).map_err(|e| format!("bad script {}: {e}", path.display()))?;
    }
    if scripted {
        cfg.script = Some(script);
    }
    Ok(Args { cfg, timeout })
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) if e.is_empty() => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("photo-cruiser: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    if args.cfg.script.is_some() {
        // Watchdog: a stuck script must never leave a window behind.
        let timeout = args.timeout;
        std::thread::spawn(move || {
            std::thread::sleep(timeout);
            eprintln!("photo-cruiser: script timed out after {timeout:?}");
            std::process::exit(3);
        });
    }
    if let Err(e) = app::application(args.cfg).run() {
        eprintln!("photo-cruiser: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::from(script::EXIT_CODE.load(Ordering::SeqCst) as u8)
}
