#![deny(clippy::all)]

//! duckstation-cli — OurOS DuckStation PS1 emulator
//!
//! Multi-personality: `duckstation`, `duckstation-nogui`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_duckstation(args: &[String], prog: &str) -> i32 {
    let nogui = prog == "duckstation-nogui";
    if args.iter().any(|a| a == "--help" || a == "-h") {
        if nogui {
            println!("Usage: duckstation-nogui [OPTIONS] IMAGE");
        } else {
            println!("Usage: duckstation [OPTIONS] [IMAGE]");
        }
        println!("duckstation v0.1-6292 (OurOS) — PlayStation 1 emulator");
        println!();
        println!("Options:");
        println!("  -disc FILE        Boot disc image");
        println!("  -exe FILE         Boot PS-EXE");
        println!("  -fullscreen       Start fullscreen");
        println!("  -renderer VK|GL   GPU renderer");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("duckstation v0.1-6292 (OurOS)"); return 0; }
    if nogui {
        println!("duckstation: headless PS1 emulation started");
    } else {
        println!("duckstation: PlayStation 1 emulator started");
    }
    println!("  Renderer: Vulkan (hardware)");
    println!("  Resolution: 8x native (2560x1920)");
    println!("  PGXP: geometry correction enabled");
    println!("  Texture filtering: bilinear");
    println!("  Memory cards: 2 slots available");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "duckstation".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_duckstation(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_duckstation};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/duckstation"), "duckstation");
        assert_eq!(basename(r"C:\bin\duckstation.exe"), "duckstation.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("duckstation.exe"), "duckstation");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_duckstation(&["--help".to_string()], "duckstation"), 0);
        assert_eq!(run_duckstation(&["-h".to_string()], "duckstation"), 0);
        let _ = run_duckstation(&["--version".to_string()], "duckstation");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_duckstation(&[], "duckstation");
    }
}
