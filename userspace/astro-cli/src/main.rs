#![deny(clippy::all)]

//! astro-cli — Slate OS Astro web framework CLI
//!
//! Single personality: `astro`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_astro(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: astro COMMAND [OPTIONS]");
        println!("Astro v4.8.0 (Slate OS) — The web framework for content-driven sites");
        println!();
        println!("Commands:");
        println!("  dev             Start dev server");
        println!("  build           Build for production");
        println!("  preview         Preview production build");
        println!("  check           Check project for errors");
        println!("  sync            Generate content collection types");
        println!("  add             Add integrations/adapters");
        println!("  preferences     Manage user preferences");
        println!("  telemetry       Manage telemetry");
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("astro  v4.8.0");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("dev");
    match cmd {
        "dev" => {
            println!("  astro  v4.8.0 started in 234ms");
            println!();
            println!("  Local    http://localhost:4321/");
            println!("  Network  http://192.168.1.100:4321/");
            println!();
            println!("  watching for file changes...");
        }
        "build" => {
            println!("  astro  v4.8.0 build started...");
            println!("  generating static routes...");
            println!("  /index.html                  +12ms");
            println!("  /about/index.html            +8ms");
            println!("  /blog/index.html             +15ms");
            println!("  /blog/post-1/index.html      +6ms");
            println!("  Completed in 1.23s");
            println!("  dist/  4 pages, 12 assets");
        }
        "preview" => {
            println!("  astro  v4.8.0 preview server");
            println!("  Local    http://localhost:4321/");
        }
        "check" => {
            println!("  astro  checking project...");
            println!("  0 errors, 0 warnings");
        }
        "sync" => println!("  Content collection types generated."),
        "add" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("tailwind");
            println!("  Adding @astrojs/{}...", pkg);
            println!("  Done.");
        }
        _ => println!("astro {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "astro".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_astro(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_astro};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/astro"), "astro");
        assert_eq!(basename(r"C:\bin\astro.exe"), "astro.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("astro.exe"), "astro");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_astro(&["--help".to_string()], "astro"), 0);
        assert_eq!(run_astro(&["-h".to_string()], "astro"), 0);
        let _ = run_astro(&["--version".to_string()], "astro");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_astro(&[], "astro");
    }
}
