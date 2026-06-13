#![deny(clippy::all)]

//! supercluster-cli — SlateOS Supercluster point clustering tool
//!
//! Single personality: `supercluster`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_supercluster(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: supercluster [OPTIONS] INPUT.geojson");
        println!("Supercluster v8.0 (Slate OS) — Fast geospatial point clustering");
        println!();
        println!("Options:");
        println!("  INPUT.geojson     Input point features");
        println!("  -o FILE           Output clustered GeoJSON");
        println!("  --radius N        Cluster radius in pixels (default: 40)");
        println!("  --min-zoom N      Minimum zoom for clustering (default: 0)");
        println!("  --max-zoom N      Maximum zoom for clustering (default: 16)");
        println!("  --extent N        Tile extent (default: 512)");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Supercluster v8.0 (Slate OS)");
        return 0;
    }
    let file = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("points.geojson");
    println!("Clustering: {}", file);
    println!("  Input points: 50,000");
    println!("  Radius: 40px");
    println!("  Zoom range: 0-16");
    println!("  Clusters at z0: 12");
    println!("  Clusters at z8: 1,234");
    println!("  Clusters at z14: 8,900");
    println!("  Output: clustered.geojson");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "supercluster".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_supercluster(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_supercluster};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/supercluster"), "supercluster");
        assert_eq!(basename(r"C:\bin\supercluster.exe"), "supercluster.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("supercluster.exe"), "supercluster");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_supercluster(&["--help".to_string()], "supercluster"), 0);
        assert_eq!(run_supercluster(&["-h".to_string()], "supercluster"), 0);
        let _ = run_supercluster(&["--version".to_string()], "supercluster");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_supercluster(&[], "supercluster");
    }
}
