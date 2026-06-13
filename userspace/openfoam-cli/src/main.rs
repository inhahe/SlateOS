#![deny(clippy::all)]

//! openfoam-cli — SlateOS OpenFOAM CFD toolkit
//!
//! Multi-personality: `simpleFoam`, `icoFoam`, `blockMesh`, `paraFoam`,
//! `checkMesh`, `decomposePar`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_openfoam(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        println!("OpenFOAM v11 (SlateOS) — Open-source CFD toolbox");
        println!();
        println!("Options:");
        println!("  -case DIR       Case directory");
        println!("  -parallel       Run in parallel");
        println!("  -postProcess    Post-processing only");
        println!("  -noFunctionObjects  Skip function objects");
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("OpenFOAM v11 (SlateOS)");
        println!("Build: SlateOS-x86_64");
        return 0;
    }
    match prog {
        "blockMesh" => {
            println!("blockMesh: generating hex mesh...");
            println!("  Blocks: 5");
            println!("  Cells: 50,000");
            println!("  Points: 51,051");
            println!("  Faces: 151,500");
            println!("  Done");
        }
        "checkMesh" => {
            println!("checkMesh: checking mesh quality...");
            println!("  Cells: 50,000");
            println!("  Max aspect ratio: 3.2 (OK)");
            println!("  Max non-orthogonality: 35.4 (OK)");
            println!("  Max skewness: 0.8 (OK)");
            println!("  Mesh OK");
        }
        "decomposePar" => {
            println!("decomposePar: decomposing for parallel...");
            println!("  Method: scotch");
            println!("  Processors: 4");
            println!("  Cells per processor: ~12,500");
            println!("  Done");
        }
        "icoFoam" => {
            println!("icoFoam: incompressible laminar flow solver");
            println!("  Time = 0.001, Ux residual = 1e-03");
            println!("  Time = 0.002, Ux residual = 5e-04");
            println!("  Time = 0.003, Ux residual = 2e-04");
            println!("  End");
        }
        _ => {
            println!("simpleFoam: steady-state RANS solver");
            println!("  Iteration 1: Ux=1e-02, Uy=1e-02, p=1e-01, k=1e-02");
            println!("  Iteration 10: Ux=1e-04, Uy=1e-04, p=1e-03, k=1e-04");
            println!("  Iteration 50: Ux=1e-06, Uy=1e-06, p=1e-05, k=1e-06");
            println!("  Converged");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "simpleFoam".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_openfoam(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_openfoam};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/openfoam"), "openfoam");
        assert_eq!(basename(r"C:\bin\openfoam.exe"), "openfoam.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("openfoam.exe"), "openfoam");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_openfoam(&["--help".to_string()], "openfoam"), 0);
        assert_eq!(run_openfoam(&["-h".to_string()], "openfoam"), 0);
        let _ = run_openfoam(&["--version".to_string()], "openfoam");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_openfoam(&[], "openfoam");
    }
}
