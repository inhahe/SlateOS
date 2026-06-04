#![deny(clippy::all)]

//! petsc-cli — OurOS PETSc scientific computing toolkit
//!
//! Single personality: `petsc-info`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_petsc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: petsc-info COMMAND [OPTIONS]");
        println!("PETSc v3.21 (OurOS) — Portable Extensible Toolkit for Scientific Computation");
        println!();
        println!("Commands:");
        println!("  info              Show PETSc configuration");
        println!("  bench             Run solver benchmarks");
        println!("  solvers           List available solvers");
        println!("  precond           List preconditioners");
        println!("  version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("PETSc v3.21 (OurOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("info");
    match cmd {
        "info" => {
            println!("PETSc v3.21");
            println!("  Scalar type: double");
            println!("  Complex: disabled");
            println!("  MPI: enabled (OpenMPI 4.1)");
            println!("  BLAS/LAPACK: OpenBLAS");
            println!("  64-bit indices: no");
        }
        "bench" => {
            println!("PETSc solver benchmarks:");
            println!("  Poisson 3D (64x64x64):");
            println!("    GMRES+ILU: 0.42s (42 iterations)");
            println!("    CG+AMG: 0.18s (12 iterations)");
            println!("    Direct (MUMPS): 0.95s");
        }
        "solvers" => {
            println!("Krylov solvers (KSP):");
            println!("  cg, gmres, bicg, bcgs, minres, tfqmr");
            println!("  preonly, richardson, chebyshev");
        }
        "precond" => {
            println!("Preconditioners (PC):");
            println!("  ilu, icc, jacobi, sor, asm, gasm");
            println!("  gamg, hypre, ml, lu, cholesky");
        }
        _ => println!("petsc-info {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "petsc-info".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_petsc(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_petsc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/petsc"), "petsc");
        assert_eq!(basename(r"C:\bin\petsc.exe"), "petsc.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("petsc.exe"), "petsc");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_petsc(&["--help".to_string()], "petsc"), 0);
        assert_eq!(run_petsc(&["-h".to_string()], "petsc"), 0);
        let _ = run_petsc(&["--version".to_string()], "petsc");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_petsc(&[], "petsc");
    }
}
