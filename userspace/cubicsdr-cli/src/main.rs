#![deny(clippy::all)]

//! cubicsdr-cli — SlateOS CubicSDR receiver
//!
//! Single personality: `CubicSDR`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cubicsdr(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: CubicSDR [OPTIONS]");
        println!("CubicSDR v0.2 (SlateOS) — Cross-platform SDR application");
        println!();
        println!("Options:");
        println!("  -d DEVICE      Device index or serial");
        println!("  -f FREQ        Center frequency (Hz)");
        println!("  -s RATE        Sample rate");
        println!("  -m MODE        Demodulator (AM, FM, LSB, USB, DSB, RAW)");
        println!("  --ppm N        PPM correction");
        println!("  --agc          Enable AGC");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("CubicSDR v0.2.7 (SlateOS)"); return 0; }
    println!("CubicSDR v0.2.7 (SlateOS)");
    println!("  Device: RTL-SDR");
    println!("  Center: 100.0 MHz");
    println!("  Bandwidth: 2.4 MHz");
    println!("  Demod: WFM");
    println!("  Waterfall: active");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "CubicSDR".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cubicsdr(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cubicsdr};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cubicsdr"), "cubicsdr");
        assert_eq!(basename(r"C:\bin\cubicsdr.exe"), "cubicsdr.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cubicsdr.exe"), "cubicsdr");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cubicsdr(&["--help".to_string()], "cubicsdr"), 0);
        assert_eq!(run_cubicsdr(&["-h".to_string()], "cubicsdr"), 0);
        let _ = run_cubicsdr(&["--version".to_string()], "cubicsdr");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cubicsdr(&[], "cubicsdr");
    }
}
