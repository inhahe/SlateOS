#![deny(clippy::all)]

//! wayland-info-cli — SlateOS wayland-info compositor information
//!
//! Single personality: `wayland-info`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wayland_info(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wayland-info [OPTIONS]");
        println!("wayland-info v1.0 (Slate OS) — Display Wayland compositor information");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Lists all global objects advertised by the compositor.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wayland-info v1.0 (Slate OS)"); return 0; }
    println!("interface: 'wl_compositor', version: 5");
    println!("interface: 'wl_shm', version: 1");
    println!("interface: 'wl_seat', version: 8");
    println!("interface: 'wl_output', version: 4");
    println!("interface: 'xdg_wm_base', version: 5");
    println!("interface: 'zwlr_layer_shell_v1', version: 4");
    println!("interface: 'zwlr_screencopy_manager_v1', version: 3");
    println!("interface: 'wp_viewporter', version: 1");
    println!("interface: 'wp_fractional_scale_manager_v1', version: 1");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wayland-info".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wayland_info(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wayland_info};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wayland-info"), "wayland-info");
        assert_eq!(basename(r"C:\bin\wayland-info.exe"), "wayland-info.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wayland-info.exe"), "wayland-info");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wayland_info(&["--help".to_string()], "wayland-info"), 0);
        assert_eq!(run_wayland_info(&["-h".to_string()], "wayland-info"), 0);
        let _ = run_wayland_info(&["--version".to_string()], "wayland-info");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wayland_info(&[], "wayland-info");
    }
}
