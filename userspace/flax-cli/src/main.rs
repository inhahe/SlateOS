#![deny(clippy::all)]

//! flax-cli — Slate OS Flax game engine
//!
//! Single personality: `flax`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_flax(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: flax [COMMAND] [OPTIONS]");
        println!("Flax Engine v1.8 (Slate OS) — Modern AAA-quality game engine");
        println!();
        println!("Commands:");
        println!("  new PROJECT        Create new project");
        println!("  build              Build project");
        println!("  cook               Cook content for target");
        println!("  package PLATFORM   Package for platform");
        println!("  run                Run editor");
        println!("  test               Run tests");
        println!("  generate           Generate project files");
        println!();
        println!("Options:");
        println!("  --platform PLAT    Target (Windows/Linux/Mac/Android/iOS/PS4/PS5/XboxOne/XboxScarlett/Switch)");
        println!("  --configuration C  Debug/Development/Release");
        println!("  --arch ARCH        x64/ARM64");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Flax Engine v1.8.0 (Slate OS)"); return 0; }
    println!("Flax Engine v1.8.0 (Slate OS)");
    println!("  Scripting: C#, C++, Visual Script");
    println!("  Renderer: DirectX 11/12, Vulkan");
    println!("  Lighting: PBR, GI, baked & realtime");
    println!("  Physics: PhysX, custom 2D");
    println!("  Audio: XAudio2, OpenAL");
    println!("  Editor platforms: Windows, Linux, Mac");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "flax".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_flax(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_flax};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/flax"), "flax");
        assert_eq!(basename(r"C:\bin\flax.exe"), "flax.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("flax.exe"), "flax");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_flax(&["--help".to_string()], "flax"), 0);
        assert_eq!(run_flax(&["-h".to_string()], "flax"), 0);
        let _ = run_flax(&["--version".to_string()], "flax");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_flax(&[], "flax");
    }
}
