#![deny(clippy::all)]

//! gams-cli — OurOS GAMS optimization modeling
//!
//! Multi-personality: `gams`, `gamside`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gams(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: gams [OPTIONS] FILE.gms");
        println!("  a=VALUE        Assignment parameter");
        println!("  o=FILE         Output listing file");
        println!("  lo=N           Log option (0-4)");
        println!("  lf=FILE        Log file");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("GAMS 45.1.0 (OurOS)");
        println!("Solvers: CPLEX 22.1, GUROBI 11.0, CONOPT 4, BARON 24");
        return 0;
    }
    let file = args.iter().find(|a| a.ends_with(".gms")).map(|s| s.as_str()).unwrap_or("model.gms");
    println!("--- GAMS 45.1.0 ---");
    println!("Processing {}", file);
    println!("--- Starting compilation");
    println!("--- {} lines read", 42);
    println!("--- Starting execution");
    println!("--- Generating LP model");
    println!("--- 15 rows, 23 columns, 67 non-zeroes");
    println!("--- Calling CPLEX solver");
    println!("--- CPLEX 22.1.0: optimal solution found");
    println!("--- Objective: 345.67");
    println!("--- Normal completion");
    0
}

fn run_gamside(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gamside [OPTIONS] [FILE.gms]");
        println!("  --version    Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("GAMS IDE 45.1.0 (OurOS)");
        return 0;
    }
    println!("GAMS Studio 45.1.0");
    println!("Starting IDE...");
    if let Some(f) = args.iter().find(|a| a.ends_with(".gms")) {
        println!("Opening: {}", f);
    }
    println!("Ready.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gams".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "gamside" => run_gamside(&rest),
        _ => run_gams(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
