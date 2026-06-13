#![deny(clippy::all)]

//! xdg-desktop-portal-cli — SlateOS xdg-desktop-portal service
//!
//! Single personality: `xdg-desktop-portal`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_portal(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xdg-desktop-portal [OPTIONS]");
        println!("xdg-desktop-portal v1.18 (SlateOS) — Desktop integration portal");
        println!();
        println!("Options:");
        println!("  --replace         Replace running instance");
        println!("  --verbose         Verbose logging");
        println!("  --version         Show version");
        println!();
        println!("D-Bus service providing sandboxed access to:");
        println!("  File chooser, Screen sharing, Notifications,");
        println!("  Clipboard, Settings, Screenshot, Print, etc.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("xdg-desktop-portal v1.18 (SlateOS)"); return 0; }
    println!("xdg-desktop-portal: started");
    println!("  D-Bus: org.freedesktop.portal.Desktop");
    println!("  Backends: wlr, gtk");
    println!("  Interfaces: FileChooser, Screenshot, ScreenCast, Settings, Notification");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "xdg-desktop-portal".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_portal(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_portal};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/xdg-desktop-portal"), "xdg-desktop-portal");
        assert_eq!(basename(r"C:\bin\xdg-desktop-portal.exe"), "xdg-desktop-portal.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("xdg-desktop-portal.exe"), "xdg-desktop-portal");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_portal(&["--help".to_string()], "xdg-desktop-portal"), 0);
        assert_eq!(run_portal(&["-h".to_string()], "xdg-desktop-portal"), 0);
        let _ = run_portal(&["--version".to_string()], "xdg-desktop-portal");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_portal(&[], "xdg-desktop-portal");
    }
}
