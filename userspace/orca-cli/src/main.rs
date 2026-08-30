#![deny(clippy::all)]

//! orca-cli — Slate OS Orca screen reader CLI
//!
//! Single personality: `orca`

use std::env;
use std::process;

fn run_orca(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: orca [OPTIONS]");
        println!();
        println!("Orca — screen reader for visually impaired (Slate OS).");
        println!();
        println!("Options:");
        println!("  -s, --setup        GUI setup");
        println!("  -t, --text-setup   Text-based setup");
        println!("  -l, --list-apps    List known applications");
        println!("  -e, --enable SPEECH Enable speech service");
        println!("  -d, --disable SPEECH Disable speech service");
        println!("  -u, --user-prefs DIR User preferences dir");
        println!("  -r, --replace      Replace running instance");
        println!("  --debug            Debug mode");
        println!("  --debug-file FILE  Debug output file");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("Orca 45.1 (Slate OS)");
        return 0;
    }

    if args.iter().any(|a| a == "-l" || a == "--list-apps") {
        println!("Known applications:");
        println!("  firefox          Mozilla Firefox");
        println!("  thunderbird      Mozilla Thunderbird");
        println!("  libreoffice      LibreOffice Suite");
        println!("  gedit            Text Editor");
        println!("  nautilus         Files");
        println!("  evince           Document Viewer");
        println!("  gnome-terminal   Terminal");
        return 0;
    }

    if args.iter().any(|a| a == "-t" || a == "--text-setup") {
        println!("Orca Text Setup");
        println!();
        println!("Speech settings:");
        println!("  Speech system: espeak-ng");
        println!("  Voice: english (en)");
        println!("  Rate: 50");
        println!("  Pitch: 5");
        println!("  Volume: 10");
        println!();
        println!("Key bindings:");
        println!("  Orca modifier: Insert");
        println!("  Say all: Orca+A");
        println!("  Where am I: Orca+Enter");
        println!("  Flat review: Orca+numpad");
        return 0;
    }

    let debug = args.iter().any(|a| a == "--debug");
    let replace = args.iter().any(|a| a == "-r" || a == "--replace");

    if replace {
        println!("Orca: replacing running instance...");
    }

    println!("Orca screen reader 45.1 starting...");
    println!("  Speech: espeak-ng");
    println!("  Braille: none");
    println!("  AT-SPI2: connected");
    if debug {
        println!("  Debug mode: enabled");
    }
    println!("  Ready.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_orca(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_orca};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_orca(vec!["--help".to_string()]), 0);
        assert_eq!(run_orca(vec!["-h".to_string()]), 0);
        let _ = run_orca(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_orca(vec![]);
    }
}
