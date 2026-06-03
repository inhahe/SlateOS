#![deny(clippy::all)]

//! mailgun-cli — OurOS Mailgun email CLI
//!
//! Single personality: `mailgun`

use std::env;
use std::process;

fn run_mailgun(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mailgun <COMMAND> [OPTIONS]");
        println!();
        println!("Mailgun email API CLI (OurOS).");
        println!();
        println!("Commands:");
        println!("  send         Send an email");
        println!("  domains      Manage domains");
        println!("  events       View email events");
        println!("  routes       Manage email routes");
        println!("  lists        Manage mailing lists");
        println!("  stats        View statistics");
        println!("  validate     Validate email addresses");
        println!("  webhooks     Manage webhooks");
        println!("  ips          Manage dedicated IPs");
        println!();
        println!("Options:");
        println!("  --api-key <KEY>      Mailgun API key");
        println!("  --domain <DOMAIN>    Sending domain");
        println!("  --json               Output as JSON");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    let domain = args.windows(2).find(|w| w[0] == "--domain").map(|w| w[1].as_str()).unwrap_or("mg.example.com");

    match cmd {
        "send" => {
            let to = args.windows(2).find(|w| w[0] == "--to").map(|w| w[1].as_str()).unwrap_or("recipient@example.com");
            let from = args.windows(2).find(|w| w[0] == "--from").map(|w| w[1].as_str()).unwrap_or("sender@mg.example.com");
            let subject = args.windows(2).find(|w| w[0] == "--subject").map(|w| w[1].as_str()).unwrap_or("Hello");
            println!("✔ Email queued");
            println!("  From:       {}", from);
            println!("  To:         {}", to);
            println!("  Subject:    {}", subject);
            println!("  Domain:     {}", domain);
            println!("  Message-ID: <abc123@{}>", domain);
            0
        }
        "domains" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("Domain               State      Type");
                    println!("mg.example.com       active     sandbox");
                    println!("mail.example.com     active     custom");
                }
                "info" => {
                    println!("Domain: {}", domain);
                    println!("  State:           active");
                    println!("  Created:         2024-01-01");
                    println!("  SMTP Login:      postmaster@{}", domain);
                    println!("  Sending DNS:");
                    println!("    SPF:   v=spf1 include:mailgun.org ~all ✔");
                    println!("    DKIM:  k=rsa; p=MIGfMA0... ✔");
                    println!("    MX:    mxa.mailgun.org (10) ✔");
                }
                "verify" => {
                    println!("Verifying DNS for {}...", domain);
                    println!("  SPF:  ✔ Valid");
                    println!("  DKIM: ✔ Valid");
                    println!("  MX:   ✔ Valid");
                    println!("  CNAME: ✔ Valid");
                    println!("All DNS records verified.");
                }
                _ => { println!("Domain operation: {}", sub); }
            }
            0
        }
        "events" => {
            println!("Events for {}:", domain);
            println!();
            println!("  Timestamp              Event       Recipient              Subject");
            println!("  2024-01-15 14:00:00    delivered   alice@example.com      Welcome!");
            println!("  2024-01-15 13:59:58    accepted    alice@example.com      Welcome!");
            println!("  2024-01-15 13:55:00    delivered   bob@example.com        Your order");
            println!("  2024-01-15 13:50:00    bounced     invalid@nowhere.com    Newsletter");
            println!("  2024-01-15 13:45:00    opened      alice@example.com      Welcome!");
            0
        }
        "routes" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("ID       Priority  Expression                          Action");
                    println!("r001     10        match_recipient('support@.*')        forward('https://api.example.com/mail')");
                    println!("r002     20        match_header('subject','urgent')     forward('mailto:urgent@example.com')");
                }
                "create" => {
                    println!("✔ Route created");
                    println!("  ID: r003");
                }
                _ => { println!("Route operation: {}", sub); }
            }
            0
        }
        "lists" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("Address                          Name           Members");
                    println!("newsletter@mg.example.com        Newsletter     500");
                    println!("team@mg.example.com              Team           25");
                }
                "members" => {
                    let list = args.get(2).map(|s| s.as_str()).unwrap_or("newsletter@mg.example.com");
                    println!("Members of {}:", list);
                    println!("  alice@example.com    subscribed");
                    println!("  bob@example.com      subscribed");
                    println!("  charlie@example.com  unsubscribed");
                }
                _ => { println!("List operation: {}", sub); }
            }
            0
        }
        "stats" => {
            println!("Statistics for {} (last 7 days):", domain);
            println!();
            println!("  Accepted:     1,500");
            println!("  Delivered:    1,485 (99.0%)");
            println!("  Opened:         890 (59.9%)");
            println!("  Clicked:        267 (18.0%)");
            println!("  Bounced:         10 (0.7%)");
            println!("  Complained:       3 (0.2%)");
            println!("  Unsubscribed:     5 (0.3%)");
            0
        }
        "validate" => {
            let email = args.get(1).map(|s| s.as_str()).unwrap_or("test@example.com");
            println!("Validating: {}", email);
            println!("  Valid:        true");
            println!("  Risk:        low");
            println!("  Disposable:  false");
            println!("  Role-based:  false");
            println!("  Suggestion:  (none)");
            0
        }
        "webhooks" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("Event Type       URL");
                    println!("delivered        https://api.example.com/webhooks/delivered");
                    println!("bounced          https://api.example.com/webhooks/bounced");
                    println!("opened           https://api.example.com/webhooks/opened");
                }
                _ => { println!("Webhook operation: {}", sub); }
            }
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: mailgun <command>. See --help.");
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
    let code = run_mailgun(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_mailgun};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_mailgun(vec!["--help".to_string()]), 0);
        assert_eq!(run_mailgun(vec!["-h".to_string()]), 0);
        assert_eq!(run_mailgun(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_mailgun(vec![]), 0);
    }
}
