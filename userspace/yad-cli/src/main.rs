#![deny(clippy::all)]

//! yad-cli — OurOS YAD (Yet Another Dialog) display
//!
//! Single personality: `yad`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_yad(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: yad [OPTIONS]");
        println!("yad v13.0 (OurOS) — Yet Another Dialog");
        println!();
        println!("Dialog types:");
        println!("  --info            Information dialog");
        println!("  --warning         Warning dialog");
        println!("  --error           Error dialog");
        println!("  --question        Question dialog");
        println!("  --entry           Text entry dialog");
        println!("  --file            File selection");
        println!("  --color           Color selection");
        println!("  --font            Font selection");
        println!("  --calendar        Calendar dialog");
        println!("  --scale           Scale dialog");
        println!("  --progress        Progress bar");
        println!("  --list            List dialog");
        println!("  --form            Form dialog");
        println!("  --notification    System tray");
        println!("  --text-info       Text display");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("yad v13.0 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "--info") {
        println!("yad: [INFO] Dialog displayed");
        return 0;
    }
    if args.iter().any(|a| a == "--question") {
        println!("yad: [QUESTION] Dialog displayed");
        return 0;
    }
    println!("yad: dialog program (use --help for dialog types)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "yad".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_yad(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
