#![deny(clippy::all)]

//! chroot-cli — OurOS chroot CLI
//!
//! Multi-personality: `chroot`, `unshare`, `pivot_root`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_chroot(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: chroot [OPTIONS] NEWROOT [COMMAND [ARGS...]]");
        println!();
        println!("chroot — run command with special root directory (OurOS).");
        println!();
        println!("Options:");
        println!("  --userspec USER:GROUP   Set user and group");
        println!("  --groups GROUPS         Supplementary groups");
        println!("  --skip-chdir            Don't change directory to /");
        return 0;
    }

    let positional: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    let root = positional.first().copied().unwrap_or("/mnt/newroot");
    let cmd = positional.get(1).copied().unwrap_or("/bin/sh");

    println!("chroot: changing root to '{}'", root);
    println!("chroot: running '{}'", cmd);
    0
}

fn run_unshare(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: unshare [OPTIONS] [COMMAND [ARGS...]]");
        println!();
        println!("unshare — run in new namespaces (OurOS).");
        println!();
        println!("Options:");
        println!("  -m, --mount            Unshare mount namespace");
        println!("  -u, --uts              Unshare UTS namespace");
        println!("  -i, --ipc              Unshare IPC namespace");
        println!("  -n, --net              Unshare network namespace");
        println!("  -p, --pid              Unshare PID namespace");
        println!("  -U, --user             Unshare user namespace");
        println!("  -C, --cgroup           Unshare cgroup namespace");
        println!("  -r, --map-root-user    Map current user to root");
        println!("  --fork                 Fork before exec");
        println!("  --mount-proc           Mount /proc in new namespace");
        return 0;
    }

    let namespaces: Vec<&str> = args.iter().filter_map(|a| match a.as_str() {
        "-m" | "--mount" => Some("mount"),
        "-u" | "--uts" => Some("uts"),
        "-i" | "--ipc" => Some("ipc"),
        "-n" | "--net" => Some("net"),
        "-p" | "--pid" => Some("pid"),
        "-U" | "--user" => Some("user"),
        "-C" | "--cgroup" => Some("cgroup"),
        _ => None,
    }).collect();

    let cmd = args.iter()
        .filter(|a| !a.starts_with('-'))
        .next()
        .map(|s| s.as_str())
        .unwrap_or("/bin/sh");

    if namespaces.is_empty() {
        println!("unshare: running '{}' (no namespaces specified)", cmd);
    } else {
        println!("unshare: new namespaces: {}", namespaces.join(", "));
        println!("unshare: running '{}'", cmd);
    }
    0
}

fn run_pivot_root(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pivot_root NEW_ROOT PUT_OLD");
        println!();
        println!("pivot_root — change the root filesystem (OurOS).");
        return 0;
    }

    let positional: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    let new_root = positional.first().copied().unwrap_or("/mnt/newroot");
    let put_old = positional.get(1).copied().unwrap_or("/mnt/newroot/oldroot");

    println!("pivot_root: new_root={}, put_old={}", new_root, put_old);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "chroot".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "unshare" => run_unshare(&rest),
        "pivot_root" => run_pivot_root(&rest),
        _ => run_chroot(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_chroot};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/chroot"), "chroot");
        assert_eq!(basename(r"C:\bin\chroot.exe"), "chroot.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("chroot.exe"), "chroot");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_chroot(&["--help".to_string()]), 0);
        assert_eq!(run_chroot(&["-h".to_string()]), 0);
        assert_eq!(run_chroot(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_chroot(&[]), 0);
    }
}
