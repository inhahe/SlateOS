#![deny(clippy::all)]

//! construct-cli — OurOS Construct 3 game maker
//!
//! Single personality: `construct`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_construct(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: construct [COMMAND] [OPTIONS]");
        println!("Construct 3 r400 (OurOS) — Visual game development");
        println!();
        println!("Commands:");
        println!("  open PROJECT       Open project");
        println!("  export PROJECT     Export project (HTML5, Cordova, etc)");
        println!("  preview PROJECT    Preview project");
        println!("  validate PROJECT   Validate project");
        println!("  pack PROJECT       Pack as .c3p");
        println!("  plugins list       List installed plugins");
        println!();
        println!("Options:");
        println!("  --headless         Run without GUI");
        println!("  --target TYPE      Export target (html5/android/ios/scirra-arcade)");
        println!("  --minify           Minify exported scripts");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Construct 3 r400 (OurOS)"); return 0; }
    println!("Construct 3 r400 (OurOS)");
    println!("  Editor: https://0.0.0.0:8443");
    println!("  Project format: C3P");
    println!("  Export targets: HTML5, Android, iOS, Steam, Scirra Arcade");
    println!("  Behaviors: 50+ built-in");
    println!("  Plugins: 80+ official, 1000+ community");
    println!("  Engine: WebGL2");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "construct".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_construct(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_construct};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/construct"), "construct");
        assert_eq!(basename(r"C:\bin\construct.exe"), "construct.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("construct.exe"), "construct");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_construct(&["--help".to_string()], "construct"), 0);
        assert_eq!(run_construct(&["-h".to_string()], "construct"), 0);
        assert_eq!(run_construct(&["--version".to_string()], "construct"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_construct(&[], "construct"), 0);
    }
}
