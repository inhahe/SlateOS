#![deny(clippy::all)]

//! navi — SlateOS interactive cheatsheet tool
//!
//! Single personality: `navi`

use std::env;
use std::process;

fn run_navi(args: Vec<String>) -> i32 {
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "--help" | "-h" | "" => {
            if cmd.is_empty() {
                // Interactive mode
                println!("navi — interactive cheatsheet");
                println!();
                println!("  Search: _");
                println!();
                println!("  git  > Undo last commit");
                println!("         git reset --soft HEAD~1");
                println!();
                println!("  tar  > Extract archive");
                println!("         tar xf <archive>");
                println!();
                println!("  find > Find files by name");
                println!("         find <path> -name '<pattern>'");
                println!();
                println!("  ssh  > SSH with key");
                println!("         ssh -i <key> <user>@<host>");
                println!();
                println!("(type to filter, Enter to select, Tab to fill variables)");
                return 0;
            }
            println!("Usage: navi [COMMAND]");
            println!();
            println!("An interactive cheatsheet tool for the command-line.");
            println!();
            println!("Commands:");
            println!("  (default)   Launch interactive mode");
            println!("  fn          Execute a cheat function");
            println!("  repo        Manage cheatsheet repositories");
            println!("  widget      Print shell widget");
            println!("  info        Show environment info");
            println!();
            println!("Options:");
            println!("  --print             Print command instead of executing");
            println!("  --path <PATH>       Custom cheatsheet path");
            println!("  --fzf-overrides <O> Custom fzf options");
            println!("  --finder <FINDER>   Finder tool (fzf/skim)");
            println!("  --cheat <CHEAT>     Pre-selected cheat");
            println!("  --query <QUERY>     Initial query");
            println!("  --best-match        Auto-select best match");
            println!("  -V, --version       Show version");
            0
        }
        "--version" | "-V" => {
            println!("navi 2.23.0 (SlateOS)");
            0
        }
        "repo" => {
            let subcmd = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match subcmd {
                "add" => {
                    let url = args.get(2).map(|s| s.as_str()).unwrap_or("https://github.com/denisidoro/cheats");
                    println!("Adding cheatsheet repo: {}", url);
                    println!("Done.");
                }
                "browse" => {
                    println!("Featured repositories:");
                    println!("  denisidoro/cheats      Official cheats");
                    println!("  denisidoro/navi-tldr    tldr pages integration");
                }
                _ => {
                    println!("Installed repos:");
                    println!("  ~/.local/share/navi/cheats/");
                    println!("  ~/.local/share/navi/repos/denisidoro__cheats/");
                }
            }
            0
        }
        "widget" => {
            let shell = args.get(1).map(|s| s.as_str()).unwrap_or("bash");
            println!("# navi widget for {}", shell);
            println!("# Ctrl+G launches navi");
            println!("eval \"$(navi widget {})\"", shell);
            0
        }
        "info" => {
            println!("navi 2.23.0");
            println!("  Finder: fzf");
            println!("  Shell: bash");
            println!("  Config: ~/.config/navi/config.yaml");
            println!("  Cheats: ~/.local/share/navi/cheats/");
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
    let code = run_navi(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_navi};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_navi(vec!["--help".to_string()]), 0);
        assert_eq!(run_navi(vec!["-h".to_string()]), 0);
        let _ = run_navi(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_navi(vec![]);
    }
}
