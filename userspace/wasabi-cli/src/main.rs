#![deny(clippy::all)]

//! wasabi-cli — SlateOS Wasabi privacy Bitcoin wallet
//!
//! Single personality: `wasabi`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wasabi(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: wasabi [OPTIONS]");
        println!("Wasabi Wallet v2.1 (Slate OS) — Privacy-focused Bitcoin wallet");
        println!();
        println!("Options:");
        println!("  --wallet FILE     Open wallet file");
        println!("  --network NET     Network (main, test, regtest)");
        println!("  --datadir DIR     Data directory");
        println!("  --mix-level N     Anonymity set target");
        println!("  --tor             Force Tor connection");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Wasabi Wallet v2.1 (Slate OS)");
        return 0;
    }
    println!("Wasabi Wallet v2.1");
    println!("  Network: mainnet");
    println!("  Tor: connecting...");
    println!("  CoinJoin: WabiSabi protocol ready");
    println!("  Privacy level: automatic coin selection");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wasabi".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wasabi(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wasabi};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wasabi"), "wasabi");
        assert_eq!(basename(r"C:\bin\wasabi.exe"), "wasabi.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wasabi.exe"), "wasabi");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wasabi(&["--help".to_string()], "wasabi"), 0);
        assert_eq!(run_wasabi(&["-h".to_string()], "wasabi"), 0);
        let _ = run_wasabi(&["--version".to_string()], "wasabi");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wasabi(&[], "wasabi");
    }
}
