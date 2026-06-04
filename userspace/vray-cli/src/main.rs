#![deny(clippy::all)]

//! vray-cli — OurOS V-Ray (Chaos Group production renderer)
//!
//! Single personality: `vray`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_vray(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: vray [OPTIONS]");
        println!("V-Ray 6 (OurOS) — Chaos Group production renderer (biased + path tracing)");
        println!();
        println!("Options:");
        println!("  --render SCENE         Render a .vrscene file");
        println!("  --gpu                  V-Ray GPU (CUDA/OptiX/RTX)");
        println!("  --next                 V-Ray Next mode (RTX-accelerated)");
        println!("  --vantage              Chaos Vantage (real-time interactive)");
        println!("  --cloud                Chaos Cloud (cloud rendering)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("V-Ray 6.20.05 (OurOS)"); return 0; }
    println!("V-Ray 6.20.05 (OurOS)");
    println!("  Vendor: Chaos Software (Sofia, Bulgaria — founded 1997)");
    println!("  Founders: Peter Mitev, Vladimir Koylazov ('Vlado')");
    println!("  Merger: Chaos + Enscape merged 2022 — also acquired Cylindo, AXYZ");
    println!("  Origin: V-Ray 1.0 (2002) — became industry-standard for ArchViz, VFX");
    println!("  Integrations: 3ds Max, Maya, SketchUp, Rhino, Revit, Cinema 4D, Houdini, Modo,");
    println!("               Unreal Engine (V-Ray for UE), Blender (via plugin), Nuke");
    println!("  Pricing: V-Ray Solo $42.90/mo or $514.80/yr, V-Ray Premium $80.30/mo");
    println!("  Engines: hybrid CPU/GPU, biased (irradiance map) + unbiased (CUDA path tracing)");
    println!("  V-Ray GPU: NVIDIA OptiX (RTX), NVIDIA CUDA fallback (multi-GPU scales near-linear)");
    println!("  Materials: V-Ray Material (PBR), V-Ray ALSurface (skin), V-Ray Hair, V-Ray Fast SSS");
    println!("  Geometry: V-Ray Proxy (.vrmesh, billions of polys), Displacement, V-Ray Fur");
    println!("  Lighting: V-Ray Sun/Sky, IES lights, dome HDRI, mesh lights, light cache");
    println!("  Awards: 2017 Academy Sci-Tech Award (Engineering Emmy) for V-Ray's impact on VFX");
    println!("  Used in: Game of Thrones, Avengers, Ad Astra, every Marvel film, most car ads");
    println!("  Companion: Chaos Vantage (live raytrace viewport), Chaos Phoenix (FX sim)");
    println!("  Differentiator: gold-standard photoreal output, mature ArchViz workflow");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "vray".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_vray(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_vray};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/vray"), "vray");
        assert_eq!(basename(r"C:\bin\vray.exe"), "vray.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("vray.exe"), "vray");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_vray(&["--help".to_string()], "vray"), 0);
        assert_eq!(run_vray(&["-h".to_string()], "vray"), 0);
        let _ = run_vray(&["--version".to_string()], "vray");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_vray(&[], "vray");
    }
}
