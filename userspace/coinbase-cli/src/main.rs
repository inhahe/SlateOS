#![deny(clippy::all)]

//! coinbase-cli — Slate OS Coinbase CLI
//!
//! Single personality: `coinbase`

use std::env;
use std::process;

fn run_coinbase(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: coinbase <COMMAND> [OPTIONS]");
        println!();
        println!("Coinbase cryptocurrency CLI (Slate OS).");
        println!();
        println!("Commands:");
        println!("  accounts     List accounts");
        println!("  prices       Get prices");
        println!("  trades       List trades");
        println!("  buy          Buy crypto");
        println!("  sell         Sell crypto");
        println!("  send         Send crypto");
        println!("  addresses    List addresses");
        println!("  portfolio    Portfolio summary");
        println!("  currencies   List currencies");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "accounts" => {
            println!("Accounts:");
            println!("  Currency    Balance          Value (USD)");
            println!("  BTC         0.25000000       $10,625.00");
            println!("  ETH         2.50000000       $6,250.00");
            println!("  SOL         50.00000000      $5,000.00");
            println!("  USDC        1,000.00         $1,000.00");
            println!("  USD         500.00           $500.00");
            0
        }
        "prices" => {
            let pair = args.get(1).map(|s| s.as_str()).unwrap_or("BTC-USD");
            match pair {
                "BTC-USD" => {
                    println!("BTC/USD:");
                    println!("  Spot:    $42,500.00");
                    println!("  Buy:     $42,550.00");
                    println!("  Sell:    $42,450.00");
                    println!("  24h:     +2.3%");
                }
                "ETH-USD" => {
                    println!("ETH/USD:");
                    println!("  Spot:    $2,500.00");
                    println!("  Buy:     $2,505.00");
                    println!("  Sell:    $2,495.00");
                    println!("  24h:     +1.8%");
                }
                _ => {
                    println!("{}: $100.00 (spot)", pair);
                }
            }
            0
        }
        "trades" => {
            println!("Recent Trades:");
            println!("  Date         Type   Currency  Amount        Price         Total");
            println!("  2024-01-15   Buy    BTC       0.01000000    $42,500.00    $425.00");
            println!("  2024-01-14   Sell   ETH       1.00000000    $2,480.00     $2,480.00");
            println!("  2024-01-13   Buy    SOL       25.00000000   $98.00        $2,450.00");
            0
        }
        "buy" => {
            let currency = args.get(1).map(|s| s.as_str()).unwrap_or("BTC");
            let amount = args.windows(2).find(|w| w[0] == "--amount").map(|w| w[1].as_str()).unwrap_or("100.00");
            println!("✔ Buy order placed");
            println!("  Currency: {}", currency);
            println!("  Amount:   ${}", amount);
            println!("  Fee:      $1.49");
            println!("  Status:   completed");
            0
        }
        "sell" => {
            let currency = args.get(1).map(|s| s.as_str()).unwrap_or("BTC");
            let amount = args.windows(2).find(|w| w[0] == "--amount").map(|w| w[1].as_str()).unwrap_or("0.01");
            println!("✔ Sell order placed");
            println!("  Currency: {}", currency);
            println!("  Amount:   {} {}", amount, currency);
            println!("  Fee:      $1.49");
            println!("  Status:   completed");
            0
        }
        "send" => {
            let currency = args.get(1).map(|s| s.as_str()).unwrap_or("BTC");
            let to = args.windows(2).find(|w| w[0] == "--to").map(|w| w[1].as_str()).unwrap_or("1A1zP1...");
            let amount = args.windows(2).find(|w| w[0] == "--amount").map(|w| w[1].as_str()).unwrap_or("0.001");
            println!("✔ Send initiated");
            println!("  Currency: {}", currency);
            println!("  Amount:   {} {}", amount, currency);
            println!("  To:       {}", to);
            println!("  Network fee: 0.00001 {}", currency);
            println!("  Status:   pending");
            0
        }
        "portfolio" => {
            println!("Portfolio Summary:");
            println!("  Total Value:   $23,375.00");
            println!("  24h Change:    +$542.50 (+2.4%)");
            println!();
            println!("  Asset   Allocation   Value          24h");
            println!("  BTC     45.5%        $10,625.00     +2.3%");
            println!("  ETH     26.7%        $6,250.00      +1.8%");
            println!("  SOL     21.4%        $5,000.00      +4.2%");
            println!("  USDC    4.3%         $1,000.00      0.0%");
            println!("  USD     2.1%         $500.00        -");
            0
        }
        "currencies" => {
            println!("Supported Currencies:");
            println!("  Symbol  Name              Min Order");
            println!("  BTC     Bitcoin           $1.00");
            println!("  ETH     Ethereum          $1.00");
            println!("  SOL     Solana            $1.00");
            println!("  USDC    USD Coin          $1.00");
            println!("  DOGE    Dogecoin          $1.00");
            println!("  ADA     Cardano           $1.00");
            println!("  AVAX    Avalanche         $1.00");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: coinbase <command>. See --help.");
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
    let code = run_coinbase(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_coinbase};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_coinbase(vec!["--help".to_string()]), 0);
        assert_eq!(run_coinbase(vec!["-h".to_string()]), 0);
        let _ = run_coinbase(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_coinbase(vec![]);
    }
}
