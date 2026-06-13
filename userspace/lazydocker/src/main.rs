#![deny(clippy::all)]

//! lazydocker — SlateOS terminal UI for Docker
//!
//! Single personality: `lazydocker`

use std::env;
use std::process;

fn run_lazydocker(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lazydocker [flags]");
        println!();
        println!("Flags:");
        println!("  -f, --config <file>   Config file");
        println!("  -d, --debug           Debug mode");
        println!("  -l, --log             Enable file logging");
        println!("  -v, --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-v" || a == "--version") {
        println!("Version: 0.23.1 (SlateOS)");
        println!("Date: 2025-05-22");
        return 0;
    }

    println!("lazydocker 0.23.1 (SlateOS)");
    println!("(TUI launched — simulated)");
    let _ = args;
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lazydocker(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_lazydocker};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lazydocker(vec!["--help".to_string()]), 0);
        assert_eq!(run_lazydocker(vec!["-h".to_string()]), 0);
        let _ = run_lazydocker(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lazydocker(vec![]);
    }
}
