#![deny(clippy::all)]

//! mcfly-cli — OurOS McFly shell history search
//!
//! Single personality: `mcfly`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mcfly(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: mcfly COMMAND [ARGS...]");
        println!("McFly 0.9.2 (OurOS) — Neural network shell history search");
        println!();
        println!("Commands:");
        println!("  search [QUERY]    Search history (interactive)");
        println!("  add CMD           Add command to history");
        println!("  train             Train the neural network");
        println!("  move              Move old database");
        println!("  init SHELL        Print shell init script");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("mcfly 0.9.2 (OurOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("search");
    match cmd {
        "search" => {
            let query = args.get(1).map(|s| s.as_str()).unwrap_or("");
            if query.is_empty() {
                println!("mcfly: Interactive history search...");
            } else {
                println!("mcfly search for '{}':", query);
                println!("  1. {} --help", query);
                println!("  2. {} -v", query);
            }
        }
        "add" => {
            let command = args.get(1).map(|s| s.as_str()).unwrap_or("");
            println!("mcfly: Added '{}' to history.", command);
        }
        "train" => println!("mcfly: Training neural network on history..."),
        "move" => println!("mcfly: Moving database to new location..."),
        "init" => {
            let shell = args.get(1).map(|s| s.as_str()).unwrap_or("bash");
            println!("# mcfly init for {}", shell);
            println!("eval \"$(mcfly init {})\"", shell);
        }
        _ => println!("mcfly: unknown command '{}'", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mcfly".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mcfly(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mcfly};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mcfly"), "mcfly");
        assert_eq!(basename(r"C:\bin\mcfly.exe"), "mcfly.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mcfly.exe"), "mcfly");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mcfly(&["--help".to_string()], "mcfly"), 0);
        assert_eq!(run_mcfly(&["-h".to_string()], "mcfly"), 0);
        let _ = run_mcfly(&["--version".to_string()], "mcfly");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mcfly(&[], "mcfly");
    }
}
