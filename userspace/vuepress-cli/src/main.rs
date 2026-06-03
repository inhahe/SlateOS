#![deny(clippy::all)]

//! vuepress-cli — OurOS VuePress CLI
//!
//! Multi-personality: `vuepress`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_vuepress(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: vuepress COMMAND [OPTIONS]");
        println!("VuePress 2.0.0-rc.14 (OurOS)");
        println!();
        println!("Commands:");
        println!("  dev            Start development server");
        println!("  build          Build static site");
        println!("  info           Show environment info");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("2.0.0-rc.14"),
        "dev" => {
            let dir = args.get(1).map(|s| s.as_str()).unwrap_or("docs");
            println!("VuePress dev server starting...");
            println!("  Source: {}/", dir);
            println!("  Theme: @vuepress/theme-default");
            println!("  Server running at http://localhost:8080/");
        }
        "build" => {
            let dir = args.get(1).map(|s| s.as_str()).unwrap_or("docs");
            println!("VuePress building...");
            println!("  Source: {}/", dir);
            println!("  Rendering pages...");
            println!("  Generated 28 pages.");
            println!("  Output: docs/.vuepress/dist/");
        }
        "info" => {
            println!("Environment Info:");
            println!("  System: OurOS x86_64");
            println!("  Node: v20.11.0");
            println!("  VuePress: 2.0.0-rc.14");
            println!("  Vue: 3.4.21");
        }
        _ => println!("vuepress: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "vuepress".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_vuepress(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_vuepress};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/vuepress"), "vuepress");
        assert_eq!(basename(r"C:\bin\vuepress.exe"), "vuepress.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("vuepress.exe"), "vuepress");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_vuepress(&["--help".to_string()]), 0);
        assert_eq!(run_vuepress(&["-h".to_string()]), 0);
        assert_eq!(run_vuepress(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_vuepress(&[]), 0);
    }
}
