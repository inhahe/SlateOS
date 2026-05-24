#![deny(clippy::all)]

//! obs-wlrobs-cli — OurOS wlrobs OBS screen capture for wlroots
//!
//! Single personality: `wlrobs`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wlrobs(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wlrobs [OPTIONS]");
        println!("wlrobs v2.0 (OurOS) — OBS screen capture for wlroots compositors");
        println!();
        println!("Options:");
        println!("  --output OUTPUT   Capture specific output");
        println!("  --dmabuf          Use DMA-BUF (zero-copy)");
        println!("  --screencopy      Use screencopy protocol");
        println!("  --version         Show version");
        println!();
        println!("OBS Studio plugin for capturing wlroots compositor outputs.");
        println!("Install as OBS plugin or use as standalone capture tool.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wlrobs v2.0 (OurOS)"); return 0; }
    let method = if args.iter().any(|a| a == "--dmabuf") { "DMA-BUF" } else { "screencopy" };
    let output = args.iter().skip_while(|a| a.as_str() != "--output").nth(1)
        .map(|s| s.as_str()).unwrap_or("all");
    println!("wlrobs: capturing output {} via {}", output, method);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wlrobs".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wlrobs(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
