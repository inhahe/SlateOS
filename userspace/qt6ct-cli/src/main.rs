#![deny(clippy::all)]

//! qt6ct-cli — OurOS qt6ct Qt6 configuration tool
//!
//! Single personality: `qt6ct`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_qt6ct(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: qt6ct [OPTIONS]");
        println!("qt6ct v0.9 (OurOS) — Qt6 configuration tool");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Configure Qt6 appearance without KDE Plasma.");
        println!("Tabs: Appearance, Fonts, Icon Theme, Interface, Style Sheets");
        println!();
        println!("Set QT_QPA_PLATFORMTHEME=qt6ct to enable.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("qt6ct v0.9 (OurOS)"); return 0; }
    println!("qt6ct: Qt6 configuration");
    println!("  Style: Fusion");
    println!("  Color Scheme: darker");
    println!("  Icon Theme: Papirus");
    println!("  Font: Sans Serif, 10pt");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "qt6ct".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_qt6ct(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_qt6ct};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/qt6ct"), "qt6ct");
        assert_eq!(basename(r"C:\bin\qt6ct.exe"), "qt6ct.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("qt6ct.exe"), "qt6ct");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_qt6ct(&["--help".to_string()], "qt6ct"), 0);
        assert_eq!(run_qt6ct(&["-h".to_string()], "qt6ct"), 0);
        let _ = run_qt6ct(&["--version".to_string()], "qt6ct");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_qt6ct(&[], "qt6ct");
    }
}
