#![deny(clippy::all)]

//! trendmicro-cli — OurOS Trend Micro Maximum Security
//!
//! Single personality: `trendmicro`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tm(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: trendmicro [OPTIONS]");
        println!("Trend Micro Maximum Security 17.8 (OurOS) — Consumer + enterprise security");
        println!();
        println!("Options:");
        println!("  --scan TYPE            quick/full/custom");
        println!("  --pay-guard            Pay Guard secure browser");
        println!("  --vault                Vault encrypted folder");
        println!("  --vision-one           Trend Vision One XDR platform (enterprise)");
        println!("  --deep-security        Deep Security (server/cloud workload)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Trend Micro Maximum Security 17.8.1308 (OurOS)"); return 0; }
    println!("Trend Micro Maximum Security 17.8.1308 (OurOS)");
    println!("  Origin: Japan/US, founded 1988; Tokyo Stock Exchange listed");
    println!("  Consumer: AntiVirus+, Internet Security, Maximum Security, Premium Security Suite");
    println!("  Mobile: Trend Micro Mobile Security (Android/iOS)");
    println!("  Mac: Trend Micro Antivirus for Mac, ID Safe");
    println!("  Business: Trend Vision One (XDR), Apex One (endpoint), Deep Security");
    println!("  Cloud: Cloud One (workload, container, file storage, application, conformity)");
    println!("  Network: TippingPoint IPS, Deep Discovery (APT detection), Smart Protection Network");
    println!("  Engines: Smart Scan (cloud lookups), behavior monitoring, ML, sandbox analyzer");
    println!("  Features: AV, web threat protection, Pay Guard, parental, Vault, password mgr");
    println!("  License: annual subscription (consumer) + enterprise per-seat/per-VM");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "trendmicro".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tm(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
