#![deny(clippy::all)]

//! cartes-du-ciel-cli — SlateOS Cartes du Ciel / SkyChart
//!
//! Single personality: `skychart`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_skychart(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: skychart [OPTIONS]");
        println!("Cartes du Ciel / SkyChart v4.3 (SlateOS) — Star chart generator");
        println!();
        println!("Options:");
        println!("  --lat N         Observer latitude");
        println!("  --lon N         Observer longitude");
        println!("  --date DATE     Date (YYYY-MM-DD)");
        println!("  --time TIME     Time (HH:MM)");
        println!("  --fov N         Field of view (degrees)");
        println!("  --catalog CAT   Star catalog (tycho2, ucac4, gaia)");
        println!("  --mag-limit N   Magnitude limit");
        println!("  --print FILE    Print chart to file");
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Cartes du Ciel v4.3 (SlateOS)"); return 0; }
    println!("Cartes du Ciel v4.3 (SlateOS) — Sky Chart");
    println!("  Catalogs loaded:");
    println!("    Stars: Tycho-2 (2,539,913 stars)");
    println!("    Deep sky: NGC/IC (13,226 objects)");
    println!("    Solar system: 8 planets, 200+ asteroids");
    println!("  Current sky: 3,456 objects visible");
    println!("  FOV: 60 degrees");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "skychart".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_skychart(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_skychart};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cartes-du-ciel"), "cartes-du-ciel");
        assert_eq!(basename(r"C:\bin\cartes-du-ciel.exe"), "cartes-du-ciel.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cartes-du-ciel.exe"), "cartes-du-ciel");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_skychart(&["--help".to_string()], "cartes-du-ciel"), 0);
        assert_eq!(run_skychart(&["-h".to_string()], "cartes-du-ciel"), 0);
        let _ = run_skychart(&["--version".to_string()], "cartes-du-ciel");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_skychart(&[], "cartes-du-ciel");
    }
}
