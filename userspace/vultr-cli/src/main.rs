#![deny(clippy::all)]

//! vultr-cli — Slate OS Vultr CLI
//!
//! Single personality: `vultr-cli`

use std::env;
use std::process;

fn run_vultr(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "help") {
        println!("Usage: vultr-cli <COMMAND> [OPTIONS]");
        println!();
        println!("vultr-cli — Vultr cloud CLI (Slate OS).");
        println!();
        println!("Commands:");
        println!("  instance       Manage cloud instances");
        println!("  bare-metal     Manage bare metal servers");
        println!("  block-storage  Manage block storage");
        println!("  kubernetes     Manage Kubernetes clusters");
        println!("  database       Manage managed databases");
        println!("  dns            Manage DNS");
        println!("  firewall       Manage firewall groups/rules");
        println!("  object-storage Manage object storage");
        println!("  snapshot       Manage snapshots");
        println!("  regions        List regions");
        println!("  plans          List plans");
        println!("  os             List operating systems");
        println!("  account        Account info");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "version") {
        println!("vultr-cli v3.0.3 (Slate OS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");

    match cmd {
        "instance" => match sub {
            "list" => {
                println!("ID                                      IP              LABEL          OS              STATUS  REGION  CPU  RAM     DISK    BANDWIDTH");
                println!("abcdef12-3456-7890-abcd-ef1234567890    149.28.123.45   my-instance    Ubuntu 22.04    active  ewr     1    1024    25      1000");
            }
            "create" => {
                println!("ID                                      IP              LABEL          STATUS");
                println!("12345678-abcd-efgh-ijkl-123456789012    pending         new-instance   pending");
            }
            _ => { println!("vultr-cli instance {}: see --help.", sub); }
        },
        "kubernetes" => match sub {
            "list" => {
                println!("ID                                      LABEL          REGION  VERSION    STATUS");
                println!("abcdef12-3456-7890-abcd-ef1234567890    my-vke         ewr     v1.28.2    active");
            }
            _ => { println!("vultr-cli kubernetes {}: see --help.", sub); }
        },
        "regions" | "regions list" => {
            println!("ID     CITY                COUNTRY   CONTINENT");
            println!("ewr    New Jersey           US        North America");
            println!("ord    Chicago              US        North America");
            println!("ams    Amsterdam            NL        Europe");
            println!("sgp    Singapore            SG        Asia");
            println!("nrt    Tokyo                JP        Asia");
        }
        "plans" | "plans list" => {
            println!("ID              VCPU  RAM     DISK    BANDWIDTH   MONTHLY PRICE");
            println!("vc2-1c-1gb      1     1024    25      1.00        $5.00");
            println!("vc2-1c-2gb      1     2048    55      2.00        $10.00");
            println!("vc2-2c-4gb      2     4096    80      3.00        $20.00");
            println!("vc2-4c-8gb      4     8192    160     4.00        $40.00");
        }
        "account" => {
            println!("Name: My Account");
            println!("Email: user@example.com");
            println!("Balance: $50.00");
            println!("Pending Charges: $12.34");
        }
        "snapshot" => match sub {
            "list" => {
                println!("ID                                      DESCRIPTION       STATUS      SIZE    DATE CREATED");
                println!("abcdef12-3456-7890-abcd-ef1234567890    daily-backup      complete    25      2024-01-15");
            }
            _ => { println!("vultr-cli snapshot {}: see --help.", sub); }
        },
        _ => {
            if cmd.is_empty() {
                eprintln!("vultr-cli: no command specified. See --help.");
                return 1;
            }
            println!("vultr-cli {}: see vultr-cli {} --help.", cmd, cmd);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_vultr(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_vultr};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_vultr(vec!["--help".to_string()]), 0);
        assert_eq!(run_vultr(vec!["-h".to_string()]), 0);
        let _ = run_vultr(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_vultr(vec![]);
    }
}
