#![deny(clippy::all)]

//! julia — Slate OS Julia programming language
//!
//! Single personality: `julia`

use std::env;
use std::process;

fn run_julia(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: julia [switches] -- [programfile] [args...]");
        println!();
        println!("Switches:");
        println!("  -v, --version       Display version");
        println!("  -e <expr>           Evaluate expression");
        println!("  -E <expr>           Evaluate and show result");
        println!("  -p, --procs N       Start N worker processes");
        println!("  -t, --threads N     Number of threads");
        println!("  -q, --quiet         Quiet startup");
        println!("  --project=<dir>     Set project directory");
        println!("  --startup-file=yes|no  Load startup file");
        println!("  -O, --optimize=N    Optimization level (0-3)");
        println!("  --check-bounds=yes|no  Bounds checking");
        println!("  --compiled-modules=yes|no  Use precompiled modules");
        println!("  -i                  Interactive mode");
        return 0;
    }
    if args.iter().any(|a| a == "-v" || a == "--version") {
        println!("julia version 1.11.0 (Slate OS)");
        return 0;
    }

    let exec_expr = args.iter().position(|a| a == "-e" || a == "-E")
        .and_then(|i| args.get(i + 1));
    if let Some(expr) = exec_expr {
        println!("{}", expr);
        println!("(result simulated)");
        return 0;
    }

    let script = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str());
    if let Some(f) = script {
        println!("(running {})", f);
    } else {
        let quiet = args.iter().any(|a| a == "-q" || a == "--quiet");
        if !quiet {
            println!("               _");
            println!("   _       _ _(_)_     |  Documentation: https://docs.julialang.org");
            println!("  (_)     | (_) (_)    |");
            println!("   _ _   _| |_  __ _   |  Type \"?\" for help, \"]?\" for Pkg help.");
            println!("  | | | | | | |/ _` |  |");
            println!("  | | |_| | | | (_| |  |  Version 1.11.0 (Slate OS)");
            println!("  _/ |\\__'_|_|_|\\__'_|  |");
            println!(" |__/                   |");
        }
        println!("julia> (interactive mode — simulated)");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_julia(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_julia};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_julia(vec!["--help".to_string()]), 0);
        assert_eq!(run_julia(vec!["-h".to_string()]), 0);
        let _ = run_julia(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_julia(vec![]);
    }
}
