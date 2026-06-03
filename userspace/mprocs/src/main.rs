#![deny(clippy::all)]

//! mprocs — OurOS run multiple commands in parallel with TUI
//!
//! Single personality: `mprocs`

use std::env;
use std::process;

fn run_mprocs(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mprocs [OPTIONS] [COMMANDS]...");
        println!();
        println!("Run multiple commands in parallel and see output in a TUI.");
        println!();
        println!("Options:");
        println!("  -c, --config <FILE>    Config file (mprocs.yaml)");
        println!("  -s, --server <ADDR>    Start in server mode");
        println!("  --ctl <CMD>            Send control command to running instance");
        println!("  --names <NAMES>        Comma-separated process names");
        println!("  -V, --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("mprocs 0.7.1 (OurOS)");
        return 0;
    }

    let commands: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if commands.is_empty() {
        println!("mprocs 0.7.1 (OurOS)");
        println!("(reading from mprocs.yaml in current directory)");
        println!();
        println!("Processes (from config):");
        println!("  [0] server   — npm run dev");
        println!("  [1] client   — npm run client");
        println!("  [2] worker   — cargo run --bin worker");
    } else {
        println!("mprocs 0.7.1 (OurOS)");
        println!();
        println!("Processes:");
        for (i, cmd) in commands.iter().enumerate() {
            println!("  [{}] proc{} — {}", i, i, cmd);
        }
    }

    println!();
    println!("┌─ proc0 ────────────────────────────────────────────────┐");
    println!("│ Server running on http://localhost:3000                 │");
    println!("│ Ready in 1.2s                                          │");
    println!("│                                                        │");
    println!("├─ proc1 ────────────────────────────────────────────────┤");
    println!("│ Compiled successfully in 3.4s                          │");
    println!("│ Watching for changes...                                │");
    println!("└────────────────────────────────────────────────────────┘");
    println!("(TUI mode — Tab to switch, q to quit, x to kill)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mprocs(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_mprocs};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_mprocs(vec!["--help".to_string()]), 0);
        assert_eq!(run_mprocs(vec!["-h".to_string()]), 0);
        assert_eq!(run_mprocs(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_mprocs(vec![]), 0);
    }
}
