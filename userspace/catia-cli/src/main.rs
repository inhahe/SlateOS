#![deny(clippy::all)]

//! catia-cli — OurOS Dassault Systèmes CATIA aerospace/automotive CAD/CAE/PLM
//!
//! Single personality: `catia`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_catia(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: catia [OPTIONS] [FILE]");
        println!("Dassault CATIA V5-6R2024 / 3DEXPERIENCE (OurOS) — Aerospace/automotive CAD");
        println!();
        println!("Options:");
        println!("  -object FILE           Open CATPart/CATProduct/CATDrawing");
        println!("  -macro FILE            Run CATScript / VB macro");
        println!("  -batch                 Headless batch mode");
        println!("  --workbench WB         Switch workbench (Part Design, GSD, DMU, etc.)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Dassault CATIA V5-6R2024 (OurOS)"); return 0; }
    println!("Dassault CATIA V5-6R2024 / 3DEXPERIENCE (OurOS)");
    println!("  Industries: Aerospace, automotive, shipbuilding, industrial machinery");
    println!("  Workbenches: Part Design, Assembly, Generative Shape Design, DMU, FEA, NC");
    println!("  Surface modeling: Class-A surfaces (ICEM Surf-derived)");
    println!("  Knowledgeware: parametric design rules, optimization");
    println!("  PLM: 3DEXPERIENCE platform integration (ENOVIA)");
    println!("  Scripting: CATScript, VBScript, CAA C++ SDK");
    println!("  License: enterprise (very expensive)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "catia".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_catia(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_catia};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/catia"), "catia");
        assert_eq!(basename(r"C:\bin\catia.exe"), "catia.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("catia.exe"), "catia");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_catia(&["--help".to_string()], "catia"), 0);
        assert_eq!(run_catia(&["-h".to_string()], "catia"), 0);
        let _ = run_catia(&["--version".to_string()], "catia");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_catia(&[], "catia");
    }
}
