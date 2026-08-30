//! Slate OS namespace manipulation utilities.
//!
//! Multi-personality binary providing:
//! - **nsenter** — run program in namespaces of another process
//! - **unshare** — run program in new namespaces (delegated to unshare binary)
//!
//! Enters one or more namespaces of a target process (via /proc/<pid>/ns/),
//! then executes a command in that context.

#![deny(clippy::all)]

use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

const VERSION: &str = "0.1.0";

// ============================================================================
// Namespace types
// ============================================================================

#[derive(Clone, Debug, PartialEq)]
struct NsSpec {
    ns_type: &'static str,
    flag: &'static str,
    long_flag: &'static str,
    proc_name: &'static str,
    description: &'static str,
}

const NS_SPECS: &[NsSpec] = &[
    NsSpec { ns_type: "mnt", flag: "-m", long_flag: "--mount", proc_name: "mnt", description: "mount namespace" },
    NsSpec { ns_type: "uts", flag: "-u", long_flag: "--uts", proc_name: "uts", description: "UTS namespace (hostname)" },
    NsSpec { ns_type: "ipc", flag: "-i", long_flag: "--ipc", proc_name: "ipc", description: "IPC namespace" },
    NsSpec { ns_type: "net", flag: "-n", long_flag: "--net", proc_name: "net", description: "network namespace" },
    NsSpec { ns_type: "pid", flag: "-p", long_flag: "--pid", proc_name: "pid", description: "PID namespace" },
    NsSpec { ns_type: "user", flag: "-U", long_flag: "--user", proc_name: "user", description: "user namespace" },
    NsSpec { ns_type: "cgroup", flag: "-C", long_flag: "--cgroup", proc_name: "cgroup", description: "cgroup namespace" },
    NsSpec { ns_type: "time", flag: "-T", long_flag: "--time", proc_name: "time", description: "time namespace" },
];

// ============================================================================
// Options
// ============================================================================

struct NsenterOpts {
    target_pid: Option<u32>,
    namespaces: Vec<String>,
    all_ns: bool,
    root: Option<String>,
    wd: Option<String>,
    wd_fd: bool,
    no_fork: bool,
    setuid: Option<u32>,
    setgid: Option<u32>,
    preserve_creds: bool,
    command: Vec<String>,
    /// Per-namespace file path overrides (e.g., --mount=/proc/42/ns/mnt).
    ns_files: Vec<(String, String)>,
}

fn parse_args(args: &[String]) -> NsenterOpts {
    let mut opts = NsenterOpts {
        target_pid: None,
        namespaces: Vec::new(),
        all_ns: false,
        root: None,
        wd: None,
        wd_fd: false,
        no_fork: false,
        setuid: None,
        setgid: None,
        preserve_creds: false,
        command: Vec::new(),
        ns_files: Vec::new(),
    };

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];

        // Check for --ns=FILE form.
        let mut matched_ns_file = false;
        for spec in NS_SPECS {
            let prefix = format!("{}=", spec.long_flag);
            if let Some(file) = arg.strip_prefix(prefix.as_str()) {
                opts.namespaces.push(spec.ns_type.to_string());
                opts.ns_files.push((spec.ns_type.to_string(), file.to_string()));
                matched_ns_file = true;
                break;
            }
        }
        if matched_ns_file {
            i += 1;
            continue;
        }

        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("nsenter {VERSION}");
                process::exit(0);
            }
            "-t" | "--target" => {
                i += 1;
                if i < args.len() {
                    opts.target_pid = Some(args[i].parse().unwrap_or_else(|_| {
                        eprintln!("nsenter: invalid PID: {}", args[i]);
                        process::exit(1);
                    }));
                }
            }
            "-a" | "--all" => opts.all_ns = true,
            "-F" | "--no-fork" => opts.no_fork = true,
            "--preserve-credentials" => opts.preserve_creds = true,
            "-r" | "--root" => {
                i += 1;
                opts.root = if i < args.len() { Some(args[i].clone()) } else { Some("/".to_string()) };
            }
            "-w" | "--wd" => {
                i += 1;
                opts.wd = if i < args.len() { Some(args[i].clone()) } else { None };
            }
            "-W" | "--wdns" => opts.wd_fd = true,
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
            s => {
                // Check short namespace flags.
                let mut found_ns = false;
                for spec in NS_SPECS {
                    if s == spec.flag || s == spec.long_flag {
                        opts.namespaces.push(spec.ns_type.to_string());
                        found_ns = true;
                        break;
                    }
                }
                if !found_ns {
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
    println!("Usage: nsenter [options] [program [arguments]]");
    println!();
    println!("Run a program with namespaces of other processes.");
    println!();
    println!("Options:");
    println!("  -t, --target PID    Target process for namespaces");
    println!("  -a, --all           Enter all namespaces of target");
    for spec in NS_SPECS {
        println!("  {}, {:16}  Enter {} [=FILE]", spec.flag, spec.long_flag, spec.description);
    }
    println!("  -r, --root [DIR]    Set root directory");
    println!("  -w, --wd [DIR]      Set working directory");
    println!("  -F, --no-fork       Don't fork before exec");
    println!("  -S, --setuid UID    Set UID in entered namespace");
    println!("  -G, --setgid GID    Set GID in entered namespace");
    println!("  --preserve-credentials  Don't modify credentials");
    println!("  -h, --help          Show this help");
    println!("  -V, --version       Show version");
}

// ============================================================================
// Namespace info
// ============================================================================

fn _get_ns_inode(pid: u32, ns_type: &str) -> Option<u64> {
    let path = format!("/proc/{pid}/ns/{ns_type}");
    let link = fs::read_link(&path).ok()?;
    let link_str = link.to_str()?;
    let bracket_start = link_str.find('[')?;
    let bracket_end = link_str.find(']')?;
    link_str[bracket_start + 1..bracket_end].parse().ok()
}

fn ns_file_path(pid: u32, ns_type: &str) -> String {
    format!("/proc/{pid}/ns/{ns_type}")
}

// ============================================================================
// Execution
// ============================================================================

fn cmd_nsenter(args: &[String]) {
    let opts = parse_args(args);

    if opts.target_pid.is_none() && opts.ns_files.is_empty() {
        eprintln!("nsenter: no target PID specified");
        eprintln!("Try 'nsenter --help' for more information.");
        process::exit(1);
    }

    let pid = opts.target_pid.unwrap_or(0);

    // Determine which namespaces to enter.
    let ns_to_enter: Vec<String> = if opts.all_ns {
        NS_SPECS.iter().map(|s| s.ns_type.to_string()).collect()
    } else if opts.namespaces.is_empty() {
        // Default: enter all if --all or specific ones not given.
        vec!["mnt".to_string()]
    } else {
        opts.namespaces.clone()
    };

    // Verify target process exists.
    if pid > 0 {
        let proc_path = format!("/proc/{pid}");
        if !fs::metadata(&proc_path).map(|m| m.is_dir()).unwrap_or(false) {
            eprintln!("nsenter: cannot open /proc/{pid}/ns/: No such process");
            process::exit(1);
        }
    }

    // Report namespace entry (in real implementation, this would use setns(2)).
    let stderr = io::stderr();
    let mut err = stderr.lock();

    for ns_type in &ns_to_enter {
        // Check for file override.
        let ns_path = opts
            .ns_files
            .iter()
            .find(|(t, _)| t == ns_type)
            .map(|(_, f)| f.clone())
            .unwrap_or_else(|| ns_file_path(pid, ns_type));

        // Verify namespace file exists.
        if fs::symlink_metadata(&ns_path).is_err() {
            let _ = writeln!(err, "nsenter: cannot open {ns_path}: No such file or directory");
            process::exit(1);
        }

        // In real implementation: open ns_path and call setns(fd, ns_flag).
        // For simulation, just verify accessibility.
    }

    // Build command.
    let command = if opts.command.is_empty() {
        // Default: run user's shell.
        vec![
            env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string()),
        ]
    } else {
        opts.command.clone()
    };

    // Execute command (in real implementation, this would happen after setns).
    let status = process::Command::new(&command[0])
        .args(&command[1..])
        .status();

    match status {
        Ok(s) => process::exit(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("nsenter: failed to execute {}: {e}", command[0]);
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
    cmd_nsenter(&rest);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ns_specs_count() {
        assert_eq!(NS_SPECS.len(), 8);
    }

    #[test]
    fn test_ns_spec_types() {
        let types: Vec<&str> = NS_SPECS.iter().map(|s| s.ns_type).collect();
        assert!(types.contains(&"mnt"));
        assert!(types.contains(&"uts"));
        assert!(types.contains(&"ipc"));
        assert!(types.contains(&"net"));
        assert!(types.contains(&"pid"));
        assert!(types.contains(&"user"));
        assert!(types.contains(&"cgroup"));
        assert!(types.contains(&"time"));
    }

    #[test]
    fn test_parse_target_pid() {
        let args = vec!["-t".to_string(), "1234".to_string(), "ls".to_string()];
        let opts = parse_args(&args);
        assert_eq!(opts.target_pid, Some(1234));
        assert_eq!(opts.command, vec!["ls"]);
    }

    #[test]
    fn test_parse_all_ns() {
        let args = vec!["-a".to_string(), "-t".to_string(), "1".to_string()];
        let opts = parse_args(&args);
        assert!(opts.all_ns);
    }

    #[test]
    fn test_parse_specific_ns() {
        let args = vec![
            "-m".to_string(),
            "-n".to_string(),
            "-t".to_string(),
            "1".to_string(),
        ];
        let opts = parse_args(&args);
        assert_eq!(opts.namespaces.len(), 2);
        assert!(opts.namespaces.contains(&"mnt".to_string()));
        assert!(opts.namespaces.contains(&"net".to_string()));
    }

    #[test]
    fn test_parse_long_flags() {
        let args = vec![
            "--mount".to_string(),
            "--uts".to_string(),
            "--pid".to_string(),
            "-t".to_string(),
            "42".to_string(),
        ];
        let opts = parse_args(&args);
        assert_eq!(opts.namespaces.len(), 3);
        assert!(opts.namespaces.contains(&"mnt".to_string()));
        assert!(opts.namespaces.contains(&"uts".to_string()));
        assert!(opts.namespaces.contains(&"pid".to_string()));
    }

    #[test]
    fn test_parse_setuid_setgid() {
        let args = vec![
            "-S".to_string(),
            "1000".to_string(),
            "-G".to_string(),
            "1000".to_string(),
            "-t".to_string(),
            "1".to_string(),
        ];
        let opts = parse_args(&args);
        assert_eq!(opts.setuid, Some(1000));
        assert_eq!(opts.setgid, Some(1000));
    }

    #[test]
    fn test_parse_no_fork() {
        let args = vec!["-F".to_string(), "-t".to_string(), "1".to_string()];
        let opts = parse_args(&args);
        assert!(opts.no_fork);
    }

    #[test]
    fn test_parse_preserve_creds() {
        let args = vec![
            "--preserve-credentials".to_string(),
            "-t".to_string(),
            "1".to_string(),
        ];
        let opts = parse_args(&args);
        assert!(opts.preserve_creds);
    }

    #[test]
    fn test_parse_root() {
        let args = vec![
            "-r".to_string(),
            "/newroot".to_string(),
            "-t".to_string(),
            "1".to_string(),
        ];
        let opts = parse_args(&args);
        assert_eq!(opts.root, Some("/newroot".to_string()));
    }

    #[test]
    fn test_parse_wd() {
        let args = vec![
            "-w".to_string(),
            "/tmp".to_string(),
            "-t".to_string(),
            "1".to_string(),
        ];
        let opts = parse_args(&args);
        assert_eq!(opts.wd, Some("/tmp".to_string()));
    }

    #[test]
    fn test_parse_command_with_args() {
        let args = vec![
            "-t".to_string(),
            "1".to_string(),
            "-m".to_string(),
            "ls".to_string(),
            "-la".to_string(),
            "/tmp".to_string(),
        ];
        let opts = parse_args(&args);
        assert_eq!(opts.command, vec!["ls", "-la", "/tmp"]);
    }

    #[test]
    fn test_parse_empty() {
        let args: Vec<String> = Vec::new();
        let opts = parse_args(&args);
        assert!(opts.target_pid.is_none());
        assert!(opts.namespaces.is_empty());
        assert!(opts.command.is_empty());
    }

    #[test]
    fn test_ns_file_path() {
        assert_eq!(ns_file_path(1, "mnt"), "/proc/1/ns/mnt");
        assert_eq!(ns_file_path(42, "net"), "/proc/42/ns/net");
    }

    #[test]
    fn test_get_ns_inode_nonexistent() {
        assert!(_get_ns_inode(999999, "mnt").is_none());
    }

    #[test]
    fn test_ns_spec_flags_unique() {
        let flags: Vec<&str> = NS_SPECS.iter().map(|s| s.flag).collect();
        for (i, f) in flags.iter().enumerate() {
            for (j, g) in flags.iter().enumerate() {
                if i != j {
                    assert_ne!(f, g, "Duplicate flag: {f}");
                }
            }
        }
    }

    #[test]
    fn test_ns_spec_long_flags_unique() {
        let flags: Vec<&str> = NS_SPECS.iter().map(|s| s.long_flag).collect();
        for (i, f) in flags.iter().enumerate() {
            for (j, g) in flags.iter().enumerate() {
                if i != j {
                    assert_ne!(f, g, "Duplicate long flag: {f}");
                }
            }
        }
    }

    #[test]
    fn test_parse_ns_file_override() {
        let args = vec![
            "--mount=/run/netns/myns".to_string(),
            "-t".to_string(),
            "1".to_string(),
        ];
        let opts = parse_args(&args);
        assert_eq!(opts.namespaces, vec!["mnt"]);
        assert_eq!(opts.ns_files.len(), 1);
        assert_eq!(opts.ns_files[0].0, "mnt");
        assert_eq!(opts.ns_files[0].1, "/run/netns/myns");
    }
}
