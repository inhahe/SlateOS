#![deny(clippy::all)]

//! bricscad-cli — SlateOS Bricsys BricsCAD DWG-compatible CAD
//!
//! Single personality: `bricscad`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_brics(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bricscad [OPTIONS] [FILE]");
        println!("Bricsys BricsCAD V25 (SlateOS) — DWG-native CAD platform");
        println!();
        println!("Options:");
        println!("  /b SCRIPT              Run script (.scr)");
        println!("  /p PROFILE             Load profile");
        println!("  --edition ED           Lite/Pro/Mechanical/BIM/Ultimate");
        println!("  --lisp FILE            Load AutoLISP code");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Bricsys BricsCAD V25.1 (SlateOS)"); return 0; }
    println!("Bricsys BricsCAD V25.1 (SlateOS)");
    println!("  Editions: Lite (2D), Pro (3D), Mechanical, BIM, Ultimate");
    println!("  Format: DWG (native, no conversion), DXF, DGN, IFC");
    println!("  AI: AI-powered tools (auto-classify, copy guided, BIMify)");
    println!("  Scripting: LISP, BRX (ObjectARX-compatible C++), .NET, JavaScript");
    println!("  BIM: parametric BIM with automated quantification");
    println!("  License: perpetual (rare in CAD) + maintenance");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bricscad".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_brics(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_brics};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/bricscad"), "bricscad");
        assert_eq!(basename(r"C:\bin\bricscad.exe"), "bricscad.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("bricscad.exe"), "bricscad");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_brics(&["--help".to_string()], "bricscad"), 0);
        assert_eq!(run_brics(&["-h".to_string()], "bricscad"), 0);
        let _ = run_brics(&["--version".to_string()], "bricscad");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_brics(&[], "bricscad");
    }
}
