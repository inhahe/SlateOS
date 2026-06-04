#![deny(clippy::all)]

//! motion-cli — OurOS Apple Motion (motion graphics for Final Cut Pro)
//!
//! Single personality: `motion`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_motion(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: motion [OPTIONS]");
        println!("Apple Motion 5.7 (OurOS) — Motion graphics + visual effects (FCP companion)");
        println!();
        println!("Options:");
        println!("  --new                  New project (motion graphic / Final Cut title/effect/transition/generator)");
        println!("  --behaviors            Behaviors (procedural animation system, no keyframes)");
        println!("  --particle             Particle emitter / replicator");
        println!("  --rigging              Rigging (parameter binding)");
        println!("  --3d                   3D space (group / camera / lighting)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Apple Motion 5.7.1 (OurOS)"); return 0; }
    println!("Apple Motion 5.7.1 (OurOS)");
    println!("  Vendor: Apple Inc. — companion app to Final Cut Pro");
    println!("  History: Motion 1.0 (2004) — built on the same Final Cut Studio engine,");
    println!("           since FCP X era sold separately at $49.99 one-time");
    println!("  Platform: macOS 14.6+ only (Apple Silicon optimized)");
    println!("  Pricing: $49.99 one-time on Mac App Store (no subscription)");
    println!("  Role: design titles / effects / transitions / generators for Final Cut Pro");
    println!("       (Motion is to FCP what After Effects is to Premiere — same studio family)");
    println!("  Signature features:");
    println!("    - Behaviors: procedural animation (drag a 'Throw' behavior onto a layer, it animates),");
    println!("                no keyframing needed for most motion");
    println!("    - Particle Emitter + Replicator: built-in particle/replicator system, vast presets");
    println!("    - Rigging: bind multiple parameters to one Master control (great for templates)");
    println!("    - 3D Space: real-time 3D groups with camera + lighting + shadows + reflections");
    println!("    - Generators: noise/gradient/checkerboard/stars/text (parameterized)");
    println!("    - FCPX template export: package up to use in Final Cut as drag-drop template");
    println!("  Engine: Metal-accelerated, 32-bit float linear-light pipeline, color managed");
    println!("  Use cases: lower-thirds, broadcast graphics, animated logos, FCP custom transitions");
    println!("  Competitor: Adobe After Effects (industry leader), Blackmagic Fusion (in DaVinci)");
    println!("  Weakness: smaller community/asset market than After Effects");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "motion".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_motion(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_motion};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/motion"), "motion");
        assert_eq!(basename(r"C:\bin\motion.exe"), "motion.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("motion.exe"), "motion");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_motion(&["--help".to_string()], "motion"), 0);
        assert_eq!(run_motion(&["-h".to_string()], "motion"), 0);
        let _ = run_motion(&["--version".to_string()], "motion");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_motion(&[], "motion");
    }
}
