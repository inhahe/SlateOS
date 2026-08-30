#![deny(clippy::all)]

//! keyshot-cli — Slate OS KeyShot (Luxion product visualization renderer)
//!
//! Single personality: `keyshot`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ks(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: keyshot [OPTIONS]");
        println!("KeyShot 2024 (Slate OS) — Product / industrial design renderer (real-time on CPU)");
        println!();
        println!("Options:");
        println!("  --import FILE          Import CAD (SolidWorks/Creo/NX/Inventor/CATIA/Rhino/Fusion/STEP/IGES)");
        println!("  --new                  New scene");
        println!("  --animation            KeyShot Animation (built-in turntable + camera)");
        println!("  --keyshot-web          KeyShot Web Viewer (interactive 3D embed)");
        println!("  --keyvr                KeyVR (VR walkthrough)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("KeyShot 2024.3 (build 13.3.4) (Slate OS)"); return 0; }
    println!("KeyShot 2024.3 (build 13.3.4) (Slate OS)");
    println!("  Vendor: Luxion ApS (HQ Tustin CA — founded 2002 Denmark, US HQ since)");
    println!("  Founder: Henrik Wann Jensen (PhD CalTech, Oscar Sci-Tech 2003 for photon mapping)");
    println!("  Pricing: KeyShot Pro $999/yr, KeyShot Enterprise $1999/yr, perpetual still available");
    println!("  Niche: product visualization / industrial design — virtually no competition in this segment");
    println!("  Engine: physically-correct unbiased PT, real-time interactive on CPU (multi-core scales)");
    println!("         GPU mode added 2020 (CUDA/OptiX) for RTX acceleration");
    println!("  CAD imports: ~30 formats — SolidWorks (deepest), Creo, NX, CATIA V5/V6, Inventor,");
    println!("              Fusion 360, Rhino, Alias, OBJ, FBX, USD, STEP, IGES, JT, Parasolid");
    println!("  Materials: huge built-in library (~750 PBR materials), drag-and-drop assignment");
    println!("  Studios: KeyShot 'Studios' — variant management (one scene, many config renders)");
    println!("  HDRI editing: built-in HDRI Editor (paint highlights/reflections directly on sphere)");
    println!("  Audience: industrial designers, engineers, marketing/comms — non-3D-pro UX target");
    println!("  Famous use: Apple product reveals (rumor), every consumer-electronics packaging");
    println!("  Workflow: open CAD → drag material → drag HDRI → render — under 60 seconds first image");
    println!("  Companion: KeyShot Studio Pro (asset library), KeyShotXR (interactive web 3D)");
    println!("  Differentiator: easiest-to-learn pro renderer, by far — design-school standard");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "keyshot".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ks(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ks};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/keyshot"), "keyshot");
        assert_eq!(basename(r"C:\bin\keyshot.exe"), "keyshot.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("keyshot.exe"), "keyshot");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ks(&["--help".to_string()], "keyshot"), 0);
        assert_eq!(run_ks(&["-h".to_string()], "keyshot"), 0);
        let _ = run_ks(&["--version".to_string()], "keyshot");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ks(&[], "keyshot");
    }
}
