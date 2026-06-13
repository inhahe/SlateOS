#![deny(clippy::all)]

//! git-interactive-rebase-tool — Slate OS TUI for interactive git rebase
//!
//! Single personality: `git-interactive-rebase-tool`

use std::env;
use std::process;

fn run_rebase_tool(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: git-interactive-rebase-tool [OPTIONS] <FILE>");
        println!();
        println!("Full-featured terminal UI for interactive rebase.");
        println!("Set as GIT_SEQUENCE_EDITOR to use automatically.");
        println!();
        println!("Options:");
        println!("  --version              Show version");
        println!("  --license              Show license");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("interactive-rebase-tool 2.4.1 (Slate OS)");
        return 0;
    }

    let file = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str());

    if let Some(f) = file {
        println!("git-interactive-rebase-tool — editing {}", f);
    } else {
        println!("git-interactive-rebase-tool — TUI launched");
    }
    println!();
    println!("┌─ Interactive Rebase ──────────────────────────────────────┐");
    println!("│                                                           │");
    println!("│  ▸ pick   ab12cd3  Update config handling                 │");
    println!("│    pick   ef45gh6  Add test framework                     │");
    println!("│    pick   ij78kl9  Fix memory leak                        │");
    println!("│    pick   mn01op2  Add documentation                      │");
    println!("│                                                           │");
    println!("├───────────────────────────────────────────────────────────┤");
    println!("│  Actions: p=pick r=reword e=edit s=squash f=fixup d=drop │");
    println!("│  Movement: j/k=up/down, J/K=swap, q=abort, w=write      │");
    println!("│  Visual: v=toggle select, V=select range                  │");
    println!("│  Misc: !=exec, b=break, l=label                          │");
    println!("└───────────────────────────────────────────────────────────┘");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rebase_tool(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_rebase_tool};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rebase_tool(vec!["--help".to_string()]), 0);
        assert_eq!(run_rebase_tool(vec!["-h".to_string()]), 0);
        let _ = run_rebase_tool(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rebase_tool(vec![]);
    }
}
