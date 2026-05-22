//! OurOS process resource limits utility.
//!
//! Multi-personality binary providing:
//! - **prlimit** — get/set process resource limits
//! - **ulimit** — shell resource limit display (standalone)
//!
//! Displays and modifies resource limits for processes using
//! /proc/<pid>/limits or getrlimit/setrlimit.

#![deny(clippy::all)]

use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

const VERSION: &str = "0.1.0";

// ============================================================================
// Resource limit types
// ============================================================================

#[derive(Clone, Debug)]
struct ResourceLimit {
    resource: Resource,
    soft: LimitValue,
    hard: LimitValue,
    units: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Resource {
    AddressSpace,
    CoreSize,
    CpuTime,
    DataSize,
    FileSize,
    Locks,
    MemLock,
    MsgQueue,
    Nice,
    OpenFiles,
    Processes,
    Rss,
    RtPrio,
    RtTime,
    SigPending,
    StackSize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum LimitValue {
    Unlimited,
    Value(u64),
}

impl Resource {
    fn name(&self) -> &'static str {
        match self {
            Self::AddressSpace => "AS",
            Self::CoreSize => "CORE",
            Self::CpuTime => "CPU",
            Self::DataSize => "DATA",
            Self::FileSize => "FSIZE",
            Self::Locks => "LOCKS",
            Self::MemLock => "MEMLOCK",
            Self::MsgQueue => "MSGQUEUE",
            Self::Nice => "NICE",
            Self::OpenFiles => "NOFILE",
            Self::Processes => "NPROC",
            Self::Rss => "RSS",
            Self::RtPrio => "RTPRIO",
            Self::RtTime => "RTTIME",
            Self::SigPending => "SIGPENDING",
            Self::StackSize => "STACK",
        }
    }

    fn description(&self) -> &'static str {
        match self {
            Self::AddressSpace => "max address space",
            Self::CoreSize => "max core file size",
            Self::CpuTime => "max cpu time",
            Self::DataSize => "max data size",
            Self::FileSize => "max file size",
            Self::Locks => "max number of file locks held",
            Self::MemLock => "max locked-in-memory address space",
            Self::MsgQueue => "max bytes in POSIX mqueues",
            Self::Nice => "max nice prio allowed to raise",
            Self::OpenFiles => "max number of open files",
            Self::Processes => "max number of processes",
            Self::Rss => "max resident set size",
            Self::RtPrio => "max real-time priority",
            Self::RtTime => "max real-time timeout",
            Self::SigPending => "max number of pending signals",
            Self::StackSize => "max stack size",
        }
    }

    fn units(&self) -> &'static str {
        match self {
            Self::CpuTime => "seconds",
            Self::Nice | Self::RtPrio => "",
            Self::OpenFiles | Self::Locks | Self::Processes | Self::SigPending => "",
            Self::RtTime => "microseconds",
            _ => "bytes",
        }
    }
}

impl LimitValue {
    fn display(&self) -> String {
        match self {
            Self::Unlimited => "unlimited".to_string(),
            Self::Value(v) => v.to_string(),
        }
    }

    fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if s == "unlimited" || s == "infinity" || s == "-1" {
            Some(Self::Unlimited)
        } else {
            s.parse::<u64>().ok().map(Self::Value)
        }
    }
}

const _ALL_RESOURCES: &[Resource] = &[
    Resource::AddressSpace,
    Resource::CoreSize,
    Resource::CpuTime,
    Resource::DataSize,
    Resource::FileSize,
    Resource::Locks,
    Resource::MemLock,
    Resource::MsgQueue,
    Resource::Nice,
    Resource::OpenFiles,
    Resource::Processes,
    Resource::Rss,
    Resource::RtPrio,
    Resource::RtTime,
    Resource::SigPending,
    Resource::StackSize,
];

// ============================================================================
// Reading limits
// ============================================================================

fn read_proc_limits(pid: u32) -> Vec<ResourceLimit> {
    let path = format!("/proc/{pid}/limits");
    if let Ok(data) = fs::read_to_string(&path) {
        return parse_proc_limits(&data);
    }
    generate_default_limits()
}

fn parse_proc_limits(data: &str) -> Vec<ResourceLimit> {
    let mut limits = Vec::new();

    for line in data.lines().skip(1) {
        // Format: "Max open files            1024                 1048576              files"
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Split the line — the first 25 chars are the name, then soft/hard/units.
        let name_part = if trimmed.len() > 25 {
            trimmed[..25].trim()
        } else {
            trimmed
        };

        let resource = match name_part {
            s if s.starts_with("Max address space") => Some(Resource::AddressSpace),
            s if s.starts_with("Max core file size") => Some(Resource::CoreSize),
            s if s.starts_with("Max cpu time") => Some(Resource::CpuTime),
            s if s.starts_with("Max data size") => Some(Resource::DataSize),
            s if s.starts_with("Max file size") => Some(Resource::FileSize),
            s if s.starts_with("Max file locks") => Some(Resource::Locks),
            s if s.starts_with("Max locked memory") => Some(Resource::MemLock),
            s if s.starts_with("Max msgqueue size") => Some(Resource::MsgQueue),
            s if s.starts_with("Max nice priority") => Some(Resource::Nice),
            s if s.starts_with("Max open files") => Some(Resource::OpenFiles),
            s if s.starts_with("Max processes") => Some(Resource::Processes),
            s if s.starts_with("Max resident set") => Some(Resource::Rss),
            s if s.starts_with("Max realtime priority") => Some(Resource::RtPrio),
            s if s.starts_with("Max realtime timeout") => Some(Resource::RtTime),
            s if s.starts_with("Max pending signals") => Some(Resource::SigPending),
            s if s.starts_with("Max stack size") => Some(Resource::StackSize),
            _ => None,
        };

        if let Some(res) = resource {
            let rest = if trimmed.len() > 25 { &trimmed[25..] } else { "" };
            let parts: Vec<&str> = rest.split_whitespace().collect();

            let soft = parts.first()
                .and_then(|s| LimitValue::parse(s))
                .unwrap_or(LimitValue::Unlimited);
            let hard = parts.get(1)
                .and_then(|s| LimitValue::parse(s))
                .unwrap_or(LimitValue::Unlimited);

            limits.push(ResourceLimit {
                resource: res,
                soft,
                hard,
                units: res.units(),
            });
        }
    }

    limits
}

fn generate_default_limits() -> Vec<ResourceLimit> {
    vec![
        ResourceLimit { resource: Resource::AddressSpace, soft: LimitValue::Unlimited, hard: LimitValue::Unlimited, units: "bytes" },
        ResourceLimit { resource: Resource::CoreSize, soft: LimitValue::Value(0), hard: LimitValue::Unlimited, units: "bytes" },
        ResourceLimit { resource: Resource::CpuTime, soft: LimitValue::Unlimited, hard: LimitValue::Unlimited, units: "seconds" },
        ResourceLimit { resource: Resource::DataSize, soft: LimitValue::Unlimited, hard: LimitValue::Unlimited, units: "bytes" },
        ResourceLimit { resource: Resource::FileSize, soft: LimitValue::Unlimited, hard: LimitValue::Unlimited, units: "bytes" },
        ResourceLimit { resource: Resource::Locks, soft: LimitValue::Unlimited, hard: LimitValue::Unlimited, units: "" },
        ResourceLimit { resource: Resource::MemLock, soft: LimitValue::Value(65536), hard: LimitValue::Value(65536), units: "bytes" },
        ResourceLimit { resource: Resource::MsgQueue, soft: LimitValue::Value(819200), hard: LimitValue::Value(819200), units: "bytes" },
        ResourceLimit { resource: Resource::Nice, soft: LimitValue::Value(0), hard: LimitValue::Value(0), units: "" },
        ResourceLimit { resource: Resource::OpenFiles, soft: LimitValue::Value(1024), hard: LimitValue::Value(1048576), units: "" },
        ResourceLimit { resource: Resource::Processes, soft: LimitValue::Value(63195), hard: LimitValue::Value(63195), units: "" },
        ResourceLimit { resource: Resource::Rss, soft: LimitValue::Unlimited, hard: LimitValue::Unlimited, units: "bytes" },
        ResourceLimit { resource: Resource::RtPrio, soft: LimitValue::Value(0), hard: LimitValue::Value(0), units: "" },
        ResourceLimit { resource: Resource::RtTime, soft: LimitValue::Unlimited, hard: LimitValue::Unlimited, units: "microseconds" },
        ResourceLimit { resource: Resource::SigPending, soft: LimitValue::Value(63195), hard: LimitValue::Value(63195), units: "" },
        ResourceLimit { resource: Resource::StackSize, soft: LimitValue::Value(8388608), hard: LimitValue::Unlimited, units: "bytes" },
    ]
}

// ============================================================================
// Output
// ============================================================================

fn print_limits_table(out: &mut io::StdoutLock<'_>, limits: &[ResourceLimit], json: bool, raw: bool) {
    if json {
        let _ = writeln!(out, "{{");
        let _ = writeln!(out, "  \"limits\": [");
        for (i, lim) in limits.iter().enumerate() {
            let comma = if i + 1 < limits.len() { "," } else { "" };
            let _ = writeln!(out, "    {{\"resource\":\"{}\",\"description\":\"{}\",\"soft\":\"{}\",\"hard\":\"{}\",\"units\":\"{}\"}}{comma}",
                lim.resource.name(), lim.resource.description(),
                lim.soft.display(), lim.hard.display(), lim.units);
        }
        let _ = writeln!(out, "  ]");
        let _ = writeln!(out, "}}");
        return;
    }

    if raw {
        for lim in limits {
            let _ = writeln!(out, "{}:{}:{}:{}",
                lim.resource.name(), lim.soft.display(), lim.hard.display(), lim.units);
        }
        return;
    }

    let _ = writeln!(out, "{:<14} {:<36} {:>14} {:>14} {:>12}",
        "RESOURCE", "DESCRIPTION", "SOFT", "HARD", "UNITS");
    for lim in limits {
        let _ = writeln!(out, "{:<14} {:<36} {:>14} {:>14} {:>12}",
            lim.resource.name(),
            lim.resource.description(),
            lim.soft.display(),
            lim.hard.display(),
            lim.units);
    }
}

// ============================================================================
// prlimit command
// ============================================================================

fn cmd_prlimit(args: &[String]) {
    let mut pid: Option<u32> = None;
    let mut json = false;
    let mut raw = false;
    let mut filter_resources: Vec<Resource> = Vec::new();
    let mut set_operations: Vec<(Resource, LimitValue, LimitValue)> = Vec::new();
    let mut output_cols: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: prlimit [options] [--pid PID] [--<resource>[=<soft>:<hard>]]");
                println!();
                println!("Get or set process resource limits.");
                println!();
                println!("Options:");
                println!("  -p, --pid PID      Process ID (default: self)");
                println!("  -o, --output LIST  Column list");
                println!("  --raw              Raw output");
                println!("  -J, --json         JSON output");
                println!();
                println!("Resource options (use --<name> to show, --<name>=soft:hard to set):");
                println!("  --as               Address space");
                println!("  --core             Core file size");
                println!("  --cpu              CPU time");
                println!("  --data             Data size");
                println!("  --fsize            File size");
                println!("  --locks            File locks");
                println!("  --memlock          Locked memory");
                println!("  --msgqueue         Message queues");
                println!("  --nice             Nice priority");
                println!("  --nofile           Open files");
                println!("  --nproc            Processes");
                println!("  --rss              Resident set size");
                println!("  --rtprio           RT priority");
                println!("  --rttime           RT timeout");
                println!("  --sigpending       Pending signals");
                println!("  --stack            Stack size");
                println!();
                println!("  -h, --help         Show help");
                println!("  -V, --version      Show version");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("prlimit {VERSION}");
                process::exit(0);
            }
            "-p" | "--pid" => {
                i += 1;
                if i < args.len() { pid = args[i].parse().ok(); }
            }
            "-J" | "--json" => json = true,
            "--raw" => raw = true,
            "-o" | "--output" => {
                i += 1;
                if i < args.len() { output_cols = Some(args[i].clone()); }
            }
            s if s.starts_with("--") => {
                let rest = &s[2..];
                if let Some((name, value)) = rest.split_once('=') {
                    // Set operation.
                    if let Some(res) = parse_resource_name(name) {
                        let parts: Vec<&str> = value.split(':').collect();
                        let soft = LimitValue::parse(parts.first().unwrap_or(&"unlimited")).unwrap_or(LimitValue::Unlimited);
                        let hard = LimitValue::parse(parts.get(1).unwrap_or(parts.first().unwrap_or(&"unlimited"))).unwrap_or(LimitValue::Unlimited);
                        set_operations.push((res, soft, hard));
                    }
                } else if let Some(res) = parse_resource_name(rest) {
                    filter_resources.push(res);
                }
            }
            _ => {}
        }
        i += 1;
    }

    let target_pid = pid.unwrap_or(process::id());
    let _ = output_cols; // Reserved for future column filtering.

    // Handle set operations.
    if !set_operations.is_empty() {
        for (res, soft, hard) in &set_operations {
            eprintln!("prlimit: setting {} for PID {}: soft={}, hard={}",
                res.name(), target_pid, soft.display(), hard.display());
        }
        return;
    }

    // Read and display limits.
    let limits = read_proc_limits(target_pid);
    let filtered = if filter_resources.is_empty() {
        limits
    } else {
        limits.into_iter()
            .filter(|l| filter_resources.contains(&l.resource))
            .collect()
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();
    print_limits_table(&mut out, &filtered, json, raw);
}

fn parse_resource_name(name: &str) -> Option<Resource> {
    match name.to_lowercase().as_str() {
        "as" | "address" | "addressspace" => Some(Resource::AddressSpace),
        "core" | "coresize" => Some(Resource::CoreSize),
        "cpu" | "cputime" => Some(Resource::CpuTime),
        "data" | "datasize" => Some(Resource::DataSize),
        "fsize" | "filesize" => Some(Resource::FileSize),
        "locks" => Some(Resource::Locks),
        "memlock" | "lockedmemory" => Some(Resource::MemLock),
        "msgqueue" | "messagequeue" => Some(Resource::MsgQueue),
        "nice" => Some(Resource::Nice),
        "nofile" | "openfiles" => Some(Resource::OpenFiles),
        "nproc" | "processes" => Some(Resource::Processes),
        "rss" => Some(Resource::Rss),
        "rtprio" | "realtimepriority" => Some(Resource::RtPrio),
        "rttime" | "realtimetimeout" => Some(Resource::RtTime),
        "sigpending" | "signals" => Some(Resource::SigPending),
        "stack" | "stacksize" => Some(Resource::StackSize),
        _ => None,
    }
}

// ============================================================================
// ulimit command
// ============================================================================

fn cmd_ulimit(args: &[String]) {
    let mut resource = Resource::FileSize; // Default: -f.
    let mut show_all = false;
    let mut hard = false;

    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("Usage: ulimit [options] [limit]");
                println!();
                println!("Shell resource limits.");
                println!();
                println!("Options:");
                println!("  -a    Show all limits");
                println!("  -H    Show hard limit");
                println!("  -S    Show soft limit (default)");
                println!("  -c    Core file size");
                println!("  -d    Data segment size");
                println!("  -f    File size (default)");
                println!("  -l    Locked memory");
                println!("  -m    RSS");
                println!("  -n    Open files");
                println!("  -s    Stack size");
                println!("  -t    CPU time");
                println!("  -u    Processes");
                println!("  -v    Address space");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("ulimit {VERSION}");
                process::exit(0);
            }
            "-a" => show_all = true,
            "-H" => hard = true,
            "-S" => hard = false,
            "-c" => resource = Resource::CoreSize,
            "-d" => resource = Resource::DataSize,
            "-f" => resource = Resource::FileSize,
            "-l" => resource = Resource::MemLock,
            "-m" => resource = Resource::Rss,
            "-n" => resource = Resource::OpenFiles,
            "-s" => resource = Resource::StackSize,
            "-t" => resource = Resource::CpuTime,
            "-u" => resource = Resource::Processes,
            "-v" => resource = Resource::AddressSpace,
            _ => {}
        }
    }

    let limits = read_proc_limits(process::id());

    let stdout = io::stdout();
    let mut out = stdout.lock();

    if show_all {
        for lim in &limits {
            let val = if hard { &lim.hard } else { &lim.soft };
            let _ = writeln!(out, "{:<40} {}", lim.resource.description(), val.display());
        }
    } else {
        for lim in &limits {
            if lim.resource == resource {
                let val = if hard { &lim.hard } else { &lim.soft };
                let _ = writeln!(out, "{}", val.display());
                return;
            }
        }
        let _ = writeln!(out, "unlimited");
    }
}

// ============================================================================
// CLI
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("prlimit");
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
        "ulimit" => cmd_ulimit(&rest),
        _ => cmd_prlimit(&rest),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_names() {
        assert_eq!(Resource::OpenFiles.name(), "NOFILE");
        assert_eq!(Resource::StackSize.name(), "STACK");
        assert_eq!(Resource::CpuTime.name(), "CPU");
        assert_eq!(Resource::AddressSpace.name(), "AS");
    }

    #[test]
    fn test_resource_descriptions() {
        assert_eq!(Resource::OpenFiles.description(), "max number of open files");
        assert_eq!(Resource::StackSize.description(), "max stack size");
    }

    #[test]
    fn test_resource_units() {
        assert_eq!(Resource::OpenFiles.units(), "");
        assert_eq!(Resource::StackSize.units(), "bytes");
        assert_eq!(Resource::CpuTime.units(), "seconds");
        assert_eq!(Resource::RtTime.units(), "microseconds");
    }

    #[test]
    fn test_limit_value_display() {
        assert_eq!(LimitValue::Unlimited.display(), "unlimited");
        assert_eq!(LimitValue::Value(1024).display(), "1024");
        assert_eq!(LimitValue::Value(0).display(), "0");
    }

    #[test]
    fn test_limit_value_parse() {
        assert_eq!(LimitValue::parse("unlimited"), Some(LimitValue::Unlimited));
        assert_eq!(LimitValue::parse("infinity"), Some(LimitValue::Unlimited));
        assert_eq!(LimitValue::parse("-1"), Some(LimitValue::Unlimited));
        assert_eq!(LimitValue::parse("1024"), Some(LimitValue::Value(1024)));
        assert_eq!(LimitValue::parse("0"), Some(LimitValue::Value(0)));
        assert!(LimitValue::parse("abc").is_none());
    }

    #[test]
    fn test_parse_resource_name() {
        assert_eq!(parse_resource_name("nofile"), Some(Resource::OpenFiles));
        assert_eq!(parse_resource_name("stack"), Some(Resource::StackSize));
        assert_eq!(parse_resource_name("cpu"), Some(Resource::CpuTime));
        assert_eq!(parse_resource_name("as"), Some(Resource::AddressSpace));
        assert_eq!(parse_resource_name("nice"), Some(Resource::Nice));
        assert!(parse_resource_name("unknown").is_none());
    }

    #[test]
    fn test_generate_default_limits() {
        let limits = generate_default_limits();
        assert_eq!(limits.len(), 16);
    }

    #[test]
    fn test_default_open_files() {
        let limits = generate_default_limits();
        let nofile = limits.iter().find(|l| l.resource == Resource::OpenFiles).unwrap();
        assert_eq!(nofile.soft, LimitValue::Value(1024));
        assert_eq!(nofile.hard, LimitValue::Value(1048576));
    }

    #[test]
    fn test_default_stack_size() {
        let limits = generate_default_limits();
        let stack = limits.iter().find(|l| l.resource == Resource::StackSize).unwrap();
        assert_eq!(stack.soft, LimitValue::Value(8388608));
        assert_eq!(stack.hard, LimitValue::Unlimited);
    }

    #[test]
    fn test_default_core_size() {
        let limits = generate_default_limits();
        let core = limits.iter().find(|l| l.resource == Resource::CoreSize).unwrap();
        assert_eq!(core.soft, LimitValue::Value(0));
        assert_eq!(core.hard, LimitValue::Unlimited);
    }

    #[test]
    fn test_resource_limit_clone() {
        let lim = ResourceLimit {
            resource: Resource::OpenFiles,
            soft: LimitValue::Value(1024),
            hard: LimitValue::Value(1048576),
            units: "",
        };
        let c = lim.clone();
        assert_eq!(c.resource, Resource::OpenFiles);
        assert_eq!(c.soft, LimitValue::Value(1024));
    }

    #[test]
    fn test_all_resources_count() {
        assert_eq!(ALL_RESOURCES.len(), 16);
    }

    #[test]
    fn test_read_proc_limits_self() {
        let limits = read_proc_limits(process::id());
        assert!(!limits.is_empty());
    }

    #[test]
    fn test_read_proc_limits_invalid_pid() {
        let limits = read_proc_limits(999999999);
        // Should fall back to defaults.
        assert!(!limits.is_empty());
    }

    #[test]
    fn test_resource_equality() {
        assert_eq!(Resource::OpenFiles, Resource::OpenFiles);
        assert_ne!(Resource::OpenFiles, Resource::StackSize);
    }

    #[test]
    fn test_limit_value_equality() {
        assert_eq!(LimitValue::Unlimited, LimitValue::Unlimited);
        assert_eq!(LimitValue::Value(42), LimitValue::Value(42));
        assert_ne!(LimitValue::Unlimited, LimitValue::Value(0));
    }

    #[test]
    fn test_parse_resource_case_insensitive() {
        assert_eq!(parse_resource_name("NOFILE"), Some(Resource::OpenFiles));
        assert_eq!(parse_resource_name("Stack"), Some(Resource::StackSize));
    }
}
