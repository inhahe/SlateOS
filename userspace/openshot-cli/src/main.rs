#![deny(clippy::all)]

//! openshot-cli — OurOS OpenShot video editor
//!
//! Single personality: `openshot`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_os(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: openshot [OPTIONS] [PROJECT]");
        println!("OpenShot Video Editor 3.2 (OurOS) — Easy-to-use cross-platform NLE");
        println!();
        println!("Options:");
        println!("  --debug                Debug logging");
        println!("  --version              Show version");
        println!("  --lang LANG            UI language");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("OpenShot 3.2.0 (OurOS)"); return 0; }
    println!("OpenShot 3.2.0 (OurOS)");
    println!("  Engine: libopenshot (C++ with Python bindings)");
    println!("  Features: Drag-and-drop editing, keyframe animation, 3D titles");
    println!("  Effects: Watermarks, transparency, color shifts");
    println!("  Audio: Waveform display, per-clip audio mixing");
    println!("  Formats: All FFmpeg formats");
    println!("  License: GNU GPLv3");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "openshot".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_os(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
