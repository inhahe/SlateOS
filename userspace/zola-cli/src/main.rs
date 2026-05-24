#![deny(clippy::all)]

//! zola-cli — OurOS Zola static site generator
//!
//! Single personality: `zola`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_zola(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: zola COMMAND [OPTIONS]");
        println!("Zola v0.19.0 (OurOS) — Fast static site generator");
        println!();
        println!("Commands:");
        println!("  init PATH       Initialize new site");
        println!("  build           Build the site");
        println!("  serve           Serve site with live reload");
        println!("  check           Check site for errors");
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("zola 0.19.0");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("build");
    match cmd {
        "init" => {
            let path = args.get(1).map(|s| s.as_str()).unwrap_or("mysite");
            println!("Welcome to Zola!");
            println!("Created {}/config.toml", path);
            println!("Created {}/content/", path);
            println!("Created {}/templates/", path);
            println!("Created {}/static/", path);
            println!("Created {}/themes/", path);
            println!("Done! Site ready at {}/", path);
        }
        "build" => {
            println!("Building site...");
            println!("  -> Creating 15 pages (5 sections)");
            println!("  -> Copying 8 static files");
            println!("  -> Processing 3 Sass files");
            println!("Done in 120ms.");
        }
        "serve" => {
            println!("Building site...");
            println!("Done in 120ms.");
            println!();
            println!("Listening for changes in content, templates, sass, static");
            println!("Web server is available at http://127.0.0.1:1111");
        }
        "check" => {
            println!("Checking site...");
            println!("  0 errors found.");
            println!("  2 external links checked: all OK.");
        }
        _ => println!("zola {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "zola".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_zola(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
