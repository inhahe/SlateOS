#![deny(clippy::all)]

//! line-cli — Slate OS LINE messaging app (LY Corp / Naver+SoftBank)
//!
//! Single personality: `line`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_line(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: line [OPTIONS]");
        println!("LINE (Slate OS) — Japan-dominant messaging super-app");
        println!();
        println!("Options:");
        println!("  --chat                 Chat");
        println!("  --call                 Voice/video calls (incl. PSTN via LINE Out)");
        println!("  --stickers             Sticker shop");
        println!("  --pay                  LINE Pay");
        println!("  --official             Official Accounts");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("LINE 8.5.0 (Slate OS)"); return 0; }
    println!("LINE 8.5.0 (Slate OS)");
    println!("  Owner: LY Corporation (merger of LINE Corp + Yahoo! Japan, Oct 2023)");
    println!("  Parent: A Holdings (Naver + SoftBank JV)");
    println!("  Origin: built post-2011 Japan earthquake to replace failing voice networks");
    println!("  Launch: Jun 2011 — became Japan's dominant chat app within a year");
    println!("  Users: 200M+ MAU, 95M+ in Japan, strong in Thailand/Taiwan/Indonesia");
    println!("  Features: chat, voice/video, group calls, free PSTN-out (LINE Out), Timeline,");
    println!("            Stickers (cultural phenomenon — Brown Bear/Cony franchise),");
    println!("            LINE News, LINE TV, LINE Manga, LINE Music, LINE Gift");
    println!("  LINE Pay: payments incl. NaverPay link, QR/code/PayPay (Japan)");
    println!("  Official Accounts: brands broadcast to followers (replaces SMS marketing)");
    println!("  LINE Works: B2B chat (Slack/Teams competitor)");
    println!("  Stickers: paid sticker packs are major revenue driver");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "line".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_line(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_line};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/line"), "line");
        assert_eq!(basename(r"C:\bin\line.exe"), "line.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("line.exe"), "line");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_line(&["--help".to_string()], "line"), 0);
        assert_eq!(run_line(&["-h".to_string()], "line"), 0);
        let _ = run_line(&["--version".to_string()], "line");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_line(&[], "line");
    }
}
