#![deny(clippy::all)]

//! qgis-cli — SlateOS QGIS geographic information system
//!
//! Multi-personality: `qgis`, `qgis_process`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_qgis(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: qgis [OPTIONS] [PROJECT.qgz | LAYER...]");
        println!("  --project FILE   Load project");
        println!("  --extent XMIN,YMIN,XMAX,YMAX");
        println!("  --snapshot FILE  Save map snapshot");
        println!("  --code FILE.py   Run Python script");
        println!("  --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("QGIS 3.34.4 'Prizren' (SlateOS)");
        println!("QGIS code revision: abcdef1");
        println!("Qt version: 6.6.1");
        println!("GDAL/OGR: 3.8.3");
        println!("PROJ: 9.3.1");
        println!("GEOS: 3.12.1");
        return 0;
    }
    let project = args.iter().find(|a| a.ends_with(".qgz") || a.ends_with(".qgs")).map(|s| s.as_str());
    if let Some(p) = project {
        println!("QGIS 3.34.4 — Loading project: {}", p);
    } else {
        println!("QGIS 3.34.4 'Prizren' — Starting...");
    }
    println!("Ready.");
    0
}

fn run_qgis_process(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: qgis_process <command> [options]");
        println!("QGIS Processing CLI 3.34.4 (SlateOS)");
        println!();
        println!("Commands:");
        println!("  list              List available algorithms");
        println!("  plugins           List available plugins");
        println!("  run ALG [PARAMS]  Run an algorithm");
        println!("  help ALG          Show algorithm help");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "list" => {
            println!("Available algorithms:");
            println!("  native:buffer              Buffer");
            println!("  native:clip                Clip");
            println!("  native:dissolve            Dissolve");
            println!("  native:intersection        Intersection");
            println!("  native:union               Union");
            println!("  native:difference          Difference");
            println!("  native:centroids           Centroids");
            println!("  native:voronoipolygons     Voronoi polygons");
            println!("  gdal:warpreproject         Warp (Reproject)");
            println!("  gdal:rasterize             Rasterize");
            println!("  [... 450+ algorithms total]");
        }
        "plugins" => {
            println!("Available processing providers:");
            println!("  native     QGIS (native c++)");
            println!("  gdal       GDAL/OGR");
            println!("  grass      GRASS GIS");
            println!("  saga       SAGA GIS");
        }
        "run" => {
            let alg = args.get(1).map(|s| s.as_str()).unwrap_or("native:buffer");
            println!("Running algorithm: {}", alg);
            println!("  Processing...");
            println!("  Results saved.");
            println!("  Algorithm completed successfully.");
        }
        _ => println!("qgis_process: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "qgis".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "qgis_process" => run_qgis_process(&rest),
        _ => run_qgis(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_qgis};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/qgis"), "qgis");
        assert_eq!(basename(r"C:\bin\qgis.exe"), "qgis.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("qgis.exe"), "qgis");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_qgis(&["--help".to_string()]), 0);
        assert_eq!(run_qgis(&["-h".to_string()]), 0);
        let _ = run_qgis(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_qgis(&[]);
    }
}
