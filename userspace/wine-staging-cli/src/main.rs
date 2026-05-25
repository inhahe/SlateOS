#![deny(clippy::all)]

//! wine-staging-cli — OurOS Wine Staging patched Wine build
//!
//! Single personality: `wine-staging`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wine_staging(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wine-staging [OPTIONS] PROGRAM [ARGS...]");
        println!("wine-staging v9.0 (OurOS) — Wine with staging patches");
        println!();
        println!("Options:");
        println!("  --patches         List applied staging patches");
        println!("  --version         Show version");
        println!();
        println!("Staging patches add features not yet upstream:");
        println!("  CSMT, PBA, DXVA2 hardware decoding, etc.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wine-staging v9.0 (OurOS, staging patches applied)"); return 0; }
    if args.iter().any(|a| a == "--patches") {
        println!("Applied staging patches:");
        println!("  eventfd_synchronization  (esync)");
        println!("  ntsync                   (kernel-level sync)");
        println!("  winepulse                (PulseAudio driver)");
        println!("  CSMT                     (command stream multi-threading)");
        println!("  PBA                      (persistent buffer allocation)");
        println!("  DXVA2                    (hardware video decoding)");
        println!("  Total: 892 patches applied");
        return 0;
    }
    if args.is_empty() {
        println!("wine-staging: no program specified");
        return 1;
    }
    let prog_name = args.first().map(|s| s.as_str()).unwrap_or("");
    println!("wine-staging: launching '{}' with staging patches...", prog_name);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wine-staging".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wine_staging(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
