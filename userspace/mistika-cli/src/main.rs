#![deny(clippy::all)]

//! mistika-cli — SlateOS SGO Mistika Boutique color & VFX
//!
//! Single personality: `mistika`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mistika(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mistika [OPTIONS] [PROJECT]");
        println!("SGO Mistika Boutique 10 (SlateOS) — High-end VR, stitching, color, finishing");
        println!();
        println!("Options:");
        println!("  --open FILE            Open project");
        println!("  --vr                   Mistika VR mode (stereoscopic stitching)");
        println!("  --workflows            Open Workflows");
        println!("  --node-tree            Open Node Tree editor");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("SGO Mistika Boutique 10.13.5 (SlateOS)"); return 0; }
    println!("SGO Mistika Boutique 10.13.5 (SlateOS)");
    println!("  Editions: Workflows, Boutique, Ultima, VR, Insight");
    println!("  Specialties: 8K/12K finishing, stereo 3D, VR/360, HDR");
    println!("  Node tree: Resolution-independent, node-based finishing");
    println!("  Codec support: ARRI/RED/SONY/Phantom/Z-CAM/Insta360 native");
    println!("  Used in: Netflix, Disney+, IMAX, theme parks, VR experiences");
    println!("  License: subscription (monthly/yearly) / perpetual");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mistika".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mistika(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mistika};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mistika"), "mistika");
        assert_eq!(basename(r"C:\bin\mistika.exe"), "mistika.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mistika.exe"), "mistika");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mistika(&["--help".to_string()], "mistika"), 0);
        assert_eq!(run_mistika(&["-h".to_string()], "mistika"), 0);
        let _ = run_mistika(&["--version".to_string()], "mistika");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mistika(&[], "mistika");
    }
}
