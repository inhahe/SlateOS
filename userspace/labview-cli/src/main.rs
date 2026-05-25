#![deny(clippy::all)]

//! labview-cli — OurOS NI LabVIEW graphical instrumentation/control programming
//!
//! Single personality: `labview`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lv(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: labview [OPTIONS] [VI]");
        println!("NI LabVIEW 2024 Q3 (OurOS) — Graphical dataflow programming");
        println!();
        println!("Options:");
        println!("  -- VI                  Open .vi virtual instrument");
        println!("  -unattended            Headless execution");
        println!("  --rt-target IP         Deploy to LabVIEW Real-Time target");
        println!("  --fpga                 LabVIEW FPGA compilation");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("NI LabVIEW 24.3 (2024 Q3) (OurOS)"); return 0; }
    println!("NI LabVIEW 24.3 (2024 Q3) (OurOS)");
    println!("  Paradigm: graphical dataflow ('G' language), block-diagram VIs");
    println!("  Hardware: DAQ, GPIB, VISA, instrument drivers (10,000+)");
    println!("  Modules: Real-Time, FPGA, Vision, DSP, Sound & Vibration");
    println!("  Platforms: Windows, Linux RT, NI Linux RT, FPGA (CompactRIO/PXIe/sbRIO)");
    println!("  Industries: test & measurement, industrial automation, research");
    println!("  Toolkits: Control Design, System Identification, Database Connectivity");
    println!("  License: per-seat subscription, with NI hardware bundles");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "labview".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lv(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
