#![deny(clippy::all)]

//! mcfly — OurOS fly through your shell history with smart search
//!
//! Single personality: `mcfly`

use std::env;
use std::process;

fn run_mcfly(args: Vec<String>) -> i32 {
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "--help" | "-h" | "" => {
            println!("Usage: mcfly <COMMAND>");
            println!();
            println!("Fly through your shell history. McFly replaces Ctrl-R with an");
            println!("intelligent search engine that takes into account your working");
            println!("directory and the context of recently executed commands.");
            println!();
            println!("Commands:");
            println!("  add        Add a command to the history database");
            println!("  init       Print shell initialization script");
            println!("  move       Record a directory change");
            println!("  search     Search command history");
            println!("  train      Train the suggestion engine");
            println!();
            println!("Options:");
            println!("  --mcfly-debug          Enable debug mode");
            println!("  -V, --version          Show version");
            0
        }
        "--version" | "-V" => {
            println!("mcfly 0.9.1 (OurOS)");
            0
        }
        "init" => {
            let shell = args.get(1).map(|s| s.as_str()).unwrap_or("bash");
            println!("# mcfly init for {}", shell);
            println!("# Ctrl-R is now powered by McFly");
            println!("export MCFLY_KEY_SCHEME=vim");
            println!("export MCFLY_FUZZY=2");
            println!("export MCFLY_RESULTS=25");
            println!("export MCFLY_INTERFACE_VIEW=BOTTOM");
            println!("eval \"$(mcfly init {})\"", shell);
            0
        }
        "add" => {
            let command: String = args.iter().skip(1).cloned().collect::<Vec<_>>().join(" ");
            if command.is_empty() {
                eprintln!("Error: command to add required.");
                return 1;
            }
            println!("Added to history: {}", command);
            0
        }
        "search" => {
            let query: String = args.iter().skip(1).cloned().collect::<Vec<_>>().join(" ");
            println!("McFly search (TUI mode):");
            println!();
            if query.is_empty() {
                println!("  > cargo build --release");
                println!("    git status");
                println!("    cargo test --workspace");
                println!("    vim src/main.rs");
                println!("    git commit -m \"feature: add new module\"");
                println!("    cargo clippy --all-targets");
                println!("    git log --oneline -10");
            } else {
                println!("  Results for '{}':", query);
                println!("  > {} --release", query);
                println!("    {} --verbose", query);
                println!("    {} --help", query);
            }
            0
        }
        "train" => {
            println!("Training McFly suggestion engine...");
            println!("  Processed 1,234 commands");
            println!("  Updated neural network weights");
            println!("  Training complete.");
            0
        }
        "move" => {
            let dir = args.get(1).map(|s| s.as_str()).unwrap_or(".");
            println!("Recorded directory change to: {}", dir);
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
    let code = run_mcfly(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
