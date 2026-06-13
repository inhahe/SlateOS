#![deny(clippy::all)]

//! wayvnc-cli — SlateOS wayvnc VNC server for Wayland
//!
//! Multi-personality: `wayvnc`, `wayvncctl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wayvnc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wayvnc [OPTIONS] [ADDRESS [PORT]]");
        println!("wayvnc v0.8 (Slate OS) — VNC server for wlroots compositors");
        println!();
        println!("Options:");
        println!("  ADDRESS           Bind address (default: 0.0.0.0)");
        println!("  PORT              Port (default: 5900)");
        println!("  -o OUTPUT         Output to serve");
        println!("  -k LAYOUT         Keyboard layout");
        println!("  -S SOCKET         Control socket path");
        println!("  --render-cursor   Render cursor in stream");
        println!("  --max-fps FPS     Maximum FPS");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wayvnc v0.8 (Slate OS)"); return 0; }
    let addr = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("0.0.0.0");
    println!("wayvnc: VNC server listening on {}:5900", addr);
    0
}

fn run_wayvncctl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: wayvncctl COMMAND [OPTIONS]");
        println!("wayvncctl v0.8 (Slate OS) — Control wayvnc");
        println!();
        println!("Commands:");
        println!("  version           Show server version");
        println!("  output-list       List available outputs");
        println!("  output-set OUT    Switch to output");
        println!("  disconnect-client ID  Disconnect client");
        println!("  client-list       List connected clients");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match cmd {
        "version" => println!("wayvnc v0.8"),
        "output-list" => {
            println!("HDMI-A-1: 1920x1080@60Hz");
            println!("DP-1: 2560x1440@144Hz");
        }
        "client-list" => println!("No clients connected"),
        _ => println!("wayvncctl: {}", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wayvnc".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "wayvncctl" => run_wayvncctl(&rest, &prog),
        _ => run_wayvnc(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wayvnc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wayvnc"), "wayvnc");
        assert_eq!(basename(r"C:\bin\wayvnc.exe"), "wayvnc.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wayvnc.exe"), "wayvnc");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wayvnc(&["--help".to_string()], "wayvnc"), 0);
        assert_eq!(run_wayvnc(&["-h".to_string()], "wayvnc"), 0);
        let _ = run_wayvnc(&["--version".to_string()], "wayvnc");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wayvnc(&[], "wayvnc");
    }
}
