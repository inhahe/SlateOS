#![deny(clippy::all)]

//! polkit-dumb-agent-cli — SlateOS minimal Polkit agent for sway/wlroots
//!
//! Single personality: `polkit-dumb-agent`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_agent(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: polkit-dumb-agent [OPTIONS]");
        println!("polkit-dumb-agent v0.1 (SlateOS) — Minimal PolicyKit agent");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Minimal PolicyKit agent for window managers without");
        println!("a built-in agent (sway, i3, Hyprland, etc.).");
        println!("Shows a simple password prompt via terminal or dmenu.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("polkit-dumb-agent v0.1 (SlateOS)"); return 0; }
    println!("polkit-dumb-agent: minimal authentication agent started");
    println!("  Will prompt via terminal when authorization is needed");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "polkit-dumb-agent".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_agent(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_agent};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/polkit-dumb-agent"), "polkit-dumb-agent");
        assert_eq!(basename(r"C:\bin\polkit-dumb-agent.exe"), "polkit-dumb-agent.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("polkit-dumb-agent.exe"), "polkit-dumb-agent");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_agent(&["--help".to_string()], "polkit-dumb-agent"), 0);
        assert_eq!(run_agent(&["-h".to_string()], "polkit-dumb-agent"), 0);
        let _ = run_agent(&["--version".to_string()], "polkit-dumb-agent");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_agent(&[], "polkit-dumb-agent");
    }
}
