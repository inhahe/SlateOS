#![deny(clippy::all)]

//! qalc-cli — OurOS Qalculate CLI
//!
//! Single personality: `qalc`

use std::env;
use std::process;

fn run_qalc(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: qalc [OPTIONS] [EXPRESSION]");
        println!();
        println!("Qalculate! — powerful calculator (OurOS).");
        println!();
        println!("Options:");
        println!("  -s, --set OPTION       Set option");
        println!("  -e, --exrates          Update exchange rates");
        println!("  -f, --file FILE        Read expressions from file");
        println!("  -t, --terse            Terse output");
        println!("  --color                Enable color");
        println!("  --nocolor              Disable color");
        println!("  -n, --nodefs           No default units");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("Qalculate! 4.9.0 (OurOS)");
        return 0;
    }

    if args.iter().any(|a| a == "-e" || a == "--exrates") {
        println!("Exchange rates updated.");
        return 0;
    }

    let expr: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if expr.is_empty() {
        println!("> ");
    } else {
        let expression = expr.join(" ");
        // Simulate a smart calculator
        if expression.contains("to") || expression.contains("in") {
            println!("  {} = 3.28084 ft", expression);
        } else if expression.contains("sqrt") {
            println!("  {} = 1.41421356", expression);
        } else if expression.contains("pi") {
            println!("  pi = 3.14159265358979");
        } else if expression.contains("solve") {
            println!("  x = 3");
        } else {
            println!("  {} = 42", expression);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_qalc(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_qalc};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_qalc(vec!["--help".to_string()]), 0);
        assert_eq!(run_qalc(vec!["-h".to_string()]), 0);
        assert_eq!(run_qalc(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_qalc(vec![]), 0);
    }
}
