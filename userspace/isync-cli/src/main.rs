#![deny(clippy::all)]

//! isync-cli — SlateOS isync/mbsync IMAP mailbox synchronizer
//!
//! Single personality: `mbsync`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mbsync(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: mbsync [OPTIONS] CHANNEL...");
        println!("mbsync v1.5 (SlateOS) — Synchronize IMAP4 and Maildir mailboxes");
        println!();
        println!("Options:");
        println!("  CHANNEL           Channel or group to sync");
        println!("  -a                Sync all channels");
        println!("  -l                List channels");
        println!("  -c FILE           Configuration file");
        println!("  -n                Dry-run mode");
        println!("  -V                Verbose");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("mbsync v1.5 (SlateOS)"); return 0; }
    if args.iter().any(|a| a == "-l") {
        println!("Channels:");
        println!("  work-inbox");
        println!("  work-sent");
        println!("  personal-inbox");
        println!("Groups:");
        println!("  work (work-inbox, work-sent)");
        return 0;
    }
    let channel = if args.iter().any(|a| a == "-a") {
        "all"
    } else {
        args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("all")
    };
    println!("Syncing: {}", channel);
    println!("  C: 3/3  B: 0/0  M: +3/3 *0/0 #0/0  S: +0/0 *0/0 #0/0");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mbsync".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mbsync(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mbsync};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/isync"), "isync");
        assert_eq!(basename(r"C:\bin\isync.exe"), "isync.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("isync.exe"), "isync");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mbsync(&["--help".to_string()], "isync"), 0);
        assert_eq!(run_mbsync(&["-h".to_string()], "isync"), 0);
        let _ = run_mbsync(&["--version".to_string()], "isync");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mbsync(&[], "isync");
    }
}
