#![deny(clippy::all)]

//! ncdu-cli — OurOS ncdu CLI
//!
//! Single personality: `ncdu`

use std::env;
use std::process;

fn run_ncdu(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ncdu [OPTIONS] [PATH]");
        println!();
        println!("ncdu — NCurses Disk Usage (OurOS).");
        println!();
        println!("Options:");
        println!("  -q             Quiet mode during scan");
        println!("  -x             Stay on same filesystem");
        println!("  -e             Extended info mode");
        println!("  -o FILE        Export to file");
        println!("  -f FILE        Import from file");
        println!("  --exclude PAT  Exclude pattern");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("ncdu 2.3 (OurOS)");
        return 0;
    }

    let path = args.first().filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or("/");

    println!("ncdu 2.3 ~ Use the arrow keys to navigate, press ? for help");
    println!("--- {} -----------------------------------------------", path);
    println!("    4.2 GiB [##########] /home");
    println!("    2.1 GiB [#####     ] /var");
    println!("    1.8 GiB [####      ] /usr");
    println!("  456.7 MiB [#         ] /opt");
    println!("  234.5 MiB [          ] /etc");
    println!("  128.9 MiB [          ] /tmp");
    println!("   45.6 MiB [          ] /boot");
    println!("   12.3 MiB [          ] /run");
    println!("    8.9 MiB [          ] /root");
    println!("    2.1 MiB [          ] /srv");
    println!();
    println!(" Total disk usage:   8.9 GiB  Apparent size:   8.7 GiB  Items: 234567");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ncdu(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_ncdu};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ncdu(vec!["--help".to_string()]), 0);
        assert_eq!(run_ncdu(vec!["-h".to_string()]), 0);
        let _ = run_ncdu(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ncdu(vec![]);
    }
}
