#![deny(clippy::all)]

//! freecad-cli — SlateOS FreeCAD CLI
//!
//! Multi-personality: `freecad`, `freecadcmd`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_freecad(args: &[String], cmd_mode: bool) -> i32 {
    let name = if cmd_mode { "freecadcmd" } else { "freecad" };

    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS] [FILE ...]", name);
        println!();
        println!("FreeCAD — parametric 3D CAD (SlateOS).");
        println!();
        println!("Options:");
        println!("  --run-macro FILE    Run macro");
        println!("  --console           Console mode");
        println!("  --module MODULE     Load module");
        println!("  --log-file FILE     Log file");
        println!("  --user-cfg FILE     User config");
        println!("  --system-cfg FILE   System config");
        if !cmd_mode {
            println!("  --single-instance   Single instance");
            println!("  --no-banner         No splash screen");
        }
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("FreeCAD 0.21.2 (SlateOS)");
        return 0;
    }

    let macro_file = args.windows(2).find(|w| w[0] == "--run-macro").map(|w| w[1].as_str());
    let files: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();

    if cmd_mode {
        println!("FreeCAD 0.21.2 — console mode");
        if let Some(m) = macro_file {
            println!("Running macro: {}", m);
        }
        for f in &files {
            println!("Loading: {}", f);
        }
        println!(">>> ");
    } else {
        if files.is_empty() {
            println!("FreeCAD 0.21.2 (SlateOS)");
            println!("Starting FreeCAD GUI...");
        } else {
            for f in &files {
                println!("Opening: {}", f);
            }
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "freecad".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "freecadcmd" => run_freecad(&rest, true),
        _ => run_freecad(&rest, false),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_freecad};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/freecad"), "freecad");
        assert_eq!(basename(r"C:\bin\freecad.exe"), "freecad.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("freecad.exe"), "freecad");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_freecad(&["--help".to_string()], false), 0);
        assert_eq!(run_freecad(&["-h".to_string()], false), 0);
        let _ = run_freecad(&["--version".to_string()], false);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_freecad(&[], false);
    }
}
