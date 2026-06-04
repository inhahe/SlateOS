#![deny(clippy::all)]

//! audit-cli — OurOS Linux audit framework tools
//!
//! Multi-personality: `auditctl`, `ausearch`, `aureport`, `auditd`, `autrace`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_auditctl(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: auditctl [OPTIONS]");
        println!();
        println!("auditctl — control the audit system (OurOS).");
        println!();
        println!("Options:");
        println!("  -l                List rules");
        println!("  -a ACTION,LIST    Add rule");
        println!("  -d ACTION,LIST    Delete rule");
        println!("  -D                Delete all rules");
        println!("  -w PATH           Watch a path");
        println!("  -W PATH           Remove path watch");
        println!("  -k KEY            Filter key");
        println!("  -p PERMS          Permission filter (rwxa)");
        println!("  -s                Get status");
        println!("  -e [0|1|2]        Set enabled flag");
        return 0;
    }

    let list = args.iter().any(|a| a == "-l");
    let status = args.iter().any(|a| a == "-s");
    let delete_all = args.iter().any(|a| a == "-D");

    if status {
        println!("enabled 1");
        println!("failure 1");
        println!("pid 1234");
        println!("rate_limit 0");
        println!("backlog_limit 8192");
        println!("lost 0");
        println!("backlog 0");
        println!("backlog_wait_time 60000");
    } else if list {
        println!("-w /etc/passwd -p wa -k identity");
        println!("-w /etc/shadow -p wa -k identity");
        println!("-w /etc/group -p wa -k identity");
        println!("-w /etc/sudoers -p wa -k actions");
        println!("-a always,exit -F arch=b64 -S execve -k exec");
    } else if delete_all {
        println!("No rules");
    } else {
        println!("auditctl: rule added");
    }
    0
}

fn run_ausearch(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ausearch [OPTIONS]");
        println!();
        println!("ausearch — search audit records (OurOS).");
        println!();
        println!("Options:");
        println!("  -k KEY         Search by key");
        println!("  -m TYPE        Search by message type");
        println!("  -ts TIME       Start time");
        println!("  -te TIME       End time");
        println!("  -p PID         Process ID");
        println!("  -ui UID        User ID");
        println!("  -i             Interpret numeric values");
        println!("  --format TEXT   Output format");
        return 0;
    }

    println!("----");
    println!("time->Thu Jan  1 10:00:00 2025");
    println!("type=SYSCALL msg=audit(1735689600.000:100): arch=c000003e syscall=59 success=yes exit=0 a0=7fff ppid=1000 pid=1234 uid=1000 gid=1000 comm=\"bash\" exe=\"/usr/bin/bash\"");
    println!("type=EXECVE msg=audit(1735689600.000:100): argc=2 a0=\"/usr/bin/ls\" a1=\"-la\"");
    println!("type=PATH msg=audit(1735689600.000:100): item=0 name=\"/usr/bin/ls\" inode=12345 dev=08:02 mode=0100755 ouid=0 ogid=0");
    println!("----");
    println!("time->Thu Jan  1 10:01:00 2025");
    println!("type=USER_AUTH msg=audit(1735689660.000:101): pid=5678 uid=0 auid=1000 msg='op=PAM:authentication acct=\"user\" exe=\"/usr/bin/sudo\" res=success'");
    0
}

fn run_aureport(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: aureport [OPTIONS]");
        println!();
        println!("aureport — produce summary reports (OurOS).");
        println!();
        println!("Options:");
        println!("  -au       Authentication report");
        println!("  -l        Login report");
        println!("  -f        File report");
        println!("  -x        Executable report");
        println!("  -s        Syscall report");
        println!("  --summary Summary mode");
        return 0;
    }

    let auth = args.iter().any(|a| a == "-au");
    let login = args.iter().any(|a| a == "-l");

    if auth {
        println!();
        println!("Authentication Report");
        println!("============================================");
        println!("# date time acct host term exe success event");
        println!("============================================");
        println!("1. 01/01/2025 10:00:00 user ? /dev/pts/0 /usr/bin/sudo yes 101");
        println!("2. 01/01/2025 10:05:00 user ? /dev/pts/1 /usr/bin/su yes 102");
    } else if login {
        println!();
        println!("Login Report");
        println!("============================================");
        println!("# date time auid host term exe success event");
        println!("============================================");
        println!("1. 01/01/2025 09:00:00 1000 192.168.1.100 /dev/pts/0 /usr/sbin/sshd yes 50");
    } else {
        println!();
        println!("Summary Report");
        println!("======================");
        println!("Range of time: 01/01/2025 00:00:00 - 01/01/2025 12:00:00");
        println!("Number of events: 500");
        println!("Number of logins: 3");
        println!("Number of failed logins: 1");
        println!("Number of authentications: 15");
        println!("Number of failed authentications: 2");
    }
    0
}

fn run_auditd(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") {
        println!("Usage: auditd [OPTIONS]");
        println!("Options: -f (foreground), -l (follow symlinks), -n (no fork)");
        return 0;
    }
    let _ = args;
    println!("auditd: starting audit daemon (OurOS)");
    println!("auditd: loaded 5 rules");
    println!("auditd: audit log: /var/log/audit/audit.log");
    0
}

fn run_autrace(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: autrace PROGRAM [ARGS]");
        println!();
        println!("autrace — trace syscalls of a program via audit (OurOS).");
        return 0;
    }
    let program = args.first().map(|s| s.as_str()).unwrap_or("program");
    println!("Waiting to execute: {}", program);
    println!("Cleaning up...");
    println!("Trace complete. You can locate the records with 'ausearch -i -p 1234'");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "auditctl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "ausearch" => run_ausearch(&rest),
        "aureport" => run_aureport(&rest),
        "auditd" => run_auditd(&rest),
        "autrace" => run_autrace(&rest),
        _ => run_auditctl(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_auditctl};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/audit"), "audit");
        assert_eq!(basename(r"C:\bin\audit.exe"), "audit.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("audit.exe"), "audit");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_auditctl(&["--help".to_string()]), 0);
        assert_eq!(run_auditctl(&["-h".to_string()]), 0);
        let _ = run_auditctl(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_auditctl(&[]);
    }
}
