//! Slate OS cgroup management utility.
//!
//! Multi-personality binary providing:
//! - **cgcreate** — create cgroups
//! - **cgdelete** — delete cgroups
//! - **cgexec** — run a command in a cgroup
//! - **cgset** — set cgroup parameters
//! - **cgget** — get cgroup parameters
//! - **cgclassify** — move processes to a cgroup
//! - **lscgroup** — list cgroups
//! - **lssubsys** — list cgroup subsystems
//!
//! Manages Linux/SlateOS cgroups v2 hierarchy for resource control.

#![deny(clippy::all)]

use quoting::quotef_os;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;

const VERSION: &str = "0.1.0";
const CGROUP_ROOT: &str = "/sys/fs/cgroup";
/// Where the kernel lists its controllers.
const PROC_CGROUPS: &str = "/proc/cgroups";

// ============================================================================
// Data structures
// ============================================================================

#[derive(Clone, Debug)]
struct CgroupInfo {
    path: PathBuf,
    controllers: Vec<String>,
    _frozen: bool,
}

#[derive(Clone, Debug)]
struct SubsysInfo {
    name: String,
    _hierarchy: u32,
    // Reported by /proc/cgroups; consumed by the future `cgroup list`
    // verbose path that includes per-controller cgroup counts.
    #[allow(dead_code)]
    num_cgroups: u32,
    enabled: bool,
}

#[derive(Clone, Debug)]
struct _CgroupParam {
    name: String,
    value: String,
}

// ============================================================================
// Cgroup filesystem operations
// ============================================================================

fn cgroup_path(group: &str) -> PathBuf {
    let mut p = PathBuf::from(CGROUP_ROOT);
    if !group.is_empty() && group != "/" {
        p.push(group.trim_start_matches('/'));
    }
    p
}

fn read_cgroup_param(group: &str, param: &str) -> Option<String> {
    let path = cgroup_path(group).join(param);
    fs::read_to_string(&path).ok().map(|s| s.trim().to_string())
}

fn write_cgroup_param(group: &str, param: &str, value: &str) -> Result<(), String> {
    let path = cgroup_path(group).join(param);
    fs::write(&path, value).map_err(|e| format!("Failed to write {}: {}", path.display(), e))
}

// Consumed by the same verbose `cgroup list` path that uses num_cgroups
// — both are wired to the cgroup-v2 controllers file but the listing
// command isn't yet hooked into the CLI dispatch.
#[allow(dead_code)]
fn list_controllers() -> Vec<String> {
    let path = Path::new(CGROUP_ROOT).join("cgroup.controllers");
    if let Ok(data) = fs::read_to_string(&path) {
        data.split_whitespace().map(|s| s.to_string()).collect()
    } else {
        // Default controllers for non-cgroup systems.
        vec![
            "cpu".to_string(),
            "cpuset".to_string(),
            "io".to_string(),
            "memory".to_string(),
            "pids".to_string(),
            "rdma".to_string(),
            "misc".to_string(),
        ]
    }
}

fn list_cgroups_recursive(base: &Path, prefix: &str, result: &mut Vec<CgroupInfo>) {
    let dir = if prefix.is_empty() {
        base.to_path_buf()
    } else {
        base.join(prefix)
    };

    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if let Ok(ft) = entry.file_type()
                && ft.is_dir()
            {
                let name = entry.file_name().to_string_lossy().to_string();
                let child_prefix = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}/{name}")
                };

                // Read controllers for this cgroup.
                let ctrl_path = entry.path().join("cgroup.controllers");
                let controllers = if let Ok(data) = fs::read_to_string(&ctrl_path) {
                    data.split_whitespace().map(|s| s.to_string()).collect()
                } else {
                    Vec::new()
                };

                result.push(CgroupInfo {
                    path: PathBuf::from(&child_prefix),
                    controllers,
                    _frozen: false,
                });

                list_cgroups_recursive(base, &child_prefix, result);
            }
        }
    }
}

// ============================================================================
// cgcreate
// ============================================================================

fn cmd_cgcreate(args: &[String]) {
    let mut controllers: Vec<String> = Vec::new();
    let mut groups: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: cgcreate -g <controllers>:<path> [...]");
                println!();
                println!("Create cgroups.");
                println!();
                println!("Options:");
                println!("  -g CTRL:PATH   Controllers and cgroup path");
                println!("  -h, --help     Show help");
                println!("  -V, --version  Show version");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("cgcreate {VERSION}");
                process::exit(0);
            }
            "-g" => {
                i += 1;
                if i < args.len() {
                    let spec = &args[i];
                    if let Some((ctrl, path)) = spec.split_once(':') {
                        for c in ctrl.split(',') {
                            if !c.is_empty() && !controllers.contains(&c.to_string()) {
                                controllers.push(c.to_string());
                            }
                        }
                        groups.push(path.to_string());
                    } else {
                        groups.push(spec.to_string());
                    }
                }
            }
            s if !s.starts_with('-') => {
                groups.push(s.to_string());
            }
            _ => {
                eprintln!("cgcreate: unknown option: {}", args[i]);
            }
        }
        i += 1;
    }

    if groups.is_empty() {
        eprintln!("cgcreate: no cgroup path specified");
        process::exit(1);
    }

    for group in &groups {
        let path = cgroup_path(group);
        match fs::create_dir_all(&path) {
            Ok(()) => {
                eprintln!("cgcreate: created {}", path.display());
                // Enable controllers if specified.
                if !controllers.is_empty() {
                    let ctrl_str = controllers
                        .iter()
                        .map(|c| format!("+{c}"))
                        .collect::<Vec<_>>()
                        .join(" ");
                    // Write to parent's subtree_control.
                    if let Some(parent) = path.parent() {
                        let ctrl_file = parent.join("cgroup.subtree_control");
                        let _ = fs::write(&ctrl_file, &ctrl_str);
                    }
                }
            }
            Err(e) => {
                eprintln!("cgcreate: failed to create {}: {e}", path.display());
            }
        }
    }
}

// ============================================================================
// cgdelete
// ============================================================================

fn cmd_cgdelete(args: &[String]) {
    let mut groups: Vec<String> = Vec::new();
    let mut recursive = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: cgdelete [-r] <path> [...]");
                println!();
                println!("Delete cgroups.");
                println!();
                println!("Options:");
                println!("  -r             Recursive delete");
                println!("  -g CTRL:PATH   Controllers and cgroup path");
                println!("  -h, --help     Show help");
                println!("  -V, --version  Show version");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("cgdelete {VERSION}");
                process::exit(0);
            }
            "-r" => recursive = true,
            "-g" => {
                i += 1;
                if i < args.len() {
                    let spec = &args[i];
                    if let Some((_ctrl, path)) = spec.split_once(':') {
                        groups.push(path.to_string());
                    } else {
                        groups.push(spec.to_string());
                    }
                }
            }
            s if !s.starts_with('-') => {
                groups.push(s.to_string());
            }
            _ => {
                eprintln!("cgdelete: unknown option: {}", args[i]);
            }
        }
        i += 1;
    }

    if groups.is_empty() {
        eprintln!("cgdelete: no cgroup path specified");
        process::exit(1);
    }

    for group in &groups {
        let path = cgroup_path(group);
        let result = if recursive {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_dir(&path)
        };
        match result {
            Ok(()) => eprintln!("cgdelete: removed {}", path.display()),
            Err(e) => eprintln!("cgdelete: failed to remove {}: {e}", path.display()),
        }
    }
}

// ============================================================================
// cgexec
// ============================================================================

fn cmd_cgexec(args: &[String]) {
    let mut group: Option<String> = None;
    let mut cmd_args: Vec<String> = Vec::new();
    let mut sticky = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: cgexec -g <controllers>:<path> <command> [args...]");
                println!();
                println!("Run a command in a cgroup.");
                println!();
                println!("Options:");
                println!("  -g CTRL:PATH   Controllers and cgroup path");
                println!("  --sticky       Keep process in cgroup on exec");
                println!("  -h, --help     Show help");
                println!("  -V, --version  Show version");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("cgexec {VERSION}");
                process::exit(0);
            }
            "--sticky" => sticky = true,
            "-g" => {
                i += 1;
                if i < args.len() {
                    let spec = &args[i];
                    if let Some((_ctrl, path)) = spec.split_once(':') {
                        group = Some(path.to_string());
                    } else {
                        group = Some(spec.to_string());
                    }
                }
            }
            _ => {
                // Everything from here is the command.
                cmd_args = args[i..].to_vec();
                break;
            }
        }
        i += 1;
    }

    let group = match group {
        Some(g) => g,
        None => {
            eprintln!("cgexec: no cgroup specified (-g)");
            process::exit(1);
        }
    };

    if cmd_args.is_empty() {
        eprintln!("cgexec: no command specified");
        process::exit(1);
    }

    // Join the cgroup BEFORE running anything, and treat a failure as fatal.
    //
    // This used to warn and continue, under a comment reading "Continue anyway
    // -- on non-cgroup systems, simulate execution." That is the wrong shape
    // for this particular command: the caller did not ask for the command to
    // run, they asked for it to run *under these limits*. Running it without
    // them is not a weaker version of the request, it is a different and
    // unconstrained one -- and the caller who wrote `cgexec -g memory:capped`
    // is precisely the one who cannot afford that.
    let procs_path = cgroup_path(&group).join("cgroup.procs");
    let pid = process::id();
    if let Err(e) = fs::write(&procs_path, pid.to_string()) {
        eprintln!("cgexec: cannot join cgroup {group}: {e}");
        eprintln!("cgexec: refusing to run the command outside the cgroup it asked for");
        process::exit(1);
    }

    if sticky {
        // `--sticky` asks that membership survive the exec below. Under
        // cgroups v2 that is already the case -- membership is a property of
        // the process and is not reset by execve -- so the flag is accepted
        // and is a no-op. Saying so beats accepting it silently, which would
        // leave a caller unable to tell it had been honoured.
        eprintln!("cgexec: --sticky is the default under cgroups v2; membership survives exec");
    }

    // Now become the command. `exec` rather than spawn-and-wait so the command
    // inherits this process's cgroup membership directly and the caller's
    // shell sees the command's own exit status and signal disposition rather
    // than a copy relayed through us.
    let Some((prog, rest)) = cmd_args.split_first() else {
        eprintln!("cgexec: no command specified");
        process::exit(1);
    };
    let mut cmd = process::Command::new(prog);
    cmd.args(rest);

    // Exit codes follow the shell convention that callers already expect:
    // 127 for a command that is not there, 126 for one that is but cannot be
    // run. Reaching either line means the exec failed, because a successful
    // exec does not return.
    #[cfg(unix)]
    let err = {
        use std::os::unix::process::CommandExt as _;
        cmd.exec()
    };
    #[cfg(not(unix))]
    let err = match cmd.status() {
        // The development host has no `exec`. Relaying the status is not
        // identical -- a signalled child cannot be reported faithfully this
        // way -- but it is the honest approximation, and this arm does not
        // ship.
        Ok(st) => process::exit(st.code().unwrap_or(1)),
        Err(e) => e,
    };
    eprintln!("cgexec: {}: {err}", quotef_os(prog));
    process::exit(if err.kind() == io::ErrorKind::NotFound {
        127
    } else {
        126
    });
}

// ============================================================================
// cgset
// ============================================================================

fn cmd_cgset(args: &[String]) {
    let mut params: Vec<(String, String)> = Vec::new();
    let mut groups: Vec<String> = Vec::new();
    let mut copy_from: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: cgset -r <param>=<value> <path> [...]");
                println!();
                println!("Set cgroup parameters.");
                println!();
                println!("Options:");
                println!("  -r PARAM=VALUE Set parameter value");
                println!("  --copy-from G  Copy settings from another cgroup");
                println!("  -h, --help     Show help");
                println!("  -V, --version  Show version");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("cgset {VERSION}");
                process::exit(0);
            }
            "-r" => {
                i += 1;
                if i < args.len() {
                    if let Some((name, value)) = args[i].split_once('=') {
                        params.push((name.to_string(), value.to_string()));
                    } else {
                        eprintln!("cgset: invalid parameter format: {}", args[i]);
                    }
                }
            }
            "--copy-from" => {
                i += 1;
                if i < args.len() {
                    copy_from = Some(args[i].clone());
                }
            }
            s if !s.starts_with('-') => {
                groups.push(s.to_string());
            }
            _ => {
                eprintln!("cgset: unknown option: {}", args[i]);
            }
        }
        i += 1;
    }

    if groups.is_empty() {
        eprintln!("cgset: no cgroup path specified");
        process::exit(1);
    }

    // Copy params from source cgroup if specified.
    if let Some(ref src) = copy_from {
        let src_params = vec![
            "cpu.weight",
            "cpu.max",
            "memory.max",
            "memory.high",
            "io.max",
            "pids.max",
        ];
        for param_name in src_params {
            if let Some(val) = read_cgroup_param(src, param_name) {
                params.push((param_name.to_string(), val));
            }
        }
    }

    if params.is_empty() {
        eprintln!("cgset: no parameters specified");
        process::exit(1);
    }

    for group in &groups {
        for (name, value) in &params {
            match write_cgroup_param(group, name, value) {
                Ok(()) => eprintln!("cgset: {}: {name} = {value}", quotef_os(group)),
                Err(e) => eprintln!("cgset: {e}"),
            }
        }
    }
}

// ============================================================================
// cgget
// ============================================================================

fn cmd_cgget(args: &[String]) {
    let mut params: Vec<String> = Vec::new();
    let mut groups: Vec<String> = Vec::new();
    let mut all = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: cgget [-r <param>] [-a] <path> [...]");
                println!();
                println!("Get cgroup parameters.");
                println!();
                println!("Options:");
                println!("  -r PARAM       Parameter to read");
                println!("  -a, --all      Show all parameters");
                println!("  -h, --help     Show help");
                println!("  -V, --version  Show version");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("cgget {VERSION}");
                process::exit(0);
            }
            "-a" | "--all" => all = true,
            "-r" => {
                i += 1;
                if i < args.len() {
                    params.push(args[i].clone());
                }
            }
            s if !s.starts_with('-') => {
                groups.push(s.to_string());
            }
            _ => {
                eprintln!("cgget: unknown option: {}", args[i]);
            }
        }
        i += 1;
    }

    if groups.is_empty() {
        eprintln!("cgget: no cgroup path specified");
        process::exit(1);
    }

    if all {
        params = vec![
            "cgroup.type".to_string(),
            "cgroup.controllers".to_string(),
            "cgroup.subtree_control".to_string(),
            "cgroup.events".to_string(),
            "cgroup.freeze".to_string(),
            "cpu.weight".to_string(),
            "cpu.max".to_string(),
            "cpu.stat".to_string(),
            "memory.current".to_string(),
            "memory.max".to_string(),
            "memory.high".to_string(),
            "memory.min".to_string(),
            "memory.low".to_string(),
            "memory.stat".to_string(),
            "io.max".to_string(),
            "io.stat".to_string(),
            "pids.current".to_string(),
            "pids.max".to_string(),
        ];
    }

    if params.is_empty() {
        // Default: show common params.
        params = vec![
            "cpu.weight".to_string(),
            "cpu.max".to_string(),
            "memory.max".to_string(),
            "memory.current".to_string(),
            "pids.max".to_string(),
            "pids.current".to_string(),
        ];
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for group in &groups {
        let _ = writeln!(out, "{group}:");
        for param in &params {
            if let Some(value) = read_cgroup_param(group, param) {
                let _ = writeln!(out, "{param}: {value}");
            } else {
                // Show default values for common parameters.
                let default = default_param_value(param);
                let _ = writeln!(out, "{param}: {default}");
            }
        }
        let _ = writeln!(out);
    }
}

fn default_param_value(param: &str) -> &'static str {
    match param {
        "cgroup.type" => "domain",
        "cgroup.controllers" => "cpu cpuset io memory pids",
        "cgroup.subtree_control" => "",
        "cgroup.events" => "populated 0\nfrozen 0",
        "cgroup.freeze" => "0",
        "cpu.weight" => "100",
        "cpu.max" => "max 100000",
        "cpu.stat" => "usage_usec 0\nuser_usec 0\nsystem_usec 0",
        "memory.current" => "0",
        "memory.max" => "max",
        "memory.high" => "max",
        "memory.min" => "0",
        "memory.low" => "0",
        "memory.stat" => "anon 0\nfile 0\nkernel 0",
        "io.max" => "",
        "io.stat" => "",
        "pids.current" => "0",
        "pids.max" => "max",
        _ => "(unknown)",
    }
}

// ============================================================================
// cgclassify
// ============================================================================

fn cmd_cgclassify(args: &[String]) {
    let mut group: Option<String> = None;
    let mut pids: Vec<u32> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: cgclassify -g <controllers>:<path> <pid> [pid ...]");
                println!();
                println!("Move processes to a cgroup.");
                println!();
                println!("Options:");
                println!("  -g CTRL:PATH   Controllers and cgroup path");
                println!("  -h, --help     Show help");
                println!("  -V, --version  Show version");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("cgclassify {VERSION}");
                process::exit(0);
            }
            "-g" => {
                i += 1;
                if i < args.len() {
                    let spec = &args[i];
                    if let Some((_ctrl, path)) = spec.split_once(':') {
                        group = Some(path.to_string());
                    } else {
                        group = Some(spec.to_string());
                    }
                }
            }
            s if !s.starts_with('-') => {
                if let Ok(pid) = s.parse::<u32>() {
                    pids.push(pid);
                }
            }
            _ => {
                eprintln!("cgclassify: unknown option: {}", args[i]);
            }
        }
        i += 1;
    }

    let group = match group {
        Some(g) => g,
        None => {
            eprintln!("cgclassify: no cgroup specified (-g)");
            process::exit(1);
        }
    };

    if pids.is_empty() {
        eprintln!("cgclassify: no PIDs specified");
        process::exit(1);
    }

    let procs_path = cgroup_path(&group).join("cgroup.procs");
    for pid in &pids {
        match fs::write(&procs_path, pid.to_string()) {
            Ok(()) => eprintln!("cgclassify: moved PID {pid} to {group}"),
            Err(e) => eprintln!("cgclassify: failed to move PID {pid} to {group}: {e}"),
        }
    }
}

// ============================================================================
// lscgroup
// ============================================================================

fn cmd_lscgroup(args: &[String]) {
    let mut filter_ctrl: Option<String> = None;

    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("Usage: lscgroup [controller:path]");
                println!();
                println!("List cgroups.");
                println!();
                println!("Options:");
                println!("  -h, --help     Show help");
                println!("  -V, --version  Show version");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("lscgroup {VERSION}");
                process::exit(0);
            }
            s if !s.starts_with('-') => {
                filter_ctrl = Some(s.to_string());
            }
            _ => {}
        }
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    let mut cgroups = Vec::new();
    let root = Path::new(CGROUP_ROOT);

    // NO HIERARCHY MEANS NO CGROUPS, AND SAYING SO.
    //
    // This substituted `generate_default_cgroups()`: `system.slice`,
    // `user.slice` and `init.scope`, each with a controller list. Those are
    // systemd's names, SlateOS does not run systemd, and the machine this ran
    // on had no cgroup hierarchy at all -- so `lscgroup` printed three groups
    // that have never existed anywhere on the system, and a script asking
    // whether a group is present got yes.
    //
    // The two cases are told apart because they need different words: a system
    // with no cgroup filesystem cannot have cgroups, and one with an empty
    // hierarchy has none. Neither is "here are three".
    if !root.is_dir() {
        eprintln!(
            "lscgroup: {CGROUP_ROOT} does not exist -- cgroups are not available on this system"
        );
        process::exit(1);
    }
    list_cgroups_recursive(root, "", &mut cgroups);

    // Filter by controller if specified.
    let filter_controller = filter_ctrl
        .as_ref()
        .and_then(|s| s.split_once(':').map(|(c, _)| c.to_string()));

    for cg in &cgroups {
        if let Some(ref ctrl) = filter_controller
            && !cg.controllers.contains(ctrl)
        {
            continue;
        }

        let controllers = if cg.controllers.is_empty() {
            String::new()
        } else {
            format!("{}:", cg.controllers.join(","))
        };
        let _ = writeln!(out, "{}{}", controllers, cg.path.display());
    }
}

// ============================================================================
// lssubsys
// ============================================================================

fn cmd_lssubsys(args: &[String]) {
    let mut show_mount = false;
    let mut show_all = false;

    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("Usage: lssubsys [options]");
                println!();
                println!("List cgroup subsystems.");
                println!();
                println!("Options:");
                println!("  -m, --mount-points  Show mount points");
                println!("  -a, --all           Show all subsystems");
                println!("  -h, --help          Show help");
                println!("  -V, --version       Show version");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("lssubsys {VERSION}");
                process::exit(0);
            }
            "-m" | "--mount-points" => show_mount = true,
            "-a" | "--all" => show_all = true,
            _ => {}
        }
    }

    let subsystems = match read_subsystems() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("lssubsys: {e}");
            process::exit(1);
        }
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for ss in &subsystems {
        if !show_all && !ss.enabled {
            continue;
        }

        if show_mount {
            let _ = writeln!(out, "{}\t{}/{}", ss.name, CGROUP_ROOT, ss.name);
        } else {
            let status = if ss.enabled { "" } else { " (disabled)" };
            let _ = writeln!(out, "{}{status}", ss.name);
        }
    }
}

/// The controllers the kernel reports, or `Err` saying why there are none.
///
/// # What this replaced
///
/// An unreadable `/proc/cgroups` returned `generate_default_subsystems()`:
/// eight controllers -- cpu, cpuset, io, memory, pids, rdma, hugetlb, misc --
/// six of them flagged enabled, each claiming one cgroup. None of it was read
/// from anywhere. `lssubsys` answers "what resource controls does this kernel
/// have", and on a machine with no cgroup support at all it answered with a
/// plausible Linux box.
///
/// An EMPTY list is a different answer and still a real one: the file exists,
/// it lists nothing, and the caller prints nothing. Only an unreadable file is
/// `Err`, because that is the case where we do not know.
fn read_subsystems() -> Result<Vec<SubsysInfo>, String> {
    let data =
        fs::read_to_string(PROC_CGROUPS).map_err(|e| format!("cannot read {PROC_CGROUPS}: {e}"))?;
    let mut result = Vec::new();
    for line in data.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 4 {
            let name = parts[0].to_string();
            let _hierarchy = parts[1].parse().unwrap_or(0);
            let num_cgroups = parts[2].parse().unwrap_or(0);
            let enabled = parts[3] == "1";
            result.push(SubsysInfo {
                name,
                _hierarchy,
                num_cgroups,
                enabled,
            });
        }
    }
    Ok(result)
}

// ============================================================================
// CLI
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("lscgroup");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    match prog_name.as_str() {
        "cgcreate" => cmd_cgcreate(&rest),
        "cgdelete" => cmd_cgdelete(&rest),
        "cgexec" => cmd_cgexec(&rest),
        "cgset" => cmd_cgset(&rest),
        "cgget" => cmd_cgget(&rest),
        "cgclassify" => cmd_cgclassify(&rest),
        "lssubsys" => cmd_lssubsys(&rest),
        _ => cmd_lscgroup(&rest),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cgroup_path_root() {
        let p = cgroup_path("/");
        assert_eq!(p, PathBuf::from(CGROUP_ROOT));
    }

    #[test]
    fn test_cgroup_path_empty() {
        let p = cgroup_path("");
        assert_eq!(p, PathBuf::from(CGROUP_ROOT));
    }

    #[test]
    fn test_cgroup_path_nested() {
        let p = cgroup_path("system.slice/ssh.service");
        assert_eq!(
            p,
            PathBuf::from(format!("{CGROUP_ROOT}/system.slice/ssh.service"))
        );
    }

    #[test]
    fn test_cgroup_path_leading_slash() {
        let p = cgroup_path("/system.slice");
        assert_eq!(p, PathBuf::from(format!("{CGROUP_ROOT}/system.slice")));
    }

    #[test]
    fn test_list_controllers() {
        let controllers = list_controllers();
        // Should return defaults on non-cgroup systems.
        assert!(!controllers.is_empty());
    }

    #[test]
    fn test_default_param_value() {
        assert_eq!(default_param_value("cpu.weight"), "100");
        assert_eq!(default_param_value("cpu.max"), "max 100000");
        assert_eq!(default_param_value("memory.max"), "max");
        assert_eq!(default_param_value("pids.max"), "max");
        assert_eq!(default_param_value("unknown.param"), "(unknown)");
    }

    #[test]
    fn test_cgroup_info_clone() {
        let info = CgroupInfo {
            path: PathBuf::from("test.slice"),
            controllers: vec!["cpu".to_string()],
            _frozen: false,
        };
        let c = info.clone();
        assert_eq!(c.path, PathBuf::from("test.slice"));
        assert_eq!(c.controllers, vec!["cpu"]);
    }

    #[test]
    fn test_subsys_info_clone() {
        let info = SubsysInfo {
            name: "cpu".to_string(),
            _hierarchy: 0,
            num_cgroups: 5,
            enabled: true,
        };
        let c = info.clone();
        assert_eq!(c.name, "cpu");
        assert_eq!(c.num_cgroups, 5);
        assert!(c.enabled);
    }

    #[test]
    fn test_cgroup_param_clone() {
        let p = _CgroupParam {
            name: "cpu.weight".to_string(),
            value: "100".to_string(),
        };
        let c = p.clone();
        assert_eq!(c.name, "cpu.weight");
        assert_eq!(c.value, "100");
    }

    #[test]
    fn test_read_cgroup_param_missing() {
        assert!(read_cgroup_param("nonexistent", "cpu.weight").is_none());
    }

    #[test]
    fn test_write_cgroup_param_missing() {
        let result = write_cgroup_param("nonexistent", "cpu.weight", "200");
        assert!(result.is_err());
    }

    #[test]
    fn test_read_subsystems() {
        // EITHER ANSWER IS CORRECT AND THE POINT IS THAT THEY DIFFER. On a
        // host with /proc/cgroups this reads it; on one without -- this test
        // runner, among others -- it must say it could not find out rather
        // than hand back a controller list nobody read. It used to assert
        // `!is_empty()`, which passed only because the failure path invented
        // eight controllers, so the test certified the fabrication.
        match read_subsystems() {
            Ok(list) => assert!(list.iter().all(|s| !s.name.is_empty())),
            Err(e) => assert!(e.contains(PROC_CGROUPS), "unexpected error: {e}"),
        }
    }

    #[test]
    fn test_cgroup_path_multiple_levels() {
        let p = cgroup_path("a/b/c/d");
        let expected = format!("{CGROUP_ROOT}/a/b/c/d");
        assert_eq!(p, PathBuf::from(expected));
    }
}
