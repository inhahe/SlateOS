#![deny(clippy::all)]

//! maya-cli — Slate OS Autodesk Maya 3D animation
//!
//! Single personality: `maya`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_maya(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: maya [OPTIONS] [FILE]");
        println!("Autodesk Maya 2025 (Slate OS) — 3D animation, modeling, simulation");
        println!();
        println!("Options:");
        println!("  -batch             Batch (no GUI) mode");
        println!("  -command CMD       Run MEL command");
        println!("  -script FILE       Execute MEL/Python script");
        println!("  -file FILE         Open scene");
        println!("  -render            Render scene");
        println!("  -prompt            Interactive prompt mode");
        println!("  -log FILE          Log file");
        println!("  -noAutoloadPlugins Skip plugin autoload");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Autodesk Maya 2025.1 (Slate OS)"); return 0; }
    println!("Autodesk Maya 2025.1 (Slate OS)");
    println!("  Renderer: Arnold (default), V-Ray, RenderMan");
    println!("  Scripting: MEL, Python");
    println!("  Plugins: 18 loaded");
    println!("  Workspace: default");
    println!("  Recent scenes: 5");
    println!("  License: floating (autodesk-license-server:27000)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "maya".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_maya(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_maya};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/maya"), "maya");
        assert_eq!(basename(r"C:\bin\maya.exe"), "maya.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("maya.exe"), "maya");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_maya(&["--help".to_string()], "maya"), 0);
        assert_eq!(run_maya(&["-h".to_string()], "maya"), 0);
        let _ = run_maya(&["--version".to_string()], "maya");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_maya(&[], "maya");
    }
}
