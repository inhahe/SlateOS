#![deny(clippy::all)]

//! draftsight-cli — SlateOS Dassault DraftSight 2D drafting
//!
//! Single personality: `draftsight`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ds(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: draftsight [OPTIONS] [FILE]");
        println!("Dassault DraftSight 2025 (Slate OS) — Professional 2D/3D DWG CAD");
        println!();
        println!("Options:");
        println!("  /b SCRIPT              Run script");
        println!("  --edition ED           Std/Pro/Premium/Enterprise/Mechanical");
        println!("  --lisp FILE            Load LISP routine");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Dassault DraftSight 2025 SP1 (Slate OS)"); return 0; }
    println!("Dassault DraftSight 2025 SP1 (Slate OS)");
    println!("  Editions: Standard, Professional, Premium, Enterprise, Mechanical");
    println!("  Format: DWG (native), DXF, DWT, PDF — drop-in AutoCAD alternative");
    println!("  Scripting: LISP, Visual Basic Scripting, C++ API");
    println!("  3D modeling, sheet metal, constraints (Premium+)");
    println!("  Toolboxes: ANSI/ISO/DIN/JIS standard parts (Mechanical)");
    println!("  Integration: 3DEXPERIENCE platform connection");
    println!("  License: subscription");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "draftsight".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ds(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ds};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/draftsight"), "draftsight");
        assert_eq!(basename(r"C:\bin\draftsight.exe"), "draftsight.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("draftsight.exe"), "draftsight");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ds(&["--help".to_string()], "draftsight"), 0);
        assert_eq!(run_ds(&["-h".to_string()], "draftsight"), 0);
        let _ = run_ds(&["--version".to_string()], "draftsight");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ds(&[], "draftsight");
    }
}
