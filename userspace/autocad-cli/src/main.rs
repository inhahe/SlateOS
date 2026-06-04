#![deny(clippy::all)]

//! autocad-cli — OurOS Autodesk AutoCAD 2D/3D CAD
//!
//! Single personality: `autocad`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_acad(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: autocad [OPTIONS] [FILE]");
        println!("Autodesk AutoCAD 2025 (OurOS) — Industry-standard 2D/3D CAD");
        println!();
        println!("Options:");
        println!("  /b SCRIPT              Run AutoCAD script (.scr)");
        println!("  /p PROFILE             Load profile");
        println!("  /t TEMPLATE            Open with template");
        println!("  /pl PLOT_NAME          Plot drawing");
        println!("  --autolisp FILE        Load AutoLISP code");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Autodesk AutoCAD 2025.1 (OurOS)"); return 0; }
    println!("Autodesk AutoCAD 2025.1 (OurOS)");
    println!("  Industries: Architecture, MEP, Civil, Electrical, Mechanical, Mapping");
    println!("  Scripting: AutoLISP, Visual LISP, VBA, .NET (ObjectARX/AcCoreMgd)");
    println!("  Specialized toolsets: Architecture, Mechanical, Electrical, Plant 3D, etc.");
    println!("  Format: DWG (native), DXF, DWF, PDF, IFC");
    println!("  Web/Mobile: AutoCAD web app, AutoCAD mobile app");
    println!("  License: subscription (named user)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "autocad".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_acad(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_acad};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/autocad"), "autocad");
        assert_eq!(basename(r"C:\bin\autocad.exe"), "autocad.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("autocad.exe"), "autocad");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_acad(&["--help".to_string()], "autocad"), 0);
        assert_eq!(run_acad(&["-h".to_string()], "autocad"), 0);
        let _ = run_acad(&["--version".to_string()], "autocad");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_acad(&[], "autocad");
    }
}
