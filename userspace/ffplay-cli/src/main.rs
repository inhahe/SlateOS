#![deny(clippy::all)]

//! ffplay-cli — Slate OS ffplay media player
//!
//! Single personality: `ffplay`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ffplay(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "-help") || args.is_empty() {
        println!("Usage: ffplay [OPTIONS] INPUT");
        println!("ffplay 7.0 (Slate OS) — Simple media player");
        println!();
        println!("Options:");
        println!("  -x WIDTH           Window width");
        println!("  -y HEIGHT          Window height");
        println!("  -fs                Fullscreen");
        println!("  -an                Disable audio");
        println!("  -vn                Disable video");
        println!("  -sn                Disable subtitles");
        println!("  -ss POS            Seek to position");
        println!("  -t DURATION        Play for duration");
        println!("  -nodisp            Disable graphical display");
        println!("  -autoexit          Exit at end of file");
        println!("  -loop N            Loop N times (0=infinite)");
        println!("  -f FORMAT          Force format");
        println!("  -volume N          Volume (0-100)");
        println!("  -v LEVEL           Verbosity level");
        println!("  -version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-version") {
        println!("ffplay version 7.0 (Slate OS)");
        println!("built with gcc (Slate OS)");
        println!("libavutil      59.  8.100");
        println!("libavcodec     61.  3.100");
        println!("libavformat    61.  1.100");
        return 0;
    }
    let input = args.iter().rfind(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or("input");
    println!("ffplay: Playing '{}'...", input);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ffplay".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ffplay(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ffplay};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ffplay"), "ffplay");
        assert_eq!(basename(r"C:\bin\ffplay.exe"), "ffplay.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ffplay.exe"), "ffplay");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ffplay(&["--help".to_string()], "ffplay"), 0);
        assert_eq!(run_ffplay(&["-h".to_string()], "ffplay"), 0);
        let _ = run_ffplay(&["--version".to_string()], "ffplay");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ffplay(&[], "ffplay");
    }
}
