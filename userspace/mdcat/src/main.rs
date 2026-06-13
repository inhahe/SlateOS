#![deny(clippy::all)]

//! mdcat — SlateOS render Markdown in the terminal
//!
//! Single personality: `mdcat`

use std::env;
use std::process;

fn run_mdcat(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mdcat [OPTIONS] [FILE]...");
        println!();
        println!("Render Markdown to the terminal with rich formatting.");
        println!();
        println!("Options:");
        println!("  -p, --paginate       Pipe output to a pager");
        println!("  -P, --no-pager       Disable automatic paging");
        println!("  --columns <N>        Maximum columns for word wrap");
        println!("  --ansi               Only use ANSI formatting");
        println!("  --no-colour          Disable all colours and styles");
        println!("  -l, --local          Only use local resources");
        println!("  --fail               Fail on rendering errors");
        println!("  --detect-terminal    Detect terminal capabilities");
        println!("  -V, --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("mdcat 2.1.2 (Slate OS)");
        return 0;
    }
    if args.iter().any(|a| a == "--detect-terminal") {
        println!("Terminal: xterm-256color");
        println!("  True color: yes");
        println!("  Sixel graphics: yes");
        println!("  Kitty graphics: yes");
        println!("  Hyperlinks: yes (OSC 8)");
        println!("  Columns: 120");
        return 0;
    }

    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if files.is_empty() {
        println!("(reading Markdown from stdin)");
        println!();
    }

    // Simulate rendered Markdown output
    println!("\x1b[1m\x1b[36m# My Project\x1b[0m");
    println!();
    println!("A \x1b[1mbold\x1b[0m and \x1b[3mitalic\x1b[0m description of the project.");
    println!();
    println!("\x1b[1m\x1b[36m## Features\x1b[0m");
    println!();
    println!("  \x1b[33m•\x1b[0m Fast and efficient");
    println!("  \x1b[33m•\x1b[0m Cross-platform support");
    println!("  \x1b[33m•\x1b[0m Easy to use API");
    println!();
    println!("\x1b[1m\x1b[36m## Code Example\x1b[0m");
    println!();
    println!("  \x1b[2m┌─────────────────────────────────┐\x1b[0m");
    println!("  \x1b[2m│\x1b[0m \x1b[34mfn\x1b[0m \x1b[33mmain\x1b[0m() {{                    \x1b[2m│\x1b[0m");
    println!("  \x1b[2m│\x1b[0m     \x1b[35mprintln!\x1b[0m(\x1b[32m\"Hello, world!\"\x1b[0m); \x1b[2m│\x1b[0m");
    println!("  \x1b[2m│\x1b[0m }}                              \x1b[2m│\x1b[0m");
    println!("  \x1b[2m└─────────────────────────────────┘\x1b[0m");
    println!();
    println!("See the \x1b[4m\x1b[34mdocumentation\x1b[0m for more details.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mdcat(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_mdcat};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mdcat(vec!["--help".to_string()]), 0);
        assert_eq!(run_mdcat(vec!["-h".to_string()]), 0);
        let _ = run_mdcat(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mdcat(vec![]);
    }
}
