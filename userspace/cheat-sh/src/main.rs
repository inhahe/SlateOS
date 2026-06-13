#![deny(clippy::all)]

//! cheat-sh — SlateOS community-driven cheat sheets client
//!
//! Single personality: `cht`

use std::env;
use std::process;

fn run_cht(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cht [OPTIONS] <QUERY>");
        println!();
        println!("Community-driven cheat sheets (cheat.sh client).");
        println!();
        println!("Options:");
        println!("  -l, --list           List available cheat sheets");
        println!("  -s, --search <Q>     Search cheat sheets");
        println!("  -T, --no-color       Disable syntax highlighting");
        println!("  -t, --text           Plain text mode");
        println!("  -q, --quiet          Quiet mode");
        println!("  --shell <SHELL>      Set shell mode");
        println!("  -V, --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("cht 1.0.0 (Slate OS)");
        return 0;
    }
    if args.iter().any(|a| a == "-l" || a == "--list") {
        println!("Available topics:");
        println!("  :help     :intro      :list       :styles");
        println!("  bash      c           cpp         css");
        println!("  dart      docker      elixir      elm");
        println!("  git       go          haskell     java");
        println!("  js        julia       kotlin      lua");
        println!("  python    ruby        rust        scala");
        println!("  sql       swift       typescript  zig");
        println!("  (... 500+ topics)");
        return 0;
    }

    let query: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if query.is_empty() {
        eprintln!("Error: query required. Try: cht rust/hello");
        return 1;
    }

    let topic = query.join("/");
    match topic.as_str() {
        "rust/hello" | "rust" => {
            println!("# Rust — Hello World");
            println!();
            println!("fn main() {{");
            println!("    println!(\"Hello, world!\");");
            println!("}}");
            println!();
            println!("# Compile and run:");
            println!("  rustc main.rs && ./main");
            println!();
            println!("# Or with Cargo:");
            println!("  cargo new hello && cd hello && cargo run");
        }
        "git/undo" => {
            println!("# Git — Undo operations");
            println!();
            println!("# Undo last commit (keep changes):");
            println!("  git reset --soft HEAD~1");
            println!();
            println!("# Undo last commit (discard changes):");
            println!("  git reset --hard HEAD~1");
            println!();
            println!("# Unstage a file:");
            println!("  git restore --staged <file>");
            println!();
            println!("# Discard changes to a file:");
            println!("  git restore <file>");
        }
        _ => {
            println!("# {} — Quick Reference", topic);
            println!();
            println!("(cheat sheet for '{}' — simulated)", topic);
            println!();
            println!("  Example command:");
            println!("    {} --example", query[0]);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cht(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_cht};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cht(vec!["--help".to_string()]), 0);
        assert_eq!(run_cht(vec!["-h".to_string()]), 0);
        let _ = run_cht(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cht(vec![]);
    }
}
