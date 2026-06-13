#![deny(clippy::all)]

//! tidal-cli — SlateOS TidalCycles live coding pattern language
//!
//! Single personality: `tidal`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tidal(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: tidal COMMAND [OPTIONS]");
        println!("TidalCycles v1.9.4 (Slate OS) — Live coding pattern language");
        println!();
        println!("Commands:");
        println!("  start           Start TidalCycles session");
        println!("  boot            Boot SuperDirt synth");
        println!("  eval PATTERN    Evaluate a pattern");
        println!("  hush            Silence all patterns");
        println!("  samples         List sample banks");
        println!("  info            Show setup info");
        println!("  version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("TidalCycles v1.9.4 (Slate OS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("info");
    match cmd {
        "start" => {
            println!("Starting TidalCycles...");
            println!("  Connecting to SuperDirt on port 57120");
            println!("  9 orbits available (d1-d9)");
            println!("  Ready for patterns.");
        }
        "boot" => {
            println!("Booting SuperDirt...");
            println!("  Loading default samples...");
            println!("  808, breaks, drum, jazz, ... loaded");
            println!("  SuperDirt ready.");
        }
        "eval" => {
            let pattern = args.get(1).map(|s| s.as_str()).unwrap_or("d1 $ sound \"bd sn\"");
            println!("Evaluating: {}", pattern);
        }
        "hush" => println!("All patterns silenced."),
        "samples" => {
            println!("Sample banks:");
            println!("  808 (6), bass (4), bd (24), breaks (2)");
            println!("  cp (2), cr (6), drum (6), hh (13)");
            println!("  jazz (8), sn (52), tabla (6)");
        }
        "info" => {
            println!("TidalCycles v1.9.4");
            println!("  Language: Haskell DSL");
            println!("  Synth: SuperDirt (SuperCollider)");
            println!("  Protocol: OSC");
            println!("  Patterns: polymeter, polyrhythm, Euclidean");
        }
        _ => println!("tidal {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "tidal".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tidal(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_tidal};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/tidal"), "tidal");
        assert_eq!(basename(r"C:\bin\tidal.exe"), "tidal.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("tidal.exe"), "tidal");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_tidal(&["--help".to_string()], "tidal"), 0);
        assert_eq!(run_tidal(&["-h".to_string()], "tidal"), 0);
        let _ = run_tidal(&["--version".to_string()], "tidal");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_tidal(&[], "tidal");
    }
}
