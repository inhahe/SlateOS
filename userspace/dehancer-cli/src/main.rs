#![deny(clippy::all)]

//! dehancer-cli — SlateOS Dehancer film emulation plug-in
//!
//! Single personality: `dehancer`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dh(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dehancer [OPTIONS] [FILE]");
        println!("Dehancer Pro 7 (SlateOS) — Premium film emulation");
        println!();
        println!("Options:");
        println!("  --film STOCK           Choose film (Kodak Vision3, Ektachrome, Fuji Eterna, etc.)");
        println!("  --print BLOOM          Print bloom & halation");
        println!("  --bloom INTENSITY      Optical bloom");
        println!("  --grain SIZE LEVEL     Real grain (size + intensity)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Dehancer Pro 7.5.0 (SlateOS)"); return 0; }
    println!("Dehancer Pro 7.5.0 (SlateOS)");
    println!("  Film profiles: 100+ negative/print/reversal stocks");
    println!("  Modules: Film, Print, Bloom, Halation, Grain, Gate Weave");
    println!("  Color science: Custom film LUT pipeline with print emulation");
    println!("  Plug-in formats: OFX (Resolve), AE, Premiere, FCP X, photo standalone");
    println!("  License: subscription / perpetual");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dehancer".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_dh(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dh};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/dehancer"), "dehancer");
        assert_eq!(basename(r"C:\bin\dehancer.exe"), "dehancer.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("dehancer.exe"), "dehancer");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dh(&["--help".to_string()], "dehancer"), 0);
        assert_eq!(run_dh(&["-h".to_string()], "dehancer"), 0);
        let _ = run_dh(&["--version".to_string()], "dehancer");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dh(&[], "dehancer");
    }
}
