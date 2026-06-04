#![deny(clippy::all)]

//! oath-cli — OurOS OATH/OTP authentication tools
//!
//! Multi-personality: `oathtool`, `ykman`, `pamu2fcfg`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_oathtool(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: oathtool [OPTIONS] [SECRET]");
        println!();
        println!("oathtool — OATH one-time password tool (OurOS).");
        println!();
        println!("Options:");
        println!("  --totp          Time-based OTP (default)");
        println!("  --hotp          HMAC-based OTP");
        println!("  -b, --base32    Base32 encoded secret");
        println!("  -d DIGITS       Number of digits (default 6)");
        println!("  -s STEP         Time step in seconds (default 30)");
        println!("  -c COUNTER      Counter value (HOTP)");
        println!("  --now TIME      Use specific time");
        println!("  -w WINDOW       Validation window");
        println!("  -v              Verbose mode");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("oathtool (oath-toolkit) 2.6.7 (OurOS)");
        return 0;
    }

    let verbose = args.iter().any(|a| a == "-v");
    let hotp = args.iter().any(|a| a == "--hotp");
    let digits = args.windows(2)
        .find(|w| w[0] == "-d")
        .and_then(|w| w[1].parse::<usize>().ok())
        .unwrap_or(6);

    if verbose {
        if hotp {
            println!("HOTP mode");
        } else {
            println!("TOTP mode");
        }
        println!("Digits: {}", digits);
        println!("Step size (seconds): 30");
    }

    match digits {
        8 => println!("12345678"),
        _ => println!("123456"),
    }
    0
}

fn run_ykman(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: ykman [OPTIONS] COMMAND");
        println!();
        println!("ykman — YubiKey Manager CLI (OurOS).");
        println!();
        println!("Commands:");
        println!("  info          Show YubiKey device info");
        println!("  list          List connected YubiKeys");
        println!("  oath          Manage OATH credentials");
        println!("  fido          Manage FIDO applications");
        println!("  piv           Manage PIV application");
        println!("  openpgp       Manage OpenPGP application");
        println!("  otp           Manage OTP application");
        println!("  config        Manage device configuration");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("YubiKey Manager (ykman) version: 5.3.0 (OurOS)");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match subcmd {
        "info" => {
            println!("Device type: YubiKey 5 NFC");
            println!("Serial number: 12345678");
            println!("Firmware version: 5.4.3");
            println!("Form factor: Keychain (USB-A)");
            println!("Enabled USB interfaces: OTP, FIDO, CCID");
            println!("NFC transport is enabled.");
            println!();
            println!("Applications    USB         NFC");
            println!("OTP             Enabled     Enabled");
            println!("FIDO U2F        Enabled     Enabled");
            println!("FIDO2           Enabled     Enabled");
            println!("OATH            Enabled     Enabled");
            println!("PIV             Enabled     Enabled");
            println!("OpenPGP         Enabled     Enabled");
            println!("YubiHSM Auth    Enabled     Not available");
        }
        "list" => {
            println!("YubiKey 5 NFC (5.4.3) [OTP+FIDO+CCID] Serial: 12345678");
        }
        "oath" => {
            let sub2 = args.get(1).map(|s| s.as_str()).unwrap_or("accounts");
            match sub2 {
                "accounts" | "list" => {
                    println!("  GitHub:user@example.com");
                    println!("  Google:user@example.com");
                    println!("  AWS:user@example.com");
                }
                "code" => {
                    println!("  GitHub:user@example.com  123456");
                    println!("  Google:user@example.com  789012");
                }
                _ => println!("ykman oath: see --help for commands"),
            }
        }
        _ => {
            println!("ykman: see --help for commands");
        }
    }
    0
}

fn run_pamu2fcfg(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pamu2fcfg [OPTIONS]");
        println!();
        println!("pamu2fcfg — generate U2F configuration (OurOS).");
        println!();
        println!("Options:");
        println!("  -u USER    Username");
        println!("  -n         Don't verify user presence");
        println!("  -r TYPE    Resident key (discouraged, preferred, required)");
        println!("  -t TYPE    Verification (discouraged, preferred, required)");
        println!("  -o FILE    Output file");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("pamu2fcfg 1.3.0 (OurOS)");
        return 0;
    }

    let user = args.windows(2)
        .find(|w| w[0] == "-u")
        .map(|w| w[1].as_str())
        .unwrap_or("user");

    println!("{}:ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-,0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_-,es256,+presence", user);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "oathtool".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "ykman" => run_ykman(&rest),
        "pamu2fcfg" => run_pamu2fcfg(&rest),
        _ => run_oathtool(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_oathtool};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/oath"), "oath");
        assert_eq!(basename(r"C:\bin\oath.exe"), "oath.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("oath.exe"), "oath");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_oathtool(&["--help".to_string()]), 0);
        assert_eq!(run_oathtool(&["-h".to_string()]), 0);
        let _ = run_oathtool(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_oathtool(&[]);
    }
}
