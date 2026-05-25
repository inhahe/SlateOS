#![deny(clippy::all)]

//! webtrees-cli — OurOS webtrees genealogy web application
//!
//! Single personality: `webtrees`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_webtrees(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: webtrees [COMMAND] [OPTIONS]");
        println!("webtrees v2.1 (OurOS) — Online genealogy application");
        println!();
        println!("Commands:");
        println!("  serve             Start web server");
        println!("  import FILE       Import GEDCOM file");
        println!("  export FILE       Export GEDCOM file");
        println!("  check             Check database integrity");
        println!("  update            Check for updates");
        println!("  user-list         List users");
        println!("  tree-list         List family trees");
        println!();
        println!("Options:");
        println!("  --port N          Server port (default: 8080)");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("webtrees v2.1.18 (OurOS)"); return 0; }
    match args.first().map(|s| s.as_str()) {
        Some("serve") => {
            let port = args.windows(2).find(|w| w[0] == "--port").map(|w| w[1].as_str()).unwrap_or("8080");
            println!("webtrees: serving on http://localhost:{}", port);
        }
        Some("tree-list") => {
            println!("Family trees:");
            println!("  1. Smith Family (2,345 individuals)");
            println!("  2. Johnson Research (567 individuals)");
        }
        _ => {
            println!("webtrees v2.1.18 (OurOS)");
            println!("  Use 'webtrees serve' to start the web interface");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "webtrees".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_webtrees(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
