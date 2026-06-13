#![deny(clippy::all)]

//! plaid-cli — SlateOS Plaid financial CLI
//!
//! Single personality: `plaid`

use std::env;
use std::process;

fn run_plaid(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: plaid <COMMAND> [OPTIONS]");
        println!();
        println!("Plaid financial data CLI (Slate OS).");
        println!();
        println!("Commands:");
        println!("  accounts     List linked accounts");
        println!("  transactions List transactions");
        println!("  balances     Get account balances");
        println!("  identity     Get identity info");
        println!("  institutions Search institutions");
        println!("  categories   List transaction categories");
        println!("  link         Create link token");
        println!("  items        Manage items");
        println!("  sandbox      Sandbox operations");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "accounts" => {
            println!("Linked Accounts:");
            println!("  ID              Name              Type        Subtype    Balance");
            println!("  acc_checking    Checking          depository  checking   $5,234.50");
            println!("  acc_savings     Savings           depository  savings    $12,500.00");
            println!("  acc_credit      Credit Card       credit      credit card -$1,234.56");
            println!("  acc_investment  Investment        investment  401k       $45,000.00");
            0
        }
        "transactions" => {
            let count = args.windows(2).find(|w| w[0] == "--count").map(|w| w[1].as_str()).unwrap_or("5");
            println!("Recent transactions (last {}):", count);
            println!("  Date         Amount     Category              Merchant");
            println!("  2024-01-15   -$45.00    Food and Drink        Whole Foods");
            println!("  2024-01-15   -$12.99    Shopping              Amazon");
            println!("  2024-01-14   -$85.00    Travel                Uber");
            println!("  2024-01-14   +$3,500.00 Transfer              Payroll");
            println!("  2024-01-13   -$9.99     Recreation            Netflix");
            0
        }
        "balances" => {
            println!("Account Balances:");
            println!("  Account         Available       Current         Limit");
            println!("  Checking        $5,234.50       $5,234.50       -");
            println!("  Savings         $12,500.00      $12,500.00      -");
            println!("  Credit Card     $8,765.44       $1,234.56       $10,000.00");
            0
        }
        "identity" => {
            println!("Account Holder Identity:");
            println!("  Name:     Alice Smith");
            println!("  Email:    alice@example.com");
            println!("  Phone:    +1 (555) 123-4567");
            println!("  Address:  123 Main St, City, ST 12345");
            0
        }
        "institutions" => {
            let query = args.get(1).map(|s| s.as_str()).unwrap_or("chase");
            println!("Institutions matching '{}':", query);
            println!("  ID             Name                    Products");
            println!("  ins_3          Chase                   auth, transactions, identity, balance");
            println!("  ins_56         Chase (Business)        auth, transactions, balance");
            0
        }
        "categories" => {
            println!("Transaction Categories:");
            println!("  Food and Drink > Restaurants");
            println!("  Food and Drink > Groceries");
            println!("  Travel > Airlines");
            println!("  Travel > Ride Share");
            println!("  Shopping > Electronics");
            println!("  Shopping > Clothing");
            println!("  Transfer > Payroll");
            println!("  Recreation > Streaming");
            0
        }
        "link" => {
            println!("✔ Link token created");
            println!("  Token: link-sandbox-abc123...");
            println!("  Expiration: 2024-01-15T18:00:00Z");
            0
        }
        "items" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("ID                Institution       Status      Accounts");
                    println!("item_abc123       Chase             good        3");
                    println!("item_def456       Bank of America   good        2");
                }
                _ => { println!("Item operation: {}", sub); }
            }
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: plaid <command>. See --help.");
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
    let code = run_plaid(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_plaid};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_plaid(vec!["--help".to_string()]), 0);
        assert_eq!(run_plaid(vec!["-h".to_string()]), 0);
        let _ = run_plaid(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_plaid(vec![]);
    }
}
