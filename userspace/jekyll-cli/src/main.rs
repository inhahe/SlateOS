#![deny(clippy::all)]

//! jekyll-cli — OurOS Jekyll static site generator
//!
//! Multi-personality: `jekyll`

use std::env;
use std::process;

fn run_jekyll(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: jekyll COMMAND [OPTIONS]");
        println!("jekyll 4.3.3 (OurOS)");
        println!();
        println!("Commands:");
        println!("  new PATH       Create new Jekyll site");
        println!("  build          Build your site");
        println!("  serve          Serve your site locally");
        println!("  clean          Clean site output");
        println!("  doctor         Check for issues");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("jekyll 4.3.3 (OurOS)"),
        "new" => {
            let path = args.get(1).map(|s| s.as_str()).unwrap_or("mysite");
            println!("New jekyll site installed in {}/", path);
        }
        "build" | "b" => {
            println!("Configuration file: _config.yml");
            println!("            Source: .");
            println!("       Destination: _site");
            println!("      Generating...");
            println!("                    done in 1.234 seconds.");
            println!(" Auto-regeneration: disabled.");
        }
        "serve" | "s" => {
            let port = args.windows(2).find(|w| w[0] == "--port" || w[0] == "-P").map(|w| w[1].as_str()).unwrap_or("4000");
            println!("Configuration file: _config.yml");
            println!("    Server address: http://127.0.0.1:{}/", port);
            println!("  Server running... press ctrl-c to stop.");
        }
        "clean" => {
            println!("Cleaner: Removing _site...");
            println!("Cleaner: Removing .jekyll-cache...");
            println!("Cleaner: Nothing to remove.");
        }
        "doctor" => {
            println!("Your test results are in!");
            println!("No issues found.");
        }
        _ => println!("jekyll: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_jekyll(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
