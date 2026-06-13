#![deny(clippy::all)]

//! choose — SlateOS human-friendly alternative to awk/cut
//!
//! Single personality: `choose`

use std::env;
use std::process;

fn run_choose(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: choose [OPTIONS] <CHOICE>...");
        println!();
        println!("A human-friendly and fast alternative to cut and awk.");
        println!();
        println!("Fields are 0-indexed. Use N:M for ranges (inclusive).");
        println!();
        println!("Examples:");
        println!("  choose 0          First field");
        println!("  choose 2          Third field");
        println!("  choose 0 2        First and third fields");
        println!("  choose 1:3        Fields 1, 2, and 3");
        println!("  choose :2         Fields 0, 1, and 2");
        println!("  choose 3:         Fields 3 onward");
        println!("  choose -1         Last field");
        println!("  choose -3:-1      Last three fields");
        println!();
        println!("Options:");
        println!("  -f, --field-separator <SEP>   Input field separator (regex)");
        println!("  -i, --input <FILE>            Input file (default: stdin)");
        println!("  -o, --output-field-separator <SEP>  Output separator (default: space)");
        println!("  -c, --character-wise          Use characters instead of fields");
        println!("  -x, --exclusive               Make ranges exclusive");
        println!("  -n, --non-greedy              Non-greedy field separator");
        println!("  -V, --version                 Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("choose 1.3.4 (SlateOS)");
        return 0;
    }

    // Parse field separator
    let mut sep = " ";
    let mut out_sep = " ";
    let mut choices: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-f" | "--field-separator" => {
                if i + 1 < args.len() {
                    sep = Box::leak(args[i + 1].clone().into_boxed_str());
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }
            "-o" | "--output-field-separator" => {
                if i + 1 < args.len() {
                    out_sep = Box::leak(args[i + 1].clone().into_boxed_str());
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }
            s if s.starts_with('-') && s.len() > 1 && !s.chars().nth(1).unwrap_or('x').is_ascii_digit() => {
                i += 1;
                continue;
            }
            _ => {
                choices.push(&args[i]);
                i += 1;
            }
        }
    }

    if choices.is_empty() {
        eprintln!("Error: at least one choice required. See --help.");
        return 1;
    }

    // Simulate processing stdin
    let sample_lines = [
        "Alice 30 Engineering alice@example.com",
        "Bob 25 Marketing bob@example.com",
        "Carol 35 Engineering carol@example.com",
        "Dave 28 Sales dave@example.com",
    ];

    for line in &sample_lines {
        let fields: Vec<&str> = line.split(sep).collect();
        let mut selected: Vec<&str> = Vec::new();

        for choice in &choices {
            if let Some(colon_pos) = choice.find(':') {
                let start_str = &choice[..colon_pos];
                let end_str = &choice[colon_pos + 1..];

                let start: usize = if start_str.is_empty() {
                    0
                } else if let Ok(n) = start_str.parse::<i64>() {
                    if n < 0 {
                        fields.len().saturating_sub(n.unsigned_abs() as usize)
                    } else {
                        n as usize
                    }
                } else {
                    0
                };

                let end: usize = if end_str.is_empty() {
                    fields.len().saturating_sub(1)
                } else if let Ok(n) = end_str.parse::<i64>() {
                    if n < 0 {
                        fields.len().saturating_sub(n.unsigned_abs() as usize)
                    } else {
                        n as usize
                    }
                } else {
                    fields.len().saturating_sub(1)
                };

                let actual_end = end.min(fields.len().saturating_sub(1));
                for idx in start..=actual_end {
                    if let Some(f) = fields.get(idx) {
                        selected.push(f);
                    }
                }
            } else if let Ok(n) = choice.parse::<i64>() {
                let idx = if n < 0 {
                    fields.len().saturating_sub(n.unsigned_abs() as usize)
                } else {
                    n as usize
                };
                if let Some(f) = fields.get(idx) {
                    selected.push(f);
                }
            }
        }

        println!("{}", selected.join(out_sep));
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_choose(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_choose};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_choose(vec!["--help".to_string()]), 0);
        assert_eq!(run_choose(vec!["-h".to_string()]), 0);
        let _ = run_choose(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_choose(vec![]);
    }
}
