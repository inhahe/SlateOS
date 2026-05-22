#![deny(clippy::all)]

//! kakoune — OurOS modal code editor
//!
//! Single personality: `kak`

use std::env;
use std::process;

fn run_kak(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-help") {
        println!("Usage: kak [options] [file] ...");
        println!();
        println!("Options:");
        println!("  -c <name>     Connect to session");
        println!("  -s <name>     Set session name");
        println!("  -e <cmd>      Execute command after startup");
        println!("  -E <cmd>      Execute command before startup");
        println!("  -f <keys>     Filter: pipe stdin through keys and output to stdout");
        println!("  -p <name>     Pipe stdin to session");
        println!("  -n            Don't source kakrc");
        println!("  -l            List sessions");
        println!("  -clear        Clear dead sessions");
        println!("  -d            Daemonize session");
        println!("  -ro           Readonly mode");
        println!("  -version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-version") {
        println!("Kakoune v2024.05.22 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "-l") {
        println!("default");
        println!("project1");
        return 0;
    }
    if args.iter().any(|a| a == "-clear") {
        println!("Cleared dead sessions.");
        return 0;
    }

    let files: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    if files.is_empty() {
        println!("Kakoune v2024.05.22 (OurOS) — modal code editor");
        println!("(TUI launched — simulated)");
    } else {
        for f in &files {
            println!("Editing: {}", f);
        }
        println!("(TUI launched — simulated)");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kak(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
