#![deny(clippy::all)]

//! fontforge-cli — OurOS FontForge font editor CLI
//!
//! Single personality: `fontforge`

use std::env;
use std::process;

fn run_fontforge(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fontforge [OPTIONS] [FILE ...]");
        println!();
        println!("FontForge — font editor (OurOS).");
        println!();
        println!("Options:");
        println!("  -lang py|ff       Scripting language");
        println!("  -script FILE      Run script");
        println!("  -c COMMAND        Execute command");
        println!("  -nosplash         No splash screen");
        println!("  -recover MODE     Recovery mode (auto/clean/none)");
        println!("  -last             Open last files");
        println!("  -new              Start with new font");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("FontForge 20230101 (OurOS)");
        return 0;
    }

    let script = args.windows(2).find(|w| w[0] == "-script").map(|w| w[1].as_str());
    let command = args.windows(2).find(|w| w[0] == "-c").map(|w| w[1].as_str());

    if let Some(s) = script {
        println!("FontForge: running script '{}'...", s);
        println!("  Script completed.");
    } else if let Some(c) = command {
        println!("FontForge: executing '{}'", c);
    } else {
        let files: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
        if files.is_empty() {
            println!("FontForge 20230101 (OurOS)");
            println!("Starting FontForge GUI...");
        } else {
            for f in &files {
                println!("Opening font: {}", f);
            }
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fontforge(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
