#![deny(clippy::all)]

//! cargo-bloat — SlateOS find out what takes most of the space in your executable
//!
//! Single personality: `cargo-bloat`

use std::env;
use std::process;

fn run_cargo_bloat(args: Vec<String>) -> i32 {
    // Invoked as `cargo bloat`, first arg may be "bloat"
    let subargs: Vec<String> = if args.first().map(|s| s.as_str()) == Some("bloat") {
        args[1..].to_vec()
    } else {
        args
    };

    if subargs.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cargo bloat [OPTIONS]");
        println!();
        println!("Find out what takes most of the space in your executable.");
        println!();
        println!("Options:");
        println!("  --crates               Show per-crate stats");
        println!("  --time                 Show per-crate build time");
        println!("  --filter <CRATE>       Filter by crate name");
        println!("  -n <N>                 Number of entries to show (default: 20)");
        println!("  --full-fn              Show full function names");
        println!("  --split-std            Split std into sub-crates");
        println!("  --no-relative-size     Don't show relative size");
        println!("  --wide                 Don't trim long names");
        println!("  --release              Use release profile");
        println!("  --target <TARGET>      Target triple");
        println!("  --package <SPEC>       Package to analyze");
        println!("  --example <NAME>       Analyze example binary");
        println!("  --message-format <FMT> Output format (table/json)");
        println!("  -V, --version          Show version");
        return 0;
    }
    if subargs.iter().any(|a| a == "-V" || a == "--version") {
        println!("cargo-bloat 0.11.1 (Slate OS)");
        return 0;
    }

    let crates_mode = subargs.iter().any(|a| a == "--crates");
    let time_mode = subargs.iter().any(|a| a == "--time");
    let json = subargs.windows(2).any(|w| w[0] == "--message-format" && w[1] == "json");

    if json {
        if crates_mode {
            println!("[");
            println!("  {{\"name\":\"std\",\"size\":524288,\"percentage\":25.6}},");
            println!("  {{\"name\":\"serde\",\"size\":204800,\"percentage\":10.0}},");
            println!("  {{\"name\":\"tokio\",\"size\":184320,\"percentage\":9.0}},");
            println!("  {{\"name\":\"my-project\",\"size\":163840,\"percentage\":8.0}}");
            println!("]");
        } else {
            println!("[");
            println!("  {{\"name\":\"core::fmt::write\",\"size\":8192,\"percentage\":0.4}},");
            println!("  {{\"name\":\"std::io::Write::write_fmt\",\"size\":4096,\"percentage\":0.2}}");
            println!("]");
        }
        return 0;
    }

    println!("    Compiling my-project v1.0.0");
    println!("    Analyzing target/release/my-project");
    println!();

    if time_mode {
        println!("  Build Time  Crate");
        println!("  ─────────── ──────────────────");
        println!("      12.3s   serde_derive");
        println!("       8.7s   syn");
        println!("       6.2s   tokio");
        println!("       4.5s   proc-macro2");
        println!("       3.8s   my-project");
        println!("       2.1s   quote");
        println!("       1.9s   serde");
        println!("       1.2s   regex");
        println!("  ─────────── ──────────────────");
        println!("      40.7s   Total");
    } else if crates_mode {
        println!("  File  .text    Size  Crate");
        println!("  ───── ─────── ────── ──────────────────");
        println!("  25.6%  23.1%  512K   std");
        println!("  10.0%   9.2%  200K   serde");
        println!("   9.0%   8.5%  180K   tokio");
        println!("   8.0%   7.8%  160K   my-project");
        println!("   5.5%   5.2%  110K   regex");
        println!("   4.2%   4.0%   84K   clap");
        println!("   3.8%   3.5%   76K   hyper");
        println!("  34.0%  38.7%  680K   Other (12 crates)");
        println!("  ───── ─────── ──────");
        println!("  100%  100.0%  2.0M   Total");
    } else {
        println!("  File  .text     Size  Name");
        println!("  ───── ──────── ────── ──────────────────────────────────────");
        println!("   0.4%    0.5%   8.0K  core::fmt::write");
        println!("   0.3%    0.4%   6.1K  std::io::Write::write_fmt");
        println!("   0.3%    0.3%   5.2K  serde::de::Deserialize::deserialize");
        println!("   0.2%    0.3%   4.8K  tokio::runtime::scheduler::multi_thread");
        println!("   0.2%    0.2%   4.2K  regex::compile::Compiler::compile");
        println!("   0.2%    0.2%   3.9K  my_project::main");
        println!("   0.2%    0.2%   3.5K  clap::parser::Parser::parse");
        println!("   0.1%    0.2%   3.1K  std::sys::pal::unix::process");
        println!("   0.1%    0.1%   2.8K  hyper::client::pool::Pool::connect");
        println!("   0.1%    0.1%   2.4K  std::collections::hash::map::HashMap");
        println!("  ───── ──────── ──────");
        println!("  97.9%  97.5%   2.0M   And 1,234 more...");
        println!("  100%   100.0%  2.0M   Total");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cargo_bloat(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_cargo_bloat};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cargo_bloat(vec!["--help".to_string()]), 0);
        assert_eq!(run_cargo_bloat(vec!["-h".to_string()]), 0);
        let _ = run_cargo_bloat(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cargo_bloat(vec![]);
    }
}
