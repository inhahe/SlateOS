#![deny(clippy::all)]

//! revit-cli — SlateOS Autodesk Revit BIM (Building Information Modeling)
//!
//! Single personality: `revit`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_revit(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: revit [OPTIONS] [FILE]");
        println!("Autodesk Revit 2025 (SlateOS) — BIM for architecture/MEP/structural");
        println!();
        println!("Options:");
        println!("  /language LANG         UI language (ENU/CHS/DEU/JPN/...)");
        println!("  /journal FILE          Replay journal");
        println!("  --addin DLL            Load Revit add-in (.NET)");
        println!("  --dynamo SCRIPT        Run Dynamo visual programming");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Autodesk Revit 2025.1 (SlateOS)"); return 0; }
    println!("Autodesk Revit 2025.1 (SlateOS)");
    println!("  Disciplines: Architecture, Structure, MEP (mechanical/electrical/plumbing)");
    println!("  Format: .rvt/.rfa/.rte native + IFC, DWG/DXF, gbXML");
    println!("  Collaboration: Worksets, Revit Server, BIM 360 / Autodesk Docs");
    println!("  Visual programming: Dynamo (node-based)");
    println!("  Scripting: Revit API (.NET), RevitPythonShell, pyRevit");
    println!("  Construction docs, schedules, sheets, families, phasing");
    println!("  License: subscription");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "revit".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_revit(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_revit};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/revit"), "revit");
        assert_eq!(basename(r"C:\bin\revit.exe"), "revit.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("revit.exe"), "revit");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_revit(&["--help".to_string()], "revit"), 0);
        assert_eq!(run_revit(&["-h".to_string()], "revit"), 0);
        let _ = run_revit(&["--version".to_string()], "revit");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_revit(&[], "revit");
    }
}
