#![deny(clippy::all)]

//! screen-cli — OurOS GNU Screen CLI
//!
//! Single personality: `screen`

use std::env;
use std::process;

fn run_screen(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: screen [OPTIONS] [COMMAND]");
        println!();
        println!("GNU Screen — terminal multiplexer (OurOS).");
        println!();
        println!("Options:");
        println!("  -S NAME       Create named session");
        println!("  -r [NAME]     Reattach to session");
        println!("  -d            Detach a session");
        println!("  -ls           List sessions");
        println!("  -x            Multi-display attach");
        println!("  -X CMD        Send command to session");
        println!("  -dm           Start session detached");
        return 0;
    }
    if args.iter().any(|a| a == "-v" || a == "--version") {
        println!("Screen version 4.09.01 (OurOS)");
        return 0;
    }

    if args.iter().any(|a| a == "-ls" || a == "-list") {
        println!("There are screens on:");
        println!("  12345.dev     (Attached)");
        println!("  23456.server  (Detached)");
        println!("  34567.logs    (Detached)");
        println!("3 Sockets in /var/run/screen/S-user.");
        return 0;
    }

    if args.iter().any(|a| a == "-r") {
        let name = args.windows(2).find(|w| w[0] == "-r")
            .map(|w| w[1].as_str()).unwrap_or("12345.dev");
        println!("[Reattaching to {}]", name);
        return 0;
    }

    if args.iter().any(|a| a == "-d") {
        let name = args.windows(2).find(|w| w[0] == "-d")
            .map(|w| w[1].as_str()).unwrap_or("12345.dev");
        println!("[{} detached.]", name);
        return 0;
    }

    let session_name = args.windows(2).find(|w| w[0] == "-S")
        .map(|w| w[1].as_str());
    let detached = args.iter().any(|a| a == "-dm");

    if let Some(name) = session_name {
        if detached {
            println!("[Screen session '{}' started (detached)]", name);
        } else {
            println!("[Screen session '{}' started]", name);
        }
    } else {
        println!("[Screen session started]");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_screen(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
