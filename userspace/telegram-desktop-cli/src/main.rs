#![deny(clippy::all)]

//! telegram-desktop-cli — Slate OS Telegram Desktop client
//!
//! Single personality: `telegram-desktop`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_telegram(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: telegram-desktop [OPTIONS]");
        println!("telegram-desktop v4.15 (Slate OS) — Telegram messenger");
        println!();
        println!("Options:");
        println!("  -startintray      Start minimized to tray");
        println!("  -autostart        Auto-start mode");
        println!("  -debug            Debug logging");
        println!("  -workdir DIR      Working directory");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("telegram-desktop v4.15 (Slate OS)"); return 0; }
    println!("telegram-desktop: Telegram messenger started");
    println!("  Account: logged in");
    println!("  Chats: 35 active");
    println!("  Unread: 7 messages");
    println!("  Secret chats: 2");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "telegram-desktop".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_telegram(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_telegram};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/telegram-desktop"), "telegram-desktop");
        assert_eq!(basename(r"C:\bin\telegram-desktop.exe"), "telegram-desktop.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("telegram-desktop.exe"), "telegram-desktop");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_telegram(&["--help".to_string()], "telegram-desktop"), 0);
        assert_eq!(run_telegram(&["-h".to_string()], "telegram-desktop"), 0);
        let _ = run_telegram(&["--version".to_string()], "telegram-desktop");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_telegram(&[], "telegram-desktop");
    }
}
