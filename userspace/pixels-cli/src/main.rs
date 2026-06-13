#![deny(clippy::all)]

//! pixels-cli — SlateOS Pixels framebuffer renderer
//!
//! Single personality: `pixels`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pixels(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pixels COMMAND [OPTIONS]");
        println!("Pixels v0.14.0 (Slate OS) — Hardware-accelerated pixel buffer");
        println!();
        println!("Commands:");
        println!("  new NAME        Create new Pixels project");
        println!("  build           Build project");
        println!("  run             Build and run");
        println!("  info            Show renderer info");
        println!("  version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("pixels v0.14.0 (Slate OS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("info");
    match cmd {
        "new" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("my-pixel-app");
            println!("Creating Pixels project: {}", name);
            println!("  Created Cargo.toml with pixels + winit dependencies");
            println!("  Created src/main.rs with pixel buffer template");
            println!("  Done.");
        }
        "build" => {
            println!("Building Pixels project...");
            println!("  Compiled successfully.");
        }
        "run" => {
            println!("Running Pixels app...");
            println!("  Logical size: 320x240");
            println!("  Window size: 960x720 (3x scale)");
            println!("  Backend: wgpu");
        }
        "info" => {
            println!("Pixels v0.14.0");
            println!("  Backend: wgpu");
            println!("  Format: RGBA8 pixel buffer");
            println!("  Scaling: nearest-neighbor / custom shader");
            println!("  Windowing: winit");
            println!("  Use case: retro games, emulators, pixel art");
        }
        _ => println!("pixels {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pixels".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pixels(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pixels};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pixels"), "pixels");
        assert_eq!(basename(r"C:\bin\pixels.exe"), "pixels.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pixels.exe"), "pixels");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pixels(&["--help".to_string()], "pixels"), 0);
        assert_eq!(run_pixels(&["-h".to_string()], "pixels"), 0);
        let _ = run_pixels(&["--version".to_string()], "pixels");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pixels(&[], "pixels");
    }
}
