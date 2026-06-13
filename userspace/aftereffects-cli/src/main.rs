#![deny(clippy::all)]

//! aftereffects-cli — SlateOS Adobe After Effects motion graphics
//!
//! Single personality: `aftereffects`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ae(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: aftereffects [OPTIONS] [PROJECT]");
        println!("Adobe After Effects 2024 (Slate OS) — Motion graphics & VFX");
        println!();
        println!("Options:");
        println!("  -project FILE          Open project (.aep)");
        println!("  -r SCRIPT              Run ExtendScript / JSX");
        println!("  -comp NAME             Composition to work on");
        println!("  -RStemplate TEMPLATE   Render Settings template");
        println!("  -OMtemplate TEMPLATE   Output Module template");
        println!("  -output FILE           Output file");
        println!("  -s FRAME -e FRAME      Start/end frame");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Adobe After Effects 2024 v24.6.0 (Slate OS)"); return 0; }
    println!("Adobe After Effects 2024 v24.6.0 (Slate OS)");
    println!("  Engine: Multi-Frame Rendering, GPU (CUDA/Metal)");
    println!("  Scripting: ExtendScript (JSX), CEP panels");
    println!("  Effects: 270+ built-in + plug-ins (Sapphire, Red Giant)");
    println!("  Integration: Premiere Pro Dynamic Link, Cinema 4D Lite");
    println!("  License: Creative Cloud");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "aftereffects".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ae(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ae};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/aftereffects"), "aftereffects");
        assert_eq!(basename(r"C:\bin\aftereffects.exe"), "aftereffects.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("aftereffects.exe"), "aftereffects");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ae(&["--help".to_string()], "aftereffects"), 0);
        assert_eq!(run_ae(&["-h".to_string()], "aftereffects"), 0);
        let _ = run_ae(&["--version".to_string()], "aftereffects");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ae(&[], "aftereffects");
    }
}
