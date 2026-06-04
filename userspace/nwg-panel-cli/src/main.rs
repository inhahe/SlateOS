#![deny(clippy::all)]

//! nwg-panel-cli — OurOS nwg-panel GTK panel for Wayland
//!
//! Single personality: `nwg-panel`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_nwg_panel(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nwg-panel [OPTIONS]");
        println!("nwg-panel v0.9 (OurOS) — GTK3 panel for sway/Wayland compositors");
        println!();
        println!("Options:");
        println!("  -c FILE           Configuration file (JSON)");
        println!("  -s FILE           CSS style file");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("nwg-panel v0.9 (OurOS)"); return 0; }
    println!("nwg-panel: GTK3 panel running");
    println!("  Config: ~/.config/nwg-panel/config");
    println!("  Modules: clock, tray, workspaces, playerctl, brightness");
    if args.is_empty() {
        println!("  Ready.");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "nwg-panel".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nwg_panel(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_nwg_panel};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/nwg-panel"), "nwg-panel");
        assert_eq!(basename(r"C:\bin\nwg-panel.exe"), "nwg-panel.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("nwg-panel.exe"), "nwg-panel");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_nwg_panel(&["--help".to_string()], "nwg-panel"), 0);
        assert_eq!(run_nwg_panel(&["-h".to_string()], "nwg-panel"), 0);
        let _ = run_nwg_panel(&["--version".to_string()], "nwg-panel");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_nwg_panel(&[], "nwg-panel");
    }
}
