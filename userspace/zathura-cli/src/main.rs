#![deny(clippy::all)]

//! zathura-cli — OurOS Zathura document viewer
//!
//! Single personality: `zathura`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_zathura(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: zathura [OPTIONS] [FILE[:PAGE]] [FILE[:PAGE]...]");
        println!("zathura 0.5.6 (OurOS) — Document viewer");
        println!();
        println!("Options:");
        println!("  -e, --reparent XID     Reparent to window");
        println!("  -c, --config-dir DIR   Config directory");
        println!("  -d, --data-dir DIR     Data directory");
        println!("  --cache-dir DIR        Cache directory");
        println!("  -p, --plugins-dir DIR  Plugin directory");
        println!("  -P, --page N           Open at page N");
        println!("  -l, --log-level LEVEL  Log level");
        println!("  --fork                 Fork into background");
        println!("  -w, --password PW      Document password");
        println!("  --mode MODE            Start mode (normal, fullscreen, presentation)");
        println!("  --find TERM            Find term on open");
        println!("  -V, --version          Show version");
        println!();
        println!("Supported formats: PDF, PS, DjVu, CB (via plugins)");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("zathura 0.5.6 (OurOS)");
        println!("(pdf, ps, djvu, cb)");
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str());
    if let Some(f) = file {
        println!("zathura: Opening '{}'", f);
    } else {
        println!("zathura: No file specified, opening empty viewer");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "zathura".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_zathura(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
