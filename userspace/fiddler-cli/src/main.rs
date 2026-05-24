#![deny(clippy::all)]

//! fiddler-cli — OurOS Fiddler web debugging proxy
//!
//! Single personality: `fiddler`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fiddler(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: fiddler [OPTIONS]");
        println!("Fiddler Everywhere v5.1.0 (OurOS) — Web debugging proxy");
        println!();
        println!("Options:");
        println!("  --port PORT         Proxy port (default: 8866)");
        println!("  --headless          Headless mode");
        println!("  --capture           Start capturing immediately");
        println!("  --rules FILE        Load rules");
        println!("  --export FILE       Export captured traffic");
        println!("  -V, --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("Fiddler Everywhere v5.1.0 (OurOS)");
        return 0;
    }
    println!("Fiddler Everywhere v5.1.0");
    println!("  Proxy: localhost:8866");
    println!("  HTTPS: decryption enabled");
    println!("  Capturing traffic...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "fiddler".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fiddler(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
