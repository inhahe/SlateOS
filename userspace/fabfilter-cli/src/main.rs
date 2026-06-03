#![deny(clippy::all)]

//! fabfilter-cli — OurOS FabFilter plug-in bundle
//!
//! Single personality: `fabfilter`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ff(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fabfilter [PLUGIN] [OPTIONS]");
        println!("FabFilter Pro Bundle (OurOS) — Premium mixing & mastering plug-ins");
        println!();
        println!("Plugins:");
        println!("  pro-q              Pro-Q 4 (EQ + dynamic, spectrum analyzer)");
        println!("  pro-c              Pro-C 2 (compressor)");
        println!("  pro-l              Pro-L 2 (true peak limiter)");
        println!("  pro-mb             Pro-MB (multiband dynamics)");
        println!("  pro-r              Pro-R 2 (reverb)");
        println!("  pro-ds             Pro-DS (de-esser)");
        println!("  pro-g              Pro-G (gate/expander)");
        println!("  saturn             Saturn 2 (saturation/distortion)");
        println!("  timeless           Timeless 3 (delay)");
        println!();
        println!("Options:");
        println!("  --load FILE            Load .ffp preset");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("FabFilter Pro Bundle (Pro-Q 4 v4.0.0, etc.) (OurOS)"); return 0; }
    println!("FabFilter Pro Bundle (OurOS)");
    println!("  Pro-Q 4: dynamic EQ, brickwall ±30 dB, transparent/natural/linear phase");
    println!("  Pro-C 2: 8 styles, look-ahead, side chain, ratio knee");
    println!("  Pro-L 2: 8 limiting algorithms, true-peak metering");
    println!("  Saturn 2: 28 saturation models, multiband, modulation");
    println!("  GUI: legendary smooth animation & precise mouse interaction");
    println!("  Plug-in formats: VST2, VST3, AU, AAX");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "fabfilter".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ff(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ff};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/fabfilter"), "fabfilter");
        assert_eq!(basename(r"C:\bin\fabfilter.exe"), "fabfilter.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("fabfilter.exe"), "fabfilter");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_ff(&["--help".to_string()], "fabfilter"), 0);
        assert_eq!(run_ff(&["-h".to_string()], "fabfilter"), 0);
        assert_eq!(run_ff(&["--version".to_string()], "fabfilter"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_ff(&[], "fabfilter"), 0);
    }
}
