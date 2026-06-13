#![deny(clippy::all)]

//! inventor-cli — SlateOS Autodesk Inventor 3D mechanical CAD
//!
//! Single personality: `inventor`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_inv(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: inventor [OPTIONS] [FILE]");
        println!("Autodesk Inventor Professional 2025 (Slate OS) — 3D mechanical CAD");
        println!();
        println!("Options:");
        println!("  /b SCRIPT              Batch with script");
        println!("  /p PROJECT             Open project (.ipj)");
        println!("  /m MACRO               Run macro");
        println!("  --ilogic CODE          Run iLogic code");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Autodesk Inventor Pro 2025.1 (Slate OS)"); return 0; }
    println!("Autodesk Inventor Pro 2025.1 (Slate OS)");
    println!("  Modules: Stress Analysis (FEA), Dynamic Simulation, Tube & Pipe, Cable & Harness");
    println!("  Format: IPT/IAM/IDW/IPN native + STEP/IGES/Parasolid/JT");
    println!("  Automation: iLogic, VBA, Inventor API (.NET)");
    println!("  Generative design, frame generator, presentations, Inventor Nastran");
    println!("  Vault: PDM/PLM integration via Vault");
    println!("  License: subscription");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "inventor".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_inv(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_inv};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/inventor"), "inventor");
        assert_eq!(basename(r"C:\bin\inventor.exe"), "inventor.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("inventor.exe"), "inventor");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_inv(&["--help".to_string()], "inventor"), 0);
        assert_eq!(run_inv(&["-h".to_string()], "inventor"), 0);
        let _ = run_inv(&["--version".to_string()], "inventor");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_inv(&[], "inventor");
    }
}
