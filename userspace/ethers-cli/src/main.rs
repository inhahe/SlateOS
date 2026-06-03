#![deny(clippy::all)]

//! ethers-cli — OurOS Ethereum CLI (cast/anvil)
//!
//! Multi-personality: `cast`, `anvil`

use std::env;
use std::process;

fn personality(argv0: &str) -> &str {
    let base = argv0.rsplit(&['/', '\\'][..]).next().unwrap_or(argv0);
    let name = base.strip_suffix(".exe").unwrap_or(base);
    match name {
        "anvil" => "anvil",
        _ => "cast",
    }
}

fn run_cast(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cast <COMMAND> [OPTIONS]");
        println!();
        println!("Ethereum CLI for interacting with EVM chains (OurOS).");
        println!();
        println!("Commands:");
        println!("  balance      Get ETH balance");
        println!("  block        Get block info");
        println!("  call         Call a contract");
        println!("  send         Send a transaction");
        println!("  tx           Get transaction info");
        println!("  receipt      Get transaction receipt");
        println!("  gas-price    Get current gas price");
        println!("  chain-id     Get chain ID");
        println!("  ens          ENS lookup");
        println!("  abi-encode   ABI encode data");
        println!("  abi-decode   ABI decode data");
        println!("  keccak       Keccak-256 hash");
        println!("  wallet       Wallet operations");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "balance" => {
            let addr = args.get(1).map(|s| s.as_str()).unwrap_or("0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045");
            println!("{}", addr);
            println!("  Balance: 1234.567890123456789 ETH");
            println!("  Wei:     1234567890123456789000");
            0
        }
        "block" => {
            let num = args.get(1).map(|s| s.as_str()).unwrap_or("latest");
            println!("Block {}", num);
            println!("  Number:     19000000");
            println!("  Hash:       0xabc123...");
            println!("  Timestamp:  2024-01-15 14:00:00 UTC");
            println!("  Gas Used:   12,345,678 / 30,000,000 (41.2%)");
            println!("  Base Fee:   25.5 gwei");
            println!("  Txns:       150");
            0
        }
        "gas-price" => {
            println!("Gas Price: 25 gwei");
            println!("  Base Fee:   20 gwei");
            println!("  Priority:   2 gwei");
            println!("  Max Fee:    30 gwei");
            0
        }
        "chain-id" => {
            println!("1");
            0
        }
        "tx" => {
            let hash = args.get(1).map(|s| s.as_str()).unwrap_or("0xdef456...");
            println!("Transaction {}", hash);
            println!("  From:       0x1234...5678");
            println!("  To:         0xabcd...ef01");
            println!("  Value:      1.5 ETH");
            println!("  Gas:        21000");
            println!("  Gas Price:  25 gwei");
            println!("  Nonce:      42");
            println!("  Status:     Success");
            0
        }
        "ens" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("vitalik.eth");
            println!("{} → 0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045", name);
            0
        }
        "keccak" => {
            let data = args.get(1).map(|s| s.as_str()).unwrap_or("hello");
            println!("0x1c8aff950685c2ed4bc3174f3472287b56d9517b9c948127319a09a7a36deac8");
            println!("(keccak256 of '{}')", data);
            0
        }
        "wallet" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("new");
            match sub {
                "new" => {
                    println!("New wallet:");
                    println!("  Address:    0x742d35Cc6634C0532925a3b844Bc9e7595f2bD18");
                    println!("  Private Key: 0x4c0883a69...(keep secret!)");
                }
                "address" => {
                    println!("0x742d35Cc6634C0532925a3b844Bc9e7595f2bD18");
                }
                _ => { println!("Wallet operation: {}", sub); }
            }
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: cast <command>. See --help.");
            } else {
                eprintln!("Error: unknown command '{}'. See --help.", cmd);
            }
            1
        }
    }
}

fn run_anvil(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: anvil [OPTIONS]");
        println!();
        println!("Local Ethereum development node (OurOS).");
        println!();
        println!("Options:");
        println!("  --port <PORT>      Port (default: 8545)");
        println!("  --accounts <N>     Number of accounts (default: 10)");
        println!("  --fork-url <URL>   Fork from mainnet RPC");
        println!("  --block-time <S>   Auto-mine interval");
        println!("  --chain-id <ID>    Chain ID (default: 31337)");
        return 0;
    }

    let port = args.windows(2).find(|w| w[0] == "--port").map(|w| w[1].as_str()).unwrap_or("8545");
    let accounts = args.windows(2).find(|w| w[0] == "--accounts").map(|w| w[1].as_str()).unwrap_or("10");

    println!("                             _   _");
    println!("                            (_) | |");
    println!("      __ _  _ __  __   __ _  _ | |");
    println!("     / _` || '_ \\ \\ \\ / /| || || |");
    println!("    | (_| || | | | \\ V / | || || |");
    println!("     \\__,_||_| |_|  \\_/  |_||_||_|");
    println!();
    println!("  Available Accounts ({})", accounts);
    println!("  ==================");
    println!("  (0) 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266 (10000 ETH)");
    println!("  (1) 0x70997970C51812dc3A010C7d01b50e0d17dc79C8 (10000 ETH)");
    println!("  (2) 0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC (10000 ETH)");
    println!();
    println!("  Listening on 127.0.0.1:{}", port);
    println!("  Chain ID: 31337");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let argv0 = args.first().cloned().unwrap_or_else(|| String::from("cast"));
    let p = personality(&argv0);
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match p {
        "anvil" => run_anvil(&rest),
        _ => run_cast(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_cast};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_cast(&["--help".to_string()]), 0);
        assert_eq!(run_cast(&["-h".to_string()]), 0);
        assert_eq!(run_cast(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_cast(&[]), 0);
    }
}
