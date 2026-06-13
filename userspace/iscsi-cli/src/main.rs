#![deny(clippy::all)]

//! iscsi-cli — Slate OS iSCSI initiator/target tools
//!
//! Multi-personality: `iscsiadm`, `tgtadm`, `targetcli`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_iscsiadm(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: iscsiadm [OPTIONS]");
        println!();
        println!("iscsiadm — iSCSI initiator admin (Slate OS).");
        println!();
        println!("Options:");
        println!("  -m, --mode <mode>       discovery|node|session|iface");
        println!("  -t, --type <type>       sendtargets|slp|isns");
        println!("  -p, --portal <ip:port>  Target portal");
        println!("  -l, --login             Login to target");
        println!("  -u, --logout            Logout from target");
        println!("  --version               Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("iscsiadm version 2.1.9 (Slate OS)");
        return 0;
    }

    let mode = args.windows(2).find(|w| w[0] == "-m" || w[0] == "--mode")
        .map(|w| w[1].as_str()).unwrap_or("session");

    match mode {
        "discovery" => {
            println!("192.168.1.200:3260,1 iqn.2024-01.com.slateos:storage.lun0");
            println!("192.168.1.200:3260,1 iqn.2024-01.com.slateos:storage.lun1");
        }
        "session" => {
            println!("tcp: [1] 192.168.1.200:3260,1 iqn.2024-01.com.slateos:storage.lun0 (non-flash)");
        }
        "node" => {
            println!("192.168.1.200:3260,1 iqn.2024-01.com.slateos:storage.lun0");
            if args.iter().any(|a| a == "-l" || a == "--login") {
                println!("Logging in to [iface: default, target: iqn.2024-01.com.slateos:storage.lun0, portal: 192.168.1.200,3260]");
                println!("Login to [iface: default, ... portal: 192.168.1.200,3260] successful.");
            }
            if args.iter().any(|a| a == "-u" || a == "--logout") {
                println!("Logging out of session [sid: 1, target: iqn.2024-01.com.slateos:storage.lun0, portal: 192.168.1.200,3260]");
                println!("Logout of [sid: 1, ... portal: 192.168.1.200,3260] successful.");
            }
        }
        _ => println!("iscsiadm: mode '{}' completed", mode),
    }
    0
}

fn run_targetcli(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: targetcli [PATH] [COMMAND]");
        println!();
        println!("targetcli — Linux-IO target configuration (Slate OS).");
        println!("Interactive or scripted mode.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("targetcli 2.1.57 (Slate OS)");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("ls");
    if subcmd == "ls" || subcmd == "/" {
        println!("o- / ........................................................................ [...]");
        println!("  o- backstores ............................................................. [...]");
        println!("  | o- block .................................................. [Storage Objects: 1]");
        println!("  | | o- disk0 ..................... [/dev/sda (100.0GiB) write-thru activated]");
        println!("  | o- fileio ................................................. [Storage Objects: 0]");
        println!("  | o- pscsi .................................................. [Storage Objects: 0]");
        println!("  | o- ramdisk ................................................ [Storage Objects: 0]");
        println!("  o- iscsi ............................................................ [Targets: 1]");
        println!("  | o- iqn.2024-01.com.slateos:storage ..................................... [TPGs: 1]");
        println!("  |   o- tpg1 .............................................. [no-gen-acls, no-auth]");
        println!("  |     o- acls .......................................................... [ACLs: 1]");
        println!("  |     o- luns .......................................................... [LUNs: 1]");
        println!("  |     o- portals .................................................... [Portals: 1]");
        println!("  o- loopback ......................................................... [Targets: 0]");
    } else {
        println!("targetcli: command '{}' executed", subcmd);
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "iscsiadm".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "targetcli" => run_targetcli(&rest),
        "tgtadm" => { println!("tgtadm: command completed"); 0 }
        _ => run_iscsiadm(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_iscsiadm};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/iscsi"), "iscsi");
        assert_eq!(basename(r"C:\bin\iscsi.exe"), "iscsi.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("iscsi.exe"), "iscsi");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_iscsiadm(&["--help".to_string()]), 0);
        assert_eq!(run_iscsiadm(&["-h".to_string()]), 0);
        let _ = run_iscsiadm(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_iscsiadm(&[]);
    }
}
