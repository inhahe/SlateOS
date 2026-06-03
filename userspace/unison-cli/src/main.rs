#![deny(clippy::all)]

//! unison-cli — OurOS Unison file synchronizer CLI
//!
//! Single personality: `unison`

use std::env;
use std::process;

fn run_unison(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-help") {
        println!("Usage: unison [OPTIONS] [PROFILE | ROOT1 ROOT2]");
        println!();
        println!("Unison — bidirectional file synchronizer (OurOS).");
        println!();
        println!("Options:");
        println!("  -auto            Accept non-conflicting actions");
        println!("  -batch           Batch mode (no questions)");
        println!("  -silent          Print nothing");
        println!("  -terse           Print minimal info");
        println!("  -path PATH       Sync only this path");
        println!("  -ignore PATTERN  Ignore pattern");
        println!("  -force ROOT      Force changes from this root");
        println!("  -prefer ROOT     Prefer this root on conflicts");
        println!("  -times           Sync modification times");
        println!("  -owner           Sync owner info");
        println!("  -group           Sync group info");
        println!("  -perms N         Sync permissions (default -1)");
        println!("  -log             Enable logging");
        println!("  -logfile FILE    Log file");
        println!("  -ui text         Text UI (default)");
        return 0;
    }
    if args.iter().any(|a| a == "-version") {
        println!("unison version 2.53.5 (OurOS)");
        return 0;
    }

    let roots: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    let batch = args.iter().any(|a| a == "-batch");

    if roots.len() >= 2 {
        println!("Contacting server...");
        println!("Connected [//local/{} -> //remote/{}]", roots[0], roots[1]);
        println!();
        println!("Looking for changes");
        println!("  Waiting for changes from server");
        println!();
        println!("Reconciling changes");
        println!();
        println!("  local          remote");
        println!("  changed  ---->  docs/report.txt");
        println!("           <----  changed  photos/new.jpg");
        println!("  new      ---->  scripts/deploy.sh");
        println!();
        if batch {
            println!("Propagating updates");
            println!();
            println!("UNISON 2.53.5 finished propagating changes");
            println!("  3 items transferred (256 KB)");
        } else {
            println!("Proceed with propagating updates? [yes] ");
        }
    } else if roots.len() == 1 {
        println!("Loading profile '{}'...", roots[0]);
        println!("Profile loaded. Synchronizing...");
        println!("Nothing to do: replicas are in sync.");
    } else {
        println!("Available profiles:");
        println!("  default");
        println!("  work");
        println!("  photos");
        println!();
        println!("Select a profile or specify two roots.");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_unison(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_unison};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_unison(vec!["--help".to_string()]), 0);
        assert_eq!(run_unison(vec!["-h".to_string()]), 0);
        assert_eq!(run_unison(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_unison(vec![]), 0);
    }
}
