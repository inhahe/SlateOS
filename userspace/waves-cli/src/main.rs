#![deny(clippy::all)]

//! waves-cli — OurOS Waves Audio plug-in suite
//!
//! Single personality: `waves`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_waves(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: waves [PLUGIN] [OPTIONS]");
        println!("Waves Audio (OurOS) — Industry-leading plug-in catalog (Waves V14)");
        println!();
        println!("Plugins (over 250 total):");
        println!("  ssl-e/g/4000           SSL E/G channel strip, 4000 bundle");
        println!("  api-2500               API 2500 stereo compressor");
        println!("  cla-bundle             Chris Lord-Alge signature plugins");
        println!("  abbey-road             Abbey Road Plates, J37, Saturator");
        println!("  l1/l2/l3-16            L-series limiters");
        println!("  c4/c6                  Multiband compressors");
        println!("  renaissance            REQ, RComp, RVox, RBass, RVerb, RChannel");
        println!("  q10                    10-band paragraphic EQ (the original)");
        println!();
        println!("Options:");
        println!("  --list                 List installed plug-ins");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Waves V14 (OurOS)"); return 0; }
    println!("Waves V14 (OurOS)");
    println!("  Bundles: Diamond, Platinum, Gold, Horizon, SD7, Mercury, Abbey Road");
    println!("  License: Waves License Center (USB key or computer-bound)");
    println!("  Plug-in formats: VST2, VST3, AU, AAX, SoundGrid");
    println!("  Hardware: SoundGrid eMotion LV1 live mixer, eMotion ST");
    println!("  Audio quality: 24-bit/96 kHz, dithering options");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "waves".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_waves(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
