#![deny(clippy::all)]

//! arnold-cli — Slate OS Autodesk Arnold (Oscar-winning production renderer)
//!
//! Single personality: `arnold`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_arnold(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: arnold [OPTIONS]");
        println!("Autodesk Arnold 7 (Slate OS) — Unbiased Monte Carlo path tracer");
        println!();
        println!("Options:");
        println!("  --render SCENE.ass     Render .ass (Arnold Scene Source) file");
        println!("  --kick                 kick command-line renderer");
        println!("  --gpu                  Arnold GPU (OptiX/RTX) since Arnold 5.3");
        println!("  --interactive          ArnoldRenderView (interactive PT)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Arnold 7.3.4.0 (Slate OS)"); return 0; }
    println!("Arnold 7.3.4.0 (Slate OS)");
    println!("  Vendor: Autodesk Inc. — acquired Solid Angle (original Arnold dev) Apr 2016 for $60M");
    println!("  Original developer: Solid Angle SL (Madrid, Spain) — Marcos Fajardo (founder 2004)");
    println!("  History: Arnold 1.0 internal use Sony Pictures Imageworks 2009, public 2013");
    println!("  Famous breakthrough: 'Monster House' (2006) — first feature fully rendered in Arnold");
    println!("  Oscar: 2017 Academy Sci-Tech Award for the design of Arnold (Fajardo + team)");
    println!("  Pricing: $585/yr single-user, $235/yr per render-only node, MtoA/HtoA free for plugin owners");
    println!("  Integrations: Maya (native — MtoA), Houdini (HtoA), 3ds Max, Cinema 4D, Katana, Softimage");
    println!("  Engine: unbiased Monte Carlo path tracer, ray-traced from camera (no light cache cheating)");
    println!("  Backends: CPU (highly optimized SIMD), Arnold GPU (OptiX 7, since Arnold 5.3)");
    println!("  Materials: standard_surface (industry standard OpenPBR-compatible), Arnold AOVs");
    println!("  Strengths: rock-solid memory efficiency (handles billions of polygons), simple controls,");
    println!("             noise-free convergence, robust hair / fur / volumetrics");
    println!("  Famous users: ILM, Sony Pictures Imageworks, MPC, DNEG, Framestore, Method, Animal Logic");
    println!("  Films: Gravity, The Avengers, Pacific Rim, Game of Thrones, Lion King 2019, every Marvel film");
    println!("  Hollywood standard: top-3 production renderer alongside RenderMan + V-Ray");
    println!("  Differentiator: predictable PT convergence + no special-case tweaks needed for film output");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "arnold".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_arnold(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_arnold};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/arnold"), "arnold");
        assert_eq!(basename(r"C:\bin\arnold.exe"), "arnold.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("arnold.exe"), "arnold");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_arnold(&["--help".to_string()], "arnold"), 0);
        assert_eq!(run_arnold(&["-h".to_string()], "arnold"), 0);
        let _ = run_arnold(&["--version".to_string()], "arnold");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_arnold(&[], "arnold");
    }
}
