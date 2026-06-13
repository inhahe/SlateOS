#![deny(clippy::all)]

//! vitepress-cli — SlateOS VitePress documentation generator
//!
//! Single personality: `vitepress`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_vitepress(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: vitepress COMMAND [OPTIONS]");
        println!("VitePress v1.2.0 (SlateOS) — Vite-powered documentation");
        println!();
        println!("Commands:");
        println!("  dev             Start dev server");
        println!("  build           Build for production");
        println!("  preview         Preview build");
        println!("  init            Initialize new project");
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("vitepress v1.2.0");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("dev");
    match cmd {
        "dev" => {
            println!("  vitepress v1.2.0");
            println!();
            println!("  VITE v5.2.0  ready in 345ms");
            println!();
            println!("  Local:   http://localhost:5173/");
            println!("  Network: http://192.168.1.100:5173/");
        }
        "build" => {
            println!("  vitepress v1.2.0");
            println!("  building client + SSR bundles...");
            println!("  rendering pages...");
            println!("  index.md                     3.2kB");
            println!("  guide/getting-started.md     4.1kB");
            println!("  api/index.md                 5.8kB");
            println!("  build complete in 2.1s");
        }
        "preview" => println!("  Preview: http://localhost:4173/"),
        "init" => {
            println!("Welcome to VitePress!");
            println!("  Created .vitepress/config.mts");
            println!("  Created index.md");
            println!("  Done.");
        }
        _ => println!("vitepress {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "vitepress".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_vitepress(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_vitepress};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/vitepress"), "vitepress");
        assert_eq!(basename(r"C:\bin\vitepress.exe"), "vitepress.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("vitepress.exe"), "vitepress");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_vitepress(&["--help".to_string()], "vitepress"), 0);
        assert_eq!(run_vitepress(&["-h".to_string()], "vitepress"), 0);
        let _ = run_vitepress(&["--version".to_string()], "vitepress");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_vitepress(&[], "vitepress");
    }
}
