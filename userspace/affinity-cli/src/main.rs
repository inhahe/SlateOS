#![deny(clippy::all)]

//! affinity-cli — SlateOS Affinity creative suite
//!
//! Single personality: `affinity`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_affinity(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: affinity [OPTIONS] [FILE]");
        println!("Affinity Suite 2 (SlateOS) — Photo + Designer + Publisher (perpetual license)");
        println!();
        println!("Options:");
        println!("  --photo                Launch Affinity Photo");
        println!("  --designer             Launch Affinity Designer");
        println!("  --publisher            Launch Affinity Publisher");
        println!("  --export FORMAT FILE   Export (afphoto/png/jpg/pdf/svg)");
        println!("  --macro FILE           Run macro");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Affinity Suite 2.5 (SlateOS)"); return 0; }
    println!("Affinity Suite 2.5 (SlateOS)");
    println!("  Apps: Affinity Photo 2, Designer 2, Publisher 2");
    println!("  Engine: Metal / Vulkan GPU acceleration");
    println!("  Personas: switch between vector, pixel, export modes in one app");
    println!("  Color: 32-bit per channel, OpenColorIO, RGB/CMYK/LAB/Grayscale");
    println!("  License: perpetual (no subscription)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "affinity".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_affinity(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_affinity};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/affinity"), "affinity");
        assert_eq!(basename(r"C:\bin\affinity.exe"), "affinity.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("affinity.exe"), "affinity");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_affinity(&["--help".to_string()], "affinity"), 0);
        assert_eq!(run_affinity(&["-h".to_string()], "affinity"), 0);
        let _ = run_affinity(&["--version".to_string()], "affinity");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_affinity(&[], "affinity");
    }
}
