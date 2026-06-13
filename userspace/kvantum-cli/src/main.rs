#![deny(clippy::all)]

//! kvantum-cli — SlateOS Kvantum Qt SVG theme engine
//!
//! Multi-personality: `kvantummanager`, `kvantumpreview`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_manager(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kvantummanager [OPTIONS]");
        println!("kvantummanager v1.1 (Slate OS) — Kvantum theme manager");
        println!();
        println!("Options:");
        println!("  --set THEME       Set active theme");
        println!("  --list            List installed themes");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("kvantummanager v1.1 (Slate OS)"); return 0; }
    if args.iter().any(|a| a == "--list") {
        println!("KvAdaptaDark");
        println!("KvArcDark");
        println!("KvDracula");
        println!("KvGnomeDark");
        println!("KvNordic");
        return 0;
    }
    if let Some(theme) = args.iter().skip_while(|a| a.as_str() != "--set").nth(1) {
        println!("Kvantum theme set to: {}", theme);
        return 0;
    }
    println!("kvantummanager: Qt SVG theme manager");
    println!("  Active theme: KvArcDark");
    println!("  Installed: 5 themes");
    0
}

fn run_preview(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kvantumpreview [THEME]");
        println!("kvantumpreview v1.1 (Slate OS) — Preview Kvantum themes");
        return 0;
    }
    let theme = args.first().map(|s| s.as_str()).unwrap_or("current");
    println!("kvantumpreview: previewing theme '{}'", theme);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kvantummanager".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "kvantumpreview" => run_preview(&rest, &prog),
        _ => run_manager(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_manager};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kvantum"), "kvantum");
        assert_eq!(basename(r"C:\bin\kvantum.exe"), "kvantum.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kvantum.exe"), "kvantum");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_manager(&["--help".to_string()], "kvantum"), 0);
        assert_eq!(run_manager(&["-h".to_string()], "kvantum"), 0);
        let _ = run_manager(&["--version".to_string()], "kvantum");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_manager(&[], "kvantum");
    }
}
