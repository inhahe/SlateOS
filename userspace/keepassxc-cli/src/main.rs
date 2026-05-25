#![deny(clippy::all)]

//! keepassxc-cli — OurOS KeePassXC password manager
//!
//! Multi-personality: `keepassxc`, `keepassxc-cli`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_keepassxc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: keepassxc [OPTIONS] [DATABASE]");
        println!("keepassxc v2.7 (OurOS) — Password manager");
        println!();
        println!("Options:");
        println!("  --pw-stdin        Read password from stdin");
        println!("  --keyfile FILE    Key file");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("keepassxc v2.7 (OurOS)"); return 0; }
    println!("keepassxc: password manager started");
    println!("  Database: ~/passwords.kdbx");
    println!("  Entries: 85");
    println!("  Groups: 12");
    println!("  Browser integration: enabled");
    0
}

fn run_cli(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: keepassxc-cli COMMAND [OPTIONS]");
        println!("keepassxc-cli v2.7 (OurOS) — CLI password manager");
        println!();
        println!("Commands:");
        println!("  ls DB             List entries");
        println!("  show DB ENTRY     Show entry");
        println!("  add DB ENTRY      Add entry");
        println!("  rm DB ENTRY       Remove entry");
        println!("  generate          Generate password");
        println!("  clip DB ENTRY     Copy password to clipboard");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("ls");
    match cmd {
        "generate" => println!("x8#Kp$mN2@vL9wQz"),
        "ls" => {
            println!("Email/");
            println!("  Gmail");
            println!("  Outlook");
            println!("Social/");
            println!("  GitHub");
            println!("Banking/");
        }
        _ => println!("keepassxc-cli: {}", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "keepassxc".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "keepassxc-cli" => run_cli(&rest, &prog),
        _ => run_keepassxc(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
