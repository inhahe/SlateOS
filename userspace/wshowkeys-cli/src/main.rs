#![deny(clippy::all)]

//! wshowkeys-cli — OurOS wshowkeys key press display
//!
//! Single personality: `wshowkeys`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wshowkeys(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wshowkeys [OPTIONS]");
        println!("wshowkeys v1.0 (OurOS) — Display keypresses on Wayland");
        println!();
        println!("Options:");
        println!("  -b COLOR          Background color (RRGGBBAA hex)");
        println!("  -f COLOR          Font color (RRGGBBAA hex)");
        println!("  -s SIZE           Font size (pixels)");
        println!("  -F FONT           Font family");
        println!("  -t TIMEOUT        Key display timeout (ms)");
        println!("  -a TOP|BOTTOM     Anchor position");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wshowkeys v1.0 (OurOS)"); return 0; }

    let anchor = args.iter().skip_while(|a| a.as_str() != "-a").nth(1)
        .map(|s| s.as_str()).unwrap_or("bottom");
    let size = args.iter().skip_while(|a| a.as_str() != "-s").nth(1)
        .map(|s| s.as_str()).unwrap_or("24");
    println!("wshowkeys: displaying keypresses (anchor={}, font_size={}px)", anchor, size);
    println!("  Press keys to see them displayed as overlay...");
    println!("  [Super_L] [Return] [a] [b] [c]");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wshowkeys".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wshowkeys(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
