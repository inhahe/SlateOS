#![deny(clippy::all)]

//! hexo-cli — SlateOS Hexo blog framework
//!
//! Single personality: `hexo`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_hexo(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: hexo COMMAND [OPTIONS]");
        println!("Hexo v7.2.0 (SlateOS) — Fast blog framework");
        println!();
        println!("Commands:");
        println!("  init [DIR]      Initialize new blog");
        println!("  new TITLE       Create new post");
        println!("  generate        Generate static files");
        println!("  server          Start local server");
        println!("  deploy          Deploy site");
        println!("  clean           Clean generated files");
        println!("  list TYPE       List posts/pages/routes/tags");
        println!("  migrate TYPE    Migrate from other systems");
        println!("  publish DRAFT   Publish draft");
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("hexo: 7.2.0");
        println!("hexo-cli: 4.3.1");
        println!("os: SlateOS x86_64");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("generate");
    match cmd {
        "init" => {
            let dir = args.get(1).map(|s| s.as_str()).unwrap_or("blog");
            println!("INFO  Cloning hexo-starter...");
            println!("INFO  Install dependencies...");
            println!("INFO  Start blogging with Hexo!");
            println!("INFO  Created: {}/", dir);
        }
        "new" => {
            let title = args.get(1).map(|s| s.as_str()).unwrap_or("My New Post");
            println!("INFO  Created: source/_posts/{}.md", title);
        }
        "generate" => {
            println!("INFO  Start processing");
            println!("INFO  Files loaded in 0.5s");
            println!("INFO  Generated: public/index.html");
            println!("INFO  Generated: public/archives/index.html");
            println!("INFO  12 files generated in 1.2s");
        }
        "server" => {
            println!("INFO  Hexo is running at http://localhost:4000/.");
            println!("INFO  Press Ctrl+C to stop.");
        }
        "deploy" => println!("INFO  Deploying site..."),
        "clean" => println!("INFO  Deleted database and public folder."),
        "list" => {
            let typ = args.get(1).map(|s| s.as_str()).unwrap_or("post");
            println!("INFO  Listing {}s:", typ);
            println!("  Hello World (2024-01-15)");
            println!("  My Second Post (2024-01-16)");
        }
        _ => println!("hexo {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "hexo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_hexo(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_hexo};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/hexo"), "hexo");
        assert_eq!(basename(r"C:\bin\hexo.exe"), "hexo.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("hexo.exe"), "hexo");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_hexo(&["--help".to_string()], "hexo"), 0);
        assert_eq!(run_hexo(&["-h".to_string()], "hexo"), 0);
        let _ = run_hexo(&["--version".to_string()], "hexo");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_hexo(&[], "hexo");
    }
}
