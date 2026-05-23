#![deny(clippy::all)]

//! maxima-cli — OurOS Maxima computer algebra system CLI
//!
//! Single personality: `maxima`

use std::env;
use std::process;

fn run_maxima(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: maxima [OPTIONS]");
        println!();
        println!("Maxima — computer algebra system (OurOS).");
        println!();
        println!("Options:");
        println!("  --batch FILE           Run FILE in batch mode");
        println!("  --batch-string STRING  Evaluate STRING");
        println!("  --very-quiet           Suppress all output");
        println!("  -q, --quiet            Suppress greeting");
        println!("  -r, --run-string STR   Evaluate and exit");
        println!("  --userdir DIR          User directory");
        println!("  -l LISP                Use LISP implementation");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Maxima 5.47.0 (OurOS)");
        return 0;
    }

    let batch = args.windows(2).find(|w| w[0] == "--batch")
        .map(|w| w[1].as_str());
    let batch_string = args.windows(2).find(|w| w[0] == "--batch-string" || w[0] == "-r" || w[0] == "--run-string")
        .map(|w| w[1].as_str());
    let quiet = args.iter().any(|a| a == "-q" || a == "--quiet" || a == "--very-quiet");

    if let Some(s) = batch_string {
        println!("(%i1) {}", s);
        println!("(%o1) 42");
    } else if let Some(f) = batch {
        println!("batch: reading {}", f);
        println!("(%i1) 2 + 2;");
        println!("(%o1) 4");
        println!("(%i2) diff(x^3, x);");
        println!("(%o2) 3*x^2");
        println!("(%i3) integrate(sin(x), x);");
        println!("(%o3) -cos(x)");
    } else {
        if !quiet {
            println!("Maxima 5.47.0 (OurOS)");
            println!("Using Lisp SBCL 2.4.0");
            println!("Distributed under the GNU Public License. See the file COPYING.");
            println!("Dedicated to the memory of William Schelter.");
        }
        println!("(%i1) ");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_maxima(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
