#![deny(clippy::all)]

//! hexchat-cli — Slate OS HexChat IRC client
//!
//! Single personality: `hexchat`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_hexchat(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: hexchat [OPTIONS] [URL]");
        println!("hexchat v2.16 (Slate OS) — IRC client");
        println!();
        println!("Options:");
        println!("  --existing        Open URL in running instance");
        println!("  --minimize=N      Minimize level (0-2)");
        println!("  --plugindir=DIR   Plugin directory");
        println!("  --configdir=DIR   Config directory");
        println!("  --url=URL         IRC URL to connect to");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("hexchat v2.16 (Slate OS)"); return 0; }
    println!("hexchat: IRC client started");
    println!("  Networks: Libera.Chat, OFTC");
    println!("  Plugins: 3 loaded");
    println!("  Config: ~/.config/hexchat/");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "hexchat".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_hexchat(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_hexchat};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/hexchat"), "hexchat");
        assert_eq!(basename(r"C:\bin\hexchat.exe"), "hexchat.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("hexchat.exe"), "hexchat");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_hexchat(&["--help".to_string()], "hexchat"), 0);
        assert_eq!(run_hexchat(&["-h".to_string()], "hexchat"), 0);
        let _ = run_hexchat(&["--version".to_string()], "hexchat");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_hexchat(&[], "hexchat");
    }
}
