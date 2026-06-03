#![deny(clippy::all)]

//! captureone-cli — OurOS Capture One Pro professional RAW workflow
//!
//! Single personality: `captureone`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_c1(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: captureone [OPTIONS]");
        println!("Capture One Pro 16.5 (OurOS) — Pro RAW developer + tethering");
        println!();
        println!("Options:");
        println!("  --catalog PATH         Open catalog (.cocatalog)");
        println!("  --session PATH         Open session (folder-based workflow)");
        println!("  --tether               Tethered capture (USB/Ethernet from camera)");
        println!("  --styles               Styles (preset bundles)");
        println!("  --layers               Adjustment layers (local edits with brushes/masks)");
        println!("  --capture-pilot        Capture Pilot (iOS remote viewer for clients)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Capture One Pro 16.5.2.12 (OurOS)"); return 0; }
    println!("Capture One Pro 16.5.2.12 (OurOS)");
    println!("  Vendor: Capture One A/S (Copenhagen, Denmark; founded 2019 spinoff from Phase One)");
    println!("  Owner: Axcel (Danish PE), and minority Blackstone — separate from Phase One since 2021");
    println!("  Engine: in-house — widely regarded as best-in-class RAW conversion (esp. skin tones)");
    println!("  Heritage: built by Phase One for medium-format digital backs (P+, IQ series)");
    println!("  Camera support: 600+ cameras; deep integration with Phase One/Hasselblad medium format");
    println!("  Pricing: subscription ($24/mo or $179/yr) or one-time perpetual ($299) — both available");
    println!("  Editions: Pro (full), Pro Fujifilm/Nikon/Sony (brand-locked, cheaper), Express (free)");
    println!("  Catalog vs Session: catalogs (Lightroom-like DB), Sessions (folder-based, no DB)");
    println!("  Tethering: gold-standard — used by commercial/fashion/product shoots");
    println!("  Color editor: Advanced/Skin Tone — selective color masking by hue/sat/lum ranges");
    println!("  Layers: brushed local adjustments, gradient masks, luminosity masks, AI mask");
    println!("  Strengths: color science, tethering, raw rendering, no Adobe ecosystem lock-in");
    println!("  Market: pro studios, fashion, advertising, architecture, fine art photographers");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "captureone".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_c1(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_c1};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/captureone"), "captureone");
        assert_eq!(basename(r"C:\bin\captureone.exe"), "captureone.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("captureone.exe"), "captureone");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_c1(&["--help".to_string()], "captureone"), 0);
        assert_eq!(run_c1(&["-h".to_string()], "captureone"), 0);
        assert_eq!(run_c1(&["--version".to_string()], "captureone"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_c1(&[], "captureone"), 0);
    }
}
