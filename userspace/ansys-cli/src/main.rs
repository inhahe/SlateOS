#![deny(clippy::all)]

//! ansys-cli — SlateOS Ansys multiphysics engineering simulation
//!
//! Single personality: `ansys`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ansys(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ansys [OPTIONS] [FILE]");
        println!("Ansys 2024 R2 (Slate OS) — Engineering simulation (multiphysics)");
        println!();
        println!("Options:");
        println!("  -b                     Batch mode (no GUI)");
        println!("  -i INPUT               Input file (.dat/.inp/.wbpj)");
        println!("  -p PRODUCT             Product (ane3fl/mech/cfd/hfss/maxwell/lsdyna)");
        println!("  -np N                  Parallel processes");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Ansys 2024 R2 (Slate OS)"); return 0; }
    println!("Ansys 2024 R2 (Slate OS)");
    println!("  Products: Mechanical (FEA), Fluent/CFX (CFD), HFSS/Maxwell (EM)");
    println!("  Products: LS-DYNA (explicit), Discovery, Workbench, SpaceClaim");
    println!("  Scripting: APDL (Mechanical), Python (PyAnsys), Workbench scripting");
    println!("  HPC: distributed parallel (MPI), GPU acceleration");
    println!("  Industries: aerospace, automotive, energy, electronics, materials");
    println!("  License: enterprise (per-solver tokens)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ansys".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ansys(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ansys};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ansys"), "ansys");
        assert_eq!(basename(r"C:\bin\ansys.exe"), "ansys.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ansys.exe"), "ansys");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ansys(&["--help".to_string()], "ansys"), 0);
        assert_eq!(run_ansys(&["-h".to_string()], "ansys"), 0);
        let _ = run_ansys(&["--version".to_string()], "ansys");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ansys(&[], "ansys");
    }
}
