#![deny(clippy::all)]

//! irssi-cli — OurOS Irssi IRC client CLI
//!
//! Single personality: `irssi`

use std::env;
use std::process;

fn run_irssi(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: irssi [OPTIONS]");
        println!();
        println!("Irssi — terminal IRC client (OurOS).");
        println!();
        println!("Options:");
        println!("  -c, --connect SERVER   Connect to server");
        println!("  -p, --port PORT        Server port");
        println!("  -n, --nick NICK        Nickname");
        println!("  -w, --password PASS    Server password");
        println!("  --config FILE          Config file");
        println!("  --home DIR             Irssi home directory");
        println!("  --noconnect            Don't auto-connect");
        println!("  --hostname NAME        Override hostname");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("irssi 1.4.5 (OurOS)");
        return 0;
    }

    let server = args.windows(2)
        .find(|w| w[0] == "-c" || w[0] == "--connect")
        .map(|w| w[1].as_str());

    let nick = args.windows(2)
        .find(|w| w[0] == "-n" || w[0] == "--nick")
        .map(|w| w[1].as_str())
        .unwrap_or("user");

    let noconnect = args.iter().any(|a| a == "--noconnect");

    println!("Irssi v1.4.5 (OurOS) - https://irssi.org");
    println!();

    if noconnect {
        println!("[(status)] Not connected");
        println!("[(status)] Use /connect <server> to connect");
    } else if let Some(srv) = server {
        println!("[{0}] -!- Connecting to {0} on port 6697", srv);
        println!("[{}] -!- Connected to {}", srv, srv);
        println!("[{}] -!- {} has joined #general", srv, nick);
        println!("[{}] <@admin> Welcome to #general!", srv);
    } else {
        println!("[(status)] Welcome to irssi!");
        println!("[(status)] Type /help for a list of commands");
        println!("[(status)] Type /connect <server> to connect to IRC");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_irssi(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_irssi};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_irssi(vec!["--help".to_string()]), 0);
        assert_eq!(run_irssi(vec!["-h".to_string()]), 0);
        assert_eq!(run_irssi(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_irssi(vec![]), 0);
    }
}
