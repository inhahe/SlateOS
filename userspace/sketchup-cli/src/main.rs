#![deny(clippy::all)]

//! sketchup-cli — Slate OS Trimble SketchUp 3D modeling
//!
//! Single personality: `sketchup`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sketchup(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sketchup [OPTIONS] [FILE]");
        println!("Trimble SketchUp Pro 2024 (Slate OS) — 3D modeling for architecture & design");
        println!();
        println!("Options:");
        println!("  --rubyconsole          Open Ruby console");
        println!("  --script FILE          Run Ruby script");
        println!("  --export FORMAT FILE   Export to format (dae/fbx/obj/3ds/stl)");
        println!("  --layout FILE          Open in LayOut");
        println!("  --vray                 Use V-Ray renderer");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Trimble SketchUp Pro 2024.0.484 (Slate OS)"); return 0; }
    println!("Trimble SketchUp Pro 2024.0.484 (Slate OS)");
    println!("  Components: 3D Warehouse access (4.5M+ models)");
    println!("  Extensions: 1,200+ in Extension Warehouse");
    println!("  Scripting: Ruby API");
    println!("  Companion apps: LayOut, Style Builder");
    println!("  Renderers: V-Ray, Enscape, Twinmotion bridge");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sketchup".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sketchup(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sketchup};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sketchup"), "sketchup");
        assert_eq!(basename(r"C:\bin\sketchup.exe"), "sketchup.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sketchup.exe"), "sketchup");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sketchup(&["--help".to_string()], "sketchup"), 0);
        assert_eq!(run_sketchup(&["-h".to_string()], "sketchup"), 0);
        let _ = run_sketchup(&["--version".to_string()], "sketchup");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sketchup(&[], "sketchup");
    }
}
