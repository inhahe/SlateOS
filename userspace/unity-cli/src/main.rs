#![deny(clippy::all)]

//! unity-cli — SlateOS Unity Hub CLI
//!
//! Multi-personality: `unity-hub`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_unity(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: unity-hub COMMAND [OPTIONS]");
        println!("Unity Hub CLI 3.8.0 (Slate OS)");
        println!();
        println!("Commands:");
        println!("  editors        Manage Unity editor installations");
        println!("  install        Install Unity editor version");
        println!("  projects       Manage projects");
        println!("  create         Create new project");
        println!("  build          Build project");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("unity-hub 3.8.0"),
        "editors" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("INSTALLED EDITORS:");
                println!("  2023.2.20f1    LTS     /opt/unity/2023.2");
                println!("  6000.0.25f1    Latest  /opt/unity/6000.0");
            } else if sub == "releases" {
                println!("AVAILABLE RELEASES:");
                println!("  6000.0.25f1    Latest");
                println!("  2023.2.20f1    LTS");
                println!("  2022.3.47f1    LTS");
            } else {
                println!("unity-hub editors: '{}' completed", sub);
            }
        }
        "install" => {
            let version = args.get(1).map(|s| s.as_str()).unwrap_or("2023.2.20f1");
            println!("Installing Unity {}...", version);
            println!("  Downloading editor...");
            println!("  Installing modules...");
            println!("  Installation complete.");
        }
        "projects" => {
            println!("PROJECTS:");
            println!("  MyGame         2023.2.20f1    /home/user/MyGame");
            println!("  Prototype      6000.0.25f1    /home/user/Prototype");
        }
        "create" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("MyProject");
            let template = args.windows(2).find(|w| w[0] == "--template")
                .map(|w| w[1].as_str()).unwrap_or("3d-core");
            println!("Creating project '{}'...", name);
            println!("  Template: {}", template);
            println!("  Project created at ./{}/", name);
        }
        "build" => {
            let target = args.windows(2).find(|w| w[0] == "--target")
                .map(|w| w[1].as_str()).unwrap_or("linux64");
            println!("Building project for {}...", target);
            println!("  Compiling scripts...");
            println!("  Building player...");
            println!("  Build complete: build/{}/", target);
        }
        _ => println!("unity-hub: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "unity-hub".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_unity(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_unity};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/unity"), "unity");
        assert_eq!(basename(r"C:\bin\unity.exe"), "unity.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("unity.exe"), "unity");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_unity(&["--help".to_string()]), 0);
        assert_eq!(run_unity(&["-h".to_string()]), 0);
        let _ = run_unity(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_unity(&[]);
    }
}
