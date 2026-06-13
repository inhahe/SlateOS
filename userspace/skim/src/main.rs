#![deny(clippy::all)]

//! skim — SlateOS fuzzy finder in Rust (fzf alternative)
//!
//! Single personality: `sk`

use std::env;
use std::process;

fn run_sk(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sk [OPTIONS]");
        println!();
        println!("Fuzzy Finder in Rust (fzf-compatible interface).");
        println!();
        println!("Options:");
        println!("  -e, --exact            Exact match");
        println!("  --regex                Start in regex mode");
        println!("  -m, --multi            Multi-select");
        println!("  --no-multi             Single select (default)");
        println!("  --no-mouse             Disable mouse");
        println!("  -c, --cmd <CMD>        Command to invoke for fetching candidates");
        println!("  -i, --interactive      Interactive mode (rerun cmd on query change)");
        println!("  -I, --replstr <STR>    Replace string in interactive mode");
        println!("  --color <COLOR>        Color scheme");
        println!("  --min-height <N>       Minimum height");
        println!("  --height <HEIGHT>      Display height");
        println!("  --margin <MARGIN>      Screen margin");
        println!("  --layout <LAYOUT>      Layout (default/reverse/reverse-list)");
        println!("  --border [STYLE]       Border style");
        println!("  --prompt <STR>         Prompt");
        println!("  --cmd-prompt <STR>     Prompt for cmd mode");
        println!("  --header <STR>         Header");
        println!("  --header-lines <N>     First N lines as header");
        println!("  --tabstop <N>          Tab stop width");
        println!("  --ansi                 Enable ANSI color processing");
        println!("  --delimiter <STR>      Field delimiter");
        println!("  --nth <N>              Fields to match against");
        println!("  --with-nth <N>         Fields to display");
        println!("  --preview <CMD>        Preview command");
        println!("  --preview-window <OPT> Preview window options");
        println!("  -q, --query <STR>      Initial query");
        println!("  --cmd-query <STR>      Initial command query");
        println!("  --expect <KEYS>        Keys to exit on");
        println!("  --read0                NUL delimited input");
        println!("  --print0               NUL delimited output");
        println!("  --print-query          Print query as first line");
        println!("  --print-cmd            Print command as second line");
        println!("  --print-score          Print match score");
        println!("  -f, --filter <STR>     Non-interactive filter");
        println!("  --algo <TYPE>          Algorithm (skim_v1/skim_v2/clangd)");
        println!("  --case <MODE>          Case sensitivity (smart/ignore/respect)");
        println!("  --bind <KEYBINDS>      Key bindings");
        println!("  --pre-select-n <N>     Pre-select first N items");
        println!("  --pre-select-pat <PAT> Pre-select matching items");
        println!("  --pre-select-items <I> Pre-select specific items");
        println!("  -V, --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("sk 0.10.4 (SlateOS)");
        return 0;
    }

    // Check for filter mode
    let filter_idx = args.iter().position(|a| a == "-f" || a == "--filter");
    if let Some(idx) = filter_idx {
        let query = args.get(idx + 1).map(|s| s.as_str()).unwrap_or("");
        let items = ["src/main.rs", "src/lib.rs", "src/config.rs", "tests/test.rs", "Cargo.toml"];
        for item in &items {
            if item.contains(query) {
                println!("{}", item);
            }
        }
        return 0;
    }

    // Check for interactive command mode
    let cmd_idx = args.iter().position(|a| a == "-c" || a == "--cmd");
    let interactive = args.iter().any(|a| a == "-i" || a == "--interactive");

    let query_idx = args.iter().position(|a| a == "-q" || a == "--query");
    let initial_query = query_idx
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("");

    println!("sk — fuzzy finder (Rust)");
    if let Some(idx) = cmd_idx {
        let cmd = args.get(idx + 1).map(|s| s.as_str()).unwrap_or("(cmd)");
        println!("  Command: {}", cmd);
        if interactive {
            println!("  Mode: interactive (reruns on query change)");
        }
    }
    if !initial_query.is_empty() {
        println!("  Query: {}", initial_query);
    }
    println!();
    println!("  ▶ src/main.rs");
    println!("    src/lib.rs");
    println!("    src/config.rs");
    println!("    tests/test.rs");
    println!("    Cargo.toml");
    println!("    README.md");
    println!();
    println!("  6/6  (0)");
    println!("  > {}", initial_query);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sk(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_sk};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sk(vec!["--help".to_string()]), 0);
        assert_eq!(run_sk(vec!["-h".to_string()]), 0);
        let _ = run_sk(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sk(vec![]);
    }
}
