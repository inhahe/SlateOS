#![deny(clippy::all)]

//! edius-cli — SlateOS Grass Valley EDIUS broadcast NLE
//!
//! Single personality: `edius`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_edius(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: edius [OPTIONS] [PROJECT]");
        println!("Grass Valley EDIUS X Pro (Slate OS) — Broadcast-grade NLE");
        println!();
        println!("Options:");
        println!("  --open FILE            Open .ezp project");
        println!("  --pkg                  Open EDIUS Package");
        println!("  --background-export    Background export mode");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Grass Valley EDIUS X Pro 11.30 (Slate OS)"); return 0; }
    println!("Grass Valley EDIUS X Pro 11.30 (Slate OS)");
    println!("  Editions: Pro, Workgroup, Elite");
    println!("  Used in: News broadcast, sports production, documentary");
    println!("  Realtime editing: 4K/8K HDR, multi-format timeline");
    println!("  Codecs: All broadcast formats native (XDCAM/AVC-Intra/ProRes/DNxHR)");
    println!("  Audio: Up to 16 channels per track");
    println!("  License: perpetual + Pro Updates / subscription");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "edius".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_edius(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_edius};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/edius"), "edius");
        assert_eq!(basename(r"C:\bin\edius.exe"), "edius.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("edius.exe"), "edius");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_edius(&["--help".to_string()], "edius"), 0);
        assert_eq!(run_edius(&["-h".to_string()], "edius"), 0);
        let _ = run_edius(&["--version".to_string()], "edius");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_edius(&[], "edius");
    }
}
