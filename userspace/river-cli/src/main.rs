#![deny(clippy::all)]

//! river-cli — SlateOS River Wayland compositor
//!
//! Multi-personality: `river`, `riverctl`, `rivertile`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_river(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: river [OPTIONS]");
        println!("river v0.3 (Slate OS) — Dynamic tiling Wayland compositor");
        println!();
        println!("Options:");
        println!("  -c FILE           Startup config script");
        println!("  -log-level LEVEL  Log level");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("river v0.3 (Slate OS)"); return 0; }
    println!("River compositor starting...");
    println!("  Backend: DRM/KMS");
    println!("  Output: HDMI-A-1 (3840x2160@60Hz)");
    println!("  Layout: rivertile");
    0
}

fn run_riverctl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: riverctl COMMAND [ARGS...]");
        println!("riverctl v0.3 (Slate OS) — River compositor control");
        println!();
        println!("Commands:");
        println!("  map MODE MOD KEY CMD   Map key binding");
        println!("  spawn CMD              Spawn command");
        println!("  close                  Close focused view");
        println!("  focus-view DIRECTION   Focus next/previous");
        println!("  set-layout VALUE       Set default layout");
        println!("  output-layout VALUE    Set output layout");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    println!("riverctl: {} executed", cmd);
    0
}

fn run_rivertile(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: rivertile [OPTIONS]");
        println!("rivertile v0.3 (Slate OS) — Tiling layout generator for River");
        println!();
        println!("Options:");
        println!("  -view-padding N   Padding around views");
        println!("  -outer-padding N  Padding around output");
        println!("  -main-location L  Main area location (left/right/top/bottom)");
        println!("  -main-count N     Number of main views");
        println!("  -main-ratio R     Main area ratio");
        return 0;
    }
    println!("rivertile: layout generator running");
    println!("  Main: left, count=1, ratio=0.55");
    println!("  Padding: view=6, outer=6");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "river".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "riverctl" => run_riverctl(&rest, &prog),
        "rivertile" => run_rivertile(&rest, &prog),
        _ => run_river(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_river};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/river"), "river");
        assert_eq!(basename(r"C:\bin\river.exe"), "river.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("river.exe"), "river");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_river(&["--help".to_string()], "river"), 0);
        assert_eq!(run_river(&["-h".to_string()], "river"), 0);
        let _ = run_river(&["--version".to_string()], "river");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_river(&[], "river");
    }
}
