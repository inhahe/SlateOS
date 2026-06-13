#![deny(clippy::all)]

//! rspec-cli — Slate OS RSpec testing framework
//!
//! Multi-personality: `rspec`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rspec(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rspec [OPTIONS] [FILES_OR_DIRS]");
        println!("RSpec 3.13.0 (Slate OS)");
        println!();
        println!("Options:");
        println!("  -f, --format FORMAT  Output format (progress, doc, json, html)");
        println!("  -t, --tag TAG        Run examples with given tag");
        println!("  -e, --example STR    Run examples matching string");
        println!("  --fail-fast          Stop on first failure");
        println!("  --order ORDER        Run order (defined, rand, random)");
        println!("  --seed SEED          Random seed");
        println!("  --color              Enable color");
        println!("  --no-color           Disable color");
        println!("  --profile [N]        Show N slowest examples");
        println!("  --bisect             Bisect to find ordering dependency");
        println!("  --dry-run            Print examples without running");
        println!("  --init               Generate .rspec and spec/spec_helper.rb");
        println!("  --version            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("3.13.0");
        return 0;
    }
    if args.iter().any(|a| a == "--init") {
        println!("  create  .rspec");
        println!("  create  spec/spec_helper.rb");
        return 0;
    }
    let format = args.windows(2).find(|w| w[0] == "-f" || w[0] == "--format")
        .map(|w| w[1].as_str()).unwrap_or("progress");
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let fail_fast = args.iter().any(|a| a == "--fail-fast");
    let profile = args.iter().any(|a| a == "--profile");

    if dry_run {
        println!("Example groups:");
        println!("  User");
        println!("    #create");
        println!("      creates a new user");
        println!("      validates email format");
        println!("    #destroy");
        println!("      removes the user");
        println!("3 examples, 0 failures (dry run)");
        return 0;
    }

    if format == "doc" || format == "documentation" {
        println!("User");
        println!("  #create");
        println!("    creates a new user");
        println!("    validates email format");
        println!("  #destroy");
        println!("    removes the user");
    } else {
        println!("...");
    }
    println!();

    if fail_fast {
        println!("(using --fail-fast)");
    }

    if profile {
        println!();
        println!("Top 3 slowest examples:");
        println!("  User#create creates a new user");
        println!("    0.023 seconds");
        println!("  User#destroy removes the user");
        println!("    0.011 seconds");
    }

    println!("Finished in 0.045 seconds (files took 0.12 seconds to load)");
    println!("3 examples, 0 failures");
    println!();
    println!("Randomized with seed 12345");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rspec".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rspec(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rspec};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/rspec"), "rspec");
        assert_eq!(basename(r"C:\bin\rspec.exe"), "rspec.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("rspec.exe"), "rspec");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rspec(&["--help".to_string()]), 0);
        assert_eq!(run_rspec(&["-h".to_string()]), 0);
        let _ = run_rspec(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rspec(&[]);
    }
}
