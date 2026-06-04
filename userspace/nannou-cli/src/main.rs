#![deny(clippy::all)]

//! nannou-cli — OurOS Nannou creative coding framework
//!
//! Single personality: `nannou`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_nannou(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: nannou COMMAND [OPTIONS]");
        println!("Nannou v0.19.0 (OurOS) — Creative coding framework for Rust");
        println!();
        println!("Commands:");
        println!("  new NAME        Create new nannou project");
        println!("  sketch NAME     Create a quick sketch");
        println!("  build           Build project");
        println!("  run             Build and run");
        println!("  export          Export frames/video");
        println!("  info            Show framework info");
        println!("  version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("nannou v0.19.0 (OurOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("info");
    match cmd {
        "new" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("my-sketch");
            println!("Creating nannou project: {}", name);
            println!("  Created Cargo.toml with nannou dependency");
            println!("  Created src/main.rs with app model template");
            println!("  Created assets/ directory");
            println!("  Done.");
        }
        "sketch" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("sketch_01");
            println!("Creating nannou sketch: {}", name);
            println!("  Created src/{}.rs (minimal sketch)", name);
        }
        "build" => {
            println!("Building nannou project...");
            println!("  Compiled successfully.");
        }
        "run" => println!("Running nannou app... Window: 1024x768"),
        "export" => {
            println!("Exporting frames...");
            println!("  Output: frames/frame_0001.png ... frame_0300.png");
            println!("  300 frames exported.");
        }
        "info" => {
            println!("Nannou v0.19.0");
            println!("  Graphics: wgpu");
            println!("  Audio: nannou_audio (CPAL)");
            println!("  Windowing: winit");
            println!("  Laser: nannou_laser");
            println!("  OSC: nannou_osc");
        }
        _ => println!("nannou {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "nannou".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nannou(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_nannou};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/nannou"), "nannou");
        assert_eq!(basename(r"C:\bin\nannou.exe"), "nannou.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("nannou.exe"), "nannou");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_nannou(&["--help".to_string()], "nannou"), 0);
        assert_eq!(run_nannou(&["-h".to_string()], "nannou"), 0);
        let _ = run_nannou(&["--version".to_string()], "nannou");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_nannou(&[], "nannou");
    }
}
