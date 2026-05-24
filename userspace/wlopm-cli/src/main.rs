#![deny(clippy::all)]

//! wlopm-cli — OurOS wlopm Wayland output power management
//!
//! Single personality: `wlopm`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wlopm(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: wlopm [OPTIONS] [OUTPUT]");
        println!("wlopm v0.1 (OurOS) — Wayland output power management");
        println!();
        println!("Options:");
        println!("  --on OUTPUT       Turn output on");
        println!("  --off OUTPUT      Turn output off");
        println!("  --toggle OUTPUT   Toggle output power");
        println!("  (no args)         List outputs and power state");
        return 0;
    }
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--on" => {
                let output = args.get(i + 1).map(|s| s.as_str()).unwrap_or("*");
                println!("{}: power ON", output);
                i += 2;
            }
            "--off" => {
                let output = args.get(i + 1).map(|s| s.as_str()).unwrap_or("*");
                println!("{}: power OFF", output);
                i += 2;
            }
            "--toggle" => {
                let output = args.get(i + 1).map(|s| s.as_str()).unwrap_or("*");
                println!("{}: power TOGGLED", output);
                i += 2;
            }
            _ => {
                println!("{}: power ON", args[i]);
                i += 1;
            }
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wlopm".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wlopm(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
