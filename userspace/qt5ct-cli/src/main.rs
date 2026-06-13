#![deny(clippy::all)]

//! qt5ct-cli — Slate OS qt5ct Qt5 configuration tool
//!
//! Single personality: `qt5ct`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_qt5ct(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: qt5ct [OPTIONS]");
        println!("qt5ct v1.8 (Slate OS) — Qt5 configuration tool");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Configure Qt5 appearance without KDE Plasma.");
        println!("Tabs: Appearance, Fonts, Icon Theme, Interface, Style Sheets");
        println!();
        println!("Set QT_QPA_PLATFORMTHEME=qt5ct to enable.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("qt5ct v1.8 (Slate OS)"); return 0; }
    println!("qt5ct: Qt5 configuration");
    println!("  Style: Fusion");
    println!("  Color Scheme: darker");
    println!("  Icon Theme: Papirus");
    println!("  Font: Sans Serif, 10pt");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "qt5ct".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_qt5ct(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_qt5ct};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/qt5ct"), "qt5ct");
        assert_eq!(basename(r"C:\bin\qt5ct.exe"), "qt5ct.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("qt5ct.exe"), "qt5ct");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_qt5ct(&["--help".to_string()], "qt5ct"), 0);
        assert_eq!(run_qt5ct(&["-h".to_string()], "qt5ct"), 0);
        let _ = run_qt5ct(&["--version".to_string()], "qt5ct");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_qt5ct(&[], "qt5ct");
    }
}
