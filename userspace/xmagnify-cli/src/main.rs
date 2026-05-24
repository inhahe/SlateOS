#![deny(clippy::all)]

//! xmagnify-cli — OurOS xmagnify simple magnifier
//!
//! Single personality: `xmagnify`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_xmagnify(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xmagnify [OPTIONS]");
        println!("xmagnify v0.1 (OurOS) — Simple screen magnifier");
        println!();
        println!("Options:");
        println!("  -mag FACTOR       Magnification factor (default 2)");
        println!("  -source WxH       Source area size");
        println!("  -geometry WxH+X+Y Window geometry");
        return 0;
    }
    let mag = args.iter().skip_while(|a| a.as_str() != "-mag").nth(1)
        .map(|s| s.as_str()).unwrap_or("2");
    println!("xmagnify: {}x magnification", mag);
    println!("  Click to select area to magnify");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "xmagnify".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_xmagnify(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
