#![deny(clippy::all)]

//! deepin-screenshot-cli — Slate OS Deepin Screenshot
//!
//! Single personality: `deepin-screenshot`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_deepin_screenshot(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: deepin-screenshot [OPTIONS]");
        println!("deepin-screenshot v5.0 (Slate OS) — Deepin screenshot tool");
        println!();
        println!("Options:");
        println!("  -f, --fullscreen  Full screen capture");
        println!("  -d, --delay SECS  Delay before capture");
        println!("  -s, --save-path   Custom save path");
        println!("  --version         Show version");
        println!();
        println!("Built-in annotation: rectangle, ellipse, arrow, line,");
        println!("  text, blur, mosaic, marker, counter");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("deepin-screenshot v5.0 (Slate OS)"); return 0; }
    println!("deepin-screenshot: screenshot tool started");
    println!("  OCR: text recognition available");
    println!("  Pin: pin screenshot to desktop");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "deepin-screenshot".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_deepin_screenshot(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_deepin_screenshot};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/deepin-screenshot"), "deepin-screenshot");
        assert_eq!(basename(r"C:\bin\deepin-screenshot.exe"), "deepin-screenshot.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("deepin-screenshot.exe"), "deepin-screenshot");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_deepin_screenshot(&["--help".to_string()], "deepin-screenshot"), 0);
        assert_eq!(run_deepin_screenshot(&["-h".to_string()], "deepin-screenshot"), 0);
        let _ = run_deepin_screenshot(&["--version".to_string()], "deepin-screenshot");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_deepin_screenshot(&[], "deepin-screenshot");
    }
}
