#![deny(clippy::all)]

//! twinmotion-cli — SlateOS Twinmotion (Epic/Unreal-powered ArchViz)
//!
//! Single personality: `twinmotion`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tm(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: twinmotion [OPTIONS]");
        println!("Twinmotion 2024 (Slate OS) — Real-time ArchViz on Unreal Engine 5");
        println!();
        println!("Options:");
        println!("  --import FILE          Import (Revit/SketchUp/Archicad/Rhino/Vectorworks)");
        println!("  --datasmith            Datasmith Direct Link (live host sync)");
        println!("  --quixel               Quixel Megascans library (free for Twinmotion)");
        println!("  --to-unreal            Open project in Unreal Engine 5 (Datasmith export)");
        println!("  --presenter            Twinmotion Cloud Presenter (web 3D walk-through)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Twinmotion 2024.1 (Slate OS)"); return 0; }
    println!("Twinmotion 2024.1 (Slate OS)");
    println!("  Vendor: Epic Games (Cary, NC) — acquired Twinmotion's parent Abvent May 2019");
    println!("  Original developer: Ka-Ra → Abvent (French firm) → Epic");
    println!("  Pricing: $499/yr (perpetual licenses still available) — FREE for students/educators");
    println!("           Free for architects until 2020, then commercial");
    println!("  Engine: Unreal Engine 5 under the hood — Lumen GI, Nanite virtualized geo");
    println!("  Workflow: drop in BIM/CAD → place vegetation/people → render in real time");
    println!("           seconds, not minutes — interactive 60fps walkthrough always");
    println!("  Killer features:");
    println!("    - Direct Link / Datasmith: live sync with Revit, ArchiCAD, SketchUp, Rhino, Vectorworks");
    println!("    - Quixel Megascans library — FREE for Twinmotion users (huge value)");
    println!("    - Path tracer (since 2022) for final stills with ray-traced indirect lighting");
    println!("    - Twinmotion Presenter — share interactive walkthroughs via web link");
    println!("    - Twinmotion → Unreal Engine 5 path for high-end interactive experiences");
    println!("  Library: 1000s of trees, people, cars, furniture, terrains, materials");
    println!("  Audience: architects, BIM users (Revit/ArchiCAD), urban planners, real-estate marketing");
    println!("  Hardware: needs RTX 30 series+ for path tracing, RTX 20+ for raster mode");
    println!("  Competitor: Lumion (Act-3D), Enscape (Chaos — also real-time)");
    println!("  Differentiator: Unreal Engine pedigree + Epic Megascans bundle + path to UE5");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "twinmotion".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tm(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_tm};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/twinmotion"), "twinmotion");
        assert_eq!(basename(r"C:\bin\twinmotion.exe"), "twinmotion.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("twinmotion.exe"), "twinmotion");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_tm(&["--help".to_string()], "twinmotion"), 0);
        assert_eq!(run_tm(&["-h".to_string()], "twinmotion"), 0);
        let _ = run_tm(&["--version".to_string()], "twinmotion");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_tm(&[], "twinmotion");
    }
}
