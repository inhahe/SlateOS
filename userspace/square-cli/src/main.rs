#![deny(clippy::all)]

//! square-cli — OurOS Square CLI
//!
//! Single personality: `square`

use std::env;
use std::process;

fn run_square(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: square <COMMAND> [OPTIONS]");
        println!();
        println!("Square developer CLI (OurOS).");
        println!();
        println!("Commands:");
        println!("  payments     Manage payments");
        println!("  orders       Manage orders");
        println!("  customers    Manage customers");
        println!("  catalog      Manage catalog");
        println!("  inventory    Manage inventory");
        println!("  locations    List locations");
        println!("  sandbox      Sandbox management");
        println!("  webhooks     Manage webhooks");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "payments" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("ID                      Amount     Status       Source      Created");
                    println!("pmt_abc123def456        $25.00     COMPLETED    CARD        2024-01-15 14:00");
                    println!("pmt_ghi789jkl012        $42.50     COMPLETED    CARD        2024-01-15 13:30");
                    println!("pmt_mno345pqr678        $15.00     COMPLETED    CASH        2024-01-15 12:00");
                }
                _ => { println!("Payment operation: {}", sub); }
            }
            0
        }
        "orders" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("ID                      Total      Items  State       Created");
                    println!("ord_abc123              $67.50     3      COMPLETED   2024-01-15");
                    println!("ord_def456              $25.00     1      OPEN        2024-01-15");
                }
                _ => { println!("Order operation: {}", sub); }
            }
            0
        }
        "customers" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("ID                 Name             Email                    Visits");
                    println!("cust_abc123        Alice Smith      alice@example.com        15");
                    println!("cust_def456        Bob Jones        bob@example.com          8");
                }
                _ => { println!("Customer operation: {}", sub); }
            }
            0
        }
        "catalog" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("ID                Type        Name                Price");
                    println!("cat_abc123        ITEM        Coffee              $5.00");
                    println!("cat_def456        ITEM        Sandwich            $12.50");
                    println!("cat_ghi789        MODIFIER    Extra Shot          $1.50");
                }
                _ => { println!("Catalog operation: {}", sub); }
            }
            0
        }
        "locations" => {
            println!("ID                Name              Address                     Status");
            println!("loc_abc123        Main Store        123 Main St, City, ST       ACTIVE");
            println!("loc_def456        Downtown          456 Market St, City, ST     ACTIVE");
            0
        }
        "inventory" => {
            println!("Item              Location       Quantity    State");
            println!("Coffee Beans      Main Store     45          IN_STOCK");
            println!("Sandwich Bread    Main Store     20          IN_STOCK");
            println!("Coffee Beans      Downtown       12          LOW_STOCK");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: square <command>. See --help.");
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
    let code = run_square(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_square};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_square(vec!["--help".to_string()]), 0);
        assert_eq!(run_square(vec!["-h".to_string()]), 0);
        assert_eq!(run_square(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_square(vec![]), 0);
    }
}
