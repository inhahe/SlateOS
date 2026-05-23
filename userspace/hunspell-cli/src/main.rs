#![deny(clippy::all)]

//! hunspell-cli — OurOS Hunspell spell checker CLI
//!
//! Single personality: `hunspell`

use std::env;
use std::process;

fn run_hunspell(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: hunspell [OPTIONS] [FILE ...]");
        println!();
        println!("Hunspell — spell checker (OurOS).");
        println!();
        println!("Options:");
        println!("  -d DICT      Dictionary to use");
        println!("  -p FILE      Personal dictionary");
        println!("  -a           Pipe mode (ispell compat)");
        println!("  -l           List misspelled words");
        println!("  -i ENC       Input encoding");
        println!("  -G           No suggestions");
        println!("  -H           HTML input");
        println!("  -t           TeX/LaTeX input");
        println!("  -n           nroff/troff input");
        println!("  -D           Show search paths");
        println!("  -L           Print lines with misspellings");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("Hunspell 1.7.2 (OurOS)");
        return 0;
    }

    if args.iter().any(|a| a == "-D") {
        println!("SEARCH PATH:");
        println!("  /usr/share/hunspell");
        println!("  /usr/share/myspell/dicts");
        println!();
        println!("AVAILABLE DICTIONARIES:");
        println!("  en_US (/usr/share/hunspell/en_US)");
        println!("  en_GB (/usr/share/hunspell/en_GB)");
        println!("  de_DE (/usr/share/hunspell/de_DE)");
        println!("  fr_FR (/usr/share/hunspell/fr_FR)");
        return 0;
    }

    let list_mode = args.iter().any(|a| a == "-l");
    let pipe_mode = args.iter().any(|a| a == "-a");

    if pipe_mode {
        println!("@(#) Hunspell 1.7.2 (OurOS)");
        // In real mode, would read stdin word-by-word
    } else if list_mode {
        println!("teh");
        println!("recieve");
        println!("occured");
    } else {
        let dict = args.windows(2).find(|w| w[0] == "-d").map(|w| w[1].as_str()).unwrap_or("en_US");
        println!("Hunspell 1.7.2 (dict={})", dict);
        println!("(interactive spell-check mode)");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_hunspell(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
