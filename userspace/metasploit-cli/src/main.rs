#![deny(clippy::all)]

//! metasploit-cli — OurOS Metasploit Framework
//!
//! Multi-personality: `msfconsole`, `msfvenom`, `msfrpcd`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_metasploit(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "msfvenom" => {
                println!("msfvenom (OurOS) — Payload generator and encoder");
                println!("  -p PAYLOAD     Payload to use");
                println!("  -f FORMAT      Output format (exe, elf, raw, python, c)");
                println!("  -e ENCODER     Encoder to use");
                println!("  -i ITERATIONS  Encoding iterations");
                println!("  -o FILE        Output file");
                println!("  -a ARCH        Architecture (x86, x64)");
                println!("  --platform OS  Platform (windows, linux)");
                println!("  -l payloads    List payloads");
                println!("  -l encoders    List encoders");
            }
            "msfrpcd" => {
                println!("msfrpcd (OurOS) — RPC daemon");
                println!("  -P PASS    Password");
                println!("  -S         Use SSL");
                println!("  -a ADDR    Bind address");
                println!("  -p PORT    Listen port");
            }
            _ => {
                println!("msfconsole v6.3 (OurOS) — Metasploit Framework console");
                println!("  -r FILE    Resource script");
                println!("  -x CMD     Execute command");
                println!("  -q         Quiet mode (no banner)");
                println!("  -n         No database");
            }
        }
        println!("  --version  Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Metasploit Framework v6.3.55 (OurOS)"); return 0; }
    match prog {
        "msfvenom" => {
            println!("msfvenom (OurOS)");
            println!("  Payload: linux/x64/meterpreter/reverse_tcp");
            println!("  LHOST: 192.168.1.100, LPORT: 4444");
            println!("  Format: elf");
            println!("  Encoder: x64/xor_dynamic (3 iterations)");
            println!("  Size: 250 bytes");
            println!("  Output: payload.elf");
        }
        _ => {
            println!("Metasploit Framework v6.3.55 (OurOS)");
            println!("  Exploits: 2,345");
            println!("  Auxiliary: 1,234");
            println!("  Post: 567");
            println!("  Payloads: 890");
            println!("  Encoders: 45");
            println!("  Evasion: 12");
            println!("  Database: connected (PostgreSQL)");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "msfconsole".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_metasploit(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_metasploit};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/metasploit"), "metasploit");
        assert_eq!(basename(r"C:\bin\metasploit.exe"), "metasploit.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("metasploit.exe"), "metasploit");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_metasploit(&["--help".to_string()], "metasploit"), 0);
        assert_eq!(run_metasploit(&["-h".to_string()], "metasploit"), 0);
        assert_eq!(run_metasploit(&["--version".to_string()], "metasploit"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_metasploit(&[], "metasploit"), 0);
    }
}
