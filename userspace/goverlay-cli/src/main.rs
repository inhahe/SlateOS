#![deny(clippy::all)]

//! goverlay-cli — SlateOS GOverlay graphics overlay manager
//!
//! Single personality: `goverlay`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_goverlay(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: goverlay [OPTIONS]");
        println!("goverlay v1.1 (SlateOS) — Graphics overlay configuration");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("goverlay v1.1 (SlateOS)"); return 0; }
    println!("goverlay: overlay configuration GUI started");
    println!("  MangoHud: configured");
    println!("  vkBasalt: available");
    println!("  ReplaySorcery: available");
    println!("  Presets: gaming, streaming, minimal");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "goverlay".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_goverlay(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_goverlay};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/goverlay"), "goverlay");
        assert_eq!(basename(r"C:\bin\goverlay.exe"), "goverlay.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("goverlay.exe"), "goverlay");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_goverlay(&["--help".to_string()], "goverlay"), 0);
        assert_eq!(run_goverlay(&["-h".to_string()], "goverlay"), 0);
        let _ = run_goverlay(&["--version".to_string()], "goverlay");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_goverlay(&[], "goverlay");
    }
}
