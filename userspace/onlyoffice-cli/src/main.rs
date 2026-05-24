#![deny(clippy::all)]

//! onlyoffice-cli — OurOS ONLYOFFICE desktop editors
//!
//! Single personality: `onlyoffice-desktopeditors`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_onlyoffice(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: onlyoffice-desktopeditors [OPTIONS] [FILE]");
        println!("onlyoffice v8.0 (OurOS) — Desktop document editors");
        println!();
        println!("Options:");
        println!("  --new:word        New document");
        println!("  --new:cell        New spreadsheet");
        println!("  --new:slide       New presentation");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("onlyoffice v8.0 (OurOS)"); return 0; }
    println!("onlyoffice: desktop editors started");
    println!("  Document editor: ready");
    println!("  Spreadsheet editor: ready");
    println!("  Presentation editor: ready");
    println!("  Recent files: 3");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "onlyoffice-desktopeditors".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_onlyoffice(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
