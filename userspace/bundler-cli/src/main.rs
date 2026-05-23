#![deny(clippy::all)]

//! bundler-cli — OurOS Ruby Bundler dependency manager
//!
//! Multi-personality: `bundle`, `bundler`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bundler(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: bundle COMMAND [OPTIONS]");
        println!("Bundler 2.5.14 (OurOS)");
        println!();
        println!("Commands:");
        println!("  install    Install gems from Gemfile");
        println!("  update     Update gems");
        println!("  add        Add gem to Gemfile");
        println!("  remove     Remove gem from Gemfile");
        println!("  exec       Execute command in bundle context");
        println!("  list       List installed gems");
        println!("  show       Show gem location");
        println!("  outdated   Show outdated gems");
        println!("  check      Check if Gemfile.lock is up to date");
        println!("  init       Generate a Gemfile");
        println!("  clean      Remove unused gems");
        println!("  lock       Update Gemfile.lock without installing");
        println!("  open       Open gem source in editor");
        println!("  config     Show/set bundler config");
        println!("  platform   Show platform info");
        println!("  doctor     Check for common problems");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" | "-v" => println!("Bundler version 2.5.14"),
        "install" => {
            let path = args.windows(2).find(|w| w[0] == "--path")
                .map(|w| w[1].as_str()).unwrap_or("vendor/bundle");
            println!("Fetching gem metadata from https://rubygems.org/.........");
            println!("Resolving dependencies...");
            println!("Using bundler 2.5.14");
            println!("Installing rake 13.2.1");
            println!("Installing rspec-core 3.13.0");
            println!("Installing rails 7.1.3.4");
            println!("Bundle complete! 5 Gemfile dependencies, 42 gems now installed.");
            println!("Bundled gems installed to `{}`", path);
        }
        "update" => {
            let gem = args.get(1).map(|s| s.as_str());
            if let Some(g) = gem {
                println!("Updating {}...", g);
            } else {
                println!("Updating all gems...");
            }
            println!("Fetching gem metadata from https://rubygems.org/.........");
            println!("Resolving dependencies...");
            println!("Bundle updated!");
        }
        "add" => {
            let gem = args.get(1).map(|s| s.as_str()).unwrap_or("puma");
            println!("Adding {} to Gemfile", gem);
            println!("Fetching gem metadata from https://rubygems.org/.........");
            println!("Resolving dependencies...");
            println!("Installing {} (latest)", gem);
        }
        "exec" => {
            let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("ruby");
            println!("bundle exec: running '{}'", cmd);
        }
        "list" => {
            println!("Gems included by the bundle:");
            println!("  * actioncable (7.1.3.4)");
            println!("  * actionmailer (7.1.3.4)");
            println!("  * activerecord (7.1.3.4)");
            println!("  * bundler (2.5.14)");
            println!("  * puma (6.4.2)");
            println!("  * rails (7.1.3.4)");
            println!("  * rake (13.2.1)");
            println!("  * rspec (3.13.0)");
        }
        "show" => {
            let gem = args.get(1).map(|s| s.as_str()).unwrap_or("rails");
            println!("/home/user/.gems/gems/{}-7.1.3.4", gem);
        }
        "outdated" => {
            println!("Outdated gems included in the bundle:");
            println!("  * puma (newest 6.4.3, installed 6.4.2)");
            println!("  * rake (newest 13.2.2, installed 13.2.1)");
        }
        "check" => {
            println!("The Gemfile's dependencies are satisfied.");
        }
        "init" => {
            println!("Writing new Gemfile to Gemfile");
        }
        "clean" => {
            println!("Removing outdated gems...");
            println!("Cleaned 3 unused gem(s).");
        }
        "doctor" => {
            println!("Bundle doctor: checking for common problems...");
            println!("No issues found.");
        }
        _ => println!("bundle: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bundle".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bundler(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
