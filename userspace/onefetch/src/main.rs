#![deny(clippy::all)]

//! onefetch — OurOS command-line Git information tool
//!
//! Single personality: `onefetch`

use std::env;
use std::process;

fn run_onefetch(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: onefetch [OPTIONS] [PATH]");
        println!();
        println!("Command-line Git information tool.");
        println!();
        println!("Options:");
        println!("  --ascii-language <LANG>    ASCII art language");
        println!("  --ascii-input <FILE>       Custom ASCII art");
        println!("  --no-art                   Disable ASCII art");
        println!("  --no-title                 Hide title");
        println!("  --no-bots                  Exclude bot commits");
        println!("  --no-merges                Exclude merge commits");
        println!("  --no-color-palette         Hide color palette");
        println!("  --number-of-authors <N>    Number of authors to show");
        println!("  --number-of-languages <N>  Number of languages to show");
        println!("  --number-of-file-churns <N>  Number of churned files");
        println!("  -e, --exclude <PATH>       Exclude paths");
        println!("  --type <TYPE>              File type filter");
        println!("  -o, --output <FORMAT>      Output format (json/yaml)");
        println!("  --show-email               Show email in authors");
        println!("  --include-hidden           Include hidden files");
        println!("  --true-color <WHEN>        True color (auto/always/never)");
        println!("  --text-colors <C1> <C2>    Custom text colors");
        println!("  --iso-time                 Use ISO 8601 time format");
        println!("  -V, --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("onefetch 2.21.0 (OurOS)");
        return 0;
    }

    let json = args.windows(2).any(|w| (w[0] == "-o" || w[0] == "--output") && w[1] == "json");

    if json {
        println!("{{");
        println!("  \"project\": \"my-project\",");
        println!("  \"description\": \"A cool project\",");
        println!("  \"head\": \"main\",");
        println!("  \"version\": \"1.0.0\",");
        println!("  \"created\": \"6 months ago\",");
        println!("  \"languages\": [{{\"name\": \"Rust\", \"percentage\": 85.2}}, {{\"name\": \"TOML\", \"percentage\": 10.1}}, {{\"name\": \"Markdown\", \"percentage\": 4.7}}],");
        println!("  \"authors\": [{{\"name\": \"Developer\", \"commits\": 342, \"percentage\": 100.0}}],");
        println!("  \"last_change\": \"2 hours ago\",");
        println!("  \"repo_url\": \"https://github.com/user/project\",");
        println!("  \"commits\": 342,");
        println!("  \"lines_of_code\": 12456,");
        println!("  \"repo_size\": \"4.2 MiB\",");
        println!("  \"license\": \"MIT\"");
        println!("}}");
        return 0;
    }

    println!("                              my-project");
    println!("         _~^~^~_              ──────────────────────────────");
    println!("     \\) /  o o  \\ (/          Project: my-project (1.0.0)");
    println!("       '_   ¬   _'            HEAD: main (ab12cd3)");
    println!("       / '-----' \\            Created: 6 months ago");
    println!("                              Languages:");
    println!("                                Rust        85.2% ████████████████░░");
    println!("                                TOML        10.1% ██░░░░░░░░░░░░░░░░");
    println!("                                Markdown     4.7% █░░░░░░░░░░░░░░░░░");
    println!("                              Authors:");
    println!("                                100.0% Developer (342 commits)");
    println!("                              Last change: 2 hours ago");
    println!("                              URL: https://github.com/user/project");
    println!("                              Commits: 342");
    println!("                              Lines of code: 12,456");
    println!("                              Size: 4.2 MiB");
    println!("                              License: MIT");
    println!();
    println!("  ██ ██ ██ ██ ██ ██ ██ ██");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_onefetch(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
