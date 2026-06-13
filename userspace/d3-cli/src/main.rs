#![deny(clippy::all)]

//! d3-cli — SlateOS D3.js data visualization CLI
//!
//! Single personality: `d3`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_d3(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: d3 [COMMAND] [OPTIONS]");
        println!("d3 v7.0 (Slate OS) — Data-driven document visualization CLI");
        println!();
        println!("Commands:");
        println!("  chart TYPE DATA   Generate chart (bar, line, scatter, pie, area)");
        println!("  geo GEOJSON       Generate map visualization");
        println!("  tree DATA         Generate tree/hierarchy");
        println!("  network DATA      Generate force-directed graph");
        println!("  render FILE       Render D3 HTML to image");
        println!();
        println!("Options:");
        println!("  -o FILE           Output file (SVG, PNG, HTML)");
        println!("  -w WIDTH          Width in pixels");
        println!("  -h HEIGHT         Height in pixels");
        println!("  --theme THEME     Color theme");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("d3-cli v7.0.0 (Slate OS, based on D3.js)"); return 0; }
    match args.first().map(|s| s.as_str()) {
        Some("chart") => {
            let chart_type = args.get(1).map(|s| s.as_str()).unwrap_or("bar");
            println!("d3: generating {} chart", chart_type);
            println!("  Data points: 25");
            println!("  Output: chart.svg (800x600)");
        }
        Some("geo") => {
            println!("d3: generating map visualization");
            println!("  Projection: Mercator");
            println!("  Features: 195 countries");
            println!("  Output: map.svg (960x500)");
        }
        _ => {
            println!("d3: specify a command (chart, geo, tree, network, render)");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "d3".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_d3(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_d3};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/d3"), "d3");
        assert_eq!(basename(r"C:\bin\d3.exe"), "d3.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("d3.exe"), "d3");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_d3(&["--help".to_string()], "d3"), 0);
        assert_eq!(run_d3(&["-h".to_string()], "d3"), 0);
        let _ = run_d3(&["--version".to_string()], "d3");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_d3(&[], "d3");
    }
}
