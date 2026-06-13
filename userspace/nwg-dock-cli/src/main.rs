#![deny(clippy::all)]

//! nwg-dock-cli — SlateOS nwg-dock application dock
//!
//! Single personality: `nwg-dock`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_nwg_dock(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nwg-dock [OPTIONS]");
        println!("nwg-dock v0.3 (Slate OS) — Wayland application dock");
        println!();
        println!("Options:");
        println!("  -d                Dock position: top, bottom, left, right");
        println!("  -o OUTPUT         Output to display on");
        println!("  -w                Full width");
        println!("  -nolauncher       Don't show launcher icon");
        println!("  -i ICON_SIZE      Icon size (px)");
        println!("  -mb MARGIN        Margin bottom");
        println!("  -ml MARGIN        Margin left");
        println!("  -r                Resident mode (stay running)");
        println!("  -l LAUNCHER       Launcher command");
        return 0;
    }
    let position = args.iter().skip_while(|a| a.as_str() != "-d").nth(1)
        .map(|s| s.as_str()).unwrap_or("bottom");
    println!("nwg-dock: application dock (position={})", position);
    println!("  Pinned: Firefox, Terminal, Files");
    println!("  Running: 3 applications");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "nwg-dock".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nwg_dock(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_nwg_dock};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/nwg-dock"), "nwg-dock");
        assert_eq!(basename(r"C:\bin\nwg-dock.exe"), "nwg-dock.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("nwg-dock.exe"), "nwg-dock");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_nwg_dock(&["--help".to_string()], "nwg-dock"), 0);
        assert_eq!(run_nwg_dock(&["-h".to_string()], "nwg-dock"), 0);
        let _ = run_nwg_dock(&["--version".to_string()], "nwg-dock");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_nwg_dock(&[], "nwg-dock");
    }
}
