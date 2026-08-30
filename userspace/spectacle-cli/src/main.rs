#![deny(clippy::all)]

//! spectacle-cli — Slate OS KDE Spectacle screenshot tool
//!
//! Single personality: `spectacle`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_spectacle(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: spectacle [OPTIONS]");
        println!("spectacle v24.02 (Slate OS) — KDE screenshot utility");
        println!();
        println!("Options:");
        println!("  -f, --fullscreen  Capture full screen");
        println!("  -a, --activewindow Capture active window");
        println!("  -r, --region      Capture rectangular region");
        println!("  -d, --delay SECS  Delay before capture");
        println!("  -b, --background  Run without GUI");
        println!("  -o FILE           Output file");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("spectacle v24.02 (Slate OS)"); return 0; }
    println!("spectacle: screenshot captured");
    println!("  Mode: full screen");
    println!("  Size: 1920x1080");
    println!("  Format: PNG");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "spectacle".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_spectacle(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_spectacle};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/spectacle"), "spectacle");
        assert_eq!(basename(r"C:\bin\spectacle.exe"), "spectacle.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("spectacle.exe"), "spectacle");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_spectacle(&["--help".to_string()], "spectacle"), 0);
        assert_eq!(run_spectacle(&["-h".to_string()], "spectacle"), 0);
        let _ = run_spectacle(&["--version".to_string()], "spectacle");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_spectacle(&[], "spectacle");
    }
}
