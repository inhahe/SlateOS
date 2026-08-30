#![deny(clippy::all)]

//! vkbasalt-cli — Slate OS vkBasalt Vulkan post-processing
//!
//! Single personality: `vkbasalt`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_vkbasalt(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: vkbasalt [OPTIONS]");
        println!("vkbasalt v0.3 (Slate OS) — Vulkan post-processing layer");
        println!();
        println!("Options:");
        println!("  --config FILE     Configuration file");
        println!("  --list-effects    List available effects");
        println!("  --version         Show version");
        println!();
        println!("Environment:");
        println!("  ENABLE_VKBASALT=1 Enable the layer");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("vkbasalt v0.3 (Slate OS)"); return 0; }
    if args.iter().any(|a| a == "--list-effects") {
        println!("Available effects:");
        println!("  cas              Contrast Adaptive Sharpening");
        println!("  fxaa             Fast Approximate Anti-Aliasing");
        println!("  smaa             Subpixel Morphological Anti-Aliasing");
        println!("  deband           Debanding");
        println!("  lut              Color lookup table");
        return 0;
    }
    println!("vkbasalt: Vulkan post-processing layer");
    println!("  Status: installed (use ENABLE_VKBASALT=1 to activate)");
    println!("  Effects: cas, fxaa, smaa, deband, lut");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "vkbasalt".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_vkbasalt(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_vkbasalt};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/vkbasalt"), "vkbasalt");
        assert_eq!(basename(r"C:\bin\vkbasalt.exe"), "vkbasalt.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("vkbasalt.exe"), "vkbasalt");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_vkbasalt(&["--help".to_string()], "vkbasalt"), 0);
        assert_eq!(run_vkbasalt(&["-h".to_string()], "vkbasalt"), 0);
        let _ = run_vkbasalt(&["--version".to_string()], "vkbasalt");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_vkbasalt(&[], "vkbasalt");
    }
}
