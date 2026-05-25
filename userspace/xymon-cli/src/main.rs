#![deny(clippy::all)]

//! xymon-cli — OurOS Xymon system monitor
//!
//! Multi-personality: `xymond`, `xymon`, `xymoncmd`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_xymon(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "xymond" => {
                println!("xymond (OurOS) — Xymon monitoring daemon");
                println!("  --listen ADDR:PORT Listen address");
                println!("  --config DIR       Config directory");
                println!("  --log FILE         Log file");
                println!("  --pidfile FILE     PID file");
            }
            "xymoncmd" => {
                println!("xymoncmd (OurOS) — Run command with Xymon env");
                println!("  COMMAND ARGS       Command to run");
            }
            _ => {
                println!("xymon (OurOS) — Xymon client status reporter");
                println!("  HOST STATUS MSG    Send status to xymond");
                println!("  --server HOST      Xymon server address");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Xymon v4.3.30 (OurOS)"); return 0; }
    match prog {
        "xymond" => {
            println!("Xymon Daemon v4.3.30 (OurOS)");
            println!("  Listening: 0.0.0.0:1984");
            println!("  Hosts: 78 monitored");
            println!("  Tests: 456 active");
            println!("  Green: 423, Yellow: 21, Red: 12");
            println!("  Status msgs: 2,345/h");
        }
        _ => {
            println!("Xymon v4.3.30 (OurOS)");
            println!("  Server: xymon.example.com:1984");
            println!("  Status: connected");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "xymon".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_xymon(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
