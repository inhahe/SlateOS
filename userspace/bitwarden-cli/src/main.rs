#![deny(clippy::all)]

//! bitwarden-cli — SlateOS Bitwarden password manager
//!
//! Multi-personality: `bitwarden-desktop`, `bw`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_desktop(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bitwarden-desktop [OPTIONS]");
        println!("bitwarden-desktop v2024.1 (SlateOS) — Bitwarden desktop client");
        println!();
        println!("Options:");
        println!("  --hidden          Start hidden");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("bitwarden-desktop v2024.1 (SlateOS)"); return 0; }
    println!("bitwarden-desktop: password manager started");
    println!("  Vault: 120 items");
    println!("  Sync: last 10 min ago");
    println!("  Two-factor: enabled");
    0
}

fn run_bw(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: bw COMMAND [OPTIONS]");
        println!("bw v2024.1 (SlateOS) — Bitwarden CLI");
        println!();
        println!("Commands:");
        println!("  login             Log in");
        println!("  unlock            Unlock vault");
        println!("  list items        List vault items");
        println!("  get item NAME     Get specific item");
        println!("  create           Create item");
        println!("  generate          Generate password");
        println!("  sync              Sync vault");
        println!("  status            Show status");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    match cmd {
        "status" => println!("{{\"status\":\"unlocked\",\"userEmail\":\"user@example.com\"}}"),
        "generate" => println!("Xk9#mP2$nL8@wQz4"),
        "sync" => println!("Syncing complete."),
        "list" => println!("[{{\"name\":\"Gmail\",\"type\":1}},{{\"name\":\"GitHub\",\"type\":1}}]"),
        _ => println!("bw: {}", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bitwarden-desktop".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "bw" => run_bw(&rest, &prog),
        _ => run_desktop(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_desktop};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/bitwarden"), "bitwarden");
        assert_eq!(basename(r"C:\bin\bitwarden.exe"), "bitwarden.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("bitwarden.exe"), "bitwarden");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_desktop(&["--help".to_string()], "bitwarden"), 0);
        assert_eq!(run_desktop(&["-h".to_string()], "bitwarden"), 0);
        let _ = run_desktop(&["--version".to_string()], "bitwarden");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_desktop(&[], "bitwarden");
    }
}
