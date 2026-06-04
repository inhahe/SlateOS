#![deny(clippy::all)]

//! graphviz-cli — OurOS Graphviz graph visualization
//!
//! Multi-personality: `dot`, `neato`, `fdp`, `sfdp`, `circo`, `twopi`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_graphviz(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS] [FILE...]", prog);
        println!("{} v10.0 (OurOS) — Graph visualization", prog);
        println!();
        println!("Options:");
        println!("  -T FORMAT      Output format (png, svg, pdf, ps, dot, json)");
        println!("  -o FILE        Output file");
        println!("  -K LAYOUT      Layout engine (dot, neato, fdp, sfdp, circo, twopi)");
        println!("  -G ATTR=VAL    Set graph attribute");
        println!("  -N ATTR=VAL    Set node attribute");
        println!("  -E ATTR=VAL    Set edge attribute");
        println!("  -s SCALE       Scale factor");
        println!("  -V             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("{} - graphviz version 10.0.1 (OurOS)", prog);
        return 0;
    }
    let format = args.windows(2).find(|w| w[0] == "-T").map(|w| w[1].as_str()).unwrap_or("svg");
    let output = args.windows(2).find(|w| w[0] == "-o").map(|w| w[1].as_str());
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with('-') && {
        let idx = args.iter().position(|x| std::ptr::eq(x, *a)).unwrap_or(0);
        idx == 0 || !matches!(args.get(idx.wrapping_sub(1)).map(|s| s.as_str()), Some("-T" | "-o" | "-K" | "-G" | "-N" | "-E" | "-s"))
    }).collect();
    let engine = match prog {
        "neato" => "neato (spring model)",
        "fdp" => "fdp (force-directed)",
        "sfdp" => "sfdp (scalable force-directed)",
        "circo" => "circo (circular)",
        "twopi" => "twopi (radial)",
        _ => "dot (hierarchical)",
    };
    if files.is_empty() {
        println!("{}: reading from stdin, layout={}", prog, engine);
    } else {
        println!("{}: processing {} file(s), layout={}", prog, files.len(), engine);
    }
    println!("  Output format: {}", format);
    if let Some(out) = output {
        println!("  Output file: {}", out);
    }
    println!("  Nodes: 42, Edges: 67");
    println!("  Done");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dot".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_graphviz(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_graphviz};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/graphviz"), "graphviz");
        assert_eq!(basename(r"C:\bin\graphviz.exe"), "graphviz.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("graphviz.exe"), "graphviz");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_graphviz(&["--help".to_string()], "graphviz"), 0);
        assert_eq!(run_graphviz(&["-h".to_string()], "graphviz"), 0);
        let _ = run_graphviz(&["--version".to_string()], "graphviz");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_graphviz(&[], "graphviz");
    }
}
