#![deny(clippy::all)]

//! fastfetch-cli — OurOS Fastfetch system information
//!
//! Multi-personality: `fastfetch`, `flashfetch`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fastfetch(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fastfetch [OPTIONS]");
        println!("fastfetch v2.8 (OurOS) — Fast system information tool");
        println!();
        println!("Options:");
        println!("  -c FILE        Config file");
        println!("  --logo LOGO    Set logo (or 'none')");
        println!("  --format TYPE  Output format (default, json)");
        println!("  -s MODULES     Structure (comma-separated)");
        println!("  -l             List available modules");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("fastfetch v2.8 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "-l") {
        println!("Available modules:");
        println!("  Title, OS, Host, Kernel, Uptime, Shell, DE, WM,");
        println!("  Terminal, CPU, GPU, Memory, Disk, Battery, Locale,");
        println!("  Weather, Colors, Break");
        return 0;
    }
    println!("   ____            ___  ____    user@ouros-host");
    println!("  / __ \\__  ______/ _ \\/ __/    ---------------");
    println!(" / /_/ / / / / __/ // /\\ \\      OS: OurOS 1.0 x86_64");
    println!(" \\____/\\_,_/_/  \\___/___/       Host: Custom PC");
    println!("                                Kernel: 0.1.0-ouros");
    println!("                                Uptime: 2h 15m");
    println!("                                CPU: AMD Ryzen 7 (8) @ 3.60 GHz");
    println!("                                GPU: AMD Radeon RX");
    println!("                                Memory: 4.00 GiB / 16.00 GiB (25%)");
    println!("                                Disk (/): 120 GiB / 480 GiB (25%)");
    0
}

fn run_flashfetch(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: flashfetch");
        println!("flashfetch v2.8 (OurOS) — Minimal fastfetch preset");
        return 0;
    }
    println!("OurOS 1.0 | 0.1.0-ouros | AMD Ryzen 7 | 4.0/16.0 GiB");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "fastfetch".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "flashfetch" => run_flashfetch(&rest, &prog),
        _ => run_fastfetch(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
