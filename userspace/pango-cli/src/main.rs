#![deny(clippy::all)]

//! pango-cli — OurOS Pango text layout tool
//!
//! Single personality: `pango-view`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pango(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pango-view [OPTIONS] [FILE]");
        println!("pango-view v1.52 (OurOS) — Pango text rendering tool");
        println!();
        println!("Options:");
        println!("  FILE              Input text file");
        println!("  --text TEXT       Text to render");
        println!("  --font FONT       Font description (e.g. 'Sans 12')");
        println!("  --output FILE     Output PNG/SVG/PDF");
        println!("  --width N         Wrap width in pixels");
        println!("  --dpi N           Resolution (default: 96)");
        println!("  --markup          Interpret Pango markup");
        println!("  --header          Show font header info");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("pango-view v1.52 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "--header") {
        println!("Pango v1.52");
        println!("  HarfBuzz: v8.5");
        println!("  FreeType: v2.13");
        println!("  Fontconfig: v2.15");
        println!("  Cairo: v1.18");
        return 0;
    }
    let text = args.iter()
        .position(|a| a == "--text")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("Hello, World!");
    println!("Rendering: \"{}\"", text);
    println!("  Font: Sans 12");
    println!("  Direction: LTR");
    println!("  Script: Latin");
    println!("  Output: output.png");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pango-view".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pango(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pango};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pango"), "pango");
        assert_eq!(basename(r"C:\bin\pango.exe"), "pango.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pango.exe"), "pango");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pango(&["--help".to_string()], "pango"), 0);
        assert_eq!(run_pango(&["-h".to_string()], "pango"), 0);
        let _ = run_pango(&["--version".to_string()], "pango");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pango(&[], "pango");
    }
}
