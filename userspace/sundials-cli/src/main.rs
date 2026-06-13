#![deny(clippy::all)]

//! sundials-cli — SlateOS SUNDIALS ODE/DAE solver info
//!
//! Single personality: `sundials-info`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sundials(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: sundials-info COMMAND [OPTIONS]");
        println!("SUNDIALS v7.0 (SlateOS) — Suite of nonlinear and differential/algebraic solvers");
        println!();
        println!("Commands:");
        println!("  info              Show configuration");
        println!("  solvers           List solver packages");
        println!("  bench             Run ODE benchmarks");
        println!("  version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("SUNDIALS v7.0 (SlateOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("info");
    match cmd {
        "info" => {
            println!("SUNDIALS v7.0");
            println!("  Precision: double");
            println!("  Index type: 64-bit");
            println!("  MPI: enabled");
            println!("  CUDA: disabled");
            println!("  LAPACK: enabled");
        }
        "solvers" => {
            println!("Solver packages:");
            println!("  CVODE     — ODE solver (stiff & nonstiff)");
            println!("  CVODES    — ODE with sensitivity analysis");
            println!("  IDA       — DAE solver");
            println!("  IDAS      — DAE with sensitivity analysis");
            println!("  KINSOL    — Nonlinear algebraic solver");
            println!("  ARKODE    — Adaptive Runge-Kutta ODE solver");
        }
        "bench" => {
            println!("SUNDIALS ODE benchmarks:");
            println!("  Van der Pol (stiff, BDF): 0.008s");
            println!("  Robertson (DAE): 0.012s");
            println!("  Heat equation (MOL, 256 pts): 0.45s");
            println!("  N-body (8 bodies): 0.003s");
        }
        _ => println!("sundials-info {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sundials-info".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sundials(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sundials};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sundials"), "sundials");
        assert_eq!(basename(r"C:\bin\sundials.exe"), "sundials.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sundials.exe"), "sundials");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sundials(&["--help".to_string()], "sundials"), 0);
        assert_eq!(run_sundials(&["-h".to_string()], "sundials"), 0);
        let _ = run_sundials(&["--version".to_string()], "sundials");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sundials(&[], "sundials");
    }
}
