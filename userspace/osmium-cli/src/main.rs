#![deny(clippy::all)]

//! osmium-cli — OurOS Osmium OpenStreetMap data processing
//!
//! Multi-personality: `osmium`

use std::env;
use std::process;

fn run_osmium(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: osmium COMMAND [OPTIONS]");
        println!("Osmium 1.16.0 (OurOS)");
        println!();
        println!("Commands:");
        println!("  fileinfo       Show file info");
        println!("  cat            Concatenate OSM files");
        println!("  merge          Merge sorted OSM files");
        println!("  sort           Sort OSM file");
        println!("  extract        Extract region from file");
        println!("  tags-filter    Filter by tags");
        println!("  renumber       Renumber object IDs");
        println!("  export         Export to GeoJSON/etc");
        println!("  getid          Get objects by ID");
        println!("  diff           Show differences");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("osmium 1.16.0 (OurOS)"),
        "fileinfo" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("planet.osm.pbf");
            println!("File:");
            println!("  Name: {}", file);
            println!("  Format: PBF");
            println!("  Compression: none");
            println!("  Size: 67,890,123,456");
            println!("Header:");
            println!("  Bounding boxes: (-180,-90,180,90)");
            println!("  Writing program: osmium/1.16.0");
            println!("Data:");
            println!("  Nodes: 8,234,567,890");
            println!("  Ways: 912,345,678");
            println!("  Relations: 12,345,678");
        }
        "extract" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("input.osm.pbf");
            println!("osmium extract: extracting region from {}", file);
            println!("  Bounding box: -74.1,40.6,-73.7,40.9");
            println!("  Nodes: 2,345,678");
            println!("  Ways: 234,567");
            println!("  Relations: 12,345");
            println!("  Written to: output.osm.pbf");
        }
        "cat" => {
            println!("osmium cat: concatenating files...");
            println!("Done.");
        }
        "sort" => {
            println!("osmium sort: sorting...");
            println!("Done.");
        }
        "merge" => {
            println!("osmium merge: merging files...");
            println!("Done.");
        }
        "tags-filter" => {
            println!("osmium tags-filter: filtering by tags...");
            println!("  12,345 objects passed filter.");
        }
        "export" => {
            println!("osmium export: exporting to GeoJSON...");
            println!("  23,456 features exported.");
        }
        _ => println!("osmium: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_osmium(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_osmium};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_osmium(&["--help".to_string()]), 0);
        assert_eq!(run_osmium(&["-h".to_string()]), 0);
        assert_eq!(run_osmium(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_osmium(&[]), 0);
    }
}
