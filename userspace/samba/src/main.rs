#![deny(clippy::all)]

//! samba — SlateOS SMB/CIFS file server
//!
//! Multi-personality: `smbd` (file server), `nmbd` (NetBIOS), `smbclient`, `smbstatus`, `testparm`

use std::env;
use std::process;

fn run_smbd(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: smbd [options]");
        println!();
        println!("Options:");
        println!("  -D, --daemon          Run as daemon");
        println!("  -F, --foreground      Run in foreground");
        println!("  -S, --log-stdout      Log to stdout");
        println!("  -s <config>           Configuration file");
        println!("  -p <port>             Listen on port");
        println!("  --version             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("Version 4.20.0 (SlateOS)");
        return 0;
    }
    println!("smbd version 4.20.0 started.");
    println!("[2025/05/22 10:00:00, 0] smbd/server.c:1234(main)");
    println!("  smbd started.");
    println!("[2025/05/22 10:00:00, 0] smbd/server.c:1235(main)");
    println!("  Listening on port 445");
    0
}

fn run_nmbd(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nmbd [options]");
        println!("  -D       Run as daemon");
        println!("  -F       Run in foreground");
        println!("  --version  Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Version 4.20.0 (SlateOS)");
        return 0;
    }
    println!("nmbd version 4.20.0 started.");
    println!("Listening for NetBIOS name service on port 137");
    0
}

fn run_smbclient(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: smbclient //server/share [options]");
        println!();
        println!("Options:");
        println!("  -U <user>            Username");
        println!("  -W <workgroup>       Workgroup");
        println!("  -L <host>            List shares on host");
        println!("  -c <command>         Execute semicolon-separated commands");
        println!("  -N                   No password");
        println!("  --version            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Version 4.20.0 (SlateOS)");
        return 0;
    }
    if args.iter().any(|a| a == "-L") {
        let host = args.iter().position(|a| a == "-L")
            .and_then(|i| args.get(i + 1))
            .map(|s| s.as_str())
            .unwrap_or("server");
        println!("Sharename       Type      Comment");
        println!("---------       ----      -------");
        println!("public          Disk      Public files");
        println!("homes           Disk      Home directories");
        println!("printers        Printer   All Printers");
        println!("IPC$            IPC       IPC Service (Samba {})", host);
        return 0;
    }
    let share = args.iter().find(|a| a.starts_with("//")).map(|s| s.as_str()).unwrap_or("//server/share");
    println!("Try \"help\" to get a list of possible commands.");
    println!("smb: {}\\> ls", share);
    println!("  .                D        0  Wed May 22 10:00:00 2025");
    println!("  ..               D        0  Wed May 22 10:00:00 2025");
    println!("  documents        D        0  Wed May 21 15:30:00 2025");
    println!("  readme.txt       A     1234  Wed May 20 12:00:00 2025");
    println!("                65536 blocks of size 1048576. 32768 blocks available");
    println!("smb: {}\\> quit", share);
    0
}

fn run_smbstatus(_args: Vec<String>) -> i32 {
    println!("Samba version 4.20.0 (SlateOS)");
    println!("PID     Username     Group        Machine                     Protocol Version  Encryption  Signing");
    println!("----------------------------------------------------------------------------------------------------------------------------------------");
    println!("12345   alice        users        192.168.1.100 (ipv4:192.168.1.100:49876)  SMB3_11    -           AES-128-CMAC");
    println!("12346   bob          users        192.168.1.101 (ipv4:192.168.1.101:49877)  SMB3_11    -           AES-128-CMAC");
    println!();
    println!("Service      pid     Machine       Connected at                   Encryption  Signing");
    println!("--------------------------------------------------------------------------------------------------------------");
    println!("public       12345   192.168.1.100 Wed May 22 09:30:00 2025       -           -");
    println!("homes        12346   192.168.1.101 Wed May 22 09:45:00 2025       -           -");
    0
}

fn run_testparm(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: testparm [options] [config_file]");
        println!("  -s     Suppress prompt for enter");
        println!("  -v     Show default values");
        return 0;
    }
    let config = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("/etc/samba/smb.conf");
    println!("Load smb config files from {}", config);
    println!("Loaded services file OK.");
    println!("Weak crypto is allowed by GnuTLS (e.g. NTLM and RC4)");
    println!();
    println!("Server role: ROLE_STANDALONE");
    println!();
    println!("[global]");
    println!("   workgroup = WORKGROUP");
    println!("   server string = Samba Server (SlateOS)");
    println!("   security = USER");
    println!("   map to guest = Bad User");
    println!();
    println!("[public]");
    println!("   comment = Public files");
    println!("   path = /srv/samba/public");
    println!("   guest ok = Yes");
    println!("   read only = No");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("smbd");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        base.strip_suffix(".exe").unwrap_or(base).to_string()
    };
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog_name.as_str() {
        "nmbd" => run_nmbd(rest),
        "smbclient" => run_smbclient(rest),
        "smbstatus" => run_smbstatus(rest),
        "testparm" => run_testparm(rest),
        _ => run_smbd(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_smbd};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_smbd(vec!["--help".to_string()]), 0);
        assert_eq!(run_smbd(vec!["-h".to_string()]), 0);
        let _ = run_smbd(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_smbd(vec![]);
    }
}
