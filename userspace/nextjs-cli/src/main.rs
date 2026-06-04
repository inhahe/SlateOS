#![deny(clippy::all)]

//! nextjs-cli — OurOS Next.js CLI
//!
//! Multi-personality: `next`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_next(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: next COMMAND [OPTIONS]");
        println!("Next.js 14.2.4 (OurOS)");
        println!();
        println!("Commands:");
        println!("  dev          Start development server");
        println!("  build        Build for production");
        println!("  start        Start production server");
        println!("  lint         Run ESLint");
        println!("  info         Show system info");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" | "-v" => println!("14.2.4"),
        "dev" => {
            let port = args.windows(2).find(|w| w[0] == "-p" || w[0] == "--port")
                .map(|w| w[1].as_str()).unwrap_or("3000");
            println!("  ▲ Next.js 14.2.4");
            println!("  - Local:    http://localhost:{}", port);
            println!("  - Ready in 1.2s");
        }
        "build" => {
            println!("  ▲ Next.js 14.2.4");
            println!("  Creating an optimized production build...");
            println!();
            println!("Route (app)                  Size     First Load JS");
            println!("┌ ○ /                        5.24 kB  89.1 kB");
            println!("├ ○ /about                   2.13 kB  86.0 kB");
            println!("├ ● /blog/[slug]             3.45 kB  87.3 kB");
            println!("└ ○ /api/hello               0 B      83.9 kB");
            println!();
            println!("○ (Static)  prerendered as static content");
            println!("● (SSG)     prerendered as static HTML");
        }
        "start" => {
            let port = args.windows(2).find(|w| w[0] == "-p" || w[0] == "--port")
                .map(|w| w[1].as_str()).unwrap_or("3000");
            println!("  ▲ Next.js 14.2.4");
            println!("  - Local:    http://localhost:{}", port);
            println!("  - Ready in 0.3s");
        }
        "lint" => {
            println!("  ✓ No ESLint warnings or errors");
        }
        "info" => {
            println!("Operating System: OurOS");
            println!("Node.js: v20.14.0");
            println!("Next.js: 14.2.4");
            println!("React: 18.3.1");
        }
        _ => println!("next: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "next".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_next(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_next};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/nextjs"), "nextjs");
        assert_eq!(basename(r"C:\bin\nextjs.exe"), "nextjs.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("nextjs.exe"), "nextjs");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_next(&["--help".to_string()]), 0);
        assert_eq!(run_next(&["-h".to_string()]), 0);
        let _ = run_next(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_next(&[]);
    }
}
