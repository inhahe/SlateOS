#![deny(clippy::all)]

//! stride-cli — OurOS Stride game engine
//!
//! Single personality: `stride`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_stride(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: stride [COMMAND] [OPTIONS]");
        println!("Stride v4.2 (OurOS) — Open-source C# 3D game engine");
        println!();
        println!("Commands:");
        println!("  new PROJECT        Create new project");
        println!("  build              Build the project");
        println!("  package            Package for distribution");
        println!("  run                Run the game");
        println!("  test               Run tests");
        println!("  open               Open in Game Studio editor");
        println!();
        println!("Options:");
        println!("  --platform PLAT    Target platform (Windows/Linux/Android/iOS/UWP)");
        println!("  --configuration C  Build configuration (Debug/Release)");
        println!("  --graphics API     Graphics API (Direct3D11/12/OpenGL/Vulkan)");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Stride v4.2.0 (OurOS)"); return 0; }
    println!("Stride v4.2.0 (OurOS)");
    println!("  Language: C# (.NET 8)");
    println!("  Renderer: Direct3D11/12, Vulkan, OpenGL, OpenGL ES");
    println!("  Platforms: Windows, Linux, Android, iOS, UWP");
    println!("  Physics: Bullet");
    println!("  Audio: OpenAL, XAudio2");
    println!("  Editor: Game Studio (Avalonia UI)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "stride".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_stride(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_stride};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/stride"), "stride");
        assert_eq!(basename(r"C:\bin\stride.exe"), "stride.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("stride.exe"), "stride");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_stride(&["--help".to_string()], "stride"), 0);
        assert_eq!(run_stride(&["-h".to_string()], "stride"), 0);
        let _ = run_stride(&["--version".to_string()], "stride");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_stride(&[], "stride");
    }
}
