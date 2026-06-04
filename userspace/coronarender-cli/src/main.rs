#![deny(clippy::all)]

//! coronarender-cli — OurOS Corona Renderer (Chaos Group, ArchViz favorite)
//!
//! Single personality: `corona`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_corona(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: corona [OPTIONS]");
        println!("Corona Renderer 12 (OurOS) — Unbiased path tracer for ArchViz");
        println!();
        println!("Options:");
        println!("  --render SCENE         Render scene");
        println!("  --interactive          Corona Interactive (live viewport)");
        println!("  --image-editor         Corona Image Editor (post-process .cxr files)");
        println!("  --gpu                  Corona GPU (since v12, optional NVIDIA acceleration)");
        println!("  --tone-mapping         Built-in HDR tone-mapping pipeline");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Corona Renderer 12 (Hotfix 1) (OurOS)"); return 0; }
    println!("Corona Renderer 12 (Hotfix 1) (OurOS)");
    println!("  Vendor: Chaos Czech a.s. (Prague, CZ) — owned by Chaos Group since Aug 2017");
    println!("  Founders: Ondřej Karlík, Adam Hotový, Jaroslav Křivánek (Prague Charles Univ.)");
    println!("  Origin: started as Karlík's bachelor's thesis (2009), commercial 2014");
    println!("  Pricing: Corona Solo €42.90/mo or €514.80/yr (Chaos plan), often bundled w/ V-Ray");
    println!("  Integrations: 3ds Max, Cinema 4D (only — vs V-Ray's many host options)");
    println!("  Philosophy: 'simplicity by design' — almost no settings to tweak, just hit render");
    println!("  Engine: CPU-only path tracing originally, GPU mode added v12 (NVIDIA RTX/OptiX)");
    println!("  Materials: Corona Material (PBR), Corona Layered Material, Corona Sky/Sun, Corona Volume");
    println!("  Killer features:");
    println!("    - Interactive Rendering (IR): full PT preview on viewport, near-real-time");
    println!("    - Built-in tone-mapping with film curves, no need for external compositor");
    println!("    - Corona Image Editor — re-tone-map and post any saved .cxr without re-render");
    println!("    - LightMix: rebalance light intensities after render — without re-rendering");
    println!("  ArchViz fame: dominant in real-estate / architecture viz market in Europe");
    println!("  Reputation: 'V-Ray for people who don't want to read manuals' — easy to learn");
    println!("  Differentiator: cleanest noise pattern of any unbiased renderer in market");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "corona".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_corona(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_corona};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/coronarender"), "coronarender");
        assert_eq!(basename(r"C:\bin\coronarender.exe"), "coronarender.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("coronarender.exe"), "coronarender");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_corona(&["--help".to_string()], "coronarender"), 0);
        assert_eq!(run_corona(&["-h".to_string()], "coronarender"), 0);
        let _ = run_corona(&["--version".to_string()], "coronarender");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_corona(&[], "coronarender");
    }
}
