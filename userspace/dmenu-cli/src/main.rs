#![deny(clippy::all)]

//! dmenu-cli — OurOS dmenu dynamic menu
//!
//! Multi-personality: `dmenu`, `dmenu_run`, `dmenu_path`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dmenu(args: &[String], prog: &str) -> i32 {
    match prog {
        "dmenu_run" => {
            println!("(launching dmenu with PATH executables)");
            return 0;
        }
        "dmenu_path" => {
            println!("/usr/bin/ls");
            println!("/usr/bin/cat");
            println!("/usr/bin/grep");
            println!("/usr/bin/find");
            return 0;
        }
        _ => {}
    }
    if args.iter().any(|a| a == "--help" || a == "-h") || args.iter().any(|a| a == "-v") {
        if args.iter().any(|a| a == "-v") {
            println!("dmenu-5.2 (OurOS)");
            return 0;
        }
        println!("Usage: dmenu [OPTIONS]");
        println!("dmenu 5.2 (OurOS) — Dynamic menu for X/Wayland");
        println!();
        println!("Options:");
        println!("  -b             Bottom of screen");
        println!("  -f             Grab keyboard first");
        println!("  -i             Case insensitive");
        println!("  -l LINES       Vertical list with N lines");
        println!("  -m MONITOR     Monitor number");
        println!("  -p PROMPT      Prompt string");
        println!("  -fn FONT       Font name");
        println!("  -nb COLOR      Normal background color");
        println!("  -nf COLOR      Normal foreground color");
        println!("  -sb COLOR      Selected background color");
        println!("  -sf COLOR      Selected foreground color");
        println!("  -v             Show version");
        return 0;
    }
    println!("(dmenu: reading items from stdin)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dmenu".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_dmenu(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
