#![deny(clippy::all)]

//! mocha-cli — Slate OS Boris FX Mocha Pro planar tracking
//!
//! Single personality: `mocha`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mocha(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mocha [OPTIONS] [PROJECT]");
        println!("Boris FX Mocha Pro 2024 (Slate OS) — Planar tracking, rotoscoping, object removal");
        println!();
        println!("Options:");
        println!("  --track LAYER          Track a layer");
        println!("  --export FORMAT        Export tracking data (AE/Nuke/Resolve)");
        println!("  --remove               Use object removal");
        println!("  --stabilize            Stabilization mode");
        println!("  --script FILE          Run mocha Python script");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Boris FX Mocha Pro 2024.0.0 (Slate OS)"); return 0; }
    println!("Boris FX Mocha Pro 2024.0.0 (Slate OS)");
    println!("  Modules: Planar Tracker, Roto, Remove, Insert, Lens, Stabilize");
    println!("  Plug-in versions: AE, Premiere, Nuke, Fusion, Resolve, OFX hosts");
    println!("  Export formats: AE keyframes, Nuke nodes, Resolve, FBX, Alembic");
    println!("  Scripting: Python");
    println!("  License: Boris FX (subscription / perpetual)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mocha".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mocha(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mocha};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mocha"), "mocha");
        assert_eq!(basename(r"C:\bin\mocha.exe"), "mocha.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mocha.exe"), "mocha");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mocha(&["--help".to_string()], "mocha"), 0);
        assert_eq!(run_mocha(&["-h".to_string()], "mocha"), 0);
        let _ = run_mocha(&["--version".to_string()], "mocha");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mocha(&[], "mocha");
    }
}
