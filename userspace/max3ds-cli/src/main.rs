#![deny(clippy::all)]

//! max3ds-cli — Slate OS Autodesk 3ds Max 3D modeling
//!
//! Single personality: `3dsmax`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_max(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: 3dsmax [OPTIONS] [FILE]");
        println!("Autodesk 3ds Max 2025 (Slate OS) — 3D modeling, animation, rendering");
        println!();
        println!("Options:");
        println!("  -U PythonHost SCRIPT  Run MaxScript/Python script");
        println!("  -silent               No splash, suppress dialogs");
        println!("  -mxs CMD              Execute MaxScript command");
        println!("  -render SCENE         Render scene");
        println!("  -outputName FILE      Render output file");
        println!("  -frames N-M           Frame range");
        println!("  -width N              Image width");
        println!("  -height N             Image height");
        println!("  --version             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Autodesk 3ds Max 2025.2 (Slate OS)"); return 0; }
    println!("Autodesk 3ds Max 2025.2 (Slate OS)");
    println!("  Renderer: Arnold (default), V-Ray, Corona, Scanline");
    println!("  Scripting: MaxScript, Python");
    println!("  Modifiers: 200+ available");
    println!("  Plugins: 24 loaded");
    println!("  License: floating (autodesk-license-server:27000)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "3dsmax".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_max(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_max};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/max3ds"), "max3ds");
        assert_eq!(basename(r"C:\bin\max3ds.exe"), "max3ds.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("max3ds.exe"), "max3ds");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_max(&["--help".to_string()], "max3ds"), 0);
        assert_eq!(run_max(&["-h".to_string()], "max3ds"), 0);
        let _ = run_max(&["--version".to_string()], "max3ds");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_max(&[], "max3ds");
    }
}
