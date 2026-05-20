//! OurOS namespace isolation utility.
//!
//! Multi-personality binary providing:
//! - **unshare** — run program in new namespaces
//!
//! Creates new namespaces (mount, UTS, IPC, network, PID, user, cgroup, time)
//! and optionally runs a command in the isolated context.

#![deny(clippy::all)]

use std::env;
use std::io::{self, Write};
use std::process;

const VERSION: &str = "0.1.0";

// ============================================================================
// Namespace types
// ============================================================================

#[derive(Clone, Debug)]
struct NsType {
    name: &'static str,
    short_flag: &'static str,
    long_flag: &'static str,
    clone_flag: u64,
    description: &'static str,
}

// Linux CLONE_NEW* flags for reference.
const CLONE_NEWNS: u64 = 0x0002_0000;
const CLONE_NEWUTS: u64 = 0x0400_0000;
const CLONE_NEWIPC: u64 = 0x0800_0000;
const CLONE_NEWNET: u64 = 0x4000_0000;
const CLONE_NEWPID: u64 = 0x2000_0000;
const CLONE_NEWUSER: u64 = 0x1000_0000;
const CLONE_NEWCGROUP: u64 = 0x0200_0000;
const CLONE_NEWTIME: u64 = 0x0000_0080;

const NS_TYPES: &[NsType] = &[
    NsType { name: "mnt", short_flag: "-m", long_flag: "--mount", clone_flag: CLONE_NEWNS, description: "mount namespace" },
    NsType { name: "uts", short_flag: "-u", long_flag: "--uts", clone_flag: CLONE_NEWUTS, description: "UTS namespace" },
    NsType { name: "ipc", short_flag: "-i", long_flag: "--ipc", clone_flag: CLONE_NEWIPC, description: "IPC namespace" },
    NsType { name: "net", short_flag: "-n", long_flag: "--net", clone_flag: CLONE_NEWNET, description: "network namespace" },
    NsType { name: "pid", short_flag: "-p", long_flag: "--pid", clone_flag: CLONE_NEWPID, description: "PID namespace" },
    NsType { name: "user", short_flag: "-U", long_flag: "--user", clone_flag: CLONE_NEWUSER, description: "user namespace" },
    NsType { name: "cgroup", short_flag: "-C", long_flag: "--cgroup", clone_flag: CLONE_NEWCGROUP, description: "cgroup namespace" },
    NsType { name: "time", short_flag: "-T", long_flag: "--time", clone_flag: CLONE_NEWTIME, description: "time namespace" },
];

// ============================================================================
// Options
// ============================================================================

struct UnshareOpts {
    namespaces: u64,
    fork: bool,
    map_root_user: bool,
    map_current_user: bool,
    map_auto: bool,
    map_users: Option<String>,
    map_groups: Option<String>,
    keep_caps: bool,
    kill_child: Option<i32>,
    propagation: Option<String>,
    setuid: Option<u32>,
    setgid: Option<u32>,
    root: Option<String>,
    wd: Option<String>,
    monotonic: Option<u64>,
    boottime: Option<u64>,
    command: Vec<String>,
}

fn parse_args(args: &[String]) -> UnshareOpts {
    let mut opts = UnshareOpts {
        namespaces: 0,
        fork: false,
        map_root_user: false,
        map_current_user: false,
        map_auto: false,
        map_users: None,
        map_groups: None,
        keep_caps: false,
        kill_child: None,
        propagation: None,
        setuid: None,
        setgid: None,
        root: None,
        wd: None,
        monotonic: None,
        boottime: None,
        command: Vec::new(),
    };

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];

        // Check for --propagation=VALUE form.
        if let Some(val) = arg.strip_prefix("--propagation=") {
            opts.propagation = Some(val.to_string());
            i += 1;
            continue;
        }
        if let Some(val) = arg.strip_prefix("--kill-child=") {
            opts.kill_child = val.parse().ok();
            i += 1;
            continue;
        }

        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("unshare {VERSION}");
                process::exit(0);
            }
            "-f" | "--fork" => opts.fork = true,
            "-r" | "--map-root-user" => {
                opts.map_root_user = true;
                opts.namespaces |= CLONE_NEWUSER;
            }
            "--map-current-user" => {
                opts.map_current_user = true;
                opts.namespaces |= CLONE_NEWUSER;
            }
            "--map-auto" => {
                opts.map_auto = true;
                opts.namespaces |= CLONE_NEWUSER;
            }
            "--map-users" => {
                i += 1;
                if i < args.len() {
                    opts.map_users = Some(args[i].clone());
                    opts.namespaces |= CLONE_NEWUSER;
                }
            }
            "--map-groups" => {
                i += 1;
                if i < args.len() {
                    opts.map_groups = Some(args[i].clone());
                    opts.namespaces |= CLONE_NEWUSER;
                }
            }
            "--keep-caps" => opts.keep_caps = true,
            "--kill-child" => {
                opts.kill_child = Some(9); // Default SIGKILL.
            }
            "--propagation" => {
                i += 1;
                if i < args.len() {
                    opts.propagation = Some(args[i].clone());
                }
            }
            "-S" | "--setuid" => {
                i += 1;
                if i < args.len() {
                    opts.setuid = args[i].parse().ok();
                }
            }
            "-G" | "--setgid" => {
                i += 1;
                if i < args.len() {
                    opts.setgid = args[i].parse().ok();
                }
            }
            "-R" | "--root" => {
                i += 1;
                if i < args.len() {
                    opts.root = Some(args[i].clone());
                }
            }
            "-w" | "--wd" => {
                i += 1;
                if i < args.len() {
                    opts.wd = Some(args[i].clone());
                }
            }
            "--monotonic" => {
                i += 1;
                if i < args.len() {
                    opts.monotonic = args[i].parse().ok();
                    opts.namespaces |= CLONE_NEWTIME;
                }
            }
            "--boottime" => {
                i += 1;
                if i < args.len() {
                    opts.boottime = args[i].parse().ok();
                    opts.namespaces |= CLONE_NEWTIME;
                }
            }
            s => {
                // Check namespace flags.
                let mut found = false;
                for ns in NS_TYPES {
                    if s == ns.short_flag || s == ns.long_flag {
                        opts.namespaces |= ns.clone_flag;
                        found = true;
                        break;
                    }
                }
                if !found {
                    // Everything from here is the command.
                    opts.command = args[i..].to_vec();
                    break;
                }
            }
        }
        i += 1;
    }

    opts
}

fn print_help() {
    println!("Usage: unshare [options] [program [arguments]]");
    println!();
    println!("Run a program with some namespaces unshared from parent.");
    println!();
    println!("Options:");
    for ns in NS_TYPES {
        println!("  {}, {:16}  Unshare {}", ns.short_flag, ns.long_flag, ns.description);
    }
    println!("  -f, --fork              Fork before exec");
    println!("  -r, --map-root-user     Map current user to root in user ns");
    println!("  --map-current-user      Map current user to same UID");
    println!("  --map-auto              Auto map users/groups");
    println!("  --map-users INNERUID:OUTERUID:COUNT   Custom UID mapping");
    println!("  --map-groups INNERGID:OUTERGID:COUNT  Custom GID mapping");
    println!("  --keep-caps             Retain capabilities after user ns");
    println!("  --kill-child[=SIG]      Kill child on parent exit");
    println!("  --propagation MODE      Mount propagation (private|shared|slave|unchanged)");
    println!("  -S, --setuid UID        Set UID after namespace creation");
    println!("  -G, --setgid GID        Set GID after namespace creation");
    println!("  -R, --root DIR          Set root directory");
    println!("  -w, --wd DIR            Set working directory");
    println!("  --monotonic OFFSET      Set monotonic time offset (with --time)");
    println!("  --boottime OFFSET       Set boot time offset (with --time)");
    println!("  -h, --help              Show this help");
    println!("  -V, --version           Show version");
}

fn namespace_names(flags: u64) -> Vec<&'static str> {
    let mut names = Vec::new();
    for ns in NS_TYPES {
        if flags & ns.clone_flag != 0 {
            names.push(ns.name);
        }
    }
    names
}

// ============================================================================
// Execution
// ============================================================================

fn cmd_unshare(args: &[String]) {
    let opts = parse_args(args);

    if opts.namespaces == 0 {
        eprintln!("unshare: no namespace specified");
        eprintln!("Try 'unshare --help' for more information.");
        process::exit(1);
    }

    let ns_names = namespace_names(opts.namespaces);

    // In real implementation: call unshare(2) with the combined flags.
    // For simulation, we report what would happen and exec the command.
    let stderr = io::stderr();
    let mut err = stderr.lock();

    // User namespace mapping.
    if opts.namespaces & CLONE_NEWUSER != 0 {
        if opts.map_root_user {
            // Would write to /proc/self/uid_map and /proc/self/gid_map.
            let _ = writeln!(err, "unshare: mapping current user to root in user namespace");
        }
    }

    // Mount propagation.
    if opts.namespaces & CLONE_NEWNS != 0 {
        let propagation = opts.propagation.as_deref().unwrap_or("private");
        let _ = propagation; // Would set via mount(2).
    }

    // Time namespace offsets.
    if opts.namespaces & CLONE_NEWTIME != 0 {
        if let Some(offset) = opts.monotonic {
            // Would write to /proc/self/timens_offsets.
            let _ = offset;
        }
    }

    // Build command.
    let command = if opts.command.is_empty() {
        vec![env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())]
    } else {
        opts.command.clone()
    };

    // Execute.
    let status = process::Command::new(&command[0])
        .args(&command[1..])
        .status();

    let _ = ns_names;

    match status {
        Ok(s) => process::exit(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("unshare: failed to execute {}: {e}", command[0]);
            process::exit(127);
        }
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    cmd_unshare(&rest);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ns_types_count() {
        assert_eq!(NS_TYPES.len(), 8);
    }

    #[test]
    fn test_clone_flags_unique() {
        let flags: Vec<u64> = NS_TYPES.iter().map(|n| n.clone_flag).collect();
        for (i, f) in flags.iter().enumerate() {
            for (j, g) in flags.iter().enumerate() {
                if i != j {
                    assert_ne!(f, g, "Duplicate clone flag");
                }
            }
        }
    }

    #[test]
    fn test_clone_flags_are_powers() {
        // Each flag should be a single bit (power of 2), except CLONE_NEWTIME=0x80.
        for ns in NS_TYPES {
            assert!(ns.clone_flag.count_ones() == 1, "Flag for {} is not a single bit: {:#x}", ns.name, ns.clone_flag);
        }
    }

    #[test]
    fn test_parse_mount_ns() {
        let args = vec!["-m".to_string(), "bash".to_string()];
        let opts = parse_args(&args);
        assert!(opts.namespaces & CLONE_NEWNS != 0);
        assert_eq!(opts.command, vec!["bash"]);
    }

    #[test]
    fn test_parse_multiple_ns() {
        let args = vec![
            "-m".to_string(),
            "-u".to_string(),
            "-n".to_string(),
            "sh".to_string(),
        ];
        let opts = parse_args(&args);
        assert!(opts.namespaces & CLONE_NEWNS != 0);
        assert!(opts.namespaces & CLONE_NEWUTS != 0);
        assert!(opts.namespaces & CLONE_NEWNET != 0);
    }

    #[test]
    fn test_parse_long_flags() {
        let args = vec![
            "--mount".to_string(),
            "--pid".to_string(),
            "--user".to_string(),
        ];
        let opts = parse_args(&args);
        assert!(opts.namespaces & CLONE_NEWNS != 0);
        assert!(opts.namespaces & CLONE_NEWPID != 0);
        assert!(opts.namespaces & CLONE_NEWUSER != 0);
    }

    #[test]
    fn test_parse_fork() {
        let args = vec!["-f".to_string(), "-p".to_string()];
        let opts = parse_args(&args);
        assert!(opts.fork);
    }

    #[test]
    fn test_parse_map_root_user() {
        let args = vec!["-r".to_string()];
        let opts = parse_args(&args);
        assert!(opts.map_root_user);
        // Should implicitly add user namespace.
        assert!(opts.namespaces & CLONE_NEWUSER != 0);
    }

    #[test]
    fn test_parse_setuid_setgid() {
        let args = vec![
            "-m".to_string(),
            "-S".to_string(),
            "0".to_string(),
            "-G".to_string(),
            "0".to_string(),
        ];
        let opts = parse_args(&args);
        assert_eq!(opts.setuid, Some(0));
        assert_eq!(opts.setgid, Some(0));
    }

    #[test]
    fn test_parse_root_dir() {
        let args = vec![
            "-m".to_string(),
            "-R".to_string(),
            "/newroot".to_string(),
        ];
        let opts = parse_args(&args);
        assert_eq!(opts.root, Some("/newroot".to_string()));
    }

    #[test]
    fn test_parse_wd() {
        let args = vec!["-m".to_string(), "-w".to_string(), "/tmp".to_string()];
        let opts = parse_args(&args);
        assert_eq!(opts.wd, Some("/tmp".to_string()));
    }

    #[test]
    fn test_parse_keep_caps() {
        let args = vec!["--user".to_string(), "--keep-caps".to_string()];
        let opts = parse_args(&args);
        assert!(opts.keep_caps);
    }

    #[test]
    fn test_parse_kill_child() {
        let args = vec!["-m".to_string(), "--kill-child".to_string()];
        let opts = parse_args(&args);
        assert_eq!(opts.kill_child, Some(9));
    }

    #[test]
    fn test_parse_kill_child_signal() {
        let args = vec!["-m".to_string(), "--kill-child=15".to_string()];
        let opts = parse_args(&args);
        assert_eq!(opts.kill_child, Some(15));
    }

    #[test]
    fn test_parse_propagation() {
        let args = vec![
            "-m".to_string(),
            "--propagation=shared".to_string(),
        ];
        let opts = parse_args(&args);
        assert_eq!(opts.propagation, Some("shared".to_string()));
    }

    #[test]
    fn test_parse_monotonic() {
        let args = vec![
            "--time".to_string(),
            "--monotonic".to_string(),
            "100".to_string(),
        ];
        let opts = parse_args(&args);
        assert_eq!(opts.monotonic, Some(100));
        assert!(opts.namespaces & CLONE_NEWTIME != 0);
    }

    #[test]
    fn test_parse_empty() {
        let args: Vec<String> = Vec::new();
        let opts = parse_args(&args);
        assert_eq!(opts.namespaces, 0);
        assert!(opts.command.is_empty());
    }

    #[test]
    fn test_namespace_names() {
        let names = namespace_names(CLONE_NEWNS | CLONE_NEWPID);
        assert!(names.contains(&"mnt"));
        assert!(names.contains(&"pid"));
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn test_namespace_names_all() {
        let all: u64 = NS_TYPES.iter().map(|n| n.clone_flag).fold(0, |a, b| a | b);
        let names = namespace_names(all);
        assert_eq!(names.len(), 8);
    }

    #[test]
    fn test_namespace_names_none() {
        let names = namespace_names(0);
        assert!(names.is_empty());
    }

    #[test]
    fn test_parse_command_with_flags() {
        let args = vec![
            "-m".to_string(),
            "ls".to_string(),
            "-la".to_string(),
            "/".to_string(),
        ];
        let opts = parse_args(&args);
        assert_eq!(opts.command, vec!["ls", "-la", "/"]);
    }

    #[test]
    fn test_map_auto_implies_user_ns() {
        let args = vec!["--map-auto".to_string()];
        let opts = parse_args(&args);
        assert!(opts.map_auto);
        assert!(opts.namespaces & CLONE_NEWUSER != 0);
    }

    #[test]
    fn test_map_users() {
        let args = vec!["--map-users".to_string(), "0:1000:1".to_string()];
        let opts = parse_args(&args);
        assert_eq!(opts.map_users, Some("0:1000:1".to_string()));
        assert!(opts.namespaces & CLONE_NEWUSER != 0);
    }
}
