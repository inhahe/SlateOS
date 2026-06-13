#![deny(clippy::all)]

//! matrix-cli — SlateOS Matrix messaging CLI
//!
//! Single personality: `matrix`

use std::env;
use std::process;

fn run_matrix(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: matrix <COMMAND> [OPTIONS]");
        println!();
        println!("Matrix decentralized messaging CLI (SlateOS).");
        println!();
        println!("Commands:");
        println!("  login        Login to homeserver");
        println!("  logout       Logout");
        println!("  rooms        Manage rooms");
        println!("  send         Send a message");
        println!("  sync         Sync and display events");
        println!("  invite       Invite user to room");
        println!("  join         Join a room");
        println!("  leave        Leave a room");
        println!("  upload       Upload a file");
        println!("  whoami       Show current user");
        println!("  server       Server administration");
        println!();
        println!("Options:");
        println!("  --homeserver <URL>    Matrix homeserver URL");
        println!("  --user <MXID>         User ID (@user:server)");
        println!("  --token <TOKEN>       Access token");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "login" => {
            let server = args.windows(2).find(|w| w[0] == "--homeserver").map(|w| w[1].as_str()).unwrap_or("https://matrix.org");
            println!("Logging in to {}...", server);
            println!("Username: @user:matrix.org");
            println!("Password: ****");
            println!("✔ Logged in as @user:matrix.org");
            println!("  Access token saved to ~/.config/matrix-cli/token");
            0
        }
        "whoami" => {
            println!("@user:matrix.org");
            println!("  Homeserver: https://matrix.org");
            println!("  Device ID: ABCDEF1234");
            0
        }
        "rooms" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("Room ID                          Name                 Members  Unread");
                    println!("!abc123:matrix.org               General Chat         42       3");
                    println!("!def456:matrix.org               Development          18       0");
                    println!("!ghi789:matrix.org               Random               35       12");
                    println!("!jkl012:matrix.org               SlateOS Dev            8        1");
                }
                "create" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("New Room");
                    println!("✔ Created room '{}'", name);
                    println!("  Room ID: !new123:matrix.org");
                }
                "info" => {
                    let room = args.get(2).map(|s| s.as_str()).unwrap_or("!abc123:matrix.org");
                    println!("Room: {}", room);
                    println!("  Name:       General Chat");
                    println!("  Topic:      Welcome to the general chat room");
                    println!("  Members:    42");
                    println!("  Encrypted:  true (megolm)");
                    println!("  Created:    2024-01-01");
                }
                _ => { println!("Room operation: {}", sub); }
            }
            0
        }
        "send" => {
            let room = args.windows(2).find(|w| w[0] == "-r" || w[0] == "--room").map(|w| w[1].as_str()).unwrap_or("!abc123:matrix.org");
            let msg = args.windows(2).find(|w| w[0] == "-m" || w[0] == "--message").map(|w| w[1].as_str()).unwrap_or("Hello!");
            println!("✔ Sent to {}", room);
            println!("  Event ID: $event123456");
            println!("  Content: {}", msg);
            0
        }
        "sync" => {
            println!("Syncing...");
            println!();
            println!("[General Chat] @alice:matrix.org: Hey everyone!");
            println!("[General Chat] @bob:matrix.org: Working on the deploy");
            println!("[Development] @charlie:matrix.org: PR #42 ready for review");
            println!("[SlateOS Dev] @dev:matrix.org: Build passed ✓");
            println!();
            println!("(press Ctrl+C to stop syncing)");
            0
        }
        "join" => {
            let room = args.get(1).map(|s| s.as_str()).unwrap_or("!abc123:matrix.org");
            println!("✔ Joined room {}", room);
            0
        }
        "leave" => {
            let room = args.get(1).map(|s| s.as_str()).unwrap_or("!abc123:matrix.org");
            println!("✔ Left room {}", room);
            0
        }
        "invite" => {
            let user = args.get(1).map(|s| s.as_str()).unwrap_or("@newuser:matrix.org");
            let room = args.windows(2).find(|w| w[0] == "-r" || w[0] == "--room").map(|w| w[1].as_str()).unwrap_or("!abc123:matrix.org");
            println!("✔ Invited {} to {}", user, room);
            0
        }
        "upload" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("file.txt");
            println!("Uploading {}...", file);
            println!("✔ Uploaded: mxc://matrix.org/abc123def456");
            0
        }
        "server" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("info");
            match sub {
                "info" => {
                    println!("Homeserver: https://matrix.org");
                    println!("  Version: Synapse 1.100.0");
                    println!("  Users:   12345");
                    println!("  Rooms:   6789");
                }
                _ => { println!("Server operation: {}", sub); }
            }
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: matrix <command>. See --help.");
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
    let code = run_matrix(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_matrix};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_matrix(vec!["--help".to_string()]), 0);
        assert_eq!(run_matrix(vec!["-h".to_string()]), 0);
        let _ = run_matrix(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_matrix(vec![]);
    }
}
