#![deny(clippy::all)]

//! olive-cli — SlateOS Olive Editor open-source NLE
//!
//! Single personality: `olive`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_olive(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: olive [OPTIONS] [PROJECT]");
        println!("Olive Editor 0.2 (SlateOS) — Pro-grade open-source NLE (in active dev)");
        println!();
        println!("Options:");
        println!("  --open FILE            Open .ove project");
        println!("  --export FILE          Export project");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Olive Editor 0.2.0 (SlateOS)"); return 0; }
    println!("Olive Editor 0.2.0 (SlateOS)");
    println!("  Engine: Custom node-based composition graph");
    println!("  Color: OpenColorIO, 32-bit linear float internal");
    println!("  Node editor: Build effects from primitive nodes (similar to Nuke/Fusion)");
    println!("  Rendering: GPU-accelerated, multi-threaded, frame caching");
    println!("  Formats: All FFmpeg-supported");
    println!("  License: GNU GPLv3");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "olive".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_olive(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_olive};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/olive"), "olive");
        assert_eq!(basename(r"C:\bin\olive.exe"), "olive.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("olive.exe"), "olive");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_olive(&["--help".to_string()], "olive"), 0);
        assert_eq!(run_olive(&["-h".to_string()], "olive"), 0);
        let _ = run_olive(&["--version".to_string()], "olive");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_olive(&[], "olive");
    }
}
