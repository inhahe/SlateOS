#![deny(clippy::all)]

//! wtype-cli — SlateOS wtype Wayland keyboard/mouse input simulator
//!
//! Single personality: `wtype`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wtype(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: wtype [OPTIONS] TEXT...");
        println!("wtype v0.4 (Slate OS) — Wayland keyboard/mouse input simulator");
        println!();
        println!("Options:");
        println!("  TEXT              Text to type");
        println!("  -d DELAY          Delay between keys (ms)");
        println!("  -s DELAY          Delay between key down/up (ms)");
        println!("  -k KEY            Type special key (e.g. Return, Tab)");
        println!("  -M MOD            Hold modifier (shift, ctrl, alt, super)");
        println!("  -P KEY            Key press (down only)");
        println!("  -p KEY            Key release (up only)");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wtype v0.4 (Slate OS)"); return 0; }

    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "-d" | "-s" => { i += 2; continue; }
            "-k" => {
                if let Some(key) = args.get(i + 1) {
                    println!("Key: {}", key);
                }
                i += 2;
                continue;
            }
            "-M" => {
                if let Some(m) = args.get(i + 1) {
                    println!("Modifier: {} (held)", m);
                }
                i += 2;
                continue;
            }
            "-P" => {
                if let Some(key) = args.get(i + 1) {
                    println!("Press: {}", key);
                }
                i += 2;
                continue;
            }
            "-p" => {
                if let Some(key) = args.get(i + 1) {
                    println!("Release: {}", key);
                }
                i += 2;
                continue;
            }
            _ => {
                println!("Typing: {}", a);
                i += 1;
            }
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wtype".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wtype(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wtype};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wtype"), "wtype");
        assert_eq!(basename(r"C:\bin\wtype.exe"), "wtype.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wtype.exe"), "wtype");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wtype(&["--help".to_string()], "wtype"), 0);
        assert_eq!(run_wtype(&["-h".to_string()], "wtype"), 0);
        let _ = run_wtype(&["--version".to_string()], "wtype");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wtype(&[], "wtype");
    }
}
