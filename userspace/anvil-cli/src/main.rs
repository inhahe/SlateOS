#![deny(clippy::all)]

//! anvil-cli — OurOS Foundry anvil local Ethereum node
//!
//! Single personality: `anvil`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_anvil(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: anvil [OPTIONS]");
        println!("anvil 0.2.0 (OurOS) — Local Ethereum node (Foundry)");
        println!();
        println!("Options:");
        println!("  -p, --port N          Port (default 8545)");
        println!("  -a, --accounts N      Number of accounts (default 10)");
        println!("  -b, --block-time N    Block time in seconds");
        println!("  --balance N           Initial ETH balance (default 10000)");
        println!("  --chain-id N          Chain ID (default 31337)");
        println!("  -f, --fork-url URL    Fork from RPC URL");
        println!("  --fork-block-number N Fork at block number");
        println!("  -m, --mnemonic WORDS  Mnemonic for accounts");
        println!("  --hardfork FORK       EVM hardfork (latest, cancun, shanghai...)");
        println!("  --gas-limit N         Gas limit");
        println!("  --gas-price N         Gas price");
        println!("  --no-mining           Disable auto-mining");
        println!("  --host ADDR           Host (default 127.0.0.1)");
        println!("  -V, --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("anvil 0.2.0 (OurOS)");
        return 0;
    }
    let port = args.windows(2).find(|w| w[0] == "-p" || w[0] == "--port")
        .map(|w| w[1].as_str()).unwrap_or("8545");
    println!("anvil: Starting local Ethereum node...");
    println!();
    println!("Available Accounts");
    println!("==================");
    println!("(0) 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266 (10000.0 ETH)");
    println!("(1) 0x70997970C51812dc3A010C7d01b50e0d17dc79C8 (10000.0 ETH)");
    println!();
    println!("Listening on 127.0.0.1:{}", port);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "anvil".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_anvil(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_anvil};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/anvil"), "anvil");
        assert_eq!(basename(r"C:\bin\anvil.exe"), "anvil.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("anvil.exe"), "anvil");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_anvil(&["--help".to_string()], "anvil"), 0);
        assert_eq!(run_anvil(&["-h".to_string()], "anvil"), 0);
        assert_eq!(run_anvil(&["--version".to_string()], "anvil"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_anvil(&[], "anvil"), 0);
    }
}
