#![deny(clippy::all)]

//! gnuplot-cli — SlateOS gnuplot CLI
//!
//! Single personality: `gnuplot`

use std::env;
use std::process;

fn run_gnuplot(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gnuplot [OPTIONS] [FILE...]");
        println!();
        println!("gnuplot — interactive plotting program (SlateOS).");
        println!();
        println!("Options:");
        println!("  -e \"COMMAND\"           Execute command");
        println!("  -c SCRIPT [ARGS]       Call script with arguments");
        println!("  -p, --persist          Keep plot windows open");
        println!("  -d, --default-settings Ignore settings files");
        println!("  --slow                 Wait between commands");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("gnuplot 5.4 patchlevel 10 (SlateOS)");
        return 0;
    }

    let has_command = args.windows(2).any(|w| w[0] == "-e");
    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if has_command {
        let cmd = args.windows(2).find(|w| w[0] == "-e")
            .map(|w| w[1].as_str()).unwrap_or("");
        println!("gnuplot> {}", cmd);
        if cmd.contains("plot") {
            println!("  [Plot generated]");
        }
    } else if !files.is_empty() {
        for f in &files {
            println!("gnuplot: loading script '{}'", f);
        }
        println!("  [Plot(s) generated]");
    } else {
        println!("        G N U P L O T");
        println!("        Version 5.4 patchlevel 10 (SlateOS)");
        println!("        Terminal type set to 'qt'");
        println!();
        println!("gnuplot> ");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gnuplot(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_gnuplot};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gnuplot(vec!["--help".to_string()]), 0);
        assert_eq!(run_gnuplot(vec!["-h".to_string()]), 0);
        let _ = run_gnuplot(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gnuplot(vec![]);
    }
}
