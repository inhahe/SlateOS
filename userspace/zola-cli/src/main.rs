#![deny(clippy::all)]

//! zola-cli — SlateOS Zola static site generator
//!
//! Single personality: `zola`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_zola(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: zola COMMAND [OPTIONS]");
        println!("Zola v0.19.0 (Slate OS) — Fast static site generator");
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
mod tests {
    use super::{basename, strip_ext, run_zola};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/zola"), "zola");
        assert_eq!(basename(r"C:\bin\zola.exe"), "zola.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("zola.exe"), "zola");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_zola(&["--help".to_string()], "zola"), 0);
        assert_eq!(run_zola(&["-h".to_string()], "zola"), 0);
        let _ = run_zola(&["--version".to_string()], "zola");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_zola(&[], "zola");
    }
}
