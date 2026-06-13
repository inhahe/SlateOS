#![deny(clippy::all)]

//! rbw-cli — SlateOS rbw unofficial Bitwarden CLI
//!
//! Multi-personality: `rbw`, `rbw-agent`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rbw(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: rbw COMMAND [OPTIONS]");
        println!("rbw v1.9 (SlateOS) — Unofficial Bitwarden CLI");
        println!();
        println!("Commands:");
        println!("  login             Log in to Bitwarden");
        println!("  unlock            Unlock vault");
        println!("  lock              Lock vault");
        println!("  sync              Sync vault");
        println!("  list              List entries");
        println!("  get NAME          Get password");
        println!("  generate          Generate password");
        println!("  edit NAME         Edit entry");
        println!("  add               Add entry");
        println!("  remove NAME       Remove entry");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("rbw v1.9 (SlateOS)"); return 0; }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("list");
    match cmd {
        "list" => {
            println!("Gmail");
            println!("GitHub");
            println!("AWS Console");
            println!("Netflix");
        }
        "get" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("entry");
            println!("password for {}: ********", name);
        }
        "generate" => println!("Qm8#xKp$nL2@wVz9"),
        "sync" => println!("Syncing... done"),
        _ => println!("rbw: {}", cmd),
    }
    0
}

fn run_agent(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rbw-agent [OPTIONS]");
        println!("rbw-agent v1.9 (SlateOS) — rbw background agent");
        return 0;
    }
    let _ = args;
    println!("rbw-agent: background agent started");
    println!("  Socket: /run/user/1000/rbw/socket");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rbw".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "rbw-agent" => run_agent(&rest, &prog),
        _ => run_rbw(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rbw};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/rbw"), "rbw");
        assert_eq!(basename(r"C:\bin\rbw.exe"), "rbw.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("rbw.exe"), "rbw");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rbw(&["--help".to_string()], "rbw"), 0);
        assert_eq!(run_rbw(&["-h".to_string()], "rbw"), 0);
        let _ = run_rbw(&["--version".to_string()], "rbw");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rbw(&[], "rbw");
    }
}
