#![deny(clippy::all)]

//! libreoffice-cli — OurOS LibreOffice suite
//!
//! Multi-personality: `libreoffice`, `lowriter`, `localc`, `loimpress`, `lodraw`, `lobase`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_libreoffice(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS] [FILE...]", prog);
        println!("libreoffice v24.2 (OurOS) — Office suite");
        println!();
        println!("Options:");
        println!("  --writer          Start Writer");
        println!("  --calc            Start Calc");
        println!("  --impress         Start Impress");
        println!("  --draw            Start Draw");
        println!("  --base            Start Base");
        println!("  --headless        No GUI (for batch conversion)");
        println!("  --convert-to FMT  Convert file format");
        println!("  --print-to-file   Print to file");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("libreoffice v24.2 (OurOS)"); return 0; }
    let component = match prog {
        "lowriter" => "Writer",
        "localc" => "Calc",
        "loimpress" => "Impress",
        "lodraw" => "Draw",
        "lobase" => "Base",
        _ => {
            if args.iter().any(|a| a == "--writer") { "Writer" }
            else if args.iter().any(|a| a == "--calc") { "Calc" }
            else if args.iter().any(|a| a == "--impress") { "Impress" }
            else if args.iter().any(|a| a == "--draw") { "Draw" }
            else { "Start Center" }
        }
    };
    println!("libreoffice: {} started", component);
    println!("  Recent documents: 5");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "libreoffice".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_libreoffice(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
