#![deny(clippy::all)]

//! ydotool-cli — OurOS ydotool generic input automation
//!
//! Multi-personality: `ydotool`, `ydotoold`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ydotool(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: ydotool COMMAND [OPTIONS]");
        println!("ydotool v1.0 (OurOS) — Generic input automation");
        println!();
        println!("Commands:");
        println!("  type              Type text");
        println!("  key               Press key combo");
        println!("  mousemove         Move mouse");
        println!("  click             Mouse click");
        println!("  recorder          Record input events");
        println!("  bakers            Do nothing");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("type");
    match cmd {
        "type" => {
            let text = args.iter().skip(1).find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("(empty)");
            println!("Typing: {}", text);
        }
        "key" => {
            let combo = args.get(1).map(|s| s.as_str()).unwrap_or("Return");
            println!("Key combo: {}", combo);
        }
        "mousemove" => {
            let x = args.iter().skip_while(|a| a.as_str() != "-x").nth(1).map(|s| s.as_str()).unwrap_or("0");
            let y = args.iter().skip_while(|a| a.as_str() != "-y").nth(1).map(|s| s.as_str()).unwrap_or("0");
            println!("Mouse move to ({}, {})", x, y);
        }
        "click" => {
            let btn = args.get(1).map(|s| s.as_str()).unwrap_or("1");
            println!("Mouse click button {}", btn);
        }
        "recorder" => {
            println!("Recording input events... (Ctrl+C to stop)");
        }
        _ => println!("ydotool {}: unknown command", cmd),
    }
    0
}

fn run_ydotoold(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ydotoold [OPTIONS]");
        println!("ydotoold v1.0 (OurOS) — ydotool daemon");
        println!();
        println!("Options:");
        println!("  --socket-path PATH    Socket path");
        println!("  --socket-perm PERM    Socket permissions");
        return 0;
    }
    let socket = args.iter().skip_while(|a| a.as_str() != "--socket-path").nth(1)
        .map(|s| s.as_str()).unwrap_or("/tmp/.ydotool_socket");
    println!("ydotoold listening on: {}", socket);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ydotool".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "ydotoold" => run_ydotoold(&rest, &prog),
        _ => run_ydotool(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ydotool};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ydotool"), "ydotool");
        assert_eq!(basename(r"C:\bin\ydotool.exe"), "ydotool.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ydotool.exe"), "ydotool");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ydotool(&["--help".to_string()], "ydotool"), 0);
        assert_eq!(run_ydotool(&["-h".to_string()], "ydotool"), 0);
        let _ = run_ydotool(&["--version".to_string()], "ydotool");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ydotool(&[], "ydotool");
    }
}
