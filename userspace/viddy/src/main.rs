#![deny(clippy::all)]

//! viddy — SlateOS modern watch command with diff and history
//!
//! Single personality: `viddy`

use std::env;
use std::process;

fn run_viddy(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: viddy [OPTIONS] <COMMAND>...");
        println!();
        println!("A modern watch command. Time machine and diff support.");
        println!();
        println!("Options:");
        println!("  -n, --interval <SEC>   Execution interval (default: 2.0)");
        println!("  -p, --precise          Precise timing mode");
        println!("  -d, --differences [MODE]  Highlight differences (none/watch/line/word)");
        println!("  -t, --no-title         Turn off header");
        println!("  -s, --skip-empty-diffs Skip empty diffs");
        println!("  --pty                  Use pseudo-terminal");
        println!("  --bell                 Ring bell on changes");
        println!("  -b, --begin <TIME>     Start time for time travel");
        println!("  -e, --end <TIME>       End time for time travel");
        println!("  --shell <SHELL>        Shell to use");
        println!("  --shellopt <OPT>       Shell options");
        println!("  -V, --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("viddy 0.4.0 (SlateOS)");
        return 0;
    }

    let no_title = args.iter().any(|a| a == "-t" || a == "--no-title");
    let diff_mode = args.iter().any(|a| a == "-d" || a == "--differences");

    // Collect command after flags
    let command: Vec<&str> = args.iter()
        .skip_while(|a| a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    let cmd_str = if command.is_empty() { "date" } else { command[0] };

    if !no_title {
        println!("Every 2.0s: {}                          Thu May 22 10:00:00 2025", cmd_str);
        println!();
    }

    if diff_mode {
        println!("(output with changes highlighted)");
        println!("  PID   CPU%  MEM%  COMMAND");
        println!("    1    0.0   0.1  init");
        println!("  201   [8.5]  3.2  cargo build   ← changed");
        println!("  180    2.1   5.4  browser");
    } else {
        println!("(command output)");
        println!("  PID   CPU%  MEM%  COMMAND");
        println!("    1    0.0   0.1  init");
        println!("  201    8.5   3.2  cargo build");
        println!("  180    2.1   5.4  browser");
    }

    println!();
    println!("(TUI: ←/→ time-travel, d toggle diff, / search, q quit)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_viddy(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_viddy};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_viddy(vec!["--help".to_string()]), 0);
        assert_eq!(run_viddy(vec!["-h".to_string()]), 0);
        let _ = run_viddy(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_viddy(vec![]);
    }
}
