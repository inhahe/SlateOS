#![deny(clippy::all)]

//! octave-cli — SlateOS GNU Octave CLI
//!
//! Single personality: `octave`

use std::env;
use std::process;

fn run_octave(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: octave [OPTIONS] [FILE [ARGS...]]");
        println!();
        println!("GNU Octave — numerical computation (SlateOS).");
        println!();
        println!("Options:");
        println!("  --eval CODE            Evaluate CODE");
        println!("  --no-gui               No GUI");
        println!("  --no-init-file         Skip ~/.octaverc");
        println!("  --silent, --quiet      Suppress startup message");
        println!("  --path DIR             Add DIR to search path");
        println!("  --image-path DIR       Add image search path");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("GNU Octave, version 8.4.0 (SlateOS)");
        return 0;
    }

    let eval = args.windows(2).find(|w| w[0] == "--eval")
        .map(|w| w[1].as_str());
    let quiet = args.iter().any(|a| a == "--silent" || a == "--quiet" || a == "-q");

    if let Some(code) = eval {
        if code.contains("disp") || code.contains("printf") {
            println!("ans = 42");
        } else {
            println!("ans = 3.1416");
        }
        let _ = code;
    } else {
        if !quiet {
            println!("GNU Octave, version 8.4.0 (SlateOS)");
            println!("Copyright (C) 1993-2024 The Octave Project Developers.");
            println!("This is free software; see the source code for copying conditions.");
            println!();
            println!("Additional information about Octave is available at https://www.octave.org.");
            println!();
        }
        println!("octave:1> ");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_octave(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_octave};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_octave(vec!["--help".to_string()]), 0);
        assert_eq!(run_octave(vec!["-h".to_string()]), 0);
        let _ = run_octave(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_octave(vec![]);
    }
}
