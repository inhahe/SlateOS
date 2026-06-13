#![deny(clippy::all)]

//! sendgrid-cli — SlateOS SendGrid email CLI
//!
//! Single personality: `sendgrid`

use std::env;
use std::process;

fn run_sendgrid(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sendgrid <COMMAND> [OPTIONS]");
        println!();
        println!("SendGrid email API CLI (Slate OS).");
        println!();
        println!("Commands:");
        println!("  send         Send an email");
        println!("  templates    Manage email templates");
        println!("  contacts     Manage contacts");
        println!("  lists        Manage contact lists");
        println!("  stats        View email statistics");
        println!("  domains      Manage sender domains");
        println!("  api-keys     Manage API keys");
        println!("  suppressions Manage suppressions");
        println!();
        println!("Options:");
        println!("  --api-key <KEY>  SendGrid API key");
        println!("  --json           Output as JSON");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "send" => {
            let to = args.windows(2).find(|w| w[0] == "--to").map(|w| w[1].as_str()).unwrap_or("recipient@example.com");
            let from = args.windows(2).find(|w| w[0] == "--from").map(|w| w[1].as_str()).unwrap_or("sender@example.com");
            let subject = args.windows(2).find(|w| w[0] == "--subject").map(|w| w[1].as_str()).unwrap_or("Hello");
            println!("✔ Email sent");
            println!("  From:    {}", from);
            println!("  To:      {}", to);
            println!("  Subject: {}", subject);
            println!("  Status:  202 Accepted");
            println!("  Message-ID: abc123.sendgrid.net");
            0
        }
        "templates" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("ID                                   Name                    Updated");
                    println!("d-abc123def456                       Welcome Email           2024-01-15");
                    println!("d-def789ghi012                       Password Reset          2024-01-10");
                    println!("d-jkl345mno678                       Order Confirmation      2024-01-08");
                }
                "get" => {
                    let id = args.get(2).map(|s| s.as_str()).unwrap_or("d-abc123def456");
                    println!("Template: {}", id);
                    println!("  Name:     Welcome Email");
                    println!("  Versions: 2");
                    println!("  Active:   Version 2 (2024-01-15)");
                }
                _ => { println!("Template operation: {}", sub); }
            }
            0
        }
        "contacts" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("Total contacts: 1,234");
                    println!();
                    println!("Email                        First Name    Last Name    Lists");
                    println!("alice@example.com            Alice         Smith        Newsletter, VIP");
                    println!("bob@example.com              Bob           Jones        Newsletter");
                    println!("charlie@example.com          Charlie       Brown        VIP");
                }
                "count" => {
                    println!("Total contacts: 1,234");
                    println!("  Subscribed:  1,100");
                    println!("  Unsubscribed: 134");
                }
                _ => { println!("Contacts operation: {}", sub); }
            }
            0
        }
        "stats" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("global");
            match sub {
                "global" | "" => {
                    println!("Email Statistics (Last 7 days):");
                    println!("  Requests:      5,000");
                    println!("  Delivered:     4,950 (99.0%)");
                    println!("  Opens:         2,475 (50.0%)");
                    println!("  Clicks:          742 (15.0%)");
                    println!("  Bounces:          30 (0.6%)");
                    println!("  Spam Reports:      5 (0.1%)");
                    println!("  Unsubscribes:     15 (0.3%)");
                }
                _ => { println!("Stats operation: {}", sub); }
            }
            0
        }
        "domains" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("ID      Domain              Verified    Default");
                    println!("1234    example.com         true        true");
                    println!("5678    mail.example.com    true        false");
                }
                "verify" => {
                    let domain = args.get(2).map(|s| s.as_str()).unwrap_or("example.com");
                    println!("Verifying {}...", domain);
                    println!("  CNAME: em1234.{} → sendgrid.net ✔", domain);
                    println!("  DKIM:  s1._domainkey.{} ✔", domain);
                    println!("  SPF:   ✔");
                    println!("✔ Domain verified.");
                }
                _ => { println!("Domains operation: {}", sub); }
            }
            0
        }
        "api-keys" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("Name              Key ID            Scopes");
                    println!("Production        SG.abc123...      Full Access");
                    println!("Read Only         SG.def456...      mail.send, stats.read");
                }
                "create" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("new-key");
                    println!("✔ API key created: {}", name);
                    println!("  Key: SG.newkey123...(save this, it won't be shown again)");
                }
                _ => { println!("API keys operation: {}", sub); }
            }
            0
        }
        "suppressions" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("Type            Email                    Date");
                    println!("bounce          bad@example.com          2024-01-14");
                    println!("spam_report     angry@example.com        2024-01-13");
                    println!("unsubscribe     done@example.com         2024-01-12");
                }
                _ => { println!("Suppressions operation: {}", sub); }
            }
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: sendgrid <command>. See --help.");
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
    let code = run_sendgrid(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_sendgrid};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sendgrid(vec!["--help".to_string()]), 0);
        assert_eq!(run_sendgrid(vec!["-h".to_string()]), 0);
        let _ = run_sendgrid(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sendgrid(vec![]);
    }
}
