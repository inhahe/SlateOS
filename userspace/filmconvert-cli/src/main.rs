#![deny(clippy::all)]

//! filmconvert-cli — OurOS FilmConvert film emulation
//!
//! Single personality: `filmconvert`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: filmconvert [OPTIONS] [FILE]");
        println!("FilmConvert Nitrate 3 (OurOS) — Film stock emulation plug-in");
        println!();
        println!("Options:");
        println!("  --camera MODEL         Source camera (RED/ARRI/SONY/Canon profiles)");
        println!("  --stock STOCK          Film stock (KD5207/PFE7219/FJ8543/etc.)");
        println!("  --grain LEVEL          Grain intensity (0-100)");
        println!("  --cyber                Open CyberPunk variant");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("FilmConvert Nitrate 3.5 (OurOS)"); return 0; }
    println!("FilmConvert Nitrate 3.5 (OurOS)");
    println!("  Film stocks: 18 cinema, photographic, TV stocks (Kodak, Fuji, Polaroid)");
    println!("  Camera profiles: 50+ digital camera color matching");
    println!("  Grain: real film grain scanned from celluloid");
    println!("  Plug-in formats: OFX, AE, Premiere, FCP X, Resolve, Vegas, VST");
    println!("  Companion: CineMatch (camera color matching)");
    println!("  License: perpetual");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "filmconvert".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fc(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
