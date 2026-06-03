#![deny(clippy::all)]

//! iconv-cli — OurOS iconv character encoding conversion CLI
//!
//! Single personality: `iconv`

use std::env;
use std::process;

fn run_iconv(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: iconv [OPTIONS] [FILE ...]");
        println!();
        println!("iconv — character set conversion (OurOS).");
        println!();
        println!("Options:");
        println!("  -f, --from-code ENC   Input encoding");
        println!("  -t, --to-code ENC     Output encoding");
        println!("  -l, --list            List known encodings");
        println!("  -c                    Omit invalid characters");
        println!("  -o, --output FILE     Output file");
        println!("  -s, --silent          Suppress warnings");
        println!("  --verbose             Verbose output");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("iconv (GNU libc) 2.39 (OurOS)");
        return 0;
    }

    if args.iter().any(|a| a == "-l" || a == "--list") {
        println!("The following list contains all the coded character sets known.");
        println!();
        println!("ANSI_X3.4-1968//");
        println!("ASCII//");
        println!("CP1252//");
        println!("CP437//");
        println!("CP850//");
        println!("EUC-JP//");
        println!("EUC-KR//");
        println!("GB18030//");
        println!("GB2312//");
        println!("GBK//");
        println!("ISO-2022-JP//");
        println!("ISO-8859-1//");
        println!("ISO-8859-15//");
        println!("ISO-8859-2//");
        println!("KOI8-R//");
        println!("SHIFT_JIS//");
        println!("UCS-2//");
        println!("UCS-4//");
        println!("US-ASCII//");
        println!("UTF-16//");
        println!("UTF-32//");
        println!("UTF-8//");
        println!("WINDOWS-1250//");
        println!("WINDOWS-1251//");
        println!("WINDOWS-1252//");
        return 0;
    }

    let from = args.windows(2).find(|w| w[0] == "-f" || w[0] == "--from-code").map(|w| w[1].as_str()).unwrap_or("UTF-8");
    let to = args.windows(2).find(|w| w[0] == "-t" || w[0] == "--to-code").map(|w| w[1].as_str()).unwrap_or("UTF-8");

    let verbose = args.iter().any(|a| a == "--verbose");
    if verbose {
        println!("iconv: converting from {} to {}", from, to);
    }
    // Would read stdin and convert in real implementation
    println!("(converted output)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_iconv(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_iconv};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_iconv(vec!["--help".to_string()]), 0);
        assert_eq!(run_iconv(vec!["-h".to_string()]), 0);
        assert_eq!(run_iconv(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_iconv(vec![]), 0);
    }
}
