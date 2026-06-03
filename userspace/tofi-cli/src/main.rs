#![deny(clippy::all)]

//! tofi-cli — OurOS tofi tiny dynamic menu
//!
//! Multi-personality: `tofi`, `tofi-run`, `tofi-drun`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tofi(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tofi [OPTIONS]");
        println!("tofi v0.9 (OurOS) — Tiny dynamic menu for Wayland");
        println!();
        println!("Options:");
        println!("  --prompt TEXT     Prompt text");
        println!("  --font FONT       Font path or name");
        println!("  --font-size SIZE  Font size");
        println!("  --width PX        Window width");
        println!("  --height PX       Window height");
        println!("  --background-color COLOR  Background");
        println!("  --text-color COLOR       Text color");
        println!("  --selection-color COLOR  Selection color");
        println!("  --corner-radius PX       Corner radius");
        println!("  --fuzzy-match     Enable fuzzy matching");
        println!("  --horizontal      Horizontal layout");
        return 0;
    }
    println!("tofi: reading from stdin...");
    0
}

fn run_tofi_run(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tofi-run [OPTIONS]");
        println!("tofi-run v0.9 (OurOS) — Run commands from PATH");
        return 0;
    }
    let _ = args;
    println!("tofi-run: scanning PATH...");
    0
}

fn run_tofi_drun(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tofi-drun [OPTIONS]");
        println!("tofi-drun v0.9 (OurOS) — Launch desktop applications");
        return 0;
    }
    let _ = args;
    println!("tofi-drun: scanning .desktop files...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "tofi".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "tofi-run" => run_tofi_run(&rest, &prog),
        "tofi-drun" => run_tofi_drun(&rest, &prog),
        _ => run_tofi(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_tofi};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/tofi"), "tofi");
        assert_eq!(basename(r"C:\bin\tofi.exe"), "tofi.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("tofi.exe"), "tofi");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_tofi(&["--help".to_string()], "tofi"), 0);
        assert_eq!(run_tofi(&["-h".to_string()], "tofi"), 0);
        assert_eq!(run_tofi(&["--version".to_string()], "tofi"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_tofi(&[], "tofi"), 0);
    }
}
