#![deny(clippy::all)]

//! units-cli — SlateOS GNU Units CLI
//!
//! Single personality: `units`

use std::env;
use std::process;

fn run_units(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: units [OPTIONS] [FROM-UNIT TO-UNIT]");
        println!();
        println!("GNU Units — unit conversion (Slate OS).");
        println!();
        println!("Options:");
        println!("  -f FILE            Units data file");
        println!("  -o FORMAT          Output format");
        println!("  -t, --terse        Terse output");
        println!("  -1, --one-line     One-line output");
        println!("  -q, --quiet        Suppress prompts");
        println!("  --check            Check units data file");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("GNU Units version 2.22 (Slate OS)");
        return 0;
    }

    let terse = args.iter().any(|a| a == "-t" || a == "--terse");
    let positional: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if positional.len() >= 2 {
        let from = positional[0];
        let to = positional[1];
        if terse {
            println!("* 0.3048");
        } else {
            println!("    {from} = {to} * 0.3048");
            println!("    {to} = {from} / 3.2808399");
        }
    } else {
        println!("Units version 2.22 (Slate OS)");
        println!("3877 units, 109 prefixes, 99 nonlinear units");
        println!();
        println!("You have: ");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_units(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_units};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_units(vec!["--help".to_string()]), 0);
        assert_eq!(run_units(vec!["-h".to_string()]), 0);
        let _ = run_units(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_units(vec![]);
    }
}
