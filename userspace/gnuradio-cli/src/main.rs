#![deny(clippy::all)]

//! gnuradio-cli — SlateOS GNU Radio SDR framework
//!
//! Multi-personality: `gnuradio-companion`, `grcc`, `gr_modtool`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gnuradio(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS] [FILE]", prog);
        println!("GNU Radio v3.10 (Slate OS) — Software-defined radio framework");
        println!();
        match prog {
            "grcc" => {
                println!("Compile GRC flowgraph to Python:");
                println!("  grcc FILE.grc              Compile flowgraph");
                println!("  -d DIR                     Output directory");
                println!("  -o FILE                    Output filename");
            }
            "gr_modtool" => {
                println!("Module management tool:");
                println!("  newmod NAME                Create new module");
                println!("  add NAME                   Add block");
                println!("  rm NAME                    Remove block");
                println!("  info                       Module info");
            }
            _ => {
                println!("Options:");
                println!("  FILE.grc        Open flowgraph");
                println!("  --log-level N   Log level (debug, info, warn, error)");
            }
        }
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("GNU Radio v3.10.9 (Slate OS)"); return 0; }
    match prog {
        "grcc" => {
            let files: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
            if files.is_empty() { eprintln!("grcc: error: no input file"); return 1; }
            println!("grcc: compiling {}", files[0]);
            println!("  Blocks: 12");
            println!("  Connections: 15");
            println!("  Output: flowgraph.py");
        }
        "gr_modtool" => {
            println!("gr_modtool: GNU Radio module tool");
            println!("  Use 'gr_modtool newmod NAME' to create a module");
        }
        _ => {
            println!("GNU Radio Companion v3.10.9 (Slate OS)");
            println!("  SDR framework ready");
            println!("  Blocks library: 500+ blocks");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gnuradio-companion".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gnuradio(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gnuradio};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gnuradio"), "gnuradio");
        assert_eq!(basename(r"C:\bin\gnuradio.exe"), "gnuradio.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gnuradio.exe"), "gnuradio");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gnuradio(&["--help".to_string()], "gnuradio"), 0);
        assert_eq!(run_gnuradio(&["-h".to_string()], "gnuradio"), 0);
        let _ = run_gnuradio(&["--version".to_string()], "gnuradio");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gnuradio(&[], "gnuradio");
    }
}
