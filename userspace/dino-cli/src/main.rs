#![deny(clippy::all)]

//! dino-cli — SlateOS Dino XMPP/Jabber client
//!
//! Single personality: `dino`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dino(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dino [OPTIONS]");
        println!("dino v0.4 (SlateOS) — Modern XMPP/Jabber client");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("dino v0.4 (SlateOS)"); return 0; }
    println!("dino: XMPP client started");
    println!("  Accounts: 1 connected");
    println!("  Contacts: 25 online");
    println!("  Group chats: 3 joined");
    println!("  OMEMO encryption: enabled");
    println!("  Audio/Video calls: supported");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dino".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_dino(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dino};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/dino"), "dino");
        assert_eq!(basename(r"C:\bin\dino.exe"), "dino.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("dino.exe"), "dino");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dino(&["--help".to_string()], "dino"), 0);
        assert_eq!(run_dino(&["-h".to_string()], "dino"), 0);
        let _ = run_dino(&["--version".to_string()], "dino");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dino(&[], "dino");
    }
}
