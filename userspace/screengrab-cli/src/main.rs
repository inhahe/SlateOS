#![deny(clippy::all)]

//! screengrab-cli — Slate OS ScreenGrab screenshot tool
//!
//! Single personality: `screengrab`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_screengrab(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: screengrab [OPTIONS]");
        println!("screengrab v2.7 (Slate OS) — Qt-based screenshot tool");
        println!();
        println!("Options:");
        println!("  --fullscreen      Full screen capture");
        println!("  --window          Active window capture");
        println!("  --region          Region selection");
        println!("  --delay SECS      Delay before capture");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("screengrab v2.7 (Slate OS)"); return 0; }
    println!("screengrab: screenshot tool started");
    println!("  Modes: fullscreen, window, region");
    println!("  Format: PNG, JPEG, BMP");
    println!("  Upload: configured services");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "screengrab".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_screengrab(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_screengrab};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/screengrab"), "screengrab");
        assert_eq!(basename(r"C:\bin\screengrab.exe"), "screengrab.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("screengrab.exe"), "screengrab");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_screengrab(&["--help".to_string()], "screengrab"), 0);
        assert_eq!(run_screengrab(&["-h".to_string()], "screengrab"), 0);
        let _ = run_screengrab(&["--version".to_string()], "screengrab");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_screengrab(&[], "screengrab");
    }
}
