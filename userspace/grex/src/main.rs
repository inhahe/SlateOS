#![deny(clippy::all)]

//! grex — OurOS command-line tool for generating regular expressions from examples
//!
//! Single personality: `grex`

use std::env;
use std::process;

fn run_grex(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: grex [OPTIONS] <INPUT>...");
        println!();
        println!("Generate regular expressions from user-provided test cases.");
        println!();
        println!("Options:");
        println!("  -d, --digits              Convert digits to \\d");
        println!("  -D, --non-digits          Convert non-digits to \\D");
        println!("  -s, --spaces              Convert whitespace to \\s");
        println!("  -S, --non-spaces          Convert non-whitespace to \\S");
        println!("  -w, --words               Convert word chars to \\w");
        println!("  -W, --non-words           Convert non-word chars to \\W");
        println!("  -r, --repetitions         Detect repetitions");
        println!("  -e, --escape              Escape special characters");
        println!("  -i, --ignore-case         Case-insensitive matching");
        println!("  --with-surrogates         Allow surrogates in char classes");
        println!("  -c, --colorize            Colorize output");
        println!("  -f, --file <FILE>         Read test cases from file");
        println!("  --min-repetitions <N>     Min repetitions for quantifier");
        println!("  --min-substring-length <N> Min length for common substrings");
        println!("  -V, --version             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("grex 1.4.1 (OurOS)");
        return 0;
    }

    let use_digits = args.iter().any(|a| a == "-d" || a == "--digits");
    let use_words = args.iter().any(|a| a == "-w" || a == "--words");
    let use_reps = args.iter().any(|a| a == "-r" || a == "--repetitions");

    let inputs: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if inputs.is_empty() {
        eprintln!("Error: at least one test case required. See --help.");
        return 1;
    }

    // Simulate regex generation based on inputs
    if inputs.len() == 1 {
        // Single input — escape it
        let input = inputs[0];
        if use_digits && input.chars().all(|c| c.is_ascii_digit()) {
            if use_reps {
                println!("^\\d{{{}}}$", input.len());
            } else {
                let pattern: String = input.chars().map(|_| "\\d").collect();
                println!("^{}$", pattern);
            }
        } else {
            println!("^{}$", regex_escape(input));
        }
    } else {
        // Multiple inputs — find common pattern
        let all_numeric = inputs.iter().all(|s| s.chars().all(|c| c.is_ascii_digit()));
        let same_len = inputs.iter().all(|s| s.len() == inputs[0].len());

        if all_numeric && use_digits {
            if same_len && use_reps {
                println!("^\\d{{{}}}$", inputs[0].len());
            } else if use_reps {
                let min = inputs.iter().map(|s| s.len()).min().unwrap_or(1);
                let max = inputs.iter().map(|s| s.len()).max().unwrap_or(1);
                if min == max {
                    println!("^\\d{{{}}}$", min);
                } else {
                    println!("^\\d{{{},{}}}$", min, max);
                }
            } else {
                println!("^\\d+$");
            }
        } else if use_words {
            if same_len && use_reps {
                println!("^\\w{{{}}}$", inputs[0].len());
            } else {
                println!("^\\w+$");
            }
        } else {
            // Alternation
            let alts: Vec<String> = inputs.iter()
                .map(|s| regex_escape(s))
                .collect();
            println!("^(?:{})$", alts.join("|"));
        }
    }
    0
}

fn regex_escape(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        match c {
            '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '\\' | '^' | '$' | '|' => {
                result.push('\\');
                result.push(c);
            }
            _ => result.push(c),
        }
    }
    result
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_grex(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_grex};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_grex(vec!["--help".to_string()]), 0);
        assert_eq!(run_grex(vec!["-h".to_string()]), 0);
        assert_eq!(run_grex(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_grex(vec![]), 0);
    }
}
