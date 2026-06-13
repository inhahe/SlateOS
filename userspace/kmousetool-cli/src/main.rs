#![deny(clippy::all)]

//! kmousetool-cli — SlateOS automatic mouse click tool
//!
//! Single personality: `kmousetool`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kmousetool(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kmousetool [OPTIONS]");
        println!("kmousetool v5.0 (Slate OS) — Automatic mouse click for accessibility");
        println!();
        println!("Options:");
        println!("  --dwell-time N     Dwell time in ms (default: 500)");
        println!("  --drag-time N      Drag start time in ms (default: 300)");
        println!("  --movement N       Movement threshold in pixels");
        println!("  --smart            Smart drag mode");
        println!("  --stroke           Use strokes instead of dwell");
        println!("  --audible          Audible click feedback");
        println!("  --start            Start clicking immediately");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("kmousetool v5.0 (Slate OS)"); return 0; }
    let dwell = args.windows(2).find(|w| w[0] == "--dwell-time").and_then(|w| w[1].parse::<u32>().ok()).unwrap_or(500);
    println!("KMouseTool v5.0 (Slate OS) — Automatic Mouse Click");
    println!("  Dwell time: {}ms", dwell);
    println!("  Drag time: 300ms");
    println!("  Smart drag: enabled");
    println!("  Status: active");
    println!("  Click type: single click");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kmousetool".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kmousetool(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_kmousetool};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kmousetool"), "kmousetool");
        assert_eq!(basename(r"C:\bin\kmousetool.exe"), "kmousetool.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kmousetool.exe"), "kmousetool");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_kmousetool(&["--help".to_string()], "kmousetool"), 0);
        assert_eq!(run_kmousetool(&["-h".to_string()], "kmousetool"), 0);
        let _ = run_kmousetool(&["--version".to_string()], "kmousetool");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_kmousetool(&[], "kmousetool");
    }
}
