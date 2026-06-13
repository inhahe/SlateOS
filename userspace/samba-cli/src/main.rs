#![deny(clippy::all)]

//! samba-cli — Slate OS Samba/SMB tools
//!
//! Multi-personality: `smbclient`, `smbstatus`, `testparm`, `nmblookup`, `pdbedit`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_smbclient(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: smbclient [OPTIONS] //server/share");
        println!();
        println!("smbclient — SMB/CIFS client (Slate OS, Samba 4.19).");
        println!();
        println!("Options:");
        println!("  -U <user>      Username");
        println!("  -W <domain>    Workgroup");
        println!("  -L <host>      List shares");
        println!("  -c <cmd>       Execute command");
        println!("  -N             No password");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Version 4.19.4 (Slate OS)");
        return 0;
    }

    if args.iter().any(|a| a == "-L") {
        let host = args.windows(2).find(|w| w[0] == "-L").map(|w| w[1].as_str()).unwrap_or("server");
        println!("Sharename       Type      Comment");
        println!("---------       ----      -------");
        println!("public          Disk      Public files");
        println!("homes           Disk      Home directories");
        println!("printers        Printer   All Printers");
        println!("IPC$            IPC       IPC Service (Samba {} Slate OS)", host);
        return 0;
    }

    let share = args.iter().find(|a| a.starts_with("//")).map(|s| s.as_str()).unwrap_or("//server/share");
    println!("Try \"help\" to get a list of possible commands.");
    println!("smb: \\> ls");
    println!("  .                  D        0  Wed May 22 12:00:00 2024");
    println!("  ..                 D        0  Wed May 22 12:00:00 2024");
    println!("  documents          D        0  Wed May 22 11:00:00 2024");
    println!("  report.pdf         A    54321  Wed May 22 10:00:00 2024");
    println!();
    println!("\t\t65536 blocks of size 1048576. 32768 blocks available");
    let _ = share;
    0
}

fn run_smbstatus(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: smbstatus [OPTIONS]");
        println!("  -b    Brief output");
        println!("  -S    Show shares");
        println!("  -L    Show locks");
        return 0;
    }

    println!("Samba version 4.19.4 (Slate OS)");
    println!("PID     Username     Group        Machine                            Protocol Version  Encryption           Signing");
    println!("----------------------------------------------------------------------------------------------------------------------------------------");
    println!("1234    user1        staff        192.168.1.50 (ipv4:192.168.1.50)   SMB3_11           -                    AES-128-GMAC");
    println!("1235    user2        staff        192.168.1.51 (ipv4:192.168.1.51)   SMB3_11           -                    AES-128-GMAC");
    println!();
    println!("Service      pid     Machine       Connected at                     Encryption   Signing");
    println!("------------------------------------------------------------------------------------------------------------");
    println!("public       1234    192.168.1.50  Wed May 22 10:00:00 AM 2024 UTC  -            AES-128-GMAC");
    println!("homes        1235    192.168.1.51  Wed May 22 10:30:00 AM 2024 UTC  -            AES-128-GMAC");
    0
}

fn run_testparm(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: testparm [OPTIONS] [config-file]");
        return 0;
    }

    println!("Load smb config files from /etc/samba/smb.conf");
    println!("Loaded services file OK.");
    println!("Weak crypto is allowed by GnuTLS (e.g. NTLM as a compatibility fallback)");
    println!();
    println!("Server role: ROLE_STANDALONE");
    println!();
    println!("Press enter to see a dump of your service definitions");
    println!();
    println!("[global]");
    println!("\tworkgroup = WORKGROUP");
    println!("\tserver string = Samba Server (Slate OS)");
    println!("\tsecurity = USER");
    println!("\tmap to guest = Bad User");
    println!();
    println!("[public]");
    println!("\tpath = /srv/samba/public");
    println!("\tread only = No");
    println!("\tguest ok = Yes");
    let _ = args;
    0
}

fn run_nmblookup(args: &[String]) -> i32 {
    if args.is_empty() {
        println!("Usage: nmblookup [OPTIONS] <name>");
        return 0;
    }
    let name = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("WORKGROUP");
    println!("querying {} on 192.168.1.255", name);
    println!("192.168.1.100 {}<00>", name);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "smbclient".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "smbstatus" => run_smbstatus(&rest),
        "testparm" => run_testparm(&rest),
        "nmblookup" => run_nmblookup(&rest),
        "pdbedit" => {
            println!("user1:1000:User One");
            println!("user2:1001:User Two");
            0
        }
        _ => run_smbclient(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_smbclient};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/samba"), "samba");
        assert_eq!(basename(r"C:\bin\samba.exe"), "samba.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("samba.exe"), "samba");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_smbclient(&["--help".to_string()]), 0);
        assert_eq!(run_smbclient(&["-h".to_string()]), 0);
        let _ = run_smbclient(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_smbclient(&[]);
    }
}
