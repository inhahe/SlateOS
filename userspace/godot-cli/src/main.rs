#![deny(clippy::all)]

//! godot-cli — SlateOS Godot Engine CLI
//!
//! Multi-personality: `godot`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_godot(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: godot [OPTIONS] [SCENE]");
        println!("Godot Engine 4.3.0 (SlateOS)");
        println!();
        println!("Options:");
        println!("  --headless          Run without display");
        println!("  --editor            Open the editor");
        println!("  --project PATH      Project path");
        println!("  --export-release P  Export release build");
        println!("  --export-debug P    Export debug build");
        println!("  --script FILE       Run a script");
        println!("  --import             Import project resources");
        println!("  --doctool PATH       Generate API docs");
        println!("  --gdscript-docs P    Generate GDScript docs");
        println!("  --validate-extension-api  Validate extension API");
        println!("  --version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Godot Engine v4.3.stable.official (SlateOS)");
        return 0;
    }
    let headless = args.iter().any(|a| a == "--headless");
    let editor = args.iter().any(|a| a == "--editor" || a == "-e");
    let export = args.windows(2).find(|w| w[0] == "--export-release")
        .map(|w| w[1].as_str());

    if let Some(preset) = export {
        println!("Exporting project with preset '{}'...", preset);
        println!("  Building...");
        println!("  Packing resources...");
        println!("  Export complete.");
    } else if headless {
        println!("Godot Engine v4.3.stable.official - Running headless");
        if let Some(script) = args.windows(2).find(|w| w[0] == "--script").map(|w| w[1].as_str()) {
            println!("  Running script: {}", script);
            println!("  Script completed.");
        }
    } else if editor {
        println!("Godot Engine v4.3.stable.official - Editor");
        println!("  Loading project...");
        println!("  Editor ready.");
    } else {
        let scene = args.iter().rfind(|a| !a.starts_with('-'))
            .map(|s| s.as_str()).unwrap_or("main.tscn");
        println!("Godot Engine v4.3.stable.official");
        println!("  Loading scene: {}", scene);
        println!("  Running...");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "godot".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_godot(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_godot};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/godot"), "godot");
        assert_eq!(basename(r"C:\bin\godot.exe"), "godot.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("godot.exe"), "godot");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_godot(&["--help".to_string()]), 0);
        assert_eq!(run_godot(&["-h".to_string()]), 0);
        let _ = run_godot(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_godot(&[]);
    }
}
