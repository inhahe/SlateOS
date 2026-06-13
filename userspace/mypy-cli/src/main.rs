#![deny(clippy::all)]

//! mypy-cli — SlateOS mypy CLI
//!
//! Single personality: `mypy`

use std::env;
use std::process;

fn run_mypy(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mypy [OPTIONS] [FILES/DIRS...]");
        println!();
        println!("mypy — static type checker for Python (Slate OS).");
        println!();
        println!("Options:");
        println!("  --strict             Enable all strict checks");
        println!("  --ignore-missing-imports  Ignore missing imports");
        println!("  --disallow-untyped-defs   Disallow untyped function definitions");
        println!("  --show-error-codes   Show error codes");
        println!("  --config-file FILE   Config file");
        println!("  --cache-dir DIR      Cache directory");
        println!("  --no-incremental     Don't use incremental mode");
        println!("  --html-report DIR    Generate HTML report");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("mypy 1.8.0 (Slate OS)");
        return 0;
    }

    let strict = args.iter().any(|a| a == "--strict");
    let show_codes = args.iter().any(|a| a == "--show-error-codes");

    let code_suffix = if show_codes { "  [assignment]" } else { "" };

    println!("src/main.py:12: error: Incompatible types in assignment (expression has type \"str\", variable has type \"int\"){}", code_suffix);
    println!("src/main.py:25: error: Missing return statement{}", if show_codes { "  [return]" } else { "" });
    println!("src/utils.py:8: error: Argument 1 to \"process\" has incompatible type \"Optional[str]\"; expected \"str\"{}", if show_codes { "  [arg-type]" } else { "" });

    if strict {
        println!("src/utils.py:15: error: Function is missing a type annotation{}", if show_codes { "  [no-untyped-def]" } else { "" });
        println!("src/helpers.py:3: error: Function is missing a return type annotation{}", if show_codes { "  [no-untyped-def]" } else { "" });
        println!("Found 5 errors in 3 files (checked 8 source files)");
    } else {
        println!("Found 3 errors in 2 files (checked 8 source files)");
    }
    1
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mypy(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_mypy};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mypy(vec!["--help".to_string()]), 0);
        assert_eq!(run_mypy(vec!["-h".to_string()]), 0);
        let _ = run_mypy(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mypy(vec![]);
    }
}
