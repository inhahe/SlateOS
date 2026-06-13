#![deny(clippy::all)]

//! lightwave-cli — SlateOS NewTek LightWave 3D
//!
//! Single personality: `lightwave`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lw(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lightwave [OPTIONS] [FILE]");
        println!("LightWave 3D 2024 (Slate OS) — Modeler + Layout 3D suite");
        println!();
        println!("Options:");
        println!("  --modeler             Launch Modeler");
        println!("  --layout              Launch Layout");
        println!("  -3 CONFIG             Use configuration");
        println!("  -c SCENE              Render scene");
        println!("  -f FORMAT             Output format");
        println!("  --version             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("LightWave 3D 2024.0.1 (Slate OS)"); return 0; }
    println!("LightWave 3D 2024.0.1 (Slate OS)");
    println!("  Applications: Modeler, Layout");
    println!("  Renderer: VPR, F-Prime, Octane plug-in");
    println!("  Scripting: LScript, Python");
    println!("  Features: ZBrush bridge, Genoma rigging, Bullet dynamics");
    println!("  Surface editor: PBR + node-based");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lightwave".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lw(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lw};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lightwave"), "lightwave");
        assert_eq!(basename(r"C:\bin\lightwave.exe"), "lightwave.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lightwave.exe"), "lightwave");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lw(&["--help".to_string()], "lightwave"), 0);
        assert_eq!(run_lw(&["-h".to_string()], "lightwave"), 0);
        let _ = run_lw(&["--version".to_string()], "lightwave");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lw(&[], "lightwave");
    }
}
