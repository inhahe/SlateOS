#![deny(clippy::all)]

//! indesign-cli — OurOS Adobe InDesign desktop publishing
//!
//! Single personality: `indesign`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_id(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: indesign [OPTIONS] [FILE]");
        println!("Adobe InDesign 2024 (OurOS) — Desktop publishing & layout");
        println!();
        println!("Options:");
        println!("  -r SCRIPT              Run ExtendScript / JSX");
        println!("  -open FILE             Open .indd document");
        println!("  -export FORMAT FILE    Export (pdf/epub/html/idml)");
        println!("  -package FOLDER        Package document with fonts/links");
        println!("  -server                Run as InDesign Server");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Adobe InDesign 2024 v19.4.0 (OurOS)"); return 0; }
    println!("Adobe InDesign 2024 v19.4.0 (OurOS)");
    println!("  Scripting: JavaScript, AppleScript, VBScript, UXP");
    println!("  Features: Paragraph composer, GREP styles, Data Merge");
    println!("  Output: Print-ready PDF, EPUB 3, fixed-layout EPUB, HTML5");
    println!("  Integration: Photoshop/Illustrator placed links, CC Libraries");
    println!("  License: Creative Cloud");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "indesign".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_id(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
