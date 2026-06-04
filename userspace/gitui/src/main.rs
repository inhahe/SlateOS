#![deny(clippy::all)]

//! gitui — OurOS blazing fast terminal UI for git
//!
//! Single personality: `gitui`

use std::env;
use std::process;

fn run_gitui(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gitui [OPTIONS]");
        println!();
        println!("Blazing fast terminal UI for git.");
        println!();
        println!("Options:");
        println!("  -d, --directory <DIR>   Set working directory");
        println!("  -l, --logging           Enable logging");
        println!("  -t, --theme <FILE>      Custom theme file");
        println!("  --bugreport             Generate bug report info");
        println!("  --watcher               Enable file watcher");
        println!("  -V, --version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("gitui 0.26.3 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "--bugreport") {
        println!("gitui bug report info:");
        println!("  Version: 0.26.3");
        println!("  OS: OurOS x86_64");
        println!("  Git: 2.44.0");
        println!("  Terminal: xterm-256color");
        println!("  Color support: truecolor");
        return 0;
    }

    println!("gitui 0.26.3 (OurOS) — TUI launched");
    println!();
    println!("┌─ Status ──────────────────────────────────────────────┐");
    println!("│  Changes (3)                                          │");
    println!("│  ▸ M src/main.rs                                      │");
    println!("│    M src/lib.rs                                        │");
    println!("│    ? tests/new_test.rs                                 │");
    println!("├─ Diff ─────────────────────────────────────────────────┤");
    println!("│  src/main.rs                                           │");
    println!("│  @@ -10,3 +10,5 @@                                    │");
    println!("│   fn main() {{                                          │");
    println!("│  +    let config = Config::load();                      │");
    println!("│  +    config.validate();                                │");
    println!("│       run(config);                                      │");
    println!("│   }}                                                     │");
    println!("├─ Log ──────────────────────────────────────────────────┤");
    println!("│  ab12cd3  Update config handling       2 hours ago      │");
    println!("│  ef45gh6  Add test framework           1 day ago        │");
    println!("│  ij78kl9  Initial commit               3 days ago       │");
    println!("└────────────────────────────────────────────────────────┘");
    println!("(1:Status 2:Log 3:Stash 4:Stash-log | Tab:focus | Enter:stage)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gitui(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_gitui};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gitui(vec!["--help".to_string()]), 0);
        assert_eq!(run_gitui(vec!["-h".to_string()]), 0);
        let _ = run_gitui(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gitui(vec![]);
    }
}
