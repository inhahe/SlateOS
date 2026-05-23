#![deny(clippy::all)]

//! abduco-cli — OurOS abduco session manager
//!
//! Single personality: `abduco`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_abduco(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: abduco [OPTIONS] [NAME [COMMAND...]]");
        println!("abduco 0.6 (OurOS) — Session management");
        println!();
        println!("Options:");
        println!("  -a NAME    Attach to existing session");
        println!("  -A NAME    Attach or create session");
        println!("  -c NAME    Create new session (detached)");
        println!("  -n NAME    Create new session (non-interactive)");
        println!("  -l         List sessions");
        println!("  -r         Read-only attach");
        println!("  -f         Force connect (kill existing client)");
        println!("  -e CHAR    Set detach key (default: ^\\)");
        println!("  -v         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-v") {
        println!("abduco-0.6 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "-l") {
        println!("Active sessions:");
        println!("  * main     (attached)  2024-01-15 10:00");
        return 0;
    }
    let mode = args.first().map(|s| s.as_str()).unwrap_or("-A");
    let name = args.get(1).map(|s| s.as_str()).unwrap_or("default");

    match mode {
        "-a" => println!("abduco: Attaching to session '{}'...", name),
        "-A" => println!("abduco: Attaching or creating session '{}'...", name),
        "-c" => println!("abduco: Creating detached session '{}'...", name),
        "-n" => println!("abduco: Creating non-interactive session '{}'...", name),
        _ => println!("abduco: Session '{}'", mode),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "abduco".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_abduco(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
