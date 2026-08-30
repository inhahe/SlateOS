#![deny(clippy::all)]

//! i3-cli — Slate OS i3 window manager tools
//!
//! Multi-personality: `i3-msg`, `i3-nagbar`, `i3-input`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_i3(args: &[String], prog: &str) -> i32 {
    match prog {
        "i3-nagbar" => {
            if args.iter().any(|a| a == "--help" || a == "-h") {
                println!("Usage: i3-nagbar [OPTIONS]");
                println!("  -t TYPE      Type (warning, error)");
                println!("  -m TEXT      Message text");
                println!("  -b LABEL CMD Button label and command");
                return 0;
            }
            let msg = args.windows(2).find(|w| w[0] == "-m")
                .map(|w| w[1].as_str()).unwrap_or("Configuration error");
            println!("i3-nagbar: {}", msg);
            return 0;
        }
        "i3-input" => {
            if args.iter().any(|a| a == "--help" || a == "-h") {
                println!("Usage: i3-input [OPTIONS]");
                println!("  -s SOCKET    i3 IPC socket path");
                println!("  -p PROMPT    Prompt text");
                println!("  -F FORMAT    Format string for command");
                return 0;
            }
            println!("i3-input: waiting for input...");
            return 0;
        }
        _ => {} // i3-msg
    }
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: i3-msg [OPTIONS] MESSAGE");
        println!("i3-msg (i3 4.23) (Slate OS)");
        println!();
        println!("Options:");
        println!("  -t TYPE      Message type (command, get_workspaces, get_outputs,");
        println!("               get_tree, get_marks, get_bar_config, get_version,");
        println!("               get_binding_modes, get_config, subscribe)");
        println!("  -s SOCKET    IPC socket path");
        println!("  -q           Quiet mode");
        return 0;
    }
    let msg_type = args.windows(2).find(|w| w[0] == "-t")
        .map(|w| w[1].as_str());
    let message = args.iter().rfind(|a| !a.starts_with('-') && *a != "command")
        .map(|s| s.as_str());

    match msg_type {
        Some("get_workspaces") => {
            println!("[{{\"num\":1,\"name\":\"1\",\"focused\":true,\"visible\":true,\"output\":\"DP-1\"}},");
            println!(" {{\"num\":2,\"name\":\"2\",\"focused\":false,\"visible\":false,\"output\":\"DP-1\"}}]");
        }
        Some("get_outputs") => {
            println!("[{{\"name\":\"DP-1\",\"active\":true,\"primary\":true,\"rect\":{{\"x\":0,\"y\":0,\"width\":2560,\"height\":1440}}}}]");
        }
        Some("get_version") => {
            println!("{{\"major\":4,\"minor\":23,\"patch\":0,\"human_readable\":\"4.23 (Slate OS)\"}}");
        }
        _ => {
            if let Some(cmd) = message {
                println!("[{{\"success\":true}}]");
                let _c = cmd;
            } else {
                println!("i3-msg: no message specified");
            }
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "i3-msg".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_i3(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_i3};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/i3"), "i3");
        assert_eq!(basename(r"C:\bin\i3.exe"), "i3.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("i3.exe"), "i3");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_i3(&["--help".to_string()], "i3"), 0);
        assert_eq!(run_i3(&["-h".to_string()], "i3"), 0);
        let _ = run_i3(&["--version".to_string()], "i3");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_i3(&[], "i3");
    }
}
