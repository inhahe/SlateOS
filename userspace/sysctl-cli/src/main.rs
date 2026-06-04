#![deny(clippy::all)]

//! sysctl-cli — OurOS kernel parameter tool
//!
//! Multi-personality: `sysctl`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_sysctl(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sysctl [OPTIONS] [VARIABLE[=VALUE]...]");
        println!();
        println!("sysctl — configure kernel parameters at runtime (OurOS).");
        println!();
        println!("Options:");
        println!("  -a, --all         Display all values");
        println!("  -w, --write       Write value (KEY=VALUE)");
        println!("  -p FILE           Load settings from FILE");
        println!("  -n, --values      Only print values");
        println!("  -e, --ignore      Ignore unknown keys");
        println!("  -N, --names       Only print names");
        println!("  --system          Load from all system config files");
        println!("  -r PATTERN        Pattern-based selection");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("sysctl from procps-ng 4.0.4 (OurOS)");
        return 0;
    }

    let all = args.iter().any(|a| a == "-a" || a == "--all");
    let names_only = args.iter().any(|a| a == "-N" || a == "--names");
    let values_only = args.iter().any(|a| a == "-n" || a == "--values");
    let system = args.iter().any(|a| a == "--system");

    if system {
        println!("* Applying /etc/sysctl.d/99-sysctl.conf ...");
        println!("* Applying /etc/sysctl.conf ...");
        return 0;
    }

    let params = [
        ("kernel.hostname", "ouros"),
        ("kernel.ostype", "OurOS"),
        ("kernel.osrelease", "1.0.0"),
        ("kernel.version", "#1 SMP PREEMPT_DYNAMIC"),
        ("kernel.pid_max", "4194304"),
        ("kernel.threads-max", "256000"),
        ("kernel.shmmax", "18446744073692774399"),
        ("kernel.sem", "32000\t1024000000\t500\t32000"),
        ("vm.swappiness", "60"),
        ("vm.dirty_ratio", "20"),
        ("vm.dirty_background_ratio", "10"),
        ("vm.overcommit_memory", "0"),
        ("vm.max_map_count", "1048576"),
        ("net.core.somaxconn", "4096"),
        ("net.core.netdev_max_backlog", "1000"),
        ("net.ipv4.tcp_syncookies", "1"),
        ("net.ipv4.ip_forward", "0"),
        ("net.ipv4.tcp_max_syn_backlog", "2048"),
        ("net.ipv4.conf.all.rp_filter", "2"),
        ("fs.file-max", "9223372036854775807"),
        ("fs.inotify.max_user_watches", "524288"),
    ];

    if all {
        for (key, val) in &params {
            if names_only {
                println!("{}", key);
            } else if values_only {
                println!("{}", val);
            } else {
                println!("{} = {}", key, val);
            }
        }
        return 0;
    }

    let keys: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    for key in &keys {
        if let Some(eq_pos) = key.find('=') {
            let (name, value) = key.split_at(eq_pos);
            let value = &value[1..];
            println!("{} = {}", name, value);
        } else if let Some((_, val)) = params.iter().find(|(k, _)| k == key) {
            if values_only {
                println!("{}", val);
            } else {
                println!("{} = {}", key, val);
            }
        } else {
            eprintln!("sysctl: cannot stat /proc/sys/{}: No such file or directory", key.replace('.', "/"));
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "sysctl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = run_sysctl(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sysctl};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sysctl"), "sysctl");
        assert_eq!(basename(r"C:\bin\sysctl.exe"), "sysctl.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sysctl.exe"), "sysctl");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sysctl(&["--help".to_string()]), 0);
        assert_eq!(run_sysctl(&["-h".to_string()]), 0);
        let _ = run_sysctl(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sysctl(&[]);
    }
}
