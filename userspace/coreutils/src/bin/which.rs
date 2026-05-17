//! which — locate a command.
//!
//! Usage: which COMMAND...
//!   Searches PATH for each COMMAND and prints the first match.

use std::env;
use std::path::PathBuf;
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        process::exit(1);
    }

    let path_var = env::var("PATH").unwrap_or_default();
    let dirs: Vec<&str> = path_var.split(':').collect();

    let mut failed = false;
    for cmd in &args {
        // If it contains a slash, check it directly
        if cmd.contains('/') {
            let p = PathBuf::from(cmd);
            if p.exists() {
                println!("{}", p.display());
                continue;
            }
            failed = true;
            continue;
        }

        let mut found = false;
        for dir in &dirs {
            let candidate = PathBuf::from(dir).join(cmd);
            if candidate.exists() {
                println!("{}", candidate.display());
                found = true;
                break;
            }
        }
        if !found {
            eprintln!("which: no {cmd} in ({path_var})");
            failed = true;
        }
    }

    if failed {
        process::exit(1);
    }
}
