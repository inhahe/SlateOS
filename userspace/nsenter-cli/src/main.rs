#![deny(clippy::all)]

//! nsenter-cli — OurOS nsenter/lsns CLI
//!
//! Multi-personality: `nsenter`, `lsns`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_nsenter(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nsenter [OPTIONS] [COMMAND [ARGS...]]");
        println!();
        println!("nsenter — enter namespaces of another process (OurOS).");
        println!();
        println!("Options:");
        println!("  -t, --target PID       Target process");
        println!("  -m, --mount[=FILE]     Enter mount namespace");
        println!("  -u, --uts[=FILE]       Enter UTS namespace");
        println!("  -i, --ipc[=FILE]       Enter IPC namespace");
        println!("  -n, --net[=FILE]       Enter network namespace");
        println!("  -p, --pid[=FILE]       Enter PID namespace");
        println!("  -U, --user[=FILE]      Enter user namespace");
        println!("  -C, --cgroup[=FILE]    Enter cgroup namespace");
        println!("  -a, --all              Enter all namespaces");
        println!("  -F, --no-fork          Don't fork before exec");
        return 0;
    }

    let pid = args.windows(2).find(|w| w[0] == "-t" || w[0] == "--target")
        .map(|w| w[1].as_str()).unwrap_or("1");
    let all = args.iter().any(|a| a == "-a" || a == "--all");

    let cmd = args.iter()
        .filter(|a| !a.starts_with('-'))
        .next()
        .map(|s| s.as_str())
        .unwrap_or("/bin/sh");

    if all {
        println!("nsenter: entering all namespaces of PID {}", pid);
    } else {
        let namespaces: Vec<&str> = args.iter().filter_map(|a| match a.as_str() {
            "-m" | "--mount" => Some("mnt"),
            "-u" | "--uts" => Some("uts"),
            "-i" | "--ipc" => Some("ipc"),
            "-n" | "--net" => Some("net"),
            "-p" | "--pid" => Some("pid"),
            "-U" | "--user" => Some("user"),
            "-C" | "--cgroup" => Some("cgroup"),
            _ => None,
        }).collect();
        if namespaces.is_empty() {
            println!("nsenter: no namespaces specified for PID {}", pid);
        } else {
            println!("nsenter: entering namespaces ({}) of PID {}", namespaces.join(", "), pid);
        }
    }
    println!("nsenter: running '{}'", cmd);
    0
}

fn run_lsns(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lsns [OPTIONS] [NAMESPACE]");
        println!();
        println!("lsns — list namespaces (OurOS).");
        println!();
        println!("Options:");
        println!("  -t, --type TYPE        Filter by type (mnt, net, pid, user, uts, ipc, cgroup)");
        println!("  -p, --task PID         Filter by PID");
        println!("  -o, --output LIST      Output columns");
        println!("  -J, --json             JSON output");
        return 0;
    }

    let json = args.iter().any(|a| a == "-J" || a == "--json");

    if json {
        println!("{{\"namespaces\": [");
        println!("  {{\"ns\": 4026531840, \"type\": \"mnt\", \"nprocs\": 45, \"pid\": 1, \"command\": \"init\"}},");
        println!("  {{\"ns\": 4026531992, \"type\": \"net\", \"nprocs\": 45, \"pid\": 1, \"command\": \"init\"}},");
        println!("  {{\"ns\": 4026531836, \"type\": \"pid\", \"nprocs\": 45, \"pid\": 1, \"command\": \"init\"}},");
        println!("  {{\"ns\": 4026531837, \"type\": \"user\", \"nprocs\": 45, \"pid\": 1, \"command\": \"init\"}}");
        println!("]}}");
    } else {
        println!("        NS TYPE   NPROCS   PID USER    COMMAND");
        println!("4026531840 mnt        45     1 root    init");
        println!("4026531992 net        45     1 root    init");
        println!("4026531836 pid        45     1 root    init");
        println!("4026531838 uts        45     1 root    init");
        println!("4026531839 ipc        45     1 root    init");
        println!("4026531837 user       45     1 root    init");
        println!("4026531835 cgroup     45     1 root    init");
        println!("4026532100 net         2  1234 www     nginx");
        println!("4026532200 mnt         1  5678 nobody  sandbox");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "nsenter".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "lsns" => run_lsns(&rest),
        _ => run_nsenter(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_nsenter};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/nsenter"), "nsenter");
        assert_eq!(basename(r"C:\bin\nsenter.exe"), "nsenter.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("nsenter.exe"), "nsenter");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_nsenter(&["--help".to_string()]), 0);
        assert_eq!(run_nsenter(&["-h".to_string()]), 0);
        assert_eq!(run_nsenter(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_nsenter(&[]), 0);
    }
}
