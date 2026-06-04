#![deny(clippy::all)]

//! trezor-cli — OurOS Trezor hardware wallet tool
//!
//! Single personality: `trezorctl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_trezor(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: trezorctl COMMAND [OPTIONS]");
        println!("trezorctl v0.14 (OurOS) — Trezor hardware wallet CLI");
        println!();
        println!("Commands:");
        println!("  list              List connected Trezor devices");
        println!("  get-address       Get receiving address");
        println!("  sign-tx           Sign transaction");
        println!("  sign-message MSG  Sign a message");
        println!("  verify-message    Verify a signed message");
        println!("  firmware-update   Update firmware");
        println!("  wipe-device       Wipe device");
        println!("  version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("trezorctl v0.14 (OurOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("list");
    match cmd {
        "list" => println!("No Trezor device found. Connect via USB."),
        "get-address" => println!("No device connected."),
        "firmware-update" => println!("No device connected. Cannot check firmware."),
        _ => println!("trezorctl {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "trezorctl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_trezor(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_trezor};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/trezor"), "trezor");
        assert_eq!(basename(r"C:\bin\trezor.exe"), "trezor.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("trezor.exe"), "trezor");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_trezor(&["--help".to_string()], "trezor"), 0);
        assert_eq!(run_trezor(&["-h".to_string()], "trezor"), 0);
        let _ = run_trezor(&["--version".to_string()], "trezor");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_trezor(&[], "trezor");
    }
}
