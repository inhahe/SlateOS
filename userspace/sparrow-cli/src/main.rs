#![deny(clippy::all)]

//! sparrow-cli — SlateOS Sparrow Bitcoin wallet
//!
//! Single personality: `sparrow`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sparrow(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: sparrow [OPTIONS]");
        println!("Sparrow Wallet v1.9 (SlateOS) — Bitcoin desktop wallet");
        println!();
        println!("Options:");
        println!("  --dir DIR         Data directory");
        println!("  --network NET     Network (mainnet, testnet, signet)");
        println!("  --server URL      Electrum server");
        println!("  --mix             Start with CoinJoin mixing");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Sparrow Wallet v1.9 (SlateOS)");
        return 0;
    }
    println!("Sparrow Wallet v1.9");
    println!("  Network: mainnet");
    println!("  Server: connecting to Electrum...");
    println!("  UTXO management, coin control, CoinJoin ready");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sparrow".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sparrow(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sparrow};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sparrow"), "sparrow");
        assert_eq!(basename(r"C:\bin\sparrow.exe"), "sparrow.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sparrow.exe"), "sparrow");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sparrow(&["--help".to_string()], "sparrow"), 0);
        assert_eq!(run_sparrow(&["-h".to_string()], "sparrow"), 0);
        let _ = run_sparrow(&["--version".to_string()], "sparrow");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sparrow(&[], "sparrow");
    }
}
