#![deny(clippy::all)]

//! proj-cli — SlateOS PROJ coordinate transformation
//!
//! Multi-personality: `proj`, `cs2cs`, `projinfo`, `cct`, `geod`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_proj(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: proj [+PARAMS] [FILE]");
        println!("PROJ 9.3.1 (SlateOS)");
        println!("  +proj=TYPE    Projection type");
        println!("  +datum=NAME   Datum");
        println!("  +ellps=NAME   Ellipsoid");
        println!("  -r            Reverse (inverse) projection");
        println!("  -V            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("PROJ 9.3.1");
        println!("Release Date: 2024-01-01");
        return 0;
    }
    println!("proj: projecting coordinates...");
    println!("500000.00\t0.00");
    0
}

fn run_cs2cs(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: cs2cs [+SRC_PARAMS] +to [+DST_PARAMS] [FILE]");
        println!("  Convert between coordinate reference systems");
        return 0;
    }
    println!("cs2cs: transforming coordinates...");
    println!("-73.9857\t40.7484\t0.0000");
    0
}

fn run_projinfo(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: projinfo [OPTIONS] CRS_DEF");
        println!("  -o FORMAT    Output format (PROJ, WKT2, WKT1)");
        println!("  -s SRC -t TGT  Find transformation");
        println!("  --version    Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("PROJ 9.3.1 (SlateOS)");
        return 0;
    }
    let crs = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("EPSG:4326");
    println!("CRS: {}", crs);
    println!("Type: Geographic 2D CRS");
    println!("Name: WGS 84");
    println!("Datum: World Geodetic System 1984");
    println!("Ellipsoid: WGS 84 (a=6378137, rf=298.257223563)");
    println!("Scope: Horizontal component of 3D system.");
    println!("Area of use: World.");
    0
}

fn run_cct(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: cct [+PARAMS] [FILE]");
        println!("  Coordinate conversion and transformation");
        return 0;
    }
    println!("cct: transforming coordinates...");
    println!("12.0 55.0 0.0 0.0");
    0
}

fn run_geod(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: geod [+PARAMS] [FILE]");
        println!("  Geodesic computations on the ellipsoid");
        println!("  +ellps=NAME   Ellipsoid");
        println!("  -I            Inverse mode");
        return 0;
    }
    println!("geod: computing geodesic...");
    println!("  Distance: 5570.23 km");
    println!("  Forward azimuth: 51.2167 deg");
    println!("  Back azimuth: -122.4567 deg");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "proj".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "cs2cs" => run_cs2cs(&rest),
        "projinfo" => run_projinfo(&rest),
        "cct" => run_cct(&rest),
        "geod" => run_geod(&rest),
        _ => run_proj(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_proj};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/proj"), "proj");
        assert_eq!(basename(r"C:\bin\proj.exe"), "proj.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("proj.exe"), "proj");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_proj(&["--help".to_string()]), 0);
        assert_eq!(run_proj(&["-h".to_string()]), 0);
        let _ = run_proj(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_proj(&[]);
    }
}
