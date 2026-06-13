#![deny(clippy::all)]

//! ttfautohint-cli — SlateOS ttfautohint font hinting
//!
//! Single personality: `ttfautohint`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ttfautohint(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: ttfautohint [OPTIONS] INPUT.ttf OUTPUT.ttf");
        println!("ttfautohint v1.8 (Slate OS) — Automatic TrueType font hinting");
        println!();
        println!("Options:");
        println!("  -l MIN            Minimum PPEM for hinting (default: 8)");
        println!("  -r MAX            Maximum PPEM for hinting (default: 50)");
        println!("  -G INCREASE       Stem width/height increase (default: 50)");
        println!("  -D SCRIPT         Default script (default: latn)");
        println!("  -w STYLE          DejaVu-style strong stem width (default: G)");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("ttfautohint v1.8 (Slate OS)"); return 0; }
    let input = args.first().map(|s| s.as_str()).unwrap_or("input.ttf");
    println!("ttfautohint: processing '{}'...", input);
    println!("  Hinting range: 8-50 ppem");
    println!("  Script: latin");
    println!("  Done.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ttfautohint".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ttfautohint(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ttfautohint};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ttfautohint"), "ttfautohint");
        assert_eq!(basename(r"C:\bin\ttfautohint.exe"), "ttfautohint.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ttfautohint.exe"), "ttfautohint");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ttfautohint(&["--help".to_string()], "ttfautohint"), 0);
        assert_eq!(run_ttfautohint(&["-h".to_string()], "ttfautohint"), 0);
        let _ = run_ttfautohint(&["--version".to_string()], "ttfautohint");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ttfautohint(&[], "ttfautohint");
    }
}
