#![deny(clippy::all)]

//! solana-cli — OurOS Solana CLI
//!
//! Single personality: `solana`

use std::env;
use std::process;

fn run_solana(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: solana <COMMAND> [OPTIONS]");
        println!();
        println!("Solana blockchain CLI (OurOS).");
        println!();
        println!("Commands:");
        println!("  balance      Get SOL balance");
        println!("  transfer     Transfer SOL");
        println!("  airdrop      Request SOL airdrop (devnet/testnet)");
        println!("  stake        Stake SOL");
        println!("  validators   List validators");
        println!("  block        Get block info");
        println!("  transaction  Get transaction info");
        println!("  account      Get account info");
        println!("  address      Show keypair address");
        println!("  config       CLI configuration");
        println!("  cluster-version Show cluster version");
        println!("  epoch-info   Show epoch info");
        println!("  slot         Show current slot");
        println!("  deploy       Deploy a program");
        println!("  program      Program management");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("solana-cli 1.18.0 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "balance" => {
            let addr = args.get(1).map(|s| s.as_str()).unwrap_or("");
            if addr.is_empty() {
                println!("50.123456789 SOL");
            } else {
                println!("{}: 50.123456789 SOL", addr);
            }
            0
        }
        "transfer" => {
            let to = args.get(1).map(|s| s.as_str()).unwrap_or("Abc123...");
            let amount = args.get(2).map(|s| s.as_str()).unwrap_or("1.0");
            println!("Signature: 5UfD...abc123");
            println!("  From:   (default keypair)");
            println!("  To:     {}", to);
            println!("  Amount: {} SOL", amount);
            println!("  Fee:    0.000005 SOL");
            0
        }
        "airdrop" => {
            let amount = args.get(1).map(|s| s.as_str()).unwrap_or("2");
            println!("Requesting airdrop of {} SOL...", amount);
            println!("Signature: 4XyZ...def456");
            println!("{} SOL deposited.", amount);
            0
        }
        "config" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("get");
            match sub {
                "get" => {
                    println!("Config File: ~/.config/solana/cli/config.yml");
                    println!("RPC URL: https://api.mainnet-beta.solana.com");
                    println!("WebSocket URL: wss://api.mainnet-beta.solana.com");
                    println!("Keypair Path: ~/.config/solana/id.json");
                    println!("Commitment: confirmed");
                }
                "set" => {
                    let key = args.get(2).map(|s| s.as_str()).unwrap_or("--url");
                    let val = args.get(3).map(|s| s.as_str()).unwrap_or("https://api.devnet.solana.com");
                    println!("Setting {} to {}", key, val);
                }
                _ => { println!("Config operation: {}", sub); }
            }
            0
        }
        "cluster-version" => {
            println!("1.18.0");
            0
        }
        "epoch-info" => {
            println!("Block height:     250000000");
            println!("Slot:             280000000");
            println!("Epoch:            600");
            println!("Slot Index:       120000/432000");
            println!("Epoch Completed:  27.8%");
            0
        }
        "slot" => {
            println!("280000000");
            0
        }
        "validators" => {
            println!("  Identity                                     Vote Account                                 Commission  Last Vote  Root Slot  Credits  Version   Active Stake");
            println!("  Val1abc...                                   Vote1abc...                                  5%          280000000  279999998  1234567  1.18.0    1,500,000 SOL");
            println!("  Val2def...                                   Vote2def...                                  7%          280000000  279999997  1234500  1.18.0    800,000 SOL");
            println!("  Val3ghi...                                   Vote3ghi...                                  10%         279999999  279999995  1234400  1.17.35   500,000 SOL");
            0
        }
        "account" => {
            let addr = args.get(1).map(|s| s.as_str()).unwrap_or("Abc123...");
            println!("Public Key: {}", addr);
            println!("  Balance: 50.123456789 SOL");
            println!("  Owner: 11111111111111111111111111111111");
            println!("  Executable: false");
            println!("  Rent Epoch: 600");
            println!("  Data Length: 0");
            0
        }
        "address" => {
            println!("7Abc123DefGhi456JklMno789PqrStu012VwxYz3456");
            0
        }
        "deploy" => {
            let program = args.get(1).map(|s| s.as_str()).unwrap_or("program.so");
            println!("Deploying {}...", program);
            println!("  Program ID: Prog123...abc456");
            println!("  Signature:  9XyZ...ghi789");
            println!("  Deploy cost: 2.5 SOL");
            0
        }
        "block" => {
            let slot = args.get(1).map(|s| s.as_str()).unwrap_or("280000000");
            println!("Slot: {}", slot);
            println!("  Parent Slot:   {}", slot.parse::<u64>().unwrap_or(280000000) - 1);
            println!("  Block Time:    2024-01-15 14:00:00 UTC");
            println!("  Block Height:  250000000");
            println!("  Transactions:  2,500");
            println!("  Rewards:       15 (validator rewards)");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: solana <command>. See --help.");
            } else {
                eprintln!("Error: unknown command '{}'. See --help.", cmd);
            }
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_solana(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_solana};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_solana(vec!["--help".to_string()]), 0);
        assert_eq!(run_solana(vec!["-h".to_string()]), 0);
        let _ = run_solana(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_solana(vec![]);
    }
}
