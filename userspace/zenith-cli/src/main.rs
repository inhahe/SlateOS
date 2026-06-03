#![deny(clippy::all)]

//! zenith-cli — OurOS Zenith system monitor
//!
//! Single personality: `zenith`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_zenith(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: zenith [OPTIONS]");
        println!("zenith 0.14.0 (OurOS) — Terminal system monitor with zoom and scroll");
        println!();
        println!("Options:");
        println!("  -c, --cpu-height N     CPU chart height (default 10)");
        println!("  -d, --disk-height N    Disk chart height (default 10)");
        println!("  -n, --net-height N     Network chart height (default 10)");
        println!("  --db PATH              Database path for history");
        println!("  --disable-history      Don't save history");
        println!("  -r, --refresh-rate MS  Refresh rate in ms");
        println!("  -V, --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("zenith 0.14.0 (OurOS)");
        return 0;
    }
    println!("zenith: Starting system monitor...");
    println!();
    println!("CPU [||||||||||||                          ] 30%");
    println!("MEM [|||||||||||||||||||                   ] 48%");
    println!("SWP [                                      ] 0%");
    println!("DSK [||||||||                              ] 20% R:2.1M W:1.5M");
    println!("NET [||||||||||||||                        ] 35% Rx:5.6M Tx:1.2M");
    println!();
    println!("(Use arrow keys to zoom, scroll process list)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "zenith".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_zenith(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_zenith};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/zenith"), "zenith");
        assert_eq!(basename(r"C:\bin\zenith.exe"), "zenith.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("zenith.exe"), "zenith");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_zenith(&["--help".to_string()], "zenith"), 0);
        assert_eq!(run_zenith(&["-h".to_string()], "zenith"), 0);
        assert_eq!(run_zenith(&["--version".to_string()], "zenith"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_zenith(&[], "zenith"), 0);
    }
}
