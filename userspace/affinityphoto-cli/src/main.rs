#![deny(clippy::all)]

//! affinityphoto-cli — OurOS Serif Affinity Photo 2 (Canva-owned)
//!
//! Single personality: `affinityphoto`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ap(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: affinityphoto [OPTIONS]");
        println!("Affinity Photo 2 (OurOS) — Pro raster photo editor (Photoshop alternative)");
        println!();
        println!("Options:");
        println!("  --persona TYPE         photo/liquify/develop/tone-mapping/export");
        println!("  --raw                  Develop persona (RAW processing)");
        println!("  --hdr                  HDR merge");
        println!("  --panorama             Panorama stitching");
        println!("  --focus-stack          Focus stacking");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Affinity Photo 2.5.7 (OurOS)"); return 0; }
    println!("Affinity Photo 2.5.7 (OurOS)");
    println!("  Vendor: Serif Europe Ltd (Nottingham, UK; founded 1987)");
    println!("  Acquired by: Canva (Mar 2024, undisclosed terms — significant for design industry)");
    println!("  Suite: Affinity Photo + Affinity Designer + Affinity Publisher (V2 2022)");
    println!("  Pricing model: ONE-TIME purchase (no subscription) — major selling point");
    println!("                 Affinity v2 Universal License $164.99 (one-time, all 3 apps Win/Mac/iPad)");
    println!("  Engine: custom C++ engine, GPU-accelerated (Metal/D3D), 64-bit color, 1000x100K px");
    println!("  Personas: workflow-mode switching (Photo / Liquify / Develop / Tone Mapping / Export)");
    println!("  Photo features: non-destructive RAW develop, HDR merge, focus stacking, panorama,");
    println!("                  inpainting brush, frequency separation, macros, batch processing");
    println!("  Compatibility: PSD read/write, PSB, .afphoto native, OpenEXR, DNG, HEIC, AVIF");
    println!("  Sister: Affinity Designer (Illustrator alt), Affinity Publisher (InDesign alt)");
    println!("  Differentiator: no subscription, pro features, one-time license — anti-Adobe");
    println!("  Tablet: Affinity Photo for iPad (full-feature port, not Lite) — separate purchase");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "affinityphoto".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ap(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ap};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/affinityphoto"), "affinityphoto");
        assert_eq!(basename(r"C:\bin\affinityphoto.exe"), "affinityphoto.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("affinityphoto.exe"), "affinityphoto");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_ap(&["--help".to_string()], "affinityphoto"), 0);
        assert_eq!(run_ap(&["-h".to_string()], "affinityphoto"), 0);
        assert_eq!(run_ap(&["--version".to_string()], "affinityphoto"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_ap(&[], "affinityphoto"), 0);
    }
}
