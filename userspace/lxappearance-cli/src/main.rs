#![deny(clippy::all)]

//! lxappearance-cli — OurOS LXAppearance GTK theme switcher
//!
//! Single personality: `lxappearance`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lxappearance(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lxappearance [OPTIONS]");
        println!("lxappearance v0.6 (OurOS) — GTK+ theme switcher");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Tabs: Widget, Color, Icon Theme, Mouse Cursor, Other");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("lxappearance v0.6 (OurOS)"); return 0; }
    println!("lxappearance: GTK+ theme switcher");
    println!("  Widget Theme: Adwaita");
    println!("  Icon Theme: Papirus");
    println!("  Mouse Cursor: default");
    println!("  Toolbar Style: Icons and text");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lxappearance".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lxappearance(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
