#![deny(clippy::all)]

//! braintree-cli — Slate OS Braintree payment CLI
//!
//! Single personality: `braintree`

use std::env;
use std::process;

fn run_braintree(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: braintree <COMMAND> [OPTIONS]");
        println!();
        println!("Braintree payment gateway CLI (Slate OS).");
        println!();
        println!("Commands:");
        println!("  transactions  Manage transactions");
        println!("  customers     Manage customers");
        println!("  plans         Manage subscription plans");
        println!("  subscriptions Manage subscriptions");
        println!("  disputes      Manage disputes");
        println!("  merchants     Merchant account info");
        println!("  sandbox       Sandbox tools");
        println!("  status        Gateway status");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "transactions" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("ID            Amount      Type    Status          Created");
                    println!("txn_abc123    $99.00      sale    settled         2024-01-15");
                    println!("txn_def456    $49.99      sale    submitted       2024-01-15");
                    println!("txn_ghi789    $25.00      refund  settled         2024-01-14");
                }
                "sale" => {
                    let amount = args.windows(2).find(|w| w[0] == "--amount").map(|w| w[1].as_str()).unwrap_or("99.00");
                    println!("✔ Transaction created");
                    println!("  ID:     txn_new123");
                    println!("  Amount: ${}", amount);
                    println!("  Status: submitted_for_settlement");
                }
                "find" => {
                    let id = args.get(2).map(|s| s.as_str()).unwrap_or("txn_abc123");
                    println!("Transaction: {}", id);
                    println!("  Amount:     $99.00");
                    println!("  Type:       sale");
                    println!("  Status:     settled");
                    println!("  Card:       Visa ending in 1234");
                    println!("  Customer:   cus_abc123");
                }
                "refund" => {
                    let id = args.get(2).map(|s| s.as_str()).unwrap_or("txn_abc123");
                    println!("✔ Refund created for {}", id);
                    println!("  Refund ID: txn_ref123");
                }
                _ => { println!("Transaction operation: {}", sub); }
            }
            0
        }
        "customers" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("ID              Name             Email                  Cards");
                    println!("cus_abc123      Alice Smith      alice@example.com      2");
                    println!("cus_def456      Bob Jones        bob@example.com        1");
                }
                _ => { println!("Customer operation: {}", sub); }
            }
            0
        }
        "plans" => {
            println!("ID              Name              Price       Interval");
            println!("plan_pro        Pro Plan          $29.99      monthly");
            println!("plan_enterprise Enterprise        $99.99      monthly");
            println!("plan_starter    Starter           $9.99       monthly");
            0
        }
        "subscriptions" => {
            println!("ID                Customer       Plan           Status      Next Billing");
            println!("sub_abc123        cus_abc123     Pro Plan       Active      2024-02-15");
            println!("sub_def456        cus_def456     Starter        Active      2024-02-08");
            0
        }
        "disputes" => {
            println!("ID              Amount      Reason                    Status");
            println!("dsp_abc123      $99.00      fraud                     open");
            println!("dsp_def456      $49.99      product_not_received      won");
            0
        }
        "merchants" => {
            println!("Merchant Account:");
            println!("  ID:           merchant_abc123");
            println!("  Status:       active");
            println!("  Currency:     USD");
            println!("  Environment:  sandbox");
            0
        }
        "status" => {
            println!("Braintree Gateway Status: ✔ Operational");
            println!("  Transactions: Operational");
            println!("  Webhooks:     Operational");
            println!("  Control Panel: Operational");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: braintree <command>. See --help.");
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
    let code = run_braintree(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_braintree};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_braintree(vec!["--help".to_string()]), 0);
        assert_eq!(run_braintree(vec!["-h".to_string()]), 0);
        let _ = run_braintree(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_braintree(vec![]);
    }
}
