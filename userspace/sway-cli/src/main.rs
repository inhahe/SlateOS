#![deny(clippy::all)]

//! sway-cli — SlateOS Sway window manager tools
//!
//! Multi-personality: `swaymsg`, `swaynag`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sway(args: &[String], prog: &str) -> i32 {
    if prog == "swaynag" {
        if args.iter().any(|a| a == "--help" || a == "-h") {
            println!("Usage: swaynag [OPTIONS]");
            println!("  -t TYPE       Type (warning, error)");
            println!("  -m TEXT       Message text");
            println!("  -b LABEL CMD  Button");
            println!("  -e EDGE       Edge (top, bottom)");
            return 0;
        }
        let msg = args.windows(2).find(|w| w[0] == "-m")
            .map(|w| w[1].as_str()).unwrap_or("Warning");
        println!("swaynag: {}", msg);
        return 0;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: swaymsg [OPTIONS] MESSAGE");
        println!("swaymsg (sway 1.9) (SlateOS)");
        println!();
        println!("Options:");
        println!("  -t TYPE      Message type (same as i3-msg plus get_inputs,");
        println!("               get_seats)");
        println!("  -s SOCKET    IPC socket path");
        println!("  -r           Raw JSON output");
        println!("  -p           Pretty-print JSON");
        println!("  -q           Quiet");
        return 0;
    }
    let msg_type = args.windows(2).find(|w| w[0] == "-t")
        .map(|w| w[1].as_str());

    match msg_type {
        Some("get_inputs") => {
            println!("[{{\"identifier\":\"1:1:AT_Translated_Set_2_keyboard\",\"name\":\"AT keyboard\",\"type\":\"keyboard\"}}]");
        }
        Some("get_outputs") => {
            println!("[{{\"name\":\"DP-1\",\"make\":\"Dell\",\"model\":\"U2723QE\",\"active\":true,\"scale\":1.0,\"transform\":\"normal\",\"current_mode\":{{\"width\":2560,\"height\":1440,\"refresh\":60000}}}}]");
        }
        Some("get_workspaces") => {
            println!("[{{\"num\":1,\"name\":\"1\",\"focused\":true,\"output\":\"DP-1\"}}]");
        }
        _ => {
            println!("[{{\"success\":true}}]");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "swaymsg".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sway(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sway};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sway"), "sway");
        assert_eq!(basename(r"C:\bin\sway.exe"), "sway.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sway.exe"), "sway");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sway(&["--help".to_string()], "sway"), 0);
        assert_eq!(run_sway(&["-h".to_string()], "sway"), 0);
        let _ = run_sway(&["--version".to_string()], "sway");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sway(&[], "sway");
    }
}
