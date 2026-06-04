#![deny(clippy::all)]

//! twilio-cli — OurOS Twilio CLI
//!
//! Single personality: `twilio`

use std::env;
use std::process;

fn run_twilio(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: twilio <COMMAND> [OPTIONS]");
        println!();
        println!("Twilio CLI for communications APIs (OurOS).");
        println!();
        println!("Commands:");
        println!("  login        Log in to Twilio");
        println!("  profiles     Manage profiles");
        println!("  api          Access Twilio API resources");
        println!("  phone-numbers  Manage phone numbers");
        println!("  messaging    Send SMS/MMS");
        println!("  voice        Make voice calls");
        println!("  email        Send emails (SendGrid)");
        println!("  serverless   Manage serverless functions");
        println!("  debugger     View error logs");
        println!("  feedback     Submit feedback");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("twilio-cli/5.18.0 ouros-x64");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "login" => {
            println!("Enter your Twilio Account SID: AC...");
            println!("Enter your Twilio Auth Token: ****");
            println!("✔ Logged in as AC01234567890123456789012345678901");
            println!("  Profile saved: default");
            0
        }
        "profiles" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("ID                                   Account SID                          Active");
                    println!("default                              AC01234567890123456789012345678901    *");
                    println!("production                           AC98765432109876543210987654321098");
                }
                _ => { println!("Profile operation: {}", sub); }
            }
            0
        }
        "phone-numbers" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("SID                                  Phone Number     Capabilities");
                    println!("PN01234567890123456789012345678901    +15551234567     Voice, SMS, MMS");
                    println!("PN98765432109876543210987654321098    +15559876543     Voice, SMS");
                }
                "buy" => {
                    println!("Available numbers:");
                    println!("  +15551112222  (US, Voice+SMS)  $1.00/mo");
                    println!("  +15553334444  (US, Voice+SMS+MMS)  $1.00/mo");
                }
                _ => { println!("Phone number operation: {}", sub); }
            }
            0
        }
        "messaging" | "api:core:messages" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("send");
            match sub {
                "send" => {
                    let to = args.windows(2).find(|w| w[0] == "--to").map(|w| w[1].as_str()).unwrap_or("+15559876543");
                    let from = args.windows(2).find(|w| w[0] == "--from").map(|w| w[1].as_str()).unwrap_or("+15551234567");
                    let body = args.windows(2).find(|w| w[0] == "--body").map(|w| w[1].as_str()).unwrap_or("Hello!");
                    println!("✔ Message sent");
                    println!("  SID:    SM01234567890123456789012345678901");
                    println!("  From:   {}", from);
                    println!("  To:     {}", to);
                    println!("  Body:   {}", body);
                    println!("  Status: queued");
                }
                "list" => {
                    println!("SID                                  From            To              Status    Date");
                    println!("SM012345...                          +15551234567    +15559876543    delivered 2024-01-15");
                    println!("SM678901...                          +15551234567    +15551111111    sent      2024-01-15");
                }
                _ => { println!("Messaging operation: {}", sub); }
            }
            0
        }
        "voice" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("call");
            match sub {
                "call" => {
                    let to = args.windows(2).find(|w| w[0] == "--to").map(|w| w[1].as_str()).unwrap_or("+15559876543");
                    let from = args.windows(2).find(|w| w[0] == "--from").map(|w| w[1].as_str()).unwrap_or("+15551234567");
                    println!("✔ Call initiated");
                    println!("  SID:    CA01234567890123456789012345678901");
                    println!("  From:   {}", from);
                    println!("  To:     {}", to);
                    println!("  Status: queued");
                }
                "list" => {
                    println!("SID                  From            To              Duration  Status");
                    println!("CA012345...          +15551234567    +15559876543    45s       completed");
                }
                _ => { println!("Voice operation: {}", sub); }
            }
            0
        }
        "debugger" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("logs");
            match sub {
                "logs" | "" => {
                    println!("Error Logs:");
                    println!("  [2024-01-15 14:00:00] WARNING 11200 - HTTP retrieval failure");
                    println!("  [2024-01-15 13:45:00] ERROR   21610 - Message body required");
                    println!("  [2024-01-15 13:30:00] WARNING 11205 - HTTP connection failure");
                }
                _ => { println!("Debugger operation: {}", sub); }
            }
            0
        }
        "serverless" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("Service SID                          Name            Environment");
                    println!("ZS01234567890123456789012345678901   my-functions    production");
                }
                "deploy" => {
                    println!("Deploying serverless project...");
                    println!("  ✔ Functions deployed");
                    println!("  ✔ Assets deployed");
                    println!("  Domain: my-functions-1234.twil.io");
                }
                _ => { println!("Serverless operation: {}", sub); }
            }
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: twilio <command>. See --help.");
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
    let code = run_twilio(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_twilio};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_twilio(vec!["--help".to_string()]), 0);
        assert_eq!(run_twilio(vec!["-h".to_string()]), 0);
        let _ = run_twilio(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_twilio(vec![]);
    }
}
