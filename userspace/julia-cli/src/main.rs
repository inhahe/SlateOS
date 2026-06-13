#![deny(clippy::all)]

//! julia-cli — Slate OS Julia language
//!
//! Multi-personality: `julia`

use std::env;
use std::process;

fn run_julia(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: julia [OPTIONS] [SCRIPT.jl] [ARGS]");
        println!("  -e CODE        Evaluate code");
        println!("  -p N           Launch N worker processes");
        println!("  -t N           Use N threads");
        println!("  --project DIR  Set project directory");
        println!("  --startup-file={{yes|no}}");
        println!("  --compiled-modules={{yes|no}}");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("julia version 1.10.2 (Slate OS)");
        return 0;
    }
    if args.iter().any(|a| a == "-e") {
        let code = args.windows(2).find(|w| w[0] == "-e").map(|w| w[1].as_str()).unwrap_or("println(\"hello\")");
        println!("julia> {}", code);
        println!("[executed]");
        return 0;
    }
    let script = args.iter().find(|a| a.ends_with(".jl")).map(|s| s.as_str());
    if let Some(s) = script {
        println!("julia: loading '{}'", s);
        println!("[script completed]");
    } else {
        println!("               _");
        println!("   _       _ _(_)_     |  Documentation: https://docs.julialang.org");
        println!("  (_)     | (_) (_)    |");
        println!("   _ _   _| |_  __ _   |  Type \"?\" for help, \"]?\" for Pkg help.");
        println!("  | | | | | | |/ _` |  |");
        println!("  | | |_| | | | (_| |  |  Version 1.10.2 (Slate OS)");
        println!(" _/ |\\__'_|_|_|\\__'_|  |");
        println!("|__/                   |");
        println!();
        println!("julia>");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_julia(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_julia};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_julia(&["--help".to_string()]), 0);
        assert_eq!(run_julia(&["-h".to_string()]), 0);
        let _ = run_julia(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_julia(&[]);
    }
}
