//! Slate OS nologin shell.
//!
//! Displays a message and exits non-zero. It is the shell given to system
//! accounts that should not have interactive logins.
//!
//! # It used to answer to `true` and `false` as well
//!
//! `argv[0]` chose between three personalities, and two of them were
//! `process::exit(0)` and `process::exit(1)`. Neither could run --
//! `coreutils` produces both names -- and neither should: GNU's `true` and
//! `false` accept `--help` and `--version`, and these ignored every argument.
//!
//! Of all the shadowed names in the tree this pair was worth removing first.
//! `true` and `false` run in nearly every shell script on the system, and the
//! program shadowing them exists to REFUSE and exit non-zero. Had packaging
//! ever installed this copy as `/bin/true`, every `while true` and every
//! `cmd || true` on the machine would have changed meaning.

#![deny(clippy::all)]

use std::env;
use std::ffi::OsString;
use std::fs;
use std::process;

const VERSION: &str = "0.1.0";
const NOLOGIN_MSG_FILE: &str = "/etc/nologin.txt";

const DEFAULT_MESSAGE: &str = "This account is currently not available.";

fn cmd_nologin(args: &[OsString]) {
    for arg in args {
        // `""` for a word that is not valid Unicode. `nologin` takes no
        // operands and every option it has is ASCII, so such a word matches
        // nothing and falls to the ignoring arm below -- which is what it did
        // before, except that the process now survives long enough to do it.
        match arg.to_str().unwrap_or("") {
            "-h" | "--help" => {
                println!("Usage: nologin [options]");
                println!();
                println!("Politely refuse a login.");
                println!("Displays /etc/nologin.txt if it exists, otherwise a default message.");
                println!();
                println!("Options:");
                println!("  -h, --help     Show this help");
                println!("  --version      Show version");
                // Even --help exits non-zero for nologin.
                process::exit(1);
            }
            "--version" => {
                println!("nologin {VERSION}");
                process::exit(1);
            }
            _ => {}
        }
    }

    // Display custom message or default.
    let message =
        fs::read_to_string(NOLOGIN_MSG_FILE).unwrap_or_else(|_| DEFAULT_MESSAGE.to_string());
    eprintln!("{message}");
    process::exit(1);
}

fn main() {
    // `args_os`, not `args`: the latter's iterator unwraps, so ANY argument
    // holding a byte that is not valid Unicode aborted the process with a
    // Rust panic message. For `nologin` of all programs that is the wrong
    // answer twice over -- it is the shell a locked account gets, so its job
    // is to refuse politely and exit 1, not to crash.
    let args: Vec<OsString> = env::args_os().collect();

    // No personality probe: this binary is `nologin` under every name.
    let rest: Vec<OsString> = args.into_iter().skip(1).collect();
    cmd_nologin(&rest);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_message() {
        assert!(!DEFAULT_MESSAGE.is_empty());
        assert!(DEFAULT_MESSAGE.contains("not available"));
    }

    #[test]
    fn test_version_constant() {
        assert_eq!(VERSION, "0.1.0");
    }

    #[test]
    fn test_nologin_msg_path() {
        assert_eq!(NOLOGIN_MSG_FILE, "/etc/nologin.txt");
    }
}
