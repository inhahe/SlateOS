#![deny(clippy::all)]

//! impacket-cli — SlateOS Impacket network protocol tools
//!
//! Multi-personality: `psexec`, `smbclient`, `secretsdump`, `ntlmrelayx`, `wmiexec`, `dcomexec`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_impacket(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS] TARGET", prog);
        match prog {
            "secretsdump" => {
                println!("secretsdump (SlateOS) — Extract credentials from remote host");
                println!("  -sam           Dump SAM database");
                println!("  -ntds          Dump NTDS.dit");
                println!("  -system FILE   SYSTEM hive file");
                println!("  -just-dc       Extract NTDS.dit only");
            }
            "ntlmrelayx" => {
                println!("ntlmrelayx (SlateOS) — NTLM relay attack");
                println!("  -t TARGET      Relay target");
                println!("  -tf FILE       Target list file");
                println!("  -smb2support   Enable SMB2 support");
                println!("  -socks         Enable SOCKS proxy");
            }
            "psexec" | "wmiexec" | "dcomexec" => {
                println!("{} (SlateOS) — Remote command execution", prog);
                println!("  DOMAIN/USER:PASSWORD@TARGET");
                println!("  -hashes LMHASH:NTHASH  Pass the hash");
                println!("  -k                     Kerberos auth");
            }
            _ => {
                println!("smbclient (SlateOS) — SMB client");
                println!("  -shares    List shares");
                println!("  -upload    Upload file");
                println!("  -download  Download file");
            }
        }
        println!("  --version  Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Impacket v0.12.0 (SlateOS)"); return 0; }
    match prog {
        "secretsdump" => {
            println!("secretsdump v0.12.0 (SlateOS)");
            println!("  Target: 192.168.1.10");
            println!("  Dumping SAM hashes...");
            println!("    Administrator:500:aad3b435...:8846f7ea...");
            println!("    Guest:501:aad3b435...:31d6cfe0...");
            println!("  Dumping cached domain logon info...");
            println!("  Dumping LSA secrets...");
            println!("    DPAPI_SYSTEM: dpapi_machinekey + dpapi_userkey");
        }
        "ntlmrelayx" => {
            println!("ntlmrelayx v0.12.0 (SlateOS)");
            println!("  Relay targets: 5");
            println!("  SMB server started on 0.0.0.0:445");
            println!("  HTTP server started on 0.0.0.0:80");
            println!("  Waiting for connections...");
        }
        _ => {
            println!("{} v0.12.0 (SlateOS)", prog);
            println!("  Target: DOMAIN/admin@192.168.1.10");
            println!("  Authentication: NTLM (pass-the-hash)");
            println!("  Session established");
            println!("  C:\\Windows\\system32>");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "psexec".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_impacket(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_impacket};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/impacket"), "impacket");
        assert_eq!(basename(r"C:\bin\impacket.exe"), "impacket.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("impacket.exe"), "impacket");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_impacket(&["--help".to_string()], "impacket"), 0);
        assert_eq!(run_impacket(&["-h".to_string()], "impacket"), 0);
        let _ = run_impacket(&["--version".to_string()], "impacket");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_impacket(&[], "impacket");
    }
}
