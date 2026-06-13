#![deny(clippy::all)]

//! octanerender-cli — Slate OS OTOY Octane Render (GPU path tracer)
//!
//! Single personality: `octane`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_octane(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: octane [OPTIONS]");
        println!("OTOY Octane Render 2024 (Slate OS) — GPU-only spectral path tracer");
        println!();
        println!("Options:");
        println!("  --render SCENE         Render an .orbx scene file");
        println!("  --interactive          Octane Live (real-time viewport)");
        println!("  --rndr-network         Octane Render Network (peer GPU farm)");
        println!("  --orbx-bridge          ORBX Live (network sync between hosts)");
        println!("  --ai-denoise           Spectral AI denoiser (training set: trillions of samples)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Octane Render 2024.1.1 (Slate OS)"); return 0; }
    println!("Octane Render 2024.1.1 (Slate OS)");
    println!("  Vendor: OTOY Inc. (HQ Los Angeles, CA — founded 2008)");
    println!("  Founder/CEO: Jules Urbach");
    println!("  Pricing: Octane Studio+ $19.99/mo (one host) or $479.88/yr — extremely fair");
    println!("          Free tier: Octane Prime (free for Unity, Blender personal hosts)");
    println!("  History: one of FIRST mainstream GPU-only path tracers (2010), shocked the industry");
    println!("  Engine: NVIDIA CUDA + OptiX (RTX), spectral / Hero Spectral sampling");
    println!("  Integrations: Cinema 4D, Blender, 3ds Max, Maya, Houdini, LightWave, Modo, Unity,");
    println!("               Nuke, Poser, Carrara, Daz Studio, AfterEffects, ZBrush, Revit, Rhino");
    println!("  Standalone: full standalone scene editor in addition to host plugins");
    println!("  Render network: peer-to-peer GPU farm — combine all team GPUs (no farm software)");
    println!("  ORBX: open scene-graph format (texture+geo+lights+materials) for cross-host portability");
    println!("  Cloud: OctaneRender Cloud (browser-based on RNDR network — OTOY's blockchain GPU farm)");
    println!("  Killer feature: instant 'wow factor' shaders + extremely fast progressive PT");
    println!("  Famous projects: many Hollywood concept-art studios, Westworld VFX, music videos");
    println!("  Differentiator: GPU-native from day 1, no biased shortcuts, very high physical fidelity");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "octane".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_octane(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_octane};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/octanerender"), "octanerender");
        assert_eq!(basename(r"C:\bin\octanerender.exe"), "octanerender.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("octanerender.exe"), "octanerender");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_octane(&["--help".to_string()], "octanerender"), 0);
        assert_eq!(run_octane(&["-h".to_string()], "octanerender"), 0);
        let _ = run_octane(&["--version".to_string()], "octanerender");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_octane(&[], "octanerender");
    }
}
