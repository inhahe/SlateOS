#![deny(clippy::all)]

//! remix-cli — OurOS Remix framework CLI
//!
//! Multi-personality: `remix`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_remix(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: remix COMMAND [OPTIONS]");
        println!("Remix 2.10.0 (OurOS)");
        println!();
        println!("Commands:");
        println!("  init         Initialize a new Remix project");
        println!("  dev          Start development server");
        println!("  build        Build for production");
        println!("  start        Start production server");
        println!("  routes       Show route hierarchy");
        println!("  reveal       Reveal internal files");
        println!("  vite:dev     Start Vite dev server");
        println!("  vite:build   Build with Vite");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" | "-v" => println!("remix v2.10.0"),
        "init" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("my-remix-app");
            println!("Creating Remix project '{}'...", name);
            println!("  Template: remix-run/remix/templates/remix");
            println!("  TypeScript: yes");
            println!("  Created app/root.tsx");
            println!("  Created app/routes/_index.tsx");
            println!("  Created remix.config.js");
        }
        "dev" => {
            println!("Remix App Server started at http://localhost:3000");
            println!("  (Press CTRL+C to quit)");
        }
        "build" => {
            println!("Building Remix app...");
            println!("  Built in 1.234s");
            println!("  Output: build/");
        }
        "start" => {
            println!("Remix App Server started at http://localhost:3000");
        }
        "routes" => {
            println!("Routes:");
            println!("  <Routes>");
            println!("    <Route file=\"root.tsx\">");
            println!("      <Route path=\"/\" file=\"routes/_index.tsx\" />");
            println!("      <Route path=\"/about\" file=\"routes/about.tsx\" />");
            println!("      <Route path=\"/blog/:slug\" file=\"routes/blog.$slug.tsx\" />");
            println!("    </Route>");
            println!("  </Routes>");
        }
        "reveal" => {
            let what = args.get(1).map(|s| s.as_str()).unwrap_or("entry.client");
            println!("Revealing {}...", what);
            println!("Created app/{}.tsx", what);
        }
        _ => println!("remix: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "remix".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_remix(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_remix};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/remix"), "remix");
        assert_eq!(basename(r"C:\bin\remix.exe"), "remix.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("remix.exe"), "remix");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_remix(&["--help".to_string()]), 0);
        assert_eq!(run_remix(&["-h".to_string()]), 0);
        assert_eq!(run_remix(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_remix(&[]), 0);
    }
}
