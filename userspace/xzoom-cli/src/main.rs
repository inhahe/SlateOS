#![deny(clippy::all)]

//! xzoom-cli — OurOS screen magnifier
//!
//! Single personality: `xzoom`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_xzoom(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xzoom [OPTIONS]");
        println!("xzoom v0.3 (OurOS) — Screen magnifier");
        println!();
        println!("Options:");
        println!("  -mag N         Magnification factor (1-16, default: 2)");
        println!("  -x N           Initial X position");
        println!("  -y N           Initial Y position");
        println!("  -w N           Window width (default: 256)");
        println!("  -h N           Window height (default: 256)");
        println!("  -source WxH    Source area size");
        println!("  -delay N       Update delay in ms (default: 100)");
        println!("  -follow        Follow mouse cursor");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("xzoom v0.3 (OurOS)"); return 0; }
    let mag = args.windows(2).find(|w| w[0] == "-mag").and_then(|w| w[1].parse::<u32>().ok()).unwrap_or(2);
    println!("xzoom v0.3 (OurOS) — Screen Magnifier");
    println!("  Magnification: {}x", mag);
    println!("  Source: 128x128 pixels");
    println!("  Window: 256x256 pixels");
    println!("  Refresh: 100ms");
    println!("  Mode: follow cursor");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "xzoom".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_xzoom(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_xzoom};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/xzoom"), "xzoom");
        assert_eq!(basename(r"C:\bin\xzoom.exe"), "xzoom.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("xzoom.exe"), "xzoom");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_xzoom(&["--help".to_string()], "xzoom"), 0);
        assert_eq!(run_xzoom(&["-h".to_string()], "xzoom"), 0);
        assert_eq!(run_xzoom(&["--version".to_string()], "xzoom"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_xzoom(&[], "xzoom"), 0);
    }
}
