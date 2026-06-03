#![deny(clippy::all)]

//! lazygit — OurOS terminal UI for git commands
//!
//! Single personality: `lazygit`

use std::env;
use std::process;

fn run_lazygit(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lazygit [flags]");
        println!();
        println!("Flags:");
        println!("  -f, --filter <path>   Filter by path");
        println!("  -p, --path <path>     Git repository path");
        println!("  -w, --work-tree <dir> Work tree path");
        println!("  -g, --git-dir <dir>   Git directory");
        println!("  --use-config-dir <d>  Config directory");
        println!("  -l, --log             Enable file logging");
        println!("  -d, --debug           Debug mode");
        println!("  -v, --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-v" || a == "--version") {
        println!("commit=abc123, build date=2025-05-22, build source=OurOS, version=0.42.0, os=ouros, arch=amd64");
        return 0;
    }

    let path = args.iter().position(|a| a == "-p" || a == "--path")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or(".");
    println!("lazygit 0.42.0 (OurOS)");
    println!("Opening repository at: {}", path);
    println!("(TUI launched — simulated)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lazygit(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_lazygit};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_lazygit(vec!["--help".to_string()]), 0);
        assert_eq!(run_lazygit(vec!["-h".to_string()]), 0);
        assert_eq!(run_lazygit(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_lazygit(vec![]), 0);
    }
}
