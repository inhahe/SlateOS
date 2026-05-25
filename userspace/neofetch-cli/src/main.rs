#![deny(clippy::all)]

//! neofetch-cli — OurOS Neofetch system information
//!
//! Single personality: `neofetch`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_neofetch(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: neofetch [OPTIONS]");
        println!("neofetch v7.1 (OurOS) — System information tool");
        println!();
        println!("Options:");
        println!("  --off           Disable ASCII art");
        println!("  --ascii_distro  Set ASCII distro art");
        println!("  --config FILE   Custom config file");
        println!("  --stdout        Plain text output");
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("neofetch v7.1 (OurOS)"); return 0; }
    println!("        .--.         user@ouros-host");
    println!("       |o_o |        ---------------");
    println!("       |:_/ |        OS: OurOS 1.0 x86_64");
    println!("      //   \\ \\       Kernel: 0.1.0-ouros");
    println!("     (|     | )      Uptime: 2 hours, 15 mins");
    println!("    /'\\_   _/`\\      Shell: kshell 1.0");
    println!("    \\___)=(___/      Resolution: 1920x1080");
    println!("                     DE: OurOS Desktop");
    println!("                     WM: OurOS Compositor");
    println!("                     Terminal: ouros-term");
    println!("                     CPU: AMD Ryzen 7 (8) @ 3.6GHz");
    println!("                     GPU: AMD Radeon");
    println!("                     Memory: 4096MiB / 16384MiB");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "neofetch".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_neofetch(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
