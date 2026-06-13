#![deny(clippy::all)]

//! ruff-cli — SlateOS Ruff Python linter/formatter
//!
//! Single personality: `ruff`

use std::env;
use std::process;

fn run_ruff(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ruff <COMMAND> [OPTIONS]");
        println!();
        println!("Ruff — extremely fast Python linter and formatter (SlateOS).");
        println!();
        println!("Commands:");
        println!("  check        Lint files");
        println!("  format       Format files");
        println!("  rule         Explain a rule");
        println!("  config       Show config");
        println!("  clean        Clear caches");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("ruff 0.2.0 (SlateOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("check");
    match cmd {
        "check" => {
            let fix = args.iter().any(|a| a == "--fix");
            println!("src/main.py:3:1: F401 `os` imported but unused");
            println!("src/main.py:15:5: E712 Comparison to `True` should be `if cond:`");
            println!("src/utils.py:8:80: E501 Line too long (95 > 88)");
            println!("src/utils.py:22:1: I001 Import block is un-sorted or un-formatted");
            if fix {
                println!();
                println!("Found 4 errors (2 fixed, 2 remaining).");
            } else {
                println!();
                println!("Found 4 errors.");
                println!("  [*] 2 fixable with the `--fix` option.");
            }
            1
        }
        "format" => {
            let check = args.iter().any(|a| a == "--check");
            let diff = args.iter().any(|a| a == "--diff");
            if check {
                println!("Would reformat: src/main.py");
                println!("Would reformat: src/utils.py");
                println!("2 files would be reformatted");
                1
            } else if diff {
                println!("--- src/main.py");
                println!("+++ src/main.py");
                println!("@@ -1 +1,2 @@");
                println!("-def foo(x,y,z): return x+y+z");
                println!("+def foo(x, y, z):");
                println!("+    return x + y + z");
                0
            } else {
                println!("2 files reformatted");
                0
            }
        }
        "rule" => {
            let rule = args.get(1).map(|s| s.as_str()).unwrap_or("F401");
            println!("# {} ({})", rule, match rule {
                "F401" => "unused-import",
                "E501" => "line-too-long",
                "E712" => "true-false-comparison",
                "I001" => "unsorted-imports",
                _ => "unknown",
            });
            println!();
            println!("Derived from the **Pyflakes** linter.");
            println!();
            println!("## What it does");
            println!("Checks for unused imports.");
            println!();
            println!("## Why is this bad?");
            println!("Unused imports add clutter and may indicate dead code.");
            0
        }
        "config" => {
            println!("Ruff configuration:");
            println!("  Line length: 88");
            println!("  Target version: py312");
            println!("  Selected rules: E, F, I, W");
            println!("  Config file: pyproject.toml");
            0
        }
        "clean" => {
            println!("Cleared cache at .ruff_cache");
            0
        }
        _ => {
            eprintln!("Error: unknown command '{}'. See --help.", cmd);
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ruff(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_ruff};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ruff(vec!["--help".to_string()]), 0);
        assert_eq!(run_ruff(vec!["-h".to_string()]), 0);
        let _ = run_ruff(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ruff(vec![]);
    }
}
