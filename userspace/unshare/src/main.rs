//! Slate OS namespace isolation utility.
//!
//! Multi-personality binary providing:
//! - **unshare** — run program in new namespaces
//!
//! Creates new namespaces (mount, UTS, IPC, network, PID, user, cgroup, time)
//! and optionally runs a command in the isolated context.
//!
//! **Except that it cannot, on this system, and therefore refuses.**
//! `posix::unshare` validates its flags and its `CAP_SYS_ADMIN` gate and then
//! returns `ENOSYS`. Until 2026-09-15 this program called `unshare(2)` not at
//! all, printed "unshare: mapping current user to root in user namespace" --
//! present tense, about something it had not done -- and then ran the command
//! with no namespaces created. Running UNISOLATED is the harm rather than a
//! lesser version of it: `unshare` is reached for when an operation is risky
//! enough to want containing, and the command succeeded against the host.

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
    NsType {
        name: "mnt",
        short_flag: "-m",
        long_flag: "--mount",
        clone_flag: CLONE_NEWNS,
        description: "mount namespace",
    },
    NsType {
        name: "uts",
        short_flag: "-u",
        long_flag: "--uts",
        clone_flag: CLONE_NEWUTS,
        description: "UTS namespace",
    },
    NsType {
        name: "ipc",
        short_flag: "-i",
        long_flag: "--ipc",
        clone_flag: CLONE_NEWIPC,
        description: "IPC namespace",
    },
    NsType {
        name: "net",
        short_flag: "-n",
        long_flag: "--net",
        clone_flag: CLONE_NEWNET,
        description: "network namespace",
    },
    NsType {
        name: "pid",
        short_flag: "-p",
        long_flag: "--pid",
        clone_flag: CLONE_NEWPID,
        description: "PID namespace",
    },
    NsType {
        name: "user",
        short_flag: "-U",
        long_flag: "--user",
        clone_flag: CLONE_NEWUSER,
        description: "user namespace",
    },
    NsType {
        name: "cgroup",
        short_flag: "-C",
        long_flag: "--cgroup",
        clone_flag: CLONE_NEWCGROUP,
        description: "cgroup namespace",
    },
    NsType {
        name: "time",
        short_flag: "-T",
        long_flag: "--time",
        clone_flag: CLONE_NEWTIME,
        description: "time namespace",
    },
];

// ============================================================================
// Options
// ============================================================================

struct UnshareOpts {
    namespaces: u64,
    /// Options accepted, and named in the refusal.
    ///
    /// FIFTEEN FIELDS USED TO LIVE HERE -- `fork`, `keep_caps`, `setuid`,
    /// `propagation` and the rest -- every one of them written by the parser
    /// and read by nothing. Fifteen tests asserted each had been STORED,
    /// which looks like coverage and is not: a test that a value was written
    /// says nothing about whether anything reads it, and nothing did.
    ///
    /// They collapse to this because the program refuses. There is no
    /// behaviour to configure, so there is nothing to configure it WITH --
    /// only a record of what was asked for, which the refusal can then name.
    /// That makes the list read, makes the tests mean something (the refusal
    /// prints what they assert), and leaves the parser accepting every option
    /// it did before, so a script passing `--keep-caps` still gets the
    /// refusal rather than a usage error.
    requested: Vec<&'static str>,
    command: Vec<String>,
}

impl UnshareOpts {
    /// Record an option the caller asked for, once.
    fn asked(&mut self, name: &'static str) {
        if !self.requested.contains(&name) {
            self.requested.push(name);
        }
    }
}

fn parse_args(args: &[String]) -> UnshareOpts {
    let mut opts = UnshareOpts {
        namespaces: 0,
        requested: Vec::new(),
        command: Vec::new(),
    };

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];

        // Check for --propagation=VALUE form.
        // The `=VALUE` forms. The value is discarded with the rest, but the
        // arm must stay so the word is not mistaken for the command.
        if arg.starts_with("--propagation=") {
            opts.asked("--propagation");
            i += 1;
            continue;
        }
        if arg.starts_with("--kill-child=") {
            opts.asked("--kill-child");
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
            // Each arm still CONSUMES what it consumed before. Dropping a
            // value without dropping its argument would push that argument
            // into `command`, and the refusal names the command -- so a
            // `--setuid 0` would have been reported as the thing refused to
            // run.
            "-f" | "--fork" => opts.asked("--fork"),
            "-r" | "--map-root-user" => {
                opts.asked("--map-root-user");
                opts.namespaces |= CLONE_NEWUSER;
            }
            "--map-current-user" => {
                opts.asked("--map-current-user");
                opts.namespaces |= CLONE_NEWUSER;
            }
            "--map-auto" => {
                opts.asked("--map-auto");
                opts.namespaces |= CLONE_NEWUSER;
            }
            "--map-users" => {
                i += 1;
                if i < args.len() {
                    opts.asked("--map-users");
                    opts.namespaces |= CLONE_NEWUSER;
                }
            }
            "--map-groups" => {
                i += 1;
                if i < args.len() {
                    opts.asked("--map-groups");
                    opts.namespaces |= CLONE_NEWUSER;
                }
            }
            "--keep-caps" => opts.asked("--keep-caps"),
            "--kill-child" => opts.asked("--kill-child"),
            "--propagation" => {
                i += 1;
                if i < args.len() {
                    opts.asked("--propagation");
                }
            }
            "-S" | "--setuid" => {
                i += 1;
                if i < args.len() {
                    opts.asked("--setuid");
                }
            }
            "-G" | "--setgid" => {
                i += 1;
                if i < args.len() {
                    opts.asked("--setgid");
                }
            }
            "-R" | "--root" => {
                i += 1;
                if i < args.len() {
                    opts.asked("--root");
                }
            }
            "-w" | "--wd" => {
                i += 1;
                if i < args.len() {
                    opts.asked("--wd");
                }
            }
            "--monotonic" => {
                i += 1;
                if i < args.len() {
                    opts.asked("--monotonic");
                    opts.namespaces |= CLONE_NEWTIME;
                }
            }
            "--boottime" => {
                i += 1;
                if i < args.len() {
                    opts.asked("--boottime");
                    opts.namespaces |= CLONE_NEWTIME;
                }
            }
            // `--` ENDS THE OPTIONS and is not itself the command. Without
            // this arm it fell through to "everything from here is the
            // command", so `unshare -m -- true` set the command to `["--",
            // "true"]`. Measured against util-linux: `unshare -- echo hello`
            // prints `hello`, so the separator is consumed there.
            //
            // Today that only mis-names the program in the refusal
            // ("refusing to run --"), which is how it was noticed. It becomes
            // an attempt to execute `--` the day a namespace subsystem lands.
            "--" => {
                opts.command = args.get(i.saturating_add(1)..).unwrap_or_default().to_vec();
                break;
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
    println!("This system has no namespace subsystem, so unshare cannot create");
    println!("one and will not run the program. Options are parsed and checked.");
    println!();
    println!("Run a program with some namespaces unshared from parent.");
    println!();
    println!("Options:");
    for ns in NS_TYPES {
        println!(
            "  {}, {:16}  Unshare {}",
            ns.short_flag, ns.long_flag, ns.description
        );
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

    // ------------------------------------------------------------------ //
    // REFUSING, for the same reason `nsenter` does and with one difference
    // that makes this worse.
    //
    // This program never called `unshare(2)`. It printed a line claiming to
    // have mapped the user into a new user namespace -- in the PRESENT TENSE,
    // "unshare: mapping current user to root in user namespace" -- and then
    // ran the command with no namespaces created at all. `nsenter` at least
    // said nothing; this one asserted the thing it had not done.
    //
    // Running unisolated is the harm, not a lesser version of it. `unshare` is
    // reached for precisely when an operation is risky enough to want
    // containing: `unshare -m -- <mount juggling>` expects its mounts to be
    // private, and without a new mount namespace they are the host's. The
    // command runs, it succeeds, and it changes the wrong system.
    //
    // There is nothing to wire it to, and that was checked rather than
    // assumed: `posix::unshare` validates its flag set and its CAP_SYS_ADMIN
    // gate and then returns ENOSYS, "the namespace subsystem isn't wired up".
    // (Its `unshare(0) -> 0` is not an exception -- Linux defines that as a
    // successful no-op and util-linux uses it to probe for the syscall.)
    //
    // This refusal ends the day a namespace subsystem lands.
    let stderr = io::stderr();
    let mut err = stderr.lock();

    let command = if opts.command.is_empty() {
        vec![env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())]
    } else {
        opts.command.clone()
    };
    let what = command.first().map_or("a command", String::as_str);

    let _ = writeln!(
        err,
        "unshare: cannot create new {} namespace(s): this system has no \
namespace subsystem.",
        ns_names.join(", ")
    );
    let _ = writeln!(
        err,
        "unshare: unshare(2) validates its arguments and returns ENOSYS."
    );
    if !opts.requested.is_empty() {
        // Named so the refusal accounts for everything asked for, not just
        // the namespaces. A caller who passed `--keep-caps` should be able to
        // see that it was read and is going nowhere, rather than wonder.
        let _ = writeln!(
            err,
            "unshare: also requested, and equally not honoured: {}",
            opts.requested.join(", ")
        );
    }
    let _ = writeln!(
        err,
        "unshare: refusing to run {what}, because running it UNISOLATED is not \
what was asked for."
    );
    let _ = err.flush();
    process::exit(1);
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
    /// `--` ends the options; it is not the program.
    ///
    /// Without its own arm it fell through to "everything from here is the
    /// command", so `unshare -m -- true` set the command to `["--", "true"]`
    /// and the refusal said "refusing to run --". Measured against
    /// util-linux, where `unshare -- echo hello` prints `hello`.
    #[test]
    fn double_dash_ends_the_options() {
        let args = vec!["-m".to_string(), "--".to_string(), "true".to_string()];
        let opts = parse_args(&args);
        assert_eq!(opts.command, vec!["true".to_string()]);

        // With arguments of its own, including ones that look like options.
        let args = vec!["--".to_string(), "ls".to_string(), "-l".to_string()];
        let opts = parse_args(&args);
        assert_eq!(opts.command, vec!["ls".to_string(), "-l".to_string()]);

        // A trailing `--` leaves no command, rather than a command of `--`.
        let opts = parse_args(&["-m".to_string(), "--".to_string()]);
        assert!(opts.command.is_empty());
    }

    /// Options that cannot be honoured are recorded, and recorded once.
    ///
    /// The fields these replaced were written and never read, and the tests
    /// asserted only that they had been STORED -- which is what a test looks
    /// like when nothing consumes the value.
    #[test]
    fn requested_options_are_recorded_for_the_refusal() {
        let args = vec![
            "-m".to_string(),
            "--keep-caps".to_string(),
            "--setuid".to_string(),
            "0".to_string(),
            "--keep-caps".to_string(),
        ];
        let opts = parse_args(&args);
        assert_eq!(opts.requested, vec!["--keep-caps", "--setuid"]);

        // A plain invocation records nothing, so the refusal stays short.
        let opts = parse_args(&["-m".to_string()]);
        assert!(opts.requested.is_empty());
    }

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
            assert!(
                ns.clone_flag.count_ones() == 1,
                "Flag for {} is not a single bit: {:#x}",
                ns.name,
                ns.clone_flag
            );
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
        assert!(opts.requested.contains(&"--fork"));
    }

    #[test]
    fn test_parse_map_root_user() {
        let args = vec!["-r".to_string()];
        let opts = parse_args(&args);
        assert!(opts.requested.contains(&"--map-root-user"));
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
        assert!(opts.requested.contains(&"--setuid"));
        assert!(opts.requested.contains(&"--setgid"));
    }

    #[test]
    fn test_parse_root_dir() {
        let args = vec!["-m".to_string(), "-R".to_string(), "/newroot".to_string()];
        let opts = parse_args(&args);
        assert!(opts.requested.contains(&"--root"));
    }

    #[test]
    fn test_parse_wd() {
        let args = vec!["-m".to_string(), "-w".to_string(), "/tmp".to_string()];
        let opts = parse_args(&args);
        assert!(opts.requested.contains(&"--wd"));
    }

    #[test]
    fn test_parse_keep_caps() {
        let args = vec!["--user".to_string(), "--keep-caps".to_string()];
        let opts = parse_args(&args);
        assert!(opts.requested.contains(&"--keep-caps"));
    }

    #[test]
    fn test_parse_kill_child() {
        let args = vec!["-m".to_string(), "--kill-child".to_string()];
        let opts = parse_args(&args);
        assert!(opts.requested.contains(&"--kill-child"));
    }

    #[test]
    fn test_parse_kill_child_signal() {
        let args = vec!["-m".to_string(), "--kill-child=15".to_string()];
        let opts = parse_args(&args);
        assert!(opts.requested.contains(&"--kill-child"));
    }

    #[test]
    fn test_parse_propagation() {
        let args = vec!["-m".to_string(), "--propagation=shared".to_string()];
        let opts = parse_args(&args);
        assert!(opts.requested.contains(&"--propagation"));
    }

    #[test]
    fn test_parse_monotonic() {
        let args = vec![
            "--time".to_string(),
            "--monotonic".to_string(),
            "100".to_string(),
        ];
        let opts = parse_args(&args);
        assert!(opts.requested.contains(&"--monotonic"));
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
        assert!(opts.requested.contains(&"--map-auto"));
        assert!(opts.namespaces & CLONE_NEWUSER != 0);
    }

    #[test]
    fn test_map_users() {
        let args = vec!["--map-users".to_string(), "0:1000:1".to_string()];
        let opts = parse_args(&args);
        assert!(opts.requested.contains(&"--map-users"));
        assert!(opts.namespaces & CLONE_NEWUSER != 0);
    }
}
