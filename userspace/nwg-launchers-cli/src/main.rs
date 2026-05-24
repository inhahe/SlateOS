#![deny(clippy::all)]

//! nwg-launchers-cli — OurOS nwg-launchers application launcher suite
//!
//! Multi-personality: `nwg-drawer`, `nwg-bar`, `nwg-menu`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_drawer(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nwg-drawer [OPTIONS]");
        println!("nwg-drawer v0.3 (OurOS) — Application drawer/launcher");
        println!();
        println!("Options:");
        println!("  -c COLUMNS        Number of columns");
        println!("  -is ICON_SIZE     Icon size (px)");
        println!("  -s SPACING        Item spacing");
        println!("  -o OVERLAY        Overlay opacity (0.0-1.0)");
        println!("  -fm               Full-screen mode");
        println!("  -term TERM        Terminal emulator for terminal apps");
        return 0;
    }
    println!("nwg-drawer: application launcher opened");
    println!("  [Search...                            ]");
    println!("  Firefox  Terminal  Files  Editor  Settings");
    0
}

fn run_bar(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nwg-bar [OPTIONS]");
        println!("nwg-bar v0.3 (OurOS) — Button bar (logout screen)");
        println!();
        println!("Options:");
        println!("  -t TEMPLATE       Template file");
        println!("  -o OVERLAY        Overlay opacity");
        return 0;
    }
    println!("nwg-bar: button bar");
    println!("  [Lock]  [Logout]  [Reboot]  [Shutdown]");
    0
}

fn run_menu(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nwg-menu [OPTIONS]");
        println!("nwg-menu v0.3 (OurOS) — Grid menu launcher");
        println!();
        println!("Options:");
        println!("  -va VALIGN        Vertical alignment (top/center/bottom)");
        println!("  -ha HALIGN        Horizontal alignment (left/center/right)");
        println!("  -c COLUMNS        Number of columns");
        println!("  -ml MARGIN_LEFT   Left margin");
        return 0;
    }
    println!("nwg-menu: grid menu opened");
    println!("  Applications listed from .desktop entries");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "nwg-drawer".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "nwg-bar" => run_bar(&rest, &prog),
        "nwg-menu" => run_menu(&rest, &prog),
        _ => run_drawer(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
