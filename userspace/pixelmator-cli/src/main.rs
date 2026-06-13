#![deny(clippy::all)]

//! pixelmator-cli — Slate OS Pixelmator Pro (Apple-acquired) image editor
//!
//! Single personality: `pixelmator`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_px(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pixelmator [OPTIONS]");
        println!("Pixelmator Pro 3.6.6 (Slate OS) — Apple-acquired macOS image editor");
        println!();
        println!("Options:");
        println!("  --new                  New document");
        println!("  --photo-app            Pixelmator Photo (RAW-focused, iPad)");
        println!("  --ml-enhance           ML Enhance (Core ML photo correction)");
        println!("  --ml-super-resolution  Super Resolution (3x upscaling)");
        println!("  --vectormator          Vectormator (vector tools)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Pixelmator Pro 3.6.6 (Slate OS)"); return 0; }
    println!("Pixelmator Pro 3.6.6 (Slate OS)");
    println!("  Vendor: Pixelmator Team (Vilnius, Lithuania; founded 2007 by Saulius + Aidas Dailide)");
    println!("  Acquired by: Apple Inc. (Nov 2024, announced — pending regulatory approval)");
    println!("  Reason: Apple has interest in native pro creative apps on Mac (Final Cut/Logic/etc.)");
    println!("  Platforms: macOS only (Pro), Pixelmator Photo on iPad (separate app)");
    println!("  Engine: Cocoa + Metal + Core Image + Core ML — deeply integrated Apple frameworks");
    println!("  ML features: ML Enhance (auto correct), Super Resolution (3x upscale),");
    println!("              ML Match Colors, ML Denoise, ML Crop Suggestions, ML Background Removal");
    println!("  Features: layers, masks, brushes, retouch (heal/clone/repair), color adjustments,");
    println!("           vector tools, type, effects, RAW (camera RAW pipeline)");
    println!("  Format: .pxd (Pixelmator), PSD read/write, HEIF, AVIF, modern format support");
    println!("  Pricing pre-Apple: one-time $49.99 (App Store) — likely changes post-acquisition");
    println!("  Differentiator: Mac-native UX, ML features that 'just work', no subscription");
    println!("  Companion: Photomator (was Pixelmator Photo) — photo library + RAW edit");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pixelmator".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_px(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_px};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pixelmator"), "pixelmator");
        assert_eq!(basename(r"C:\bin\pixelmator.exe"), "pixelmator.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pixelmator.exe"), "pixelmator");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_px(&["--help".to_string()], "pixelmator"), 0);
        assert_eq!(run_px(&["-h".to_string()], "pixelmator"), 0);
        let _ = run_px(&["--version".to_string()], "pixelmator");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_px(&[], "pixelmator");
    }
}
