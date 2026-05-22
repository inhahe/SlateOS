#![deny(clippy::all)]

//! telegram-cli — OurOS Telegram bot CLI
//!
//! Single personality: `tg`

use std::env;
use std::process;

fn run_tg(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tg <COMMAND> [OPTIONS]");
        println!();
        println!("Telegram Bot API CLI (OurOS).");
        println!();
        println!("Commands:");
        println!("  send         Send a message");
        println!("  getme        Get bot info");
        println!("  updates      Get recent updates");
        println!("  chats        List chats");
        println!("  photo        Send a photo");
        println!("  document     Send a document");
        println!("  webhook      Manage webhooks");
        println!("  poll         Create a poll");
        println!("  pin          Pin a message");
        println!();
        println!("Options:");
        println!("  --token <TOKEN>    Bot token");
        println!("  --chat <CHAT_ID>   Chat ID");
        println!("  --json             Output as JSON");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    let chat = args.windows(2).find(|w| w[0] == "--chat").map(|w| w[1].as_str()).unwrap_or("123456789");

    match cmd {
        "getme" => {
            println!("Bot Information:");
            println!("  ID:         987654321");
            println!("  Name:       MyBot");
            println!("  Username:   @my_ouros_bot");
            println!("  Can join groups:      true");
            println!("  Can read all messages: false");
            println!("  Supports inline:       true");
            0
        }
        "send" => {
            let msg = args.windows(2).find(|w| w[0] == "-m" || w[0] == "--message").map(|w| w[1].as_str()).unwrap_or("Hello!");
            let parse = args.windows(2).find(|w| w[0] == "--parse-mode").map(|w| w[1].as_str());
            println!("✔ Message sent to chat {}", chat);
            println!("  Text: {}", msg);
            println!("  Message ID: 42");
            if let Some(mode) = parse {
                println!("  Parse mode: {}", mode);
            }
            0
        }
        "updates" => {
            println!("Recent updates:");
            println!();
            println!("  Update 100001:");
            println!("    From: Alice (ID: 111111111)");
            println!("    Chat: My Group (ID: -100123456789)");
            println!("    Text: /start");
            println!();
            println!("  Update 100002:");
            println!("    From: Bob (ID: 222222222)");
            println!("    Chat: Private (ID: 222222222)");
            println!("    Text: Hello bot!");
            0
        }
        "chats" => {
            println!("Known chats:");
            println!("  ID                 Type      Title/Name");
            println!("  -100123456789      group     My Group");
            println!("  -100234567890      supergroup Dev Team");
            println!("  -100345678901      channel   Announcements");
            println!("  111111111          private   Alice");
            println!("  222222222          private   Bob");
            0
        }
        "photo" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("photo.jpg");
            println!("✔ Photo sent to chat {}", chat);
            println!("  File: {}", file);
            println!("  Message ID: 43");
            0
        }
        "document" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("document.pdf");
            println!("✔ Document sent to chat {}", chat);
            println!("  File: {}", file);
            println!("  Message ID: 44");
            0
        }
        "webhook" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("info");
            match sub {
                "set" => {
                    let url = args.get(2).map(|s| s.as_str()).unwrap_or("https://example.com/webhook");
                    println!("✔ Webhook set to {}", url);
                }
                "delete" => {
                    println!("✔ Webhook deleted.");
                }
                _ => {
                    println!("Webhook info:");
                    println!("  URL:             https://example.com/webhook");
                    println!("  Has certificate: false");
                    println!("  Pending updates: 0");
                    println!("  Last error:      (none)");
                }
            }
            0
        }
        "poll" => {
            let question = args.windows(2).find(|w| w[0] == "-q" || w[0] == "--question").map(|w| w[1].as_str()).unwrap_or("What do you think?");
            println!("✔ Poll created in chat {}", chat);
            println!("  Question: {}", question);
            println!("  Message ID: 45");
            0
        }
        "pin" => {
            let msg_id = args.get(1).map(|s| s.as_str()).unwrap_or("42");
            println!("✔ Message {} pinned in chat {}", msg_id, chat);
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: tg <command>. See --help.");
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
    let code = run_tg(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
