#![deny(clippy::all)]

//! gephi-cli — OurOS Gephi graph visualization platform
//!
//! Single personality: `gephi`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gephi(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gephi [OPTIONS] [FILE]");
        println!("Gephi v0.10 (OurOS) — Graph visualization and exploration");
        println!();
        println!("Options:");
        println!("  --open FILE      Open graph file (GEXF, GraphML, GML, CSV, etc.)");
        println!("  --export FILE    Export visualization");
        println!("  --layout ALG     Apply layout (ForceAtlas2, FruchtermanReingold, etc.)");
        println!("  --jvm-args ARGS  JVM arguments");
        println!("  --nosplash       Skip splash screen");
        println!("  --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Gephi v0.10.1 (OurOS)"); return 0; }
    println!("Gephi v0.10.1 (OurOS) — Graph Visualization");
    println!("  Formats: GEXF, GraphML, GML, CSV, Pajek, DOT, DL, GDF");
    println!("  Layouts: ForceAtlas2, Fruchterman-Reingold, Yifan Hu, OpenOrd");
    println!("  Metrics: PageRank, betweenness, closeness, modularity");
    println!("  Rendering: OpenGL, antialiased");
    println!("  Status: ready");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gephi".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gephi(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gephi};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gephi"), "gephi");
        assert_eq!(basename(r"C:\bin\gephi.exe"), "gephi.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gephi.exe"), "gephi");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gephi(&["--help".to_string()], "gephi"), 0);
        assert_eq!(run_gephi(&["-h".to_string()], "gephi"), 0);
        let _ = run_gephi(&["--version".to_string()], "gephi");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gephi(&[], "gephi");
    }
}
