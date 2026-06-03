#![deny(clippy::all)]

//! kwallet-cli — OurOS KDE KWallet password storage
//!
//! Multi-personality: `kwalletd6`, `kwalletmanager5`, `kwallet-query`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kwalletd(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kwalletd6 [OPTIONS]");
        println!("kwalletd6 v6.0 (OurOS) — KDE Wallet daemon");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("kwalletd6 v6.0 (OurOS)"); return 0; }
    println!("kwalletd6: wallet daemon started");
    println!("  Wallets: kdewallet (default)");
    0
}

fn run_manager(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kwalletmanager5 [OPTIONS]");
        println!("kwalletmanager5 v23.08 (OurOS) — KDE Wallet Manager");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("kwalletmanager5 v23.08 (OurOS)"); return 0; }
    println!("kwalletmanager5: wallet manager started");
    println!("  Wallets: 1");
    println!("  Entries: 45");
    0
}

fn run_query(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kwallet-query [OPTIONS] WALLET");
        println!("kwallet-query v6.0 (OurOS) — Query KDE Wallet");
        println!();
        println!("Options:");
        println!("  -l                List folders");
        println!("  -r ENTRY          Read entry");
        println!("  -f FOLDER         Folder name");
        return 0;
    }
    if args.iter().any(|a| a == "-l") {
        println!("Passwords");
        println!("Form Data");
        println!("Maps");
        return 0;
    }
    println!("kwallet-query: query complete");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kwalletd6".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "kwalletmanager5" => run_manager(&rest, &prog),
        "kwallet-query" => run_query(&rest, &prog),
        _ => run_kwalletd(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_kwalletd};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kwallet"), "kwallet");
        assert_eq!(basename(r"C:\bin\kwallet.exe"), "kwallet.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kwallet.exe"), "kwallet");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_kwalletd(&["--help".to_string()], "kwallet"), 0);
        assert_eq!(run_kwalletd(&["-h".to_string()], "kwallet"), 0);
        assert_eq!(run_kwalletd(&["--version".to_string()], "kwallet"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_kwalletd(&[], "kwallet"), 0);
    }
}
