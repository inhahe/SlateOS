#![deny(clippy::all)]

//! defold-cli — Slate OS Defold game engine
//!
//! Single personality: `defold`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_defold(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: defold [COMMAND] [OPTIONS]");
        println!("Defold v1.9 (Slate OS) — Cross-platform 2D/3D game engine");
        println!();
        println!("Commands:");
        println!("  new PROJECT        Create new project");
        println!("  build              Build the project");
        println!("  bundle PLATFORM    Bundle for platform (osx/windows/linux/android/ios/html5)");
        println!("  run                Run the editor");
        println!("  resolve            Resolve dependencies");
        println!("  clean              Clean build artifacts");
        println!();
        println!("Options:");
        println!("  --project PATH     Project file");
        println!("  --output DIR       Output directory");
        println!("  --release          Build in release mode");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Defold v1.9.4 (Slate OS)"); return 0; }
    println!("Defold v1.9.4 (Slate OS)");
    println!("  Editor: running on JavaFX");
    println!("  Targets: macOS, Windows, Linux, Android, iOS, HTML5, Switch, PS4");
    println!("  Scripting: Lua 5.1");
    println!("  Renderer: OpenGL ES 3.0 / Vulkan");
    println!("  Physics: Box2D, Bullet 3D");
    println!("  Editor: free, no royalties");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "defold".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_defold(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_defold};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/defold"), "defold");
        assert_eq!(basename(r"C:\bin\defold.exe"), "defold.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("defold.exe"), "defold");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_defold(&["--help".to_string()], "defold"), 0);
        assert_eq!(run_defold(&["-h".to_string()], "defold"), 0);
        let _ = run_defold(&["--version".to_string()], "defold");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_defold(&[], "defold");
    }
}
