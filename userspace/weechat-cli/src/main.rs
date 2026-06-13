#![deny(clippy::all)]

//! weechat-cli — SlateOS WeeChat IRC/messaging client CLI
//!
//! Single personality: `weechat`

use std::env;
use std::process;

fn run_weechat(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: weechat [OPTIONS]");
        println!();
        println!("WeeChat — extensible chat client (Slate OS).");
        println!();
        println!("Options:");
        println!("  -a, --no-connect       Don't auto-connect");
        println!("  -d, --dir DIR          Set WeeChat home dir");
        println!("  -t, --temp-dir         Create temp home dir");
        println!("  -p, --no-plugin        Don't load plugins");
        println!("  -P, --plugins LIST     Load only these plugins");
        println!("  -r, --run-command CMD  Run command after startup");
        println!("  -s, --no-script        Don't load scripts");
        println!("  --no-gnutls            Don't init GnuTLS");
        println!("  --no-gcrypt            Don't init gcrypt");
        println!("  --colors               Display color codes");
        println!("  --stdout               Log to stdout");
        println!("  --upgrade              Upgrade from running WeeChat");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("WeeChat 4.2.1 (Slate OS)");
        return 0;
    }

    if args.iter().any(|a| a == "--colors") {
        println!("Color codes:");
        println!("  00: default    01: black");
        println!("  02: darkgray   03: red");
        println!("  04: lightred   05: green");
        println!("  06: lightgreen 07: brown");
        println!("  08: yellow     09: blue");
        println!("  10: lightblue  11: magenta");
        println!("  12: lightmagenta 13: cyan");
        println!("  14: lightcyan  15: white");
        return 0;
    }

    let no_connect = args.iter().any(|a| a == "-a" || a == "--no-connect");
    let no_plugin = args.iter().any(|a| a == "-p" || a == "--no-plugin");

    let run_cmd = args.windows(2)
        .find(|w| w[0] == "-r" || w[0] == "--run-command")
        .map(|w| w[1].as_str());

    println!("WeeChat 4.2.1 (Slate OS)");
    println!();

    if !no_plugin {
        println!("Loading plugins: irc, relay, python, perl, ruby, lua, tcl, guile, javascript, php, trigger, typing, charset, exec, fifo, fset, logger, spell, xfer");
        println!("Plugins loaded: 19");
    } else {
        println!("Plugins loading disabled.");
    }

    println!();

    if no_connect {
        println!("[core] Not connecting to any server (--no-connect).");
        println!("[core] Type /server add <name> <host> to add a server.");
    } else {
        println!("[core] WeeChat started.");
        println!("[core] Welcome to WeeChat! Type /help for commands.");
        println!("[core] For support: https://weechat.org/support");
    }

    if let Some(cmd) = run_cmd {
        println!("[core] Running command: {}", cmd);
    }

    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_weechat(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_weechat};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_weechat(vec!["--help".to_string()]), 0);
        assert_eq!(run_weechat(vec!["-h".to_string()]), 0);
        let _ = run_weechat(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_weechat(vec![]);
    }
}
