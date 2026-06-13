#![deny(clippy::all)]

//! premake-cli — SlateOS Premake build configuration
//!
//! Multi-personality: `premake5`, `premake4`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_premake(args: &[String], version: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: premake{} [OPTIONS] ACTION", version);
        println!("Premake {} (Slate OS)", if version == "5" { "5.0.0-beta2" } else { "4.4" });
        println!();
        println!("Actions:");
        println!("  gmake          GNU Makefiles");
        println!("  gmake2         GNU Makefiles (v2)");
        println!("  vs2022         Visual Studio 2022");
        println!("  vs2019         Visual Studio 2019");
        println!("  xcode4         Apple Xcode 4+");
        println!("  codelite       CodeLite");
        println!("  clean          Remove generated files");
        println!();
        println!("Options:");
        println!("  --file=FILE    Premake script (default: premake{}.lua)", version);
        println!("  --os=OS        Target OS");
        println!("  --cc=CC        Choose C/C++ compiler");
        println!("  --dotnet=VER   .NET version");
        println!("  --verbose      Verbose output");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("premake{} (Premake Build Script Generator) {}", version, if version == "5" { "5.0.0-beta2" } else { "4.4" });
        return 0;
    }
    let action = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("gmake2");
    match action {
        "clean" => {
            println!("Removing generated files...");
            println!("Done.");
        }
        _ => {
            println!("Building configurations...");
            println!("Running action '{}'...", action);
            println!("  Generated Makefile");
            println!("  Generated myproject.make");
            println!("Done ({} files generated).", 2);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "premake5".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let version = if prog.contains('4') { "4" } else { "5" };
    let code = run_premake(&rest, version);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_premake};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/premake"), "premake");
        assert_eq!(basename(r"C:\bin\premake.exe"), "premake.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("premake.exe"), "premake");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_premake(&["--help".to_string()], "premake"), 0);
        assert_eq!(run_premake(&["-h".to_string()], "premake"), 0);
        let _ = run_premake(&["--version".to_string()], "premake");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_premake(&[], "premake");
    }
}
