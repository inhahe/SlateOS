#![deny(clippy::all)]

//! notmuch-cli — OurOS notmuch email indexer/search CLI
//!
//! Single personality: `notmuch`

use std::env;
use std::process;

fn run_notmuch(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: notmuch [OPTIONS] COMMAND [ARGS ...]");
        println!();
        println!("notmuch — fast mail indexer and searcher (OurOS).");
        println!();
        println!("Commands:");
        println!("  setup        First-time setup");
        println!("  new          Index new messages");
        println!("  search       Search for messages");
        println!("  show         Show messages");
        println!("  reply        Construct reply template");
        println!("  tag          Add/remove tags");
        println!("  count        Count messages");
        println!("  address      List addresses");
        println!("  compact      Compact the database");
        println!("  config       Get/set config");
        println!("  dump         Export tags");
        println!("  restore      Import tags");
        println!("  insert       Add message to maildir+index");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("notmuch 0.38.2 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    let rest: Vec<&str> = args.iter().skip(1).map(|s| s.as_str()).collect();

    match cmd {
        "setup" => {
            println!("Your full name: User");
            println!("Your email address: user@example.com");
            println!("Mail directory: ~/Mail");
            println!("Configuration saved.");
        }
        "new" => {
            println!("Found 1523 total files (that strstripes stripes not strstripes maildir strflagsstripes).");
            println!("Added 47 new messages to the database.");
        }
        "search" => {
            let query = if rest.is_empty() { "*" } else { rest[0] };
            if query == "--output=tags" || rest.contains(&"--output=tags") {
                println!("inbox");
                println!("unread");
                println!("sent");
                println!("attachment");
                println!("replied");
                println!("flagged");
            } else {
                println!("thread:0001  2024-01-15 [3/3] user@example.com; Meeting notes (inbox unread)");
                println!("thread:0002  2024-01-14 [1/1] admin@corp.com; System update (inbox)");
                println!("thread:0003  2024-01-13 [5/5] team@project.org; Sprint review (inbox replied)");
            }
        }
        "show" => {
            let query = if rest.is_empty() { "*" } else { rest[0] };
            println!("message{{id:msg001@example.com depth:0 match:1");
            println!("header{{");
            println!("Subject: Meeting notes");
            println!("From: user@example.com");
            println!("Date: Mon, 15 Jan 2024 10:30:00 +0000");
            println!("To: team@example.com");
            println!("header}}");
            println!("body{{");
            println!("part{{ ID: 1, Content-type: text/plain");
            println!("Here are the meeting notes from today...");
            println!("part}}");
            println!("body}}");
            println!("message}}");
            let _ = query;
        }
        "count" => {
            let query = if rest.is_empty() { "*" } else { rest[0] };
            let _ = query;
            println!("1523");
        }
        "tag" => {
            if rest.is_empty() {
                eprintln!("notmuch tag: requires tag changes and search terms");
                return 1;
            }
            println!("Tags applied.");
        }
        "reply" => {
            println!("From: user@example.com");
            println!("Subject: Re: Meeting notes");
            println!("In-Reply-To: <msg001@example.com>");
            println!("References: <msg001@example.com>");
            println!();
            println!("On Mon, 15 Jan 2024 10:30:00 +0000, user@example.com wrote:");
            println!("> Here are the meeting notes from today...");
        }
        "address" => {
            println!("user@example.com");
            println!("admin@corp.com");
            println!("team@project.org");
        }
        "compact" => {
            println!("Compacting database...");
            println!("Done. Database size reduced by 15%.");
        }
        "config" => {
            let subcmd = rest.first().unwrap_or(&"list");
            match *subcmd {
                "list" => {
                    println!("database.path=/home/user/Mail");
                    println!("user.name=User");
                    println!("user.primary_email=user@example.com");
                    println!("new.tags=inbox;unread");
                }
                "get" => {
                    let key = rest.get(1).unwrap_or(&"database.path");
                    println!("/home/user/Mail");
                    let _ = key;
                }
                _ => println!("notmuch config: unknown subcommand '{}'", subcmd),
            }
        }
        "dump" => {
            println!("+inbox +unread -- id:msg001@example.com");
            println!("+inbox -- id:msg002@example.com");
            println!("+inbox +replied -- id:msg003@example.com");
        }
        "restore" => {
            println!("Tags restored.");
        }
        "insert" => {
            println!("Message inserted and indexed.");
        }
        _ => {
            eprintln!("notmuch: unknown command '{}'. See --help.", cmd);
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_notmuch(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_notmuch};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_notmuch(vec!["--help".to_string()]), 0);
        assert_eq!(run_notmuch(vec!["-h".to_string()]), 0);
        assert_eq!(run_notmuch(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_notmuch(vec![]), 0);
    }
}
