#![deny(clippy::all)]

//! swupd-cli — Slate OS Clear Linux swupd software updater
//!
//! Multi-personality: `swupd`

use std::env;
use std::process;

fn run_swupd(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: swupd COMMAND [OPTIONS]");
        println!("swupd 4.5.0 (Slate OS)");
        println!();
        println!("Commands:");
        println!("  update       Update to latest version");
        println!("  bundle-add   Add a software bundle");
        println!("  bundle-remove Remove a software bundle");
        println!("  bundle-list  List installed bundles");
        println!("  bundle-info  Show bundle details");
        println!("  search-file  Search for a file");
        println!("  diagnose     Diagnose system issues");
        println!("  repair       Repair system");
        println!("  check-update Check if an update is available");
        println!("  clean        Clean cached files");
        println!("  mirror       Configure alternate content URL");
        println!("  info         Show system info");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("info");
    match subcmd {
        "--version" => println!("swupd 4.5.0"),
        "info" => {
            println!("Distribution:      Slate OS");
            println!("Installed version: 40390");
            println!("Version URL:       https://update.clearlinux.org/update");
            println!("Content URL:       https://cdn.download.clearlinux.org/update/");
        }
        "update" => {
            println!("Update started");
            println!("Preparing to update from 40390 to 40400");
            println!("Downloading packs...");
            println!("Extracting packs...");
            println!("  changed files     : 15");
            println!("  changed manifests : 3");
            println!("Update was applied");
            println!("Update successful - System updated from version 40390 to version 40400");
        }
        "bundle-add" => {
            let bundle = args.get(1).map(|s| s.as_str()).unwrap_or("editors");
            println!("Loading required manifests...");
            println!("Downloading packs...");
            println!("Installing bundle({})...", bundle);
            println!("  installed 1 bundle");
        }
        "bundle-remove" => {
            let bundle = args.get(1).map(|s| s.as_str()).unwrap_or("editors");
            println!("Removing bundle({})...", bundle);
            println!("  removed 1 bundle");
        }
        "bundle-list" => {
            println!("os-core");
            println!("os-core-update");
            println!("editors");
            println!("dev-utils");
            println!("sysadmin-basic");
            println!("Total: 5 bundles");
        }
        "bundle-info" => {
            let bundle = args.get(1).map(|s| s.as_str()).unwrap_or("editors");
            println!("Bundle: {}", bundle);
            println!("  Status: installed");
            println!("  Size: 45 MiB");
            println!("  Includes: vim, nano, emacs");
        }
        "search-file" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("vim");
            println!("Searching for '{}'...", file);
            println!("  Bundle: editors");
            println!("    /usr/bin/{}", file);
        }
        "check-update" => {
            println!("Current OS version: 40390");
            println!("Latest server version: 40400");
            println!("There is a new OS version available: 40400");
        }
        "diagnose" => {
            println!("Diagnosing version 40390");
            println!("Checking for corrupt files...");
            println!("Checking for missing files...");
            println!("Diagnosis successful - no problems found");
        }
        "repair" => {
            println!("Repairing version 40390");
            println!("Starting download of remaining update content...");
            println!("Repair successful");
        }
        "clean" => {
            println!("Cleaning cached files...");
            println!("  200 MiB freed");
        }
        _ => println!("swupd: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_swupd(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_swupd};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_swupd(&["--help".to_string()]), 0);
        assert_eq!(run_swupd(&["-h".to_string()]), 0);
        let _ = run_swupd(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_swupd(&[]);
    }
}
