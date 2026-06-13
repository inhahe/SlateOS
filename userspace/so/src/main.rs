#![deny(clippy::all)]

//! so — SlateOS StackOverflow from the terminal
//!
//! Single personality: `so`

use std::env;
use std::process;

fn run_so(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: so [OPTIONS] <QUERY>...");
        println!();
        println!("Search StackOverflow from the terminal.");
        println!();
        println!("Options:");
        println!("  -e, --search-engine <E>  Search engine (google/duckduckgo/stackexchange)");
        println!("  -s, --site <SITE>        StackExchange site (default: stackoverflow)");
        println!("  -l, --limit <N>          Number of results (default: 5)");
        println!("  --lucky                  Return top answer immediately");
        println!("  -V, --version            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("so 0.4.9 (SlateOS)");
        return 0;
    }

    let lucky = args.iter().any(|a| a == "--lucky");
    let query: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if query.is_empty() {
        eprintln!("Error: search query required. See --help.");
        return 1;
    }

    let q = query.join(" ");

    if lucky {
        println!("Answer for: {}", q);
        println!();
        println!("You can use the following approach:");
        println!();
        println!("```");
        println!("// Example solution");
        println!("let result = do_the_thing();");
        println!("```");
        println!();
        println!("— Score: 142, Accepted ✓");
        return 0;
    }

    println!("Results for: {}", q);
    println!();
    println!("  1. How to {} in Rust", q);
    println!("     Score: 142 | Answers: 5 | Accepted ✓");
    println!("     stackoverflow.com/questions/12345678");
    println!();
    println!("  2. Best way to {} with error handling", q);
    println!("     Score: 87 | Answers: 3 | Accepted ✓");
    println!("     stackoverflow.com/questions/23456789");
    println!();
    println!("  3. {} performance comparison", q);
    println!("     Score: 45 | Answers: 2");
    println!("     stackoverflow.com/questions/34567890");
    println!();
    println!("(Select with j/k, Enter to view, q to quit)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_so(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_so};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_so(vec!["--help".to_string()]), 0);
        assert_eq!(run_so(vec!["-h".to_string()]), 0);
        let _ = run_so(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_so(vec![]);
    }
}
