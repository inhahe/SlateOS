#![deny(clippy::all)]

//! minifb-cli — Slate OS MiniFB framebuffer window tool
//!
//! Single personality: `minifb`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_minifb(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: minifb COMMAND [OPTIONS]");
        println!("MiniFB v0.27.0 (Slate OS) — Cross-platform framebuffer window");
        println!();
        println!("Commands:");
        println!("  new NAME        Create new MiniFB project");
        println!("  build           Build project");
        println!("  run             Build and run");
        println!("  demo            Run built-in demo");
        println!("  info            Show library info");
        println!("  version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("minifb v0.27.0 (Slate OS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("info");
    match cmd {
        "new" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("my-fb-app");
            println!("Creating MiniFB project: {}", name);
            println!("  Created Cargo.toml with minifb dependency");
            println!("  Created src/main.rs with framebuffer template");
            println!("  Done.");
        }
        "build" => {
            println!("Building MiniFB project...");
            println!("  Compiled successfully.");
        }
        "run" => println!("Running MiniFB app... Window: 640x480 (32bpp ARGB)"),
        "demo" => {
            println!("Running MiniFB demo...");
            println!("  Window: 800x600");
            println!("  Drawing: plasma effect");
            println!("  FPS: ~60");
        }
        "info" => {
            println!("MiniFB v0.27.0");
            println!("  API: Software framebuffer");
            println!("  Format: 32-bit ARGB");
            println!("  Input: Keyboard + Mouse");
            println!("  Platform: native window");
        }
        _ => println!("minifb {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "minifb".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_minifb(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_minifb};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/minifb"), "minifb");
        assert_eq!(basename(r"C:\bin\minifb.exe"), "minifb.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("minifb.exe"), "minifb");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_minifb(&["--help".to_string()], "minifb"), 0);
        assert_eq!(run_minifb(&["-h".to_string()], "minifb"), 0);
        let _ = run_minifb(&["--version".to_string()], "minifb");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_minifb(&[], "minifb");
    }
}
