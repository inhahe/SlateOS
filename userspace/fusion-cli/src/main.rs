#![deny(clippy::all)]

//! fusion-cli — OurOS Blackmagic Fusion compositing
//!
//! Single personality: `fusion`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fusion(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fusion [OPTIONS] [COMP]");
        println!("Blackmagic Fusion 19 Studio (OurOS) — Node-based compositing & motion graphics");
        println!();
        println!("Options:");
        println!("  -render COMP           Render composition");
        println!("  -frames N-M            Frame range");
        println!("  -script FILE           Run Fusion Lua/Python script");
        println!("  -nogui                 Headless render mode");
        println!("  -node ID               Set node parameters");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Blackmagic Fusion 19.0 Studio (OurOS)"); return 0; }
    println!("Blackmagic Fusion 19.0 Studio (OurOS)");
    println!("  Renderer: GPU-accelerated (CUDA, Metal, Vulkan)");
    println!("  Scripting: Lua, Python");
    println!("  Tools: 250+ (Camera Tracker, Planar Tracker, Particles)");
    println!("  Integration: DaVinci Resolve Fusion page");
    println!("  License: Studio (paid) / Free");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "fusion".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fusion(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fusion};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/fusion"), "fusion");
        assert_eq!(basename(r"C:\bin\fusion.exe"), "fusion.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("fusion.exe"), "fusion");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_fusion(&["--help".to_string()], "fusion"), 0);
        assert_eq!(run_fusion(&["-h".to_string()], "fusion"), 0);
        assert_eq!(run_fusion(&["--version".to_string()], "fusion"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_fusion(&[], "fusion"), 0);
    }
}
