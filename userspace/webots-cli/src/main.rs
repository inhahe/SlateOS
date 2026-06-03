#![deny(clippy::all)]

//! webots-cli — OurOS Webots robot simulator
//!
//! Single personality: `webots`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_webots(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: webots [OPTIONS] [WORLD.wbt]");
        println!("Webots R2024a (OurOS) — Open-source robot simulator");
        println!();
        println!("Options:");
        println!("  WORLD.wbt         World file to load");
        println!("  --batch            Batch mode (no GUI)");
        println!("  --mode MODE       Simulation mode (realtime, fast, pause)");
        println!("  --minimize         Start minimized");
        println!("  --stdout           Redirect robot stdout");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Webots R2024a (OurOS)");
        return 0;
    }
    let world = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("worlds/default.wbt");
    println!("Webots R2024a — Loading: {}", world);
    println!("  Physics: ODE");
    println!("  Robots: 1");
    println!("  Sensors: lidar, camera, IMU");
    println!("  Mode: realtime");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "webots".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_webots(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_webots};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/webots"), "webots");
        assert_eq!(basename(r"C:\bin\webots.exe"), "webots.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("webots.exe"), "webots");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_webots(&["--help".to_string()], "webots"), 0);
        assert_eq!(run_webots(&["-h".to_string()], "webots"), 0);
        assert_eq!(run_webots(&["--version".to_string()], "webots"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_webots(&[], "webots"), 0);
    }
}
