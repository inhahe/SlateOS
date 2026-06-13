#![deny(clippy::all)]

//! kdenlive-cli — SlateOS Kdenlive KDE video editor
//!
//! Single personality: `kdenlive`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kd(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kdenlive [OPTIONS] [PROJECT]");
        println!("Kdenlive 24.05 (SlateOS) — KDE-based open-source NLE");
        println!();
        println!("Options:");
        println!("  -i FILE                Open project file");
        println!("  --mlt                  Use specific MLT version");
        println!("  --config FILE          Use config file");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Kdenlive 24.05.2 (SlateOS)"); return 0; }
    println!("Kdenlive 24.05.2 (SlateOS)");
    println!("  Engine: MLT framework + Qt/KF5");
    println!("  Tracks: Unlimited video & audio with grouping");
    println!("  Features: Proxy editing, Motion Tracker, AI Subtitle, Speech-to-text");
    println!("  Effects: Color correction, keying, transitions, audio mixer");
    println!("  Rendering: All FFmpeg formats with custom render profiles");
    println!("  License: GNU GPLv2+");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kdenlive".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kd(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_kd};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kdenlive"), "kdenlive");
        assert_eq!(basename(r"C:\bin\kdenlive.exe"), "kdenlive.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kdenlive.exe"), "kdenlive");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_kd(&["--help".to_string()], "kdenlive"), 0);
        assert_eq!(run_kd(&["-h".to_string()], "kdenlive"), 0);
        let _ = run_kd(&["--version".to_string()], "kdenlive");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_kd(&[], "kdenlive");
    }
}
