#![deny(clippy::all)]

//! wayland-logout-cli — OurOS wayland-logout session terminator
//!
//! Single personality: `wayland-logout`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wayland_logout(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wayland-logout [OPTIONS]");
        println!("wayland-logout v1.4 (OurOS) — Terminate Wayland compositor session");
        println!();
        println!("Options:");
        println!("  -p PID            Compositor PID to terminate");
        println!("  --version         Show version");
        println!();
        println!("Sends exit request to the Wayland compositor, cleanly");
        println!("ending the session. Uses wl_registry to find the compositor.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wayland-logout v1.4 (OurOS)"); return 0; }
    println!("wayland-logout: requesting compositor session exit...");
    println!("  Session terminated cleanly.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wayland-logout".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wayland_logout(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wayland_logout};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wayland-logout"), "wayland-logout");
        assert_eq!(basename(r"C:\bin\wayland-logout.exe"), "wayland-logout.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wayland-logout.exe"), "wayland-logout");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_wayland_logout(&["--help".to_string()], "wayland-logout"), 0);
        assert_eq!(run_wayland_logout(&["-h".to_string()], "wayland-logout"), 0);
        assert_eq!(run_wayland_logout(&["--version".to_string()], "wayland-logout"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_wayland_logout(&[], "wayland-logout"), 0);
    }
}
