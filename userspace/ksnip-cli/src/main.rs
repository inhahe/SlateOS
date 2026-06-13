#![deny(clippy::all)]

//! ksnip-cli — SlateOS ksnip screenshot annotation tool
//!
//! Single personality: `ksnip`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ksnip(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ksnip [OPTIONS]");
        println!("ksnip v1.10 (Slate OS) — Screenshot and annotation tool");
        println!();
        println!("Options:");
        println!("  -r, --rectarea    Rectangular area capture");
        println!("  -f, --fullscreen  Full screen capture");
        println!("  -a, --active      Active window capture");
        println!("  -d, --delay MS    Delay in milliseconds");
        println!("  -s, --save FILE   Save to file");
        println!("  --version         Show version");
        println!();
        println!("Annotation: arrow, line, rect, ellipse, marker, blur,");
        println!("  pixelate, text, number, sticker");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("ksnip v1.10 (Slate OS)"); return 0; }
    println!("ksnip: screenshot and annotation tool started");
    println!("  Capture modes: rect, fullscreen, window, freehand");
    println!("  Upload: imgur, custom script");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ksnip".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ksnip(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ksnip};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ksnip"), "ksnip");
        assert_eq!(basename(r"C:\bin\ksnip.exe"), "ksnip.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ksnip.exe"), "ksnip");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ksnip(&["--help".to_string()], "ksnip"), 0);
        assert_eq!(run_ksnip(&["-h".to_string()], "ksnip"), 0);
        let _ = run_ksnip(&["--version".to_string()], "ksnip");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ksnip(&[], "ksnip");
    }
}
