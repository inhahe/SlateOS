#![deny(clippy::all)]

//! darktable-cli — OurOS darktable open-source RAW photo workflow
//!
//! Single personality: `darktable`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dt(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: darktable [OPTIONS]");
        println!("darktable 5.0 (OurOS) — Open-source RAW developer + DAM");
        println!();
        println!("Options:");
        println!("  --view MODE            lighttable/darkroom/tethering/map/slideshow/print");
        println!("  --library PATH         Open library (SQLite DB of catalog)");
        println!("  --cli IMG OUTPUT       Headless export via darktable-cli");
        println!("  --noop                 No automatic preset/style");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("darktable 5.0.0 (OurOS)"); return 0; }
    println!("darktable 5.0.0 (OurOS)");
    println!("  License: GPL-3.0-or-later (free software, openly developed on GitHub)");
    println!("  Origin: started 2009 by Henrik Andersson + Pascal de Bruijn — Linux RAW workflow");
    println!("  Lead maintainer: Aurélien Pierre (since ~2021)");
    println!("  Platforms: Linux (1st-class), macOS, Windows");
    println!("  Engine: non-destructive — operations stored as XMP sidecars + SQLite DB");
    println!("  Modules: 60+ image-operation modules — exposure, color balance, denoise, lens,");
    println!("          retouch, defringe, filmic RGB (scene-referred tone mapping), diffuse/sharpen");
    println!("  Camera support: 800+ camera models (read from libraw + custom RAW samples)");
    println!("  Pipeline: scene-referred (filmic RGB), allows HDR-style high dynamic range edits");
    println!("  Tethering: live capture from supported cameras via gphoto2");
    println!("  Map view: geotagging via GPX, location editing");
    println!("  Strengths: pixel-perfect output, scriptable (Lua), GPU acceleration (OpenCL)");
    println!("  Comparable to: Adobe Lightroom (closed source) — darktable is closest FOSS alt");
    println!("  Sister tools: RawTherapee (more conservative pipeline), ART (RawTherapee fork)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "darktable".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_dt(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dt};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/darktable"), "darktable");
        assert_eq!(basename(r"C:\bin\darktable.exe"), "darktable.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("darktable.exe"), "darktable");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dt(&["--help".to_string()], "darktable"), 0);
        assert_eq!(run_dt(&["-h".to_string()], "darktable"), 0);
        let _ = run_dt(&["--version".to_string()], "darktable");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dt(&[], "darktable");
    }
}
