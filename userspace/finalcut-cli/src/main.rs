#![deny(clippy::all)]

//! finalcut-cli — OurOS Final Cut Pro (Apple pro video editor)
//!
//! Single personality: `finalcut`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fcp(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: finalcut [OPTIONS]");
        println!("Final Cut Pro 11 (OurOS) — Apple pro non-linear video editor");
        println!();
        println!("Options:");
        println!("  --new                  New library");
        println!("  --magnetic-timeline    Magnetic Timeline (signature non-track model)");
        println!("  --multicam             Multicam editing (up to 64 angles)");
        println!("  --proxy                Generate proxy media");
        println!("  --compressor           Open Compressor (encode/transcode)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Final Cut Pro 11.0.1 (OurOS)"); return 0; }
    println!("Final Cut Pro 11.0.1 (OurOS)");
    println!("  Vendor: Apple Inc. (Cupertino, CA)");
    println!("  History: Final Cut Pro 1.0 (1999) → Final Cut Pro 7 (Studio era) →");
    println!("           controversial Final Cut Pro X (2011) rewrite → matured into FCP 11");
    println!("  Platform: macOS 14.6+ only (Apple Silicon optimized, MetalFX rendering)");
    println!("  Companions: Motion (motion graphics, $49.99), Compressor ($49.99)");
    println!("  Pricing: $299.99 one-time (no subscription) — also Final Cut Pro for iPad ($4.99/mo)");
    println!("  Engine: 64-bit, Metal-accelerated, ProRes/ProRes RAW native, color-managed");
    println!("  Signature: Magnetic Timeline (no track conflicts, clips auto-rearrange),");
    println!("            Roles (audio/video role tags drive layout), Auditions (compare takes)");
    println!("  Multicam: up to 64 angles, automatic angle sync via audio/timecode");
    println!("  AI (FCP 11): Enhance Light & Color (ML auto-grade), Smooth Slo-Mo (optical flow),");
    println!("              Magnetic Mask (subject isolation), Voice Isolation");
    println!("  Formats: ProRes (all flavors), H.264/265, HEVC, MXF, DNxHD/HR via plugin");
    println!("  Export: Apple Devices, ProRes master, IMF, exchange via XML 1.11");
    println!("  Use cases: indie filmmakers, documentaries, YouTube creators, broadcast");
    println!("  Competitor: Adobe Premiere Pro, DaVinci Resolve, Avid Media Composer");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "finalcut".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fcp(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fcp};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/finalcut"), "finalcut");
        assert_eq!(basename(r"C:\bin\finalcut.exe"), "finalcut.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("finalcut.exe"), "finalcut");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_fcp(&["--help".to_string()], "finalcut"), 0);
        assert_eq!(run_fcp(&["-h".to_string()], "finalcut"), 0);
        let _ = run_fcp(&["--version".to_string()], "finalcut");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_fcp(&[], "finalcut");
    }
}
