#![deny(clippy::all)]

//! tmux — OurOS terminal multiplexer
//!
//! Single personality: `tmux`

use std::env;
use std::process;

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct _Session {
    name: String,
    windows: u32,
    _created: String,
    _attached: bool,
}

fn _sample_sessions() -> Vec<_Session> {
    vec![
        _Session { name: "main".to_string(), windows: 3, _created: "Thu May 22 10:00:00 2025".to_string(), _attached: true },
        _Session { name: "dev".to_string(), windows: 2, _created: "Thu May 22 10:30:00 2025".to_string(), _attached: false },
        _Session { name: "monitor".to_string(), windows: 1, _created: "Thu May 22 11:00:00 2025".to_string(), _attached: false },
    ]
}

// ── Main logic ────────────────────────────────────────────────────────

fn run_tmux(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "-h" | "help" => {
            println!("usage: tmux [-2CluvV] [-c shell-command] [-f file] [-L socket-name]");
            println!("            [-S socket-path] [-T features] [command [flags]]");
            println!();
            println!("Commands:");
            println!("  new-session [-s name]     Create a new session");
            println!("  attach [-t target]        Attach to a session");
            println!("  detach                    Detach from current session");
            println!("  list-sessions (ls)        List sessions");
            println!("  kill-session [-t target]  Kill a session");
            println!("  new-window [-n name]      Create a new window");
            println!("  list-windows (lsw)        List windows");
            println!("  split-window [-h|-v]      Split current pane");
            println!("  select-pane [-t target]   Select a pane");
            println!("  send-keys                 Send keys to a pane");
            println!("  source-file <file>        Load config file");
            println!("  set-option (set)          Set a session option");
            println!("  show-options (show)       Show session options");
            println!("  list-keys (lsk)           List key bindings");
            println!("  resize-pane               Resize a pane");
            println!("  capture-pane              Capture pane contents");
            println!("  -V                        Show version");
            0
        }
        "-V" | "--version" => { println!("tmux 3.4 (OurOS)"); 0 }
        "new-session" | "new" => {
            let name = cmd_args.iter().position(|a| a == "-s")
                .and_then(|i| cmd_args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("0");
            println!("[new session: {}]", name);
            0
        }
        "attach-session" | "attach" | "a" => {
            let target = cmd_args.iter().position(|a| a == "-t")
                .and_then(|i| cmd_args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("main");
            println!("[attached to session: {}]", target);
            0
        }
        "detach-client" | "detach" => { println!("[detached (from session main)]"); 0 }
        "list-sessions" | "ls" => {
            let sessions = _sample_sessions();
            for s in &sessions {
                println!("{}: {} windows (created {}){}",
                    s.name, s.windows, s._created,
                    if s._attached { " (attached)" } else { "" });
            }
            0
        }
        "kill-session" => {
            let target = cmd_args.iter().position(|a| a == "-t")
                .and_then(|i| cmd_args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("0");
            println!("kill session: {}", target);
            0
        }
        "new-window" | "neww" => {
            let name = cmd_args.iter().position(|a| a == "-n")
                .and_then(|i| cmd_args.get(i + 1))
                .map(|s| s.as_str());
            match name {
                Some(n) => println!("[new window: {}]", n),
                None => println!("[new window: 1]"),
            }
            0
        }
        "list-windows" | "lsw" => {
            println!("0: bash* (1 panes) [180x45]");
            println!("1: vim- (1 panes) [180x45]");
            println!("2: htop (1 panes) [180x45]");
            0
        }
        "split-window" | "splitw" => {
            let horizontal = cmd_args.iter().any(|a| a == "-h");
            println!("[split {} (simulated)]", if horizontal { "horizontally" } else { "vertically" });
            0
        }
        "select-pane" | "selectp" => {
            let target = cmd_args.iter().position(|a| a == "-t")
                .and_then(|i| cmd_args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("0");
            println!("[pane {} selected]", target);
            0
        }
        "send-keys" | "send" => {
            let keys: Vec<&str> = cmd_args.iter().map(|s| s.as_str()).collect();
            println!("[sent keys: {}]", keys.join(" "));
            0
        }
        "source-file" | "source" => {
            let file = cmd_args.first().map(|s| s.as_str()).unwrap_or("~/.tmux.conf");
            println!("[sourced: {}]", file);
            0
        }
        "set-option" | "set" => {
            let key = cmd_args.first().map(|s| s.as_str()).unwrap_or("option");
            let val = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("value");
            println!("[set {} = {}]", key, val);
            0
        }
        "show-options" | "show" => {
            println!("base-index 0");
            println!("default-terminal \"tmux-256color\"");
            println!("escape-time 500");
            println!("history-limit 2000");
            println!("mouse off");
            println!("prefix C-b");
            println!("renumber-windows off");
            println!("status on");
            println!("status-interval 15");
            0
        }
        "list-keys" | "lsk" => {
            println!("bind-key    C-b send-prefix");
            println!("bind-key    c   new-window");
            println!("bind-key    n   next-window");
            println!("bind-key    p   previous-window");
            println!("bind-key    l   last-window");
            println!("bind-key    d   detach-client");
            println!("bind-key    \"   split-window");
            println!("bind-key    %   split-window -h");
            println!("bind-key    x   confirm-before kill-pane");
            println!("bind-key    &   confirm-before kill-window");
            println!("bind-key    [   copy-mode");
            0
        }
        "resize-pane" | "resizep" => { println!("[pane resized (simulated)]"); 0 }
        "capture-pane" | "capturep" => { println!("[pane captured to buffer (simulated)]"); 0 }
        "kill-server" => { println!("kill server (simulated)"); 0 }
        other => { eprintln!("tmux: unknown command '{}'", other); 1 }
    }
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tmux(rest);
    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sessions() {
        let sessions = _sample_sessions();
        assert_eq!(sessions.len(), 3);
        assert_eq!(sessions[0].name, "main");
    }
}
