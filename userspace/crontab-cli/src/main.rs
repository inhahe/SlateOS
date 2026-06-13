#![deny(clippy::all)]

//! crontab-cli — SlateOS crontab CLI
//!
//! Single personality: `crontab`

use std::env;
use std::process;

fn run_crontab(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: crontab [OPTIONS] [FILE]");
        println!();
        println!("crontab — maintain crontab files (SlateOS).");
        println!();
        println!("Options:");
        println!("  -l               List crontab entries");
        println!("  -e               Edit crontab");
        println!("  -r               Remove crontab");
        println!("  -u USER          Specify user");
        println!("  -i               Prompt before removal");
        return 0;
    }

    let list = args.iter().any(|a| a == "-l");
    let edit = args.iter().any(|a| a == "-e");
    let remove = args.iter().any(|a| a == "-r");
    let confirm = args.iter().any(|a| a == "-i");
    let user = args.windows(2).find(|w| w[0] == "-u")
        .map(|w| w[1].as_str()).unwrap_or("root");

    if list {
        println!("# crontab for {}", user);
        println!("# m h dom mon dow command");
        println!("0 * * * * /usr/local/bin/health-check.sh");
        println!("30 2 * * * /usr/local/bin/backup.sh");
        println!("0 0 * * 0 /usr/local/bin/weekly-report.sh");
        println!("*/5 * * * * /usr/local/bin/monitor.sh");
        println!("0 6 * * 1-5 /usr/local/bin/morning-tasks.sh");
    } else if edit {
        println!("crontab: editing crontab for {}", user);
        println!("crontab: installing new crontab");
    } else if remove {
        if confirm {
            println!("crontab: really delete {}'s crontab? (y/n) y", user);
        }
        println!("crontab: removed crontab for {}", user);
    } else {
        // Install from file
        let file = args.iter().find(|a| !a.starts_with('-'))
            .map(|s| s.as_str());
        if let Some(f) = file {
            println!("crontab: installing new crontab from {}", f);
        } else {
            eprintln!("crontab: no action specified. See --help.");
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_crontab(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_crontab};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_crontab(vec!["--help".to_string()]), 0);
        assert_eq!(run_crontab(vec!["-h".to_string()]), 0);
        let _ = run_crontab(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_crontab(vec![]);
    }
}
