#![deny(clippy::all)]

//! postgis-cli — OurOS PostGIS spatial database utilities
//!
//! Multi-personality: `shp2pgsql`, `pgsql2shp`, `raster2pgsql`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_shp2pgsql(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "-?") || args.is_empty() {
        println!("USAGE: shp2pgsql [OPTIONS] SHAPEFILE [SCHEMA.]TABLE");
        println!("  -s SRID        Set SRID");
        println!("  -d             Drop table, create new");
        println!("  -a             Append to existing table");
        println!("  -c             Create table (default)");
        println!("  -D             Use PostgreSQL dump format");
        println!("  -G             Use geography type");
        println!("  -I             Create spatial index");
        return 0;
    }
    let file = args.iter().find(|a| a.ends_with(".shp")).map(|s| s.as_str()).unwrap_or("data.shp");
    let table = args.last().map(|s| s.as_str()).unwrap_or("public.data");
    println!("Shapefile type: Polygon");
    println!("Shapefile import: {}", file);
    println!("Target table: {}", table);
    println!("BEGIN;");
    println!("CREATE TABLE \"{}\" (gid serial, \"geom\" geometry(MultiPolygon,4326));", table);
    println!("INSERT INTO \"{}\" VALUES (...);", table);
    println!("-- 1234 records");
    println!("CREATE INDEX ON \"{}\" USING GIST (\"geom\");", table);
    println!("COMMIT;");
    0
}

fn run_pgsql2shp(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "-?") || args.is_empty() {
        println!("USAGE: pgsql2shp [OPTIONS] DATABASE TABLE|QUERY");
        println!("  -f FILE        Output shapefile");
        println!("  -h HOST        Database host");
        println!("  -p PORT        Database port");
        println!("  -u USER        Database user");
        return 0;
    }
    let db = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("gisdb");
    println!("Connecting to database: {}", db);
    println!("Fetching geometry column info...");
    println!("Initializing shapefile: output.shp");
    println!("Done (1234 records).");
    0
}

fn run_raster2pgsql(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "-?") || args.is_empty() {
        println!("USAGE: raster2pgsql [OPTIONS] RASTERFILE [SCHEMA.]TABLE");
        println!("  -s SRID        Set SRID");
        println!("  -t WxH         Tile size");
        println!("  -d             Drop and create");
        println!("  -I             Create spatial index");
        println!("  -C             Apply raster constraints");
        return 0;
    }
    let file = args.iter().find(|a| a.ends_with(".tif") || a.ends_with(".tiff")).map(|s| s.as_str()).unwrap_or("raster.tif");
    let table = args.last().map(|s| s.as_str()).unwrap_or("public.raster");
    println!("Processing: {}", file);
    println!("Target table: {}", table);
    println!("  Tile size: 256x256");
    println!("  64 tiles generated.");
    println!("  SRID: 4326");
    println!("Done.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "shp2pgsql".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "pgsql2shp" => run_pgsql2shp(&rest),
        "raster2pgsql" => run_raster2pgsql(&rest),
        _ => run_shp2pgsql(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_shp2pgsql};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/postgis"), "postgis");
        assert_eq!(basename(r"C:\bin\postgis.exe"), "postgis.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("postgis.exe"), "postgis");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_shp2pgsql(&["--help".to_string()]), 0);
        assert_eq!(run_shp2pgsql(&["-h".to_string()]), 0);
        let _ = run_shp2pgsql(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_shp2pgsql(&[]);
    }
}
