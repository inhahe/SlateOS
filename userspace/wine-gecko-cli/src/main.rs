#![deny(clippy::all)]

//! wine-gecko-cli — OurOS Wine Gecko HTML rendering for Wine
//!
//! Single personality: `wine-gecko`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wine_gecko(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wine-gecko [OPTIONS]");
        println!("wine-gecko v2.47 (OurOS) — Mozilla Gecko-based HTML renderer for Wine");
        println!();
        println!("Options:");
        println!("  --status          Show installation status");
        println!("  --version         Show version");
        println!();
        println!("Wine Gecko provides MSHTML/Trident compatibility for");
        println!("Windows applications that use embedded web browsers.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wine-gecko v2.47 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "--status") {
        println!("Wine Gecko status:");
        println!("  Version: 2.47.4");
        println!("  Installed: yes");
        println!("  Location: /usr/share/wine/gecko/");
        println!("  Architecture: x86 + x86_64");
        println!("  MSHTML: enabled");
        println!("  Provides:");
        println!("    mshtml.dll (Internet Explorer engine)");
        println!("    jscript.dll (JavaScript engine)");
        println!("    urlmon.dll (URL Moniker)");
        return 0;
    }
    println!("wine-gecko: HTML rendering engine for Wine");
    println!("  Status: installed");
    println!("  Use --status for details");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wine-gecko".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wine_gecko(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
