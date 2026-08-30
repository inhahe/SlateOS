#![deny(clippy::all)]

//! metamask-cli — Slate OS MetaMask-style Ethereum wallet
//!
//! Single personality: `metamask`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_metamask(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: metamask COMMAND [OPTIONS]");
        println!("MetaMask CLI v12.0 (Slate OS) — Ethereum wallet");
        println!();
        println!("Commands:");
        println!("  create            Create new wallet");
        println!("  import            Import from seed phrase");
        println!("  balance           Show ETH balance");
        println!("  send ADDR AMOUNT  Send ETH");
        println!("  networks          List configured networks");
        println!("  tokens            List token balances");
        println!("  sign MSG          Sign a message");
        println!("  version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("MetaMask CLI v12.0 (Slate OS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("balance");
    match cmd {
        "create" => {
            println!("Creating new wallet...");
            println!("  Address: 0x742d35Cc6634C0532925a3b844Bc9e7595f2bD21");
            println!("  Save your seed phrase securely!");
        }
        "balance" => {
            println!("Network: Ethereum Mainnet");
            println!("  ETH: 0.0000");
        }
        "networks" => {
            println!("Configured networks:");
            println!("  Ethereum Mainnet (chainId: 1)");
            println!("  Sepolia Testnet (chainId: 11155111)");
            println!("  Polygon (chainId: 137)");
            println!("  Arbitrum One (chainId: 42161)");
        }
        "tokens" => println!("No tokens found."),
        _ => println!("metamask {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "metamask".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_metamask(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_metamask};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/metamask"), "metamask");
        assert_eq!(basename(r"C:\bin\metamask.exe"), "metamask.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("metamask.exe"), "metamask");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_metamask(&["--help".to_string()], "metamask"), 0);
        assert_eq!(run_metamask(&["-h".to_string()], "metamask"), 0);
        let _ = run_metamask(&["--version".to_string()], "metamask");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_metamask(&[], "metamask");
    }
}
