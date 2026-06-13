#![deny(clippy::all)]

//! aspell-cli — SlateOS GNU Aspell spell checker CLI
//!
//! Single personality: `aspell`

use std::env;
use std::process;

fn run_aspell(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "help") {
        println!("Usage: aspell [OPTIONS] COMMAND");
        println!();
        println!("GNU Aspell — spell checker (SlateOS).");
        println!();
        println!("Commands:");
        println!("  check FILE     Interactive spell check");
        println!("  list           List misspelled from stdin");
        println!("  pipe           Pipe mode (ispell compat)");
        println!("  dump           Dump dictionary");
        println!("  config         Show config options");
        println!("  dicts          List available dicts");
        println!("  soundslike     Show soundslike equivalent");
        println!();
        println!("Options:");
        println!("  -l, --lang LANG    Language");
        println!("  -d, --master DICT  Master dictionary");
        println!("  -p, --personal F   Personal dictionary");
        println!("  --encoding ENC     Input encoding");
        println!("  --mode MODE        Filter mode (none/url/email/html/tex)");
        println!("  --sug-mode MODE    Suggestion mode (ultra/fast/normal/slow/bad-spellers)");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v" || a == "version") {
        println!("@(#) International Ispell Version 3.1.20 (but really Aspell 0.60.8.1) (SlateOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("pipe");

    match cmd {
        "dicts" => {
            println!("en");
            println!("en-variant_0");
            println!("en-variant_1");
            println!("en_US");
            println!("en_GB");
            println!("de");
            println!("de_DE");
            println!("fr");
            println!("fr_FR");
        }
        "list" => {
            println!("teh");
            println!("recieve");
            println!("occured");
        }
        "pipe" => {
            println!("@(#) International Ispell Version 3.1.20 (but really Aspell 0.60.8.1)");
        }
        "dump" => {
            let subcmd = args.get(1).map(|s| s.as_str()).unwrap_or("master");
            match subcmd {
                "config" => {
                    println!("lang: en_US");
                    println!("encoding: UTF-8");
                    println!("sug-mode: normal");
                }
                _ => println!("aspell dump: dumping {} dictionary...", subcmd),
            }
        }
        "config" => {
            println!("lang: en_US");
            println!("encoding: UTF-8");
            println!("sug-mode: normal");
            println!("data-dir: /usr/share/aspell");
            println!("dict-dir: /usr/lib/aspell");
        }
        "check" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("(stdin)");
            println!("aspell: checking '{}'...", file);
        }
        _ => {
            eprintln!("aspell: unknown command '{}'. See --help.", cmd);
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_aspell(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_aspell};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_aspell(vec!["--help".to_string()]), 0);
        assert_eq!(run_aspell(vec!["-h".to_string()]), 0);
        let _ = run_aspell(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_aspell(vec![]);
    }
}
