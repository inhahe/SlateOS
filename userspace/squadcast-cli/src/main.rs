#![deny(clippy::all)]

//! squadcast-cli — Slate OS SquadCast remote podcast recording
//!
//! Single personality: `squadcast`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: squadcast [COMMAND] [OPTIONS]");
        println!("SquadCast (Descript) (Slate OS) — Studio-quality remote recording");
        println!();
        println!("Commands:");
        println!("  schedule TITLE         Schedule a recording session");
        println!("  invite EMAIL           Invite guest");
        println!("  sessions               List sessions");
        println!("  download ID            Download separate tracks");
        println!("  transcribe ID          Auto-transcribe + diarization");
        println!();
        println!("Options:");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("SquadCast (Descript Acquired) v4.0 (Slate OS)"); return 0; }
    println!("SquadCast (Slate OS) — now part of Descript");
    println!("  Recording: Progressive Upload (local + uploads as you record)");
    println!("  Quality: 48 kHz WAV per guest, up to 1080p video");
    println!("  Reliability: Crash recovery, automatic file integrity");
    println!("  Workflow: Hand-off to Descript for transcript-based editing");
    println!("  License: Indie / Studio / Network / Producer subscription");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "squadcast".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sc(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/squadcast"), "squadcast");
        assert_eq!(basename(r"C:\bin\squadcast.exe"), "squadcast.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("squadcast.exe"), "squadcast");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sc(&["--help".to_string()], "squadcast"), 0);
        assert_eq!(run_sc(&["-h".to_string()], "squadcast"), 0);
        let _ = run_sc(&["--version".to_string()], "squadcast");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sc(&[], "squadcast");
    }
}
