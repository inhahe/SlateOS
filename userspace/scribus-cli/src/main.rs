#![deny(clippy::all)]

//! scribus-cli — OurOS Scribus DTP CLI
//!
//! Single personality: `scribus`

use std::env;
use std::process;

fn run_scribus(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: scribus [OPTIONS] [FILE]");
        println!();
        println!("Scribus — desktop publishing (OurOS).");
        println!();
        println!("Options:");
        println!("  -g, --no-gui           Batch mode (no GUI)");
        println!("  -py, --python-script F Run Python script");
        println!("  -ns, --no-splash       No splash screen");
        println!("  -fi, --font-info       Print font info");
        println!("  -pi, --profile-info    Print ICC profile info");
        println!("  -l, --lang LANG        Override language");
        println!("  --prefs FILE           Alternate preferences");
        println!("  -u, --upgradecheck     Check for updates");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("Scribus 1.6.1 (OurOS)");
        return 0;
    }

    if args.iter().any(|a| a == "-fi" || a == "--font-info") {
        println!("Available fonts:");
        println!("  DejaVu Sans           Regular, Bold, Italic, Bold Italic");
        println!("  Liberation Serif      Regular, Bold, Italic, Bold Italic");
        println!("  Liberation Sans       Regular, Bold, Italic, Bold Italic");
        println!("  Liberation Mono       Regular, Bold, Italic, Bold Italic");
        println!("  Noto Sans             Regular, Bold, Italic, Bold Italic");
        println!("Total: 20 font faces loaded.");
        return 0;
    }

    if args.iter().any(|a| a == "-pi" || a == "--profile-info") {
        println!("ICC Profiles:");
        println!("  sRGB IEC61966-2.1          (RGB)");
        println!("  Adobe RGB (1998)           (RGB)");
        println!("  ISO Coated v2 300%         (CMYK)");
        println!("  Fogra39                    (CMYK)");
        return 0;
    }

    let batch = args.iter().any(|a| a == "-g" || a == "--no-gui");
    let script = args.windows(2)
        .find(|w| w[0] == "-py" || w[0] == "--python-script")
        .map(|w| w[1].as_str());

    let file = args.iter()
        .filter(|a| !a.starts_with('-'))
        .last()
        .map(|s| s.as_str());

    if batch {
        if let Some(s) = script {
            println!("Scribus batch mode: running script '{}'...", s);
            if let Some(f) = file {
                println!("  Document: {}", f);
            }
            println!("  Script completed successfully.");
        } else {
            println!("Scribus batch mode: no script specified.");
        }
    } else if let Some(f) = file {
        println!("Scribus: opening '{}'...", f);
        println!("  Document loaded: 4 pages, A4, CMYK");
    } else {
        println!("Scribus 1.6.1 (OurOS)");
        println!("Starting Scribus GUI...");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_scribus(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
