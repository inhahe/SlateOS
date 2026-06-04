#![deny(clippy::all)]

//! tmux-cli — OurOS tmux CLI
//!
//! Single personality: `tmux`

use std::env;
use std::process;

fn run_tmux(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tmux <COMMAND> [OPTIONS]");
        println!();
        println!("tmux — terminal multiplexer (OurOS).");
        println!();
        println!("Commands:");
        println!("  new-session, new    Create a new session");
        println!("  attach, a           Attach to a session");
        println!("  detach, d           Detach from session");
        println!("  list-sessions, ls   List sessions");
        println!("  kill-session        Kill a session");
        println!("  split-window        Split current pane");
        println!("  list-windows        List windows");
        println!("  send-keys           Send keys to a pane");
        println!("  select-pane         Select a pane");
        println!("  resize-pane         Resize a pane");
        return 0;
    }
    if args.iter().any(|a| a == "-V") {
        println!("tmux 3.4 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "new-session" | "new" => {
            let name = args.windows(2).find(|w| w[0] == "-s")
                .map(|w| w[1].as_str()).unwrap_or("0");
            let detach = args.iter().any(|a| a == "-d");
            if detach {
                println!("Session '{}' created (detached)", name);
            } else {
                println!("[Session '{}' attached]", name);
                println!("  Window 0: bash");
            }
            0
        }
        "attach" | "attach-session" | "a" => {
            let target = args.windows(2).find(|w| w[0] == "-t")
                .map(|w| w[1].as_str()).unwrap_or("0");
            println!("[Attached to session '{}']", target);
            0
        }
        "detach" | "detach-client" | "d" => {
            println!("[Detached from session]");
            0
        }
        "list-sessions" | "ls" => {
            println!("dev: 3 windows (created Mon Jan 15 10:00:00 2024) (attached)");
            println!("server: 2 windows (created Mon Jan 15 09:00:00 2024)");
            println!("monitoring: 1 windows (created Sun Jan 14 14:00:00 2024)");
            0
        }
        "kill-session" => {
            let target = args.windows(2).find(|w| w[0] == "-t")
                .map(|w| w[1].as_str()).unwrap_or("0");
            println!("Session '{}' killed", target);
            0
        }
        "split-window" => {
            let horizontal = args.iter().any(|a| a == "-h");
            if horizontal {
                println!("Split pane horizontally");
            } else {
                println!("Split pane vertically");
            }
            0
        }
        "list-windows" | "lsw" => {
            let target = args.windows(2).find(|w| w[0] == "-t")
                .map(|w| w[1].as_str()).unwrap_or("dev");
            println!("Session: {}", target);
            println!("0: bash* (2 panes) [180x45]");
            println!("1: vim- (1 panes) [180x45]");
            println!("2: htop (1 panes) [180x45]");
            0
        }
        "send-keys" => {
            let target = args.windows(2).find(|w| w[0] == "-t")
                .map(|w| w[1].as_str()).unwrap_or("0");
            println!("Keys sent to pane {}", target);
            0
        }
        "select-pane" => {
            let direction = if args.iter().any(|a| a == "-L") { "left" }
                else if args.iter().any(|a| a == "-R") { "right" }
                else if args.iter().any(|a| a == "-U") { "up" }
                else if args.iter().any(|a| a == "-D") { "down" }
                else { "target" };
            println!("Selected {} pane", direction);
            0
        }
        _ => {
            if cmd.is_empty() {
                // No command = start new session
                println!("[new session started]");
                0
            } else {
                eprintln!("Error: unknown command '{}'. See --help.", cmd);
                1
            }
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tmux(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_tmux};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_tmux(vec!["--help".to_string()]), 0);
        assert_eq!(run_tmux(vec!["-h".to_string()]), 0);
        let _ = run_tmux(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_tmux(vec![]);
    }
}
