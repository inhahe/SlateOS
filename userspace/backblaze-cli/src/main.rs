#![deny(clippy::all)]

//! backblaze-cli — SlateOS Backblaze Personal Backup + B2 Cloud Storage
//!
//! Single personality: `backblaze`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bb(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: backblaze [OPTIONS]");
        println!("Backblaze (Slate OS) — Personal Backup + B2 Cloud Storage");
        println!();
        println!("Options:");
        println!("  --personal             Personal Backup (unlimited PC/Mac for $99/yr)");
        println!("  --b2 BUCKET            B2 Cloud Storage (S3-compatible object storage)");
        println!("  --restore-by-mail      Restore by Mail (HDD shipped with your data)");
        println!("  --business             Backblaze Business Backup");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Backblaze Backup 9.0.3.836 / B2 CLI 4.0 (Slate OS)"); return 0; }
    println!("Backblaze 9.0.3.836 / B2 4.0 (Slate OS)");
    println!("  Vendor: Backblaze, Inc. (San Mateo, CA; founded 2007)");
    println!("  Founders: Gleb Budman (CEO), Brian Wilson, Tim Nufire, Damon Uyeda, Casey Jones");
    println!("  IPO: NASDAQ:BLZE (Nov 2021)");
    println!("  Famous for: 'Hard Drive Stats' quarterly reports — public failure-rate data");
    println!("              from 200K+ drives in their data centers");
    println!("  Personal Backup: unlimited, $99/yr per computer, native client only (no NAS)");
    println!("  Engine: continuous incremental, AES-128 by default, user-key option (zero-knowledge)");
    println!("  B2 Cloud Storage: pay-as-you-go S3-compatible object storage");
    println!("                    Storage $6/TB/mo (vs S3 $23) — undercuts AWS deliberately");
    println!("                    Egress: free via Cloudflare/Bunny/Fastly partners (CDN)");
    println!("  Hardware: 'Storage Pods' — open-design, commodity-disk JBOD chassis");
    println!("            Drive Stats: SMR/Helium/HAMR comparisons quarterly");
    println!("  Restore: download, or 'Restore by Mail' (HDD shipped, refundable deposit)");
    println!("  Cap: 1 year version history (no Premium tier extends — was 30 days legacy)");
    println!("  Differentiator: simplicity, predictable pricing, hardware transparency");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "backblaze".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bb(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_bb};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/backblaze"), "backblaze");
        assert_eq!(basename(r"C:\bin\backblaze.exe"), "backblaze.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("backblaze.exe"), "backblaze");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_bb(&["--help".to_string()], "backblaze"), 0);
        assert_eq!(run_bb(&["-h".to_string()], "backblaze"), 0);
        let _ = run_bb(&["--version".to_string()], "backblaze");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_bb(&[], "backblaze");
    }
}
