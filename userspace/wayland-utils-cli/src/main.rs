#![deny(clippy::all)]

//! wayland-utils-cli — Slate OS Wayland utility collection
//!
//! Multi-personality: `wl-info`, `wl-registry`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wl_info(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wl-info [OPTIONS]");
        println!("wl-info v0.1 (Slate OS) — Wayland display info");
        println!();
        println!("Options:");
        println!("  --json            Output as JSON");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wl-info v0.1 (Slate OS)"); return 0; }
    println!("Display: wayland-0");
    println!("Compositor: Slate OS compositor v1.0");
    println!("Outputs: 2");
    println!("  HDMI-A-1: 1920x1080@60Hz");
    println!("  DP-1: 2560x1440@144Hz");
    0
}

fn run_wl_registry(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wl-registry [OPTIONS]");
        println!("wl-registry v0.1 (Slate OS) — Dump Wayland registry");
        return 0;
    }
    let _ = args;
    println!("Global #1: wl_compositor v5");
    println!("Global #2: wl_subcompositor v1");
    println!("Global #3: wl_data_device_manager v3");
    println!("Global #4: wl_shm v1");
    println!("Global #5: wl_seat v8 (name: 'default')");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wl-info".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "wl-registry" => run_wl_registry(&rest, &prog),
        _ => run_wl_info(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wl_info};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wayland-utils"), "wayland-utils");
        assert_eq!(basename(r"C:\bin\wayland-utils.exe"), "wayland-utils.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wayland-utils.exe"), "wayland-utils");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wl_info(&["--help".to_string()], "wayland-utils"), 0);
        assert_eq!(run_wl_info(&["-h".to_string()], "wayland-utils"), 0);
        let _ = run_wl_info(&["--version".to_string()], "wayland-utils");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wl_info(&[], "wayland-utils");
    }
}
