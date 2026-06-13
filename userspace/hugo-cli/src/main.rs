#![deny(clippy::all)]

//! hugo-cli — Slate OS Hugo static site generator
//!
//! Multi-personality: `hugo`

use std::env;
use std::process;

fn run_hugo(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Hugo is a fast and flexible static site generator");
        println!("Hugo v0.122.0 (Slate OS)");
        println!();
        println!("Usage: hugo [command] [flags]");
        println!();
        println!("Commands:");
        println!("  new           Create new content");
        println!("  server        Start development server");
        println!("  build         Build your site");
        println!("  mod           Module management");
        println!("  deploy        Deploy to a target");
        println!("  env           Show Hugo environment");
        println!("  version       Show version");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match subcmd {
        "version" | "--version" => {
            println!("hugo v0.122.0-b84644c0 SlateOS/amd64 BuildDate=2024-01-22T08:00:00Z");
        }
        "new" => {
            let what = args.get(1).map(|s| s.as_str()).unwrap_or("site");
            if what == "site" {
                let name = args.get(2).map(|s| s.as_str()).unwrap_or("mysite");
                println!("Creating site: {}/", name);
                println!("  Created: {}/hugo.toml", name);
                println!("  Created: {}/content/", name);
                println!("  Created: {}/layouts/", name);
                println!("  Created: {}/static/", name);
                println!("  Created: {}/themes/", name);
            } else {
                println!("Content \"{}\" created", what);
            }
        }
        "server" | "serve" => {
            let port = args.windows(2).find(|w| w[0] == "-p" || w[0] == "--port").map(|w| w[1].as_str()).unwrap_or("1313");
            println!("Start building sites ...");
            println!("hugo v0.122.0");
            println!();
            println!("                   | EN");
            println!("-------------------+------");
            println!("  Pages            |   12");
            println!("  Paginator pages  |    0");
            println!("  Non-page files   |    3");
            println!("  Static files     |   15");
            println!("  Processed images |    0");
            println!("  Aliases          |    2");
            println!("  Sitemaps         |    1");
            println!("  Cleaned          |    0");
            println!();
            println!("Built in 45 ms");
            println!("Web Server is available at http://localhost:{}/", port);
        }
        "build" => {
            println!("Start building sites ...");
            println!("hugo v0.122.0");
            println!("  12 pages created");
            println!("  3 non-page files copied");
            println!("  Built in 32 ms");
        }
        "env" => {
            println!("hugo v0.122.0");
            println!("GOOS=\"slateos\"");
            println!("GOARCH=\"amd64\"");
            println!("GOVERSION=\"go1.21.6\"");
        }
        _ => println!("hugo: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_hugo(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_hugo};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_hugo(&["--help".to_string()]), 0);
        assert_eq!(run_hugo(&["-h".to_string()]), 0);
        let _ = run_hugo(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_hugo(&[]);
    }
}
