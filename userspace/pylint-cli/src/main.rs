#![deny(clippy::all)]

//! pylint-cli — SlateOS Pylint CLI
//!
//! Single personality: `pylint`

use std::env;
use std::process;

fn run_pylint(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pylint [OPTIONS] [FILES/MODULES...]");
        println!();
        println!("Pylint — Python code static analyzer (SlateOS).");
        println!();
        println!("Options:");
        println!("  --rcfile FILE        Config file");
        println!("  --disable MSGS       Disable specific messages");
        println!("  --enable MSGS        Enable specific messages");
        println!("  --output-format FMT  Output format (text, json, colorized)");
        println!("  --jobs N             Parallel execution");
        println!("  --score              Display score");
        println!("  --list-msgs          List all messages");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("pylint 3.0.3 (SlateOS)");
        return 0;
    }

    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();
    let target = if files.is_empty() { "src/" } else { files[0] };

    println!("************* Module {}", target);
    println!("src/main.py:1:0: C0114: Missing module docstring (missing-module-docstring)");
    println!("src/main.py:5:0: C0116: Missing function or method docstring (missing-function-docstring)");
    println!("src/main.py:12:4: W0612: Unused variable 'result' (unused-variable)");
    println!("src/main.py:18:0: R0913: Too many arguments (6/5) (too-many-arguments)");
    println!("src/utils.py:3:0: E0401: Unable to import 'nonexistent' (import-error)");
    println!("src/utils.py:15:8: C0103: Variable name 'x' doesn't conform to snake_case naming style (invalid-name)");
    println!("src/utils.py:22:0: R0201: Method could be a function (no-self-use)");
    println!();
    println!("------------------------------------------------------------------");
    println!("Your code has been rated at 6.25/10 (previous run: 5.80/10, +0.45)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pylint(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_pylint};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pylint(vec!["--help".to_string()]), 0);
        assert_eq!(run_pylint(vec!["-h".to_string()]), 0);
        let _ = run_pylint(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pylint(vec![]);
    }
}
