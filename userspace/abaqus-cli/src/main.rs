#![deny(clippy::all)]

//! abaqus-cli — SlateOS Dassault Abaqus FEA
//!
//! Single personality: `abaqus`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_abaqus(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: abaqus [OPTIONS] [COMMAND]");
        println!("Dassault Abaqus 2024 (Slate OS) — Nonlinear FEA (SIMULIA)");
        println!();
        println!("Options:");
        println!("  cae                    Launch Abaqus/CAE GUI");
        println!("  job=NAME input=FILE    Submit analysis job (.inp)");
        println!("  --solver SOLVER        Standard (implicit) or Explicit");
        println!("  --cpus N               Number of CPUs");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Dassault Abaqus 2024 (Slate OS) — Standard/Explicit/CFD/CAE"); return 0; }
    println!("Dassault Abaqus 2024 (Slate OS)");
    println!("  Solvers: Abaqus/Standard (implicit), /Explicit, /CFD, /Electromagnetic");
    println!("  CAE: pre/post-processor with Python scripting");
    println!("  Strengths: nonlinear, contact, materials, fracture, composites");
    println!("  Scripting: Python (Abaqus Scripting Interface), Fortran UMAT/VUMAT");
    println!("  Integration: 3DEXPERIENCE platform, isight (DOE/optimization)");
    println!("  Industries: aerospace, defense, automotive, energy");
    println!("  License: enterprise (analysis tokens)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "abaqus".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_abaqus(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_abaqus};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/abaqus"), "abaqus");
        assert_eq!(basename(r"C:\bin\abaqus.exe"), "abaqus.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("abaqus.exe"), "abaqus");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_abaqus(&["--help".to_string()], "abaqus"), 0);
        assert_eq!(run_abaqus(&["-h".to_string()], "abaqus"), 0);
        let _ = run_abaqus(&["--version".to_string()], "abaqus");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_abaqus(&[], "abaqus");
    }
}
