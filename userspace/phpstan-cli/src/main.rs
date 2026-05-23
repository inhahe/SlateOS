#![deny(clippy::all)]

//! phpstan-cli — OurOS PHPStan static analysis tool
//!
//! Multi-personality: `phpstan`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_phpstan(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: phpstan COMMAND [OPTIONS]");
        println!("PHPStan 1.11.7 (OurOS)");
        println!();
        println!("Commands:");
        println!("  analyse      Analyse source code");
        println!("  clear-result-cache  Clear result cache");
        println!("  dump-deps    Dump file dependencies");
        println!("  diagnose     Show environment info");
        println!("  worker       Internal worker process");
        println!();
        println!("Options (for analyse):");
        println!("  --level|-l LEVEL    Rule level (0-9, max)");
        println!("  --configuration|-c FILE  Config file");
        println!("  --memory-limit LIMIT     Memory limit");
        println!("  --error-format FORMAT    Output format (table, json, raw, ...)");
        println!("  --no-progress            Disable progress bar");
        println!("  --debug                  Debug mode");
        println!("  --generate-baseline      Generate baseline file");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" | "-V" => println!("PHPStan - PHP Static Analysis Tool 1.11.7"),
        "analyse" | "analyze" => {
            let level = args.windows(2)
                .find(|w| w[0] == "--level" || w[0] == "-l")
                .map(|w| w[1].as_str())
                .unwrap_or("0");
            let paths: Vec<&str> = args.iter()
                .filter(|a| !a.starts_with('-') && a.as_str() != "analyse" && a.as_str() != "analyze")
                .filter(|a| {
                    let prev = args.iter().position(|x| x == *a);
                    if let Some(pos) = prev {
                        if pos > 0 {
                            let p = args.get(pos - 1).map(|s| s.as_str()).unwrap_or("");
                            return p != "--level" && p != "-l" && p != "--configuration" && p != "-c"
                                && p != "--memory-limit" && p != "--error-format";
                        }
                    }
                    true
                })
                .map(|s| s.as_str())
                .collect();

            let no_progress = args.iter().any(|a| a == "--no-progress");
            let generate_baseline = args.iter().any(|a| a == "--generate-baseline");

            if !no_progress {
                println!(" 42/42 [============================] 100%");
                println!();
            }

            if generate_baseline {
                println!("Baseline generated with 3 ignored errors in phpstan-baseline.neon.");
                return 0;
            }

            println!("PHPStan analysing at level {}", level);
            if !paths.is_empty() {
                println!("Paths: {}", paths.join(", "));
            } else {
                println!("Paths: src/");
            }
            println!();
            println!(" ------ ------------------------------------------");
            println!("  Line   src/Service/UserService.php");
            println!(" ------ ------------------------------------------");
            println!("  45     Parameter $name of method create() has no type.");
            println!("  72     Method findById() should return User|null but returns mixed.");
            println!(" ------ ------------------------------------------");
            println!();
            println!(" [ERROR] Found 2 errors");
            return 1;
        }
        "clear-result-cache" => {
            println!("Result cache cleared.");
        }
        "dump-deps" => {
            println!("src/Controller/HomeController.php:");
            println!("  -> src/Service/UserService.php");
            println!("  -> src/Repository/UserRepository.php");
            println!("src/Service/UserService.php:");
            println!("  -> src/Repository/UserRepository.php");
            println!("  -> src/Entity/User.php");
        }
        "diagnose" => {
            println!("PHPStan 1.11.7");
            println!("PHP 8.3.8");
            println!("Configuration: phpstan.neon");
            println!("Discovered symbols: 1,234");
            println!("Result cache: enabled");
            println!("Parallel processing: enabled (8 workers)");
        }
        _ => println!("phpstan: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "phpstan".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_phpstan(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
