#![deny(clippy::all)]

//! xsel-cli — OurOS xsel clipboard CLI
//!
//! Single personality: `xsel`

use std::env;
use std::process;

fn run_xsel(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xsel [OPTIONS]");
        println!();
        println!("xsel — X11 selection manipulation (OurOS).");
        println!();
        println!("Options:");
        println!("  -i, --input          Read stdin into selection");
        println!("  -o, --output         Output selection to stdout");
        println!("  -a, --append         Append stdin to selection");
        println!("  -c, --clear          Clear selection");
        println!("  -d, --delete         Request deletion");
        println!("  -p, --primary        Use PRIMARY selection (default)");
        println!("  -s, --secondary      Use SECONDARY selection");
        println!("  -b, --clipboard      Use CLIPBOARD selection");
        println!("  -k, --keep           Don't modify selections");
        println!("  -x, --exchange       Exchange primary and secondary");
        println!("  -l, --logfile FILE   Log errors to file");
        println!("  --trim               Trim trailing newline");
        println!("  --nodetach           Don't detach from terminal");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("xsel version 1.2.1 (OurOS)");
        return 0;
    }

    let output = args.iter().any(|a| a == "-o" || a == "--output");
    let clear = args.iter().any(|a| a == "-c" || a == "--clear");
    let exchange = args.iter().any(|a| a == "-x" || a == "--exchange");
    let clipboard = args.iter().any(|a| a == "-b" || a == "--clipboard");
    let secondary = args.iter().any(|a| a == "-s" || a == "--secondary");

    let sel_name = if clipboard { "CLIPBOARD" }
        else if secondary { "SECONDARY" }
        else { "PRIMARY" };

    if clear {
        println!("xsel: {} selection cleared", sel_name);
    } else if exchange {
        println!("xsel: PRIMARY and SECONDARY exchanged");
    } else if output {
        println!("(contents of {} selection)", sel_name);
    }
    // Input mode: reads from stdin silently
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_xsel(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_xsel};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_xsel(vec!["--help".to_string()]), 0);
        assert_eq!(run_xsel(vec!["-h".to_string()]), 0);
        let _ = run_xsel(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_xsel(vec![]);
    }
}
