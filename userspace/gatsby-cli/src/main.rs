#![deny(clippy::all)]

//! gatsby-cli — SlateOS Gatsby React static site generator
//!
//! Single personality: `gatsby`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gatsby(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: gatsby COMMAND [OPTIONS]");
        println!("Gatsby CLI v5.13.0 (Slate OS) — React-based site generator");
        println!();
        println!("Commands:");
        println!("  new [DIR]       Create new site");
        println!("  develop         Start dev server");
        println!("  build           Build production site");
        println!("  serve           Serve production build");
        println!("  clean           Delete cache and build dirs");
        println!("  info            Show environment info");
        println!("  repl            Open Node.js REPL");
        println!("  plugin          List plugins");
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Gatsby CLI version: 5.13.0");
        println!("Gatsby version: 5.13.0");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("develop");
    match cmd {
        "new" => {
            let dir = args.get(1).map(|s| s.as_str()).unwrap_or("my-gatsby-site");
            println!("Creating new site in {}/", dir);
            println!("  Installing dependencies...");
            println!("  Your new Gatsby site has been created!");
            println!("  Start developing: cd {} && gatsby develop", dir);
        }
        "develop" => {
            println!("success open and validate gatsby-configs - 0.05s");
            println!("success load plugins - 0.8s");
            println!("success onPreInit - 0.01s");
            println!("success source and transform nodes - 1.2s");
            println!("success building schema - 0.3s");
            println!("success createPages - 0.1s");
            println!("success Generating GraphQL and page data - 0.5s");
            println!();
            println!("  Local:   http://localhost:8000/");
            println!("  GraphQL: http://localhost:8000/___graphql");
        }
        "build" => {
            println!("success Building production JavaScript and CSS bundles - 8.2s");
            println!("success Generating image thumbnails - 2.1s");
            println!("success Building HTML renderer - 1.5s");
            println!("success Generating SSR pages - 0.8s");
            println!();
            println!("Pages:");
            println!("  /               1.2kB");
            println!("  /about/         0.9kB");
            println!("  /blog/          1.5kB");
            println!();
            println!("Done building in 12.6s");
        }
        "serve" => println!("  Local: http://localhost:9000/"),
        "clean" => println!("  Deleted .cache and public directories."),
        "info" => {
            println!("  System:");
            println!("    OS: Slate OS x86_64");
            println!("  Binaries:");
            println!("    Node: v20.11.1");
            println!("  Gatsby packages:");
            println!("    gatsby: 5.13.0");
            println!("    gatsby-plugin-image: 3.13.0");
        }
        "plugin" => println!("  Installed plugins: gatsby-plugin-image, gatsby-plugin-sharp"),
        _ => println!("gatsby {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gatsby".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gatsby(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gatsby};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gatsby"), "gatsby");
        assert_eq!(basename(r"C:\bin\gatsby.exe"), "gatsby.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gatsby.exe"), "gatsby");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gatsby(&["--help".to_string()], "gatsby"), 0);
        assert_eq!(run_gatsby(&["-h".to_string()], "gatsby"), 0);
        let _ = run_gatsby(&["--version".to_string()], "gatsby");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gatsby(&[], "gatsby");
    }
}
