#![deny(clippy::all)]

//! redshift-cli — OurOS Maxon Redshift (biased GPU production renderer)
//!
//! Single personality: `redshift`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rs(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: redshift [OPTIONS]");
        println!("Maxon Redshift 2024 (OurOS) — Biased GPU production renderer");
        println!();
        println!("Options:");
        println!("  --render SCENE         Render scene");
        println!("  --rt                   Redshift RT (real-time interactive — RTX preferred)");
        println!("  --multi-gpu            Multi-GPU parallel (linear scaling)");
        println!("  --proxy                Redshift Proxy (.rs cached geometry)");
        println!("  --hydra                Hydra render delegate (USD scenes via Houdini/Maya)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Redshift 2024.4.4 (OurOS)"); return 0; }
    println!("Redshift 2024.4.4 (OurOS)");
    println!("  Vendor: Maxon Computer GmbH (Germany) — acquired Redshift Rendering 2019");
    println!("  Original company: Redshift Rendering Technologies (Newport Beach CA, founded 2012)");
    println!("  Founders: Nicolas Burtnyk, Panagiotis Zompolas, Rob Slater");
    println!("  Pricing: Maxon One $124.91/mo (bundle: C4D + ZBrush + Redshift + Forger + Red Giant)");
    println!("  Engine: biased GPU renderer — uses irradiance cache for performance");
    println!("         (faster than unbiased on noisy interiors, with slight bias artifact tradeoff)");
    println!("  Backend: NVIDIA CUDA + OptiX (RTX) initially, AMD Metal/HIP added 2022, Apple Silicon native");
    println!("  Integrations: Cinema 4D (deepest), Houdini, Maya, 3ds Max, Blender (beta), Katana");
    println!("  Multi-GPU: superb scaling — used in render farms with 8x4090 in single workstations");
    println!("  Killer features:");
    println!("    - 'Biased' tricks (irradiance cache, brute force GI selectable) for speed");
    println!("    - Out-of-core textures (renders 256GB textures with 24GB VRAM)");
    println!("    - Proxy assets for massive scenes (forests, cities)");
    println!("    - Redshift RT mode (since 2022) for interactive RTX-accelerated viewport");
    println!("    - Hydra delegate for USD pipelines");
    println!("  Famous users: motion graphics studios (Beeple, GMUNK), broadcast designers, ads");
    println!("  Differentiator: best speed/quality tradeoff in GPU rendering — pragmatic biased path");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "redshift".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rs(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rs};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/redshift"), "redshift");
        assert_eq!(basename(r"C:\bin\redshift.exe"), "redshift.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("redshift.exe"), "redshift");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rs(&["--help".to_string()], "redshift"), 0);
        assert_eq!(run_rs(&["-h".to_string()], "redshift"), 0);
        let _ = run_rs(&["--version".to_string()], "redshift");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rs(&[], "redshift");
    }
}
