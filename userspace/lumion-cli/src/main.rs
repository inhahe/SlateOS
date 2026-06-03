#![deny(clippy::all)]

//! lumion-cli — OurOS Lumion (architectural visualization in real time)
//!
//! Single personality: `lumion`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lum(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lumion [OPTIONS]");
        println!("Lumion 2024 (OurOS) — Real-time architectural visualization");
        println!();
        println!("Options:");
        println!("  --import FILE          Import (SketchUp/Revit/ArchiCAD/Rhino/3ds/FBX/Collada/SKP)");
        println!("  --livesync             LiveSync (real-time bidirectional with Revit/SketchUp/Rhino/ArchiCAD)");
        println!("  --library              Lumion Content Library (1000s of trees/people/cars/furniture)");
        println!("  --ray-tracing          Hybrid Ray Tracing (since Lumion 12)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Lumion 2024.0.2 (build 24.0.2.4) (OurOS)"); return 0; }
    println!("Lumion 2024.0.2 (build 24.0.2.4) (OurOS)");
    println!("  Vendor: Act-3D B.V. (Warmond, Netherlands — Lumion brand wholly owned)");
    println!("  Founded: 1989 (Act-3D), Lumion product launched 2010");
    println!("  Pricing: Lumion Pro €1499/yr, Lumion Standard €749/yr (NL pricing, regional variance)");
    println!("  Niche: ArchViz for architects who AREN'T 3D specialists (vs. V-Ray power users)");
    println!("  Engine: proprietary real-time game-engine-like renderer (DirectX 11/12)");
    println!("         hybrid raytracing added Lumion 12, full path-traced still images Lumion 2023+");
    println!("  Workflow: import CAD → place trees + people from library → animate camera → render in MINUTES");
    println!("           5-second renders typical, vs. hours in V-Ray/Corona");
    println!("  Killer feature: LiveSync — change a wall in Revit/SketchUp, see it live in Lumion");
    println!("  Library: 6800+ models, 1300+ materials, 1300+ sound effects, weather/time-of-day presets");
    println!("  Effects: real-time DOF, volumetric clouds, fog, snow/rain, blooming sun, lens flares");
    println!("  Audience: architects, real-estate firms, urban planners, interior designers");
    println!("  Notable: heavy use in early-stage client presentations (where speed > photoreal)");
    println!("  Competitors: Twinmotion (Epic — newer disruptor), Enscape (Chaos — real-time too)");
    println!("  Hardware: requires beefy GPU (RTX 3060+ recommended), Windows only");
    println!("  Differentiator: speed and accessibility for non-CG-trained architects");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lumion".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lum(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lum};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lumion"), "lumion");
        assert_eq!(basename(r"C:\bin\lumion.exe"), "lumion.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lumion.exe"), "lumion");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_lum(&["--help".to_string()], "lumion"), 0);
        assert_eq!(run_lum(&["-h".to_string()], "lumion"), 0);
        assert_eq!(run_lum(&["--version".to_string()], "lumion"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_lum(&[], "lumion"), 0);
    }
}
