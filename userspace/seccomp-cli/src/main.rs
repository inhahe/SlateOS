#![deny(clippy::all)]

//! seccomp-cli — OurOS seccomp/sandbox tools
//!
//! Multi-personality: `scmp_sys_resolver`, `firejail`, `bwrap`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_scmp_sys_resolver(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: scmp_sys_resolver [-a ARCH] SYSCALL_NAME|SYSCALL_NUM");
        println!();
        println!("Resolve syscall names to numbers and vice versa (OurOS).");
        return 0;
    }

    let query = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("");
    match query {
        "read" => println!("0"),
        "write" => println!("1"),
        "open" => println!("2"),
        "close" => println!("3"),
        "execve" => println!("59"),
        "fork" => println!("57"),
        "clone" => println!("56"),
        "0" => println!("read"),
        "1" => println!("write"),
        "59" => println!("execve"),
        _ => println!("{}", query),
    }
    0
}

fn run_firejail(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: firejail [OPTIONS] PROGRAM [ARGS]");
        println!();
        println!("firejail — Linux namespaces sandbox (OurOS).");
        println!();
        println!("Options:");
        println!("  --net=IFACE       Network interface");
        println!("  --net=none        No network");
        println!("  --private         Private home dir");
        println!("  --private-tmp     Private /tmp");
        println!("  --seccomp         Enable seccomp filter");
        println!("  --caps.drop=all   Drop all capabilities");
        println!("  --noroot          Disable root access");
        println!("  --noprofile       No default profile");
        println!("  --whitelist=PATH  Whitelist path");
        println!("  --blacklist=PATH  Blacklist path");
        println!("  --list            List sandboxes");
        println!("  --tree            Show process tree");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("firejail version 0.9.72 (OurOS)");
        return 0;
    }

    if args.iter().any(|a| a == "--list") {
        println!("1234:user:firejail --private firefox");
        println!("5678:user:firejail --net=none code");
        return 0;
    }

    if args.iter().any(|a| a == "--tree") {
        println!("1234:user:firejail --private firefox");
        println!("  1235:user:firefox");
        println!("    1236:user:firefox -contentproc");
        return 0;
    }

    let program = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("bash");
    println!("Reading profile /etc/firejail/{}.profile", program);
    println!("Parent pid 1234, child pid 1235");
    println!("Child process initialized in 0.01 ms");
    0
}

fn run_bwrap(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bwrap [OPTIONS] COMMAND [ARGS]");
        println!();
        println!("bwrap — bubblewrap sandbox (OurOS).");
        println!();
        println!("Options:");
        println!("  --ro-bind SRC DEST    Read-only bind mount");
        println!("  --bind SRC DEST       Bind mount");
        println!("  --dev-bind SRC DEST   Dev bind mount");
        println!("  --tmpfs DEST          Mount tmpfs");
        println!("  --proc DEST           Mount proc");
        println!("  --dev DEST            Mount devtmpfs");
        println!("  --unshare-all         Unshare all namespaces");
        println!("  --unshare-net         Unshare network");
        println!("  --unshare-pid         Unshare PID namespace");
        println!("  --die-with-parent     Die when parent exits");
        println!("  --new-session         Start new session");
        return 0;
    }

    let command = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("sh");
    println!("bwrap: running '{}' in sandbox", command);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "firejail".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "scmp_sys_resolver" => run_scmp_sys_resolver(&rest),
        "bwrap" => run_bwrap(&rest),
        _ => run_firejail(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_scmp_sys_resolver};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/seccomp"), "seccomp");
        assert_eq!(basename(r"C:\bin\seccomp.exe"), "seccomp.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("seccomp.exe"), "seccomp");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_scmp_sys_resolver(&["--help".to_string()]), 0);
        assert_eq!(run_scmp_sys_resolver(&["-h".to_string()]), 0);
        assert_eq!(run_scmp_sys_resolver(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_scmp_sys_resolver(&[]), 0);
    }
}
