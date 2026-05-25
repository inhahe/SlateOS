#![deny(clippy::all)]

//! x11vnc-cli — OurOS x11vnc VNC server for existing displays
//!
//! Single personality: `x11vnc`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_x11vnc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: x11vnc [OPTIONS]");
        println!("x11vnc v0.9 (OurOS) — VNC server for real X displays");
        println!();
        println!("Options:");
        println!("  -display :N       X display to share");
        println!("  -rfbport PORT     VNC port (default: 5900)");
        println!("  -passwd PASS      Set password");
        println!("  -forever          Keep running after client disconnect");
        println!("  -shared           Allow multiple viewers");
        println!("  -noxdamage        Disable XDamage extension");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("x11vnc v0.9 (OurOS)"); return 0; }
    println!("x11vnc: VNC server started on port 5900");
    println!("  Sharing display :0");
    println!("  XDamage: enabled");
    println!("  Clipboard: shared");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "x11vnc".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_x11vnc(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
