#![deny(clippy::all)]

//! neutron-cli — SlateOS iZotope Neutron mixing suite
//!
//! Single personality: `neutron`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_neutron(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: neutron [OPTIONS] [TRACK]");
        println!("iZotope Neutron 4 (SlateOS) — AI mixing & channel-strip suite");
        println!();
        println!("Options:");
        println!("  --assistant            Mix Assistant (AI track analysis)");
        println!("  --target NAME          Match target sound (vocal/drum/bass/synth)");
        println!("  --unmask SOURCE        Unmask from source track");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("iZotope Neutron 4.5.0 (SlateOS)"); return 0; }
    println!("iZotope Neutron 4.5.0 (SlateOS)");
    println!("  Modules: Equalizer, Compressor (×2), Exciter, Gate, Sculptor, Transient Shaper");
    println!("  AI: Mix Assistant, Unmask, Target Library");
    println!("  Visual Mixer: drag-and-drop pan/volume across session");
    println!("  Surround: 7.1.4 multichannel, ambisonics");
    println!("  Plug-in formats: VST3, AU, AAX");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "neutron".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_neutron(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_neutron};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/neutron"), "neutron");
        assert_eq!(basename(r"C:\bin\neutron.exe"), "neutron.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("neutron.exe"), "neutron");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_neutron(&["--help".to_string()], "neutron"), 0);
        assert_eq!(run_neutron(&["-h".to_string()], "neutron"), 0);
        let _ = run_neutron(&["--version".to_string()], "neutron");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_neutron(&[], "neutron");
    }
}
