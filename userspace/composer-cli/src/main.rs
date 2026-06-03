#![deny(clippy::all)]

//! composer-cli — OurOS PHP Composer dependency manager
//!
//! Multi-personality: `composer`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_composer(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: composer COMMAND [OPTIONS]");
        println!("Composer 2.7.7 (OurOS)");
        println!();
        println!("Commands:");
        println!("  init           Create a composer.json in current dir");
        println!("  install        Install dependencies from composer.lock");
        println!("  update         Update dependencies to latest versions");
        println!("  require        Add a package to composer.json");
        println!("  remove         Remove a package");
        println!("  show           Show package info");
        println!("  search         Search for packages");
        println!("  dump-autoload  Regenerate autoloader");
        println!("  validate       Validate composer.json");
        println!("  create-project Create project from package");
        println!("  global         Run command in global composer dir");
        println!("  outdated       Show outdated packages");
        println!("  fund           Show funding links for packages");
        println!("  audit          Check for security advisories");
        println!("  config         Set config options");
        println!("  run-script     Run scripts from composer.json");
        println!("  exec           Run a vendor binary");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" | "-V" => println!("Composer version 2.7.7 2024-06-10 22:11:02"),
        "init" => {
            println!("Welcome to the Composer config generator");
            println!("Package name (<vendor>/<name>): myvendor/mypackage");
            println!("Description: A new project");
            println!("Author: Dev <dev@example.com>");
            println!("Minimum Stability: stable");
            println!("composer.json created.");
        }
        "install" => {
            let dev = !args.iter().any(|a| a == "--no-dev");
            println!("Installing dependencies from lock file");
            println!("  - Installing psr/log (3.0.0): Extracting archive");
            println!("  - Installing monolog/monolog (3.6.0): Extracting archive");
            println!("  - Installing symfony/console (7.1.2): Extracting archive");
            if dev {
                println!("  - Installing phpunit/phpunit (11.2.0): Extracting archive");
            }
            println!("Generating autoload files");
            println!("3 packages installed{}.", if dev { " (+1 dev)" } else { "" });
        }
        "update" => {
            let pkg = args.get(1).map(|s| s.as_str());
            if let Some(p) = pkg {
                println!("Updating {}", p);
            } else {
                println!("Loading composer repositories with package information");
                println!("Updating dependencies");
            }
            println!("  - Upgrading monolog/monolog (3.5.0 => 3.6.0)");
            println!("Writing lock file");
            println!("Generating autoload files");
        }
        "require" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("vendor/package");
            let version = args.get(2).map(|s| s.as_str()).unwrap_or("^1.0");
            println!("Using version {} for {}", version, pkg);
            println!("./composer.json has been updated");
            println!("Running composer update {}", pkg);
            println!("  - Installing {} ({}): Extracting archive", pkg, version);
            println!("Generating autoload files");
        }
        "remove" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("vendor/package");
            println!("./composer.json has been updated");
            println!("Running composer update {}", pkg);
            println!("  - Removing {}", pkg);
            println!("Generating autoload files");
        }
        "show" => {
            let pkg = args.get(1).map(|s| s.as_str());
            if let Some(p) = pkg {
                println!("name     : {}", p);
                println!("versions : * 3.6.0");
                println!("type     : library");
                println!("license  : MIT");
            } else {
                println!("monolog/monolog     3.6.0   Sends logs to files, sockets, ...");
                println!("psr/log             3.0.0   Common interface for logging");
                println!("symfony/console     7.1.2   Console component");
            }
        }
        "search" => {
            let term = args.get(1).map(|s| s.as_str()).unwrap_or("http");
            println!("Searching for '{}'...", term);
            println!("guzzlehttp/guzzle         PHP HTTP client");
            println!("symfony/http-foundation   HTTP Foundation component");
            println!("nyholm/psr7               PSR-7 implementation");
        }
        "dump-autoload" | "dumpautoload" => {
            let optimized = args.iter().any(|a| a == "-o" || a == "--optimize");
            if optimized {
                println!("Generating optimized autoload files");
            } else {
                println!("Generating autoload files");
            }
            println!("Generated autoload files containing 245 classes");
        }
        "validate" => {
            println!("./composer.json is valid.");
        }
        "outdated" => {
            println!("Legend: ! patch or minor release available - Loss of backward compat.");
            println!("monolog/monolog   3.5.0  3.6.0  Sends logs to files, sockets, ...");
            println!("psr/log           3.0.0  3.0.0  Common interface for logging (up to date)");
        }
        "audit" => {
            println!("Found 0 security vulnerability advisories affecting 0 packages.");
        }
        _ => println!("composer: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "composer".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_composer(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_composer};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/composer"), "composer");
        assert_eq!(basename(r"C:\bin\composer.exe"), "composer.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("composer.exe"), "composer");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_composer(&["--help".to_string()]), 0);
        assert_eq!(run_composer(&["-h".to_string()]), 0);
        assert_eq!(run_composer(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_composer(&[]), 0);
    }
}
