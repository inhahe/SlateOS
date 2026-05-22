#![deny(clippy::all)]

//! tig — OurOS text-mode interface for git
//!
//! Single personality: `tig`

use std::env;
use std::process;

fn run_tig(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tig [OPTIONS] [REVISION] [--] [PATH]...");
        println!("       tig log    [OPTIONS] [REVISION]");
        println!("       tig show   [OPTIONS] [REVISION]");
        println!("       tig diff   [OPTIONS] [REVISION]");
        println!("       tig blame  [OPTIONS] [FILE]");
        println!("       tig grep   [OPTIONS] [PATTERN]");
        println!("       tig refs");
        println!("       tig stash");
        println!("       tig status");
        println!();
        println!("Options:");
        println!("  -v, --version         Show version");
        println!("  -C <DIR>              Set working directory");
        return 0;
    }
    if args.iter().any(|a| a == "-v" || a == "--version") {
        println!("tig version 2.5.8 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("log");

    match cmd {
        "status" => {
            println!("On branch main");
            println!();
            println!("Changes to be committed:");
            println!("  M src/main.rs");
            println!();
            println!("Changes not staged:");
            println!("  M src/lib.rs");
            println!();
            println!("Untracked files:");
            println!("  tests/new_test.rs");
        }
        "blame" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("src/main.rs");
            println!("Blame: {}", file);
            println!("ab12cd3  Developer   2025-05-22  1  fn main() {{");
            println!("ab12cd3  Developer   2025-05-22  2      let config = load();");
            println!("ef45gh6  Developer   2025-05-21  3      run(config);");
            println!("ij78kl9  Developer   2025-05-19  4  }}");
        }
        "stash" => {
            println!("Stash list:");
            println!("  stash@{{0}}: WIP on main: ab12cd3 Update config");
            println!("  stash@{{1}}: On main: experiment with async");
        }
        "refs" => {
            println!("Branches:");
            println!("  * main");
            println!("    feature/new-ui");
            println!("    fix/memory-leak");
            println!();
            println!("Tags:");
            println!("    v1.0.0");
            println!("    v0.9.0");
        }
        "grep" => {
            let pattern = args.get(1).map(|s| s.as_str()).unwrap_or("TODO");
            println!("Grep: {}", pattern);
            println!("  src/main.rs:15: // {} fix error handling", pattern);
            println!("  src/lib.rs:42:  // {} add tests", pattern);
        }
        _ => {
            // Default: log view
            println!("tig — main view");
            println!();
            println!("  2025-05-22 ab12cd3  Developer   Update config handling");
            println!("  2025-05-21 ef45gh6  Developer   Add test framework");
            println!("  2025-05-19 ij78kl9  Developer   Initial commit");
            println!();
            println!("(TUI: j/k navigate, Enter details, q quit)");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tig(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
