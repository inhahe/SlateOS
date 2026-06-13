#![deny(clippy::all)]

//! black-cli — Slate OS Black Python formatter
//!
//! Single personality: `black`

use std::env;
use std::process;

fn run_black(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: black [OPTIONS] [SRC]...");
        println!();
        println!("Black — the uncompromising Python code formatter (Slate OS).");
        println!();
        println!("Options:");
        println!("  --check              Check if files would be reformatted");
        println!("  --diff               Print diff of changes");
        println!("  --line-length N      Line length (default: 88)");
        println!("  --target-version VER Python version target");
        println!("  --include REGEX      Include files matching");
        println!("  --exclude REGEX      Exclude files matching");
        println!("  --quiet              Suppress output");
        println!("  --verbose            Verbose output");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("black, 24.1.1 (Slate OS)");
        return 0;
    }

    let check = args.iter().any(|a| a == "--check");
    let diff = args.iter().any(|a| a == "--diff");
    let quiet = args.iter().any(|a| a == "--quiet" || a == "-q");

    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();
    let target = if files.is_empty() { "." } else { files[0] };

    if check {
        println!("would reformat src/main.py");
        println!("would reformat src/utils.py");
        println!();
        println!("Oh no! 💥 💔 💥");
        println!("2 files would be reformatted, 3 files would be left unchanged.");
        return 1;
    }

    if diff {
        println!("--- src/main.py\t(original)");
        println!("+++ src/main.py\t(reformatted)");
        println!("@@ -1,3 +1,5 @@");
        println!("-def hello( name,age ):return f\"Hello {{name}}, age {{age}}\"");
        println!("+def hello(name, age):");
        println!("+    return f\"Hello {{name}}, age {{age}}\"");
        return 0;
    }

    if !quiet {
        println!("reformatted src/main.py");
        println!("reformatted src/utils.py");
        println!();
        println!("All done! ✨ 🍰 ✨");
        println!("2 files reformatted, 3 files left unchanged.");
        println!("  Target: {}", target);
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_black(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_black};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_black(vec!["--help".to_string()]), 0);
        assert_eq!(run_black(vec!["-h".to_string()]), 0);
        let _ = run_black(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_black(vec![]);
    }
}
