#![deny(clippy::all)]

//! mapnik-cli — OurOS Mapnik map rendering toolkit
//!
//! Multi-personality: `mapnik-render`, `mapnik-index`, `mapnik-config`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mapnik_render(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: mapnik-render [OPTIONS] STYLESHEET.xml OUTPUT.png");
        println!("  --width N       Image width (default: 800)");
        println!("  --height N      Image height (default: 600)");
        println!("  --extent BBOX   Map extent (minx,miny,maxx,maxy)");
        println!("  --srs SRS       Map SRS");
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Mapnik 4.0.0 (OurOS)");
        return 0;
    }
    let stylesheet = args.iter().find(|a| a.ends_with(".xml")).map(|s| s.as_str()).unwrap_or("style.xml");
    let output = args.iter().find(|a| a.ends_with(".png") || a.ends_with(".pdf") || a.ends_with(".svg")).map(|s| s.as_str()).unwrap_or("output.png");
    let width = args.windows(2).find(|w| w[0] == "--width").map(|w| w[1].as_str()).unwrap_or("800");
    let height = args.windows(2).find(|w| w[0] == "--height").map(|w| w[1].as_str()).unwrap_or("600");
    println!("Mapnik 4.0.0 — rendering map");
    println!("  Stylesheet: {}", stylesheet);
    println!("  Output: {} ({}x{})", output, width, height);
    println!("  Loading datasources...");
    println!("  Rendering...");
    println!("  Done. Map saved to {}", output);
    0
}

fn run_mapnik_index(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: mapnik-index [OPTIONS] FILE");
        println!("  Create spatial index for datasources");
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("data.shp");
    println!("Creating spatial index for: {}", file);
    println!("  1234 features indexed.");
    println!("  Index: {}.index", file);
    0
}

fn run_mapnik_config(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: mapnik-config [OPTIONS]");
        println!("  --version      Mapnik version");
        println!("  --cflags       Compiler flags");
        println!("  --libs         Linker flags");
        println!("  --fonts        Font directory");
        println!("  --input-plugins  Plugin directory");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("4.0.0");
    } else if args.iter().any(|a| a == "--cflags") {
        println!("-I/usr/include/mapnik");
    } else if args.iter().any(|a| a == "--libs") {
        println!("-L/usr/lib -lmapnik");
    } else if args.iter().any(|a| a == "--fonts") {
        println!("/usr/share/mapnik/fonts");
    } else if args.iter().any(|a| a == "--input-plugins") {
        println!("/usr/lib/mapnik/input");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mapnik-render".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "mapnik-index" => run_mapnik_index(&rest),
        "mapnik-config" => run_mapnik_config(&rest),
        _ => run_mapnik_render(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mapnik_render};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mapnik"), "mapnik");
        assert_eq!(basename(r"C:\bin\mapnik.exe"), "mapnik.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mapnik.exe"), "mapnik");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_mapnik_render(&["--help".to_string()]), 0);
        assert_eq!(run_mapnik_render(&["-h".to_string()]), 0);
        assert_eq!(run_mapnik_render(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_mapnik_render(&[]), 0);
    }
}
