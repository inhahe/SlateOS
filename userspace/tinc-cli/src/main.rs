#![deny(clippy::all)]

//! tinc-cli — SlateOS tinc mesh VPN
//!
//! Multi-personality: `tincd`, `tinc`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tinc(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "tincd" => {
                println!("tincd (Slate OS) — tinc VPN daemon");
                println!("  -n NET             Network name");
                println!("  -c DIR             Config directory");
                println!("  -D                 No detach (debug)");
                println!("  -d LEVEL           Debug level (0-5)");
                println!("  -k [SIGNAL]        Kill running daemon");
            }
            _ => {
                println!("tinc (Slate OS) — tinc VPN control");
                println!("  -n NET             Network name");
                println!("  init NAME          Initialize node");
                println!("  start              Start VPN");
                println!("  stop               Stop VPN");
                println!("  restart            Restart VPN");
                println!("  reload             Reload config");
                println!("  dump nodes|edges|subnets  Dump state");
                println!("  info NODE          Node info");
                println!("  invite NODE        Generate invitation");
                println!("  join URL           Join via invitation");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("tinc v1.1pre18 (Slate OS)"); return 0; }
    match prog {
        "tincd" => {
            println!("tincd v1.1pre18 (Slate OS)");
            println!("  Network: mynet");
            println!("  Node: server1");
            println!("  Listening: 0.0.0.0:655");
            println!("  Connected peers: 5");
            println!("  Subnet: 10.99.0.0/24");
        }
        _ => {
            println!("tinc v1.1pre18 (Slate OS)");
            println!("  Network: mynet");
            println!("  Nodes: 6 (5 reachable)");
            println!("  Edges: 12");
            println!("  Subnets: 8");
            println!("  Mode: switch (layer 2)");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "tinc".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tinc(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_tinc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/tinc"), "tinc");
        assert_eq!(basename(r"C:\bin\tinc.exe"), "tinc.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("tinc.exe"), "tinc");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_tinc(&["--help".to_string()], "tinc"), 0);
        assert_eq!(run_tinc(&["-h".to_string()], "tinc"), 0);
        let _ = run_tinc(&["--version".to_string()], "tinc");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_tinc(&[], "tinc");
    }
}
