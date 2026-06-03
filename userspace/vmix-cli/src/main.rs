#![deny(clippy::all)]

//! vmix-cli — OurOS vMix professional video switcher
//!
//! Single personality: `vmix`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_vmix(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: vmix [OPTIONS] [PRESET]");
        println!("vMix Pro 27 (OurOS) — Live video mixing & production");
        println!();
        println!("Options:");
        println!("  --load FILE            Load preset");
        println!("  --input N URL          Add input N");
        println!("  --titler TEMPLATE      Open Title Designer with template");
        println!("  --record FILE          Start recording to file");
        println!("  --stream KEY           Start stream with key");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("vMix Pro 27.0.0.74 (OurOS)"); return 0; }
    println!("vMix Pro 27.0.0.74 (OurOS)");
    println!("  Editions: Basic HD, HD, 4K, Pro, Max (4K Pro features + more)");
    println!("  Inputs: 4K/HD, NDI, SRT, NewBlue FX, virtual sets, instant replay");
    println!("  Outputs: Up to 8 simultaneous streams, 4K recording");
    println!("  Built-in: Switcher, Recorder, Titler, Audio Mixer, Replay, Web controller");
    println!("  License: perpetual (Windows-native)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "vmix".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_vmix(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_vmix};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/vmix"), "vmix");
        assert_eq!(basename(r"C:\bin\vmix.exe"), "vmix.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("vmix.exe"), "vmix");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_vmix(&["--help".to_string()], "vmix"), 0);
        assert_eq!(run_vmix(&["-h".to_string()], "vmix"), 0);
        assert_eq!(run_vmix(&["--version".to_string()], "vmix"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_vmix(&[], "vmix"), 0);
    }
}
