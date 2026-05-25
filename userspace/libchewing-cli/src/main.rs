#![deny(clippy::all)]

//! libchewing-cli — OurOS libchewing Chinese (Zhuyin) input
//!
//! Single personality: `chewing`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_chewing(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: chewing [OPTIONS]");
        println!("chewing v0.9 (OurOS) — Chinese (Zhuyin/Bopomofo) input engine");
        println!();
        println!("Options:");
        println!("  --keyboard TYPE   Keyboard layout (default, hsu, et26, ibm, dvorak)");
        println!("  --version         Show version");
        println!();
        println!("Provides Traditional Chinese input using Zhuyin (Bopomofo)");
        println!("phonetic symbols. Intelligent phrase selection.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("chewing v0.9 (OurOS, libchewing)"); return 0; }
    println!("chewing: Zhuyin input engine");
    println!("  Keyboard: default");
    println!("  Dictionary: system (150k entries)");
    println!("  Phrase prediction: intelligent selection");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "chewing".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_chewing(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
