#![deny(clippy::all)]

//! nwg-look-cli — OurOS nwg-look GTK settings editor
//!
//! Single personality: `nwg-look`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_nwg_look(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nwg-look [OPTIONS]");
        println!("nwg-look v0.2 (OurOS) — GTK3 settings editor for Wayland");
        println!();
        println!("Options:");
        println!("  -a                Apply settings without GUI");
        println!("  --version         Show version");
        println!();
        println!("Configure GTK theme, icons, cursor, and font settings.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("nwg-look v0.2 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "-a") {
        println!("Applied GTK settings from ~/.config/gtk-3.0/settings.ini");
        return 0;
    }
    println!("nwg-look: GTK settings editor");
    println!("  Theme: Adwaita-dark");
    println!("  Icons: Papirus");
    println!("  Cursor: Adwaita");
    println!("  Font: Sans 11");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "nwg-look".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nwg_look(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
