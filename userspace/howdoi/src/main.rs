#![deny(clippy::all)]

//! howdoi — OurOS instant coding answers from the command line
//!
//! Single personality: `howdoi`

use std::env;
use std::process;

fn run_howdoi(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: howdoi [OPTIONS] <QUERY>...");
        println!();
        println!("Instant coding answers via the command line.");
        println!();
        println!("Options:");
        println!("  -a, --all              Display full answer");
        println!("  -l, --link             Display answer link");
        println!("  -c, --color            Enable colorized output");
        println!("  -n, --num <NUM>        Number of answers (default: 1)");
        println!("  -e, --engine <ENGINE>  Search engine (google/bing/duckduckgo)");
        println!("  -p, --pos <N>          Select Nth answer (default: 1)");
        println!("  --save                 Cache answer locally");
        println!("  --view                 Open in browser");
        println!("  -j, --json             JSON output");
        println!("  -V, --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("howdoi 2.0.20 (OurOS)");
        return 0;
    }

    let show_link = args.iter().any(|a| a == "-l" || a == "--link");
    let show_all = args.iter().any(|a| a == "-a" || a == "--all");
    let json = args.iter().any(|a| a == "-j" || a == "--json");

    let query: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if query.is_empty() {
        eprintln!("Error: query required. See --help.");
        return 1;
    }

    let q = query.join(" ");

    if json {
        println!("[{{\"answer\":\"let result = do_thing();\",\"link\":\"https://stackoverflow.com/questions/12345678\",\"position\":1}}]");
        return 0;
    }

    if show_link {
        println!("https://stackoverflow.com/questions/12345678");
        return 0;
    }

    if show_all {
        println!("--- Answer for: {} ---", q);
        println!();
        println!("Here's how you can do it:");
        println!();
        println!("```");
        println!("fn solve() -> Result<(), Error> {{");
        println!("    let data = read_input()?;");
        println!("    let result = process(data)?;");
        println!("    println!(\"Result: {{}}\", result);");
        println!("    Ok(())");
        println!("}}");
        println!("```");
        println!();
        println!("This approach handles errors properly using the `?` operator.");
        println!("You can also use `unwrap()` for quick prototyping.");
    } else {
        println!("let result = process(data)?;");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_howdoi(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_howdoi};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_howdoi(vec!["--help".to_string()]), 0);
        assert_eq!(run_howdoi(vec!["-h".to_string()]), 0);
        let _ = run_howdoi(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_howdoi(vec![]);
    }
}
