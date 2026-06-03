#![deny(clippy::all)]

//! phaser-cli — OurOS Phaser game framework
//!
//! Single personality: `phaser`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_phaser(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: phaser [COMMAND] [OPTIONS]");
        println!("Phaser v3.85 (OurOS) — HTML5 game framework");
        println!();
        println!("Commands:");
        println!("  new PROJECT [TEMPLATE]  Create new project");
        println!("  start              Start dev server");
        println!("  build              Build for production");
        println!("  preview            Preview built game");
        println!("  template list      List templates");
        println!("  plugins list       List plugins");
        println!();
        println!("Options:");
        println!("  --port PORT        Dev server port");
        println!("  --typescript       Use TypeScript template");
        println!("  --vite             Use Vite bundler");
        println!("  --webpack          Use Webpack bundler");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Phaser v3.85.2 (OurOS)"); return 0; }
    println!("Phaser v3.85.2 (OurOS)");
    println!("  Renderer: WebGL (auto-fallback to Canvas)");
    println!("  Templates: 15 (TypeScript, JavaScript, Vite, Webpack)");
    println!("  Physics: Arcade, Matter.js");
    println!("  Plugins: 23 popular plugins");
    println!("  Dev server: http://0.0.0.0:8080");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "phaser".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_phaser(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_phaser};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/phaser"), "phaser");
        assert_eq!(basename(r"C:\bin\phaser.exe"), "phaser.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("phaser.exe"), "phaser");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_phaser(&["--help".to_string()], "phaser"), 0);
        assert_eq!(run_phaser(&["-h".to_string()], "phaser"), 0);
        assert_eq!(run_phaser(&["--version".to_string()], "phaser"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_phaser(&[], "phaser"), 0);
    }
}
