#![deny(clippy::all)]

//! lxappearance-cli — SlateOS LXAppearance GTK theme switcher
//!
//! Single personality: `lxappearance`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lxappearance(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lxappearance [OPTIONS]");
        println!("lxappearance v0.6 (Slate OS) — GTK+ theme switcher");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Tabs: Widget, Color, Icon Theme, Mouse Cursor, Other");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("lxappearance v0.6 (Slate OS)"); return 0; }
    println!("lxappearance: GTK+ theme switcher");
    println!("  Widget Theme: Adwaita");
    println!("  Icon Theme: Papirus");
    println!("  Mouse Cursor: default");
    println!("  Toolbar Style: Icons and text");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lxappearance".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lxappearance(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lxappearance};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lxappearance"), "lxappearance");
        assert_eq!(basename(r"C:\bin\lxappearance.exe"), "lxappearance.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lxappearance.exe"), "lxappearance");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lxappearance(&["--help".to_string()], "lxappearance"), 0);
        assert_eq!(run_lxappearance(&["-h".to_string()], "lxappearance"), 0);
        let _ = run_lxappearance(&["--version".to_string()], "lxappearance");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lxappearance(&[], "lxappearance");
    }
}
