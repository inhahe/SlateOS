#![deny(clippy::all)]

//! kmag-cli — SlateOS KMag screen magnifier
//!
//! Single personality: `kmag`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kmag(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kmag [OPTIONS]");
        println!("kmag v24.05 (SlateOS) — KDE screen magnifier");
        println!();
        println!("Options:");
        println!("  --zoom LEVEL      Zoom level (default 2)");
        println!("  --mode MODE       follow-mouse, selection-window, whole-screen");
        println!("  --refresh RATE    Refresh interval (ms)");
        println!("  --rotation DEG    Rotation (0, 90, 180, 270)");
        println!("  --invert          Invert colors");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("kmag v24.05 (SlateOS)"); return 0; }
    let mode = args.iter().skip_while(|a| a.as_str() != "--mode").nth(1)
        .map(|s| s.as_str()).unwrap_or("follow-mouse");
    let zoom = args.iter().skip_while(|a| a.as_str() != "--zoom").nth(1)
        .map(|s| s.as_str()).unwrap_or("2");
    println!("kmag: mode={}, zoom={}x", mode, zoom);
    if args.iter().any(|a| a == "--invert") {
        println!("  Color inversion enabled");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kmag".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kmag(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_kmag};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kmag"), "kmag");
        assert_eq!(basename(r"C:\bin\kmag.exe"), "kmag.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kmag.exe"), "kmag");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_kmag(&["--help".to_string()], "kmag"), 0);
        assert_eq!(run_kmag(&["-h".to_string()], "kmag"), 0);
        let _ = run_kmag(&["--version".to_string()], "kmag");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_kmag(&[], "kmag");
    }
}
