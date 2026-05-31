// OurOS taskset - CPU affinity and scheduling tools
//
// Multi-personality binary:
//   taskset  - set/get process CPU affinity
//   chrt     - set/get real-time scheduling attributes
//   ionice   - set/get I/O scheduling class and priority
//   renice   - alter priority of running processes

#![cfg_attr(not(test), no_main)]

// ── Constants ──────────────────────────────────────────────────────────

const MAX_CPUS: usize = 1024;

// Scheduling policies
const SCHED_OTHER: u32 = 0;
const SCHED_FIFO: u32 = 1;
const SCHED_RR: u32 = 2;
const SCHED_BATCH: u32 = 3;
const SCHED_IDLE: u32 = 5;
const SCHED_DEADLINE: u32 = 6;

// I/O scheduling classes
const IOPRIO_CLASS_NONE: u32 = 0;
const IOPRIO_CLASS_RT: u32 = 1;
const IOPRIO_CLASS_BE: u32 = 2;
const IOPRIO_CLASS_IDLE: u32 = 3;

// I/O priority levels (0-7, lower = higher priority)
const IOPRIO_MAX_PRIO: u32 = 8;

// Who to apply ioprio to
const IOPRIO_WHO_PROCESS: u32 = 1;
const IOPRIO_WHO_PGRP: u32 = 2;
const IOPRIO_WHO_USER: u32 = 3;

// Nice range
const NICE_MIN: i32 = -20;
const NICE_MAX: i32 = 19;

// ── Personality Detection ──────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
enum Personality {
    Taskset,
    Chrt,
    Ionice,
    Renice,
}

fn detect_personality(argv0: &[u8]) -> Personality {
    let basename = if let Some(pos) = argv0.iter().rposition(|&b| b == b'/' || b == b'\\') {
        &argv0[pos + 1..]
    } else {
        argv0
    };

    let name = if basename.len() > 4 && basename[basename.len() - 4..].eq_ignore_ascii_case(b".exe")
    {
        &basename[..basename.len() - 4]
    } else {
        basename
    };

    if name.eq_ignore_ascii_case(b"chrt") {
        Personality::Chrt
    } else if name.eq_ignore_ascii_case(b"ionice") {
        Personality::Ionice
    } else if name.eq_ignore_ascii_case(b"renice") {
        Personality::Renice
    } else {
        Personality::Taskset
    }
}

// ── CPU Mask ───────────────────────────────────────────────────────────

#[derive(Clone)]
struct CpuMask {
    bits: Vec<u64>,
}

impl CpuMask {
    fn new(num_cpus: usize) -> Self {
        let words = (num_cpus + 63) / 64;
        CpuMask {
            bits: vec![0u64; words],
        }
    }

    fn set(&mut self, cpu: usize) {
        let word = cpu / 64;
        let bit = cpu % 64;
        if word < self.bits.len() {
            self.bits[word] |= 1u64 << bit;
        }
    }

    fn clear(&mut self, cpu: usize) {
        let word = cpu / 64;
        let bit = cpu % 64;
        if word < self.bits.len() {
            self.bits[word] &= !(1u64 << bit);
        }
    }

    fn is_set(&self, cpu: usize) -> bool {
        let word = cpu / 64;
        let bit = cpu % 64;
        if word < self.bits.len() {
            (self.bits[word] & (1u64 << bit)) != 0
        } else {
            false
        }
    }

    fn count(&self) -> usize {
        self.bits.iter().map(|w| w.count_ones() as usize).sum()
    }

    fn is_empty(&self) -> bool {
        self.bits.iter().all(|&w| w == 0)
    }

    fn from_hex(hex: &[u8]) -> Option<CpuMask> {
        // Parse hex string like "ff" or "f,f0000000" (comma-separated 32-bit groups)
        let mut mask = CpuMask::new(MAX_CPUS);

        // Remove 0x prefix if present
        let hex = if hex.starts_with(b"0x") || hex.starts_with(b"0X") {
            &hex[2..]
        } else {
            hex
        };

        // Handle comma-separated groups
        let groups: Vec<&[u8]> = hex.split(|&b| b == b',').collect();

        let mut bit_offset = 0;
        for group in groups.iter().rev() {
            // Parse each hex group
            let mut value: u64 = 0;
            for &c in *group {
                let nibble = match c {
                    b'0'..=b'9' => c - b'0',
                    b'a'..=b'f' => c - b'a' + 10,
                    b'A'..=b'F' => c - b'A' + 10,
                    _ => return None,
                };
                value = value.checked_mul(16)?;
                value = value.checked_add(nibble as u64)?;
            }

            // Set bits
            for bit in 0..64 {
                if value & (1u64 << bit) != 0 {
                    let cpu = bit_offset + bit;
                    if cpu < MAX_CPUS {
                        mask.set(cpu);
                    }
                }
            }
            bit_offset += 32; // Each comma-separated group is 32 bits
        }

        Some(mask)
    }

    fn from_list(list: &[u8]) -> Option<CpuMask> {
        // Parse CPU list like "0,1,2-5,8"
        let mut mask = CpuMask::new(MAX_CPUS);

        for part in list.split(|&b| b == b',') {
            let part = trim_bytes(part);
            if part.is_empty() {
                continue;
            }

            if let Some(dash) = part.iter().position(|&b| b == b'-') {
                // Range
                let start = parse_usize(&part[..dash])?;
                let end = parse_usize(&part[dash + 1..])?;
                if start > end || end >= MAX_CPUS {
                    return None;
                }
                for cpu in start..=end {
                    mask.set(cpu);
                }
            } else {
                // Single CPU
                let cpu = parse_usize(part)?;
                if cpu >= MAX_CPUS {
                    return None;
                }
                mask.set(cpu);
            }
        }

        Some(mask)
    }

    fn to_hex_string(&self) -> Vec<u8> {
        let hex_chars = b"0123456789abcdef";
        let mut result = Vec::new();
        let mut started = false;

        // Print from highest word to lowest
        for &word in self.bits.iter().rev() {
            if !started && word == 0 {
                continue;
            }
            started = true;

            // Print 16 hex digits per 64-bit word
            for i in (0..16).rev() {
                let nibble = ((word >> (i * 4)) & 0xf) as usize;
                if !result.is_empty() || nibble != 0 {
                    result.push(hex_chars[nibble]);
                }
            }
        }

        if result.is_empty() {
            result.push(b'0');
        }

        result
    }

    fn to_list_string(&self) -> Vec<u8> {
        let mut result = Vec::new();
        let mut cpu = 0;
        let total_bits = self.bits.len() * 64;

        while cpu < total_bits {
            if self.is_set(cpu) {
                let start = cpu;
                while cpu + 1 < total_bits && self.is_set(cpu + 1) {
                    cpu += 1;
                }
                let end = cpu;

                if !result.is_empty() {
                    result.push(b',');
                }
                result.extend_from_slice(&format_usize(start));
                if end > start {
                    result.push(b'-');
                    result.extend_from_slice(&format_usize(end));
                }
            }
            cpu += 1;
        }

        if result.is_empty() {
            result.push(b'0');
        }

        result
    }
}

// ── taskset ────────────────────────────────────────────────────────────

struct TasksetArgs {
    pid: Option<u32>,
    mask: Option<CpuMask>,
    command: Vec<Vec<u8>>,
    cpu_list: bool,
    all_tasks: bool,
    show_help: bool,
    show_version: bool,
}

fn parse_taskset_args(args: &[Vec<u8>]) -> TasksetArgs {
    let mut result = TasksetArgs {
        pid: None,
        mask: None,
        command: Vec::new(),
        cpu_list: false,
        all_tasks: false,
        show_help: false,
        show_version: false,
    };

    let mut i = 0;
    let mut positionals = Vec::new();

    while i < args.len() {
        let arg = &args[i];
        if arg == b"-h" || arg == b"--help" {
            result.show_help = true;
        } else if arg == b"-V" || arg == b"--version" {
            result.show_version = true;
        } else if arg == b"-p" || arg == b"--pid" {
            // Next positional is the PID (and optional mask before it)
            i += 1;
            if i < args.len() {
                // Could be "taskset -p mask pid" or "taskset -p pid"
                if i + 1 < args.len() && !args[i + 1].starts_with(b"-") {
                    // mask pid
                    result.mask = if result.cpu_list {
                        CpuMask::from_list(&args[i])
                    } else {
                        CpuMask::from_hex(&args[i])
                    };
                    i += 1;
                    result.pid = parse_u32(&args[i]);
                } else {
                    result.pid = parse_u32(&args[i]);
                }
            }
        } else if arg == b"-c" || arg == b"--cpu-list" {
            result.cpu_list = true;
        } else if arg == b"-a" || arg == b"--all-tasks" {
            result.all_tasks = true;
        } else if !arg.starts_with(b"-") {
            positionals.push(arg.clone());
        }
        i += 1;
    }

    // If no -p, first positional is mask, rest is command
    if result.pid.is_none() && !positionals.is_empty() {
        let mask_str = &positionals[0];
        result.mask = if result.cpu_list {
            CpuMask::from_list(mask_str)
        } else {
            CpuMask::from_hex(mask_str)
        };
        result.command = positionals[1..].to_vec();
    }

    result
}

fn cmd_taskset(args: &TasksetArgs) -> i32 {
    if args.show_help {
        print_out(b"Usage: taskset [options] [mask | -c list] [pid | cmd [args...]]\n\n");
        print_out(b"Show or change the CPU affinity of a process.\n\n");
        print_out(b"Options:\n");
        print_out(b"  -p, --pid PID       operate on existing PID\n");
        print_out(b"  -c, --cpu-list      display/specify CPUs in list format\n");
        print_out(b"  -a, --all-tasks     operate on all threads of the PID\n");
        print_out(b"  -h, --help          display this help\n");
        print_out(b"  -V, --version       display version\n\n");
        print_out(b"Examples:\n");
        print_out(b"  taskset 0x3 myapp          # Run myapp on CPUs 0,1\n");
        print_out(b"  taskset -c 0-3 myapp       # Run myapp on CPUs 0-3\n");
        print_out(b"  taskset -p 1234            # Show affinity of PID 1234\n");
        print_out(b"  taskset -p 0xf 1234        # Set PID 1234 to CPUs 0-3\n");
        return 0;
    }

    if args.show_version {
        print_out(b"taskset (OurOS util-linux) 1.0.0\n");
        return 0;
    }

    if let Some(pid) = args.pid {
        if let Some(ref mask) = args.mask {
            // Set affinity for pid
            taskset_set_affinity(pid, mask, args.all_tasks)
        } else {
            // Get affinity for pid
            taskset_get_affinity(pid, args.cpu_list)
        }
    } else if args.mask.is_some() && !args.command.is_empty() {
        // Set affinity and exec command
        taskset_exec(args)
    } else {
        print_err(b"taskset: no target specified\n");
        print_err(b"Usage: taskset [options] [mask] [pid | cmd]\n");
        1
    }
}

fn taskset_get_affinity(pid: u32, list_format: bool) -> i32 {
    // In real implementation: sched_getaffinity(pid, ...)
    // Simulate: all CPUs available
    let mut mask = CpuMask::new(MAX_CPUS);
    for i in 0..4 {
        mask.set(i);
    }

    print_out(b"pid ");
    print_out(&format_u64(pid as u64));
    print_out(b"'s current affinity ");
    if list_format {
        print_out(b"list: ");
        print_out(&mask.to_list_string());
    } else {
        print_out(b"mask: ");
        print_out(&mask.to_hex_string());
    }
    print_out(b"\n");
    0
}

fn taskset_set_affinity(pid: u32, mask: &CpuMask, _all_tasks: bool) -> i32 {
    if mask.is_empty() {
        print_err(b"taskset: empty CPU mask\n");
        return 1;
    }

    // In real implementation: sched_setaffinity(pid, ...)
    print_out(b"pid ");
    print_out(&format_u64(pid as u64));
    print_out(b"'s new affinity mask: ");
    print_out(&mask.to_hex_string());
    print_out(b"\n");
    0
}

fn taskset_exec(args: &TasksetArgs) -> i32 {
    // In real implementation: sched_setaffinity(0, ...) then execvp
    if let Some(ref mask) = args.mask {
        print_out(b"Setting CPU affinity to ");
        if args.cpu_list {
            print_out(&mask.to_list_string());
        } else {
            print_out(&mask.to_hex_string());
        }
        print_out(b" and executing: ");
        for (i, arg) in args.command.iter().enumerate() {
            if i > 0 {
                print_out(b" ");
            }
            print_out(arg);
        }
        print_out(b"\n");
    }
    0
}

// ── chrt ───────────────────────────────────────────────────────────────

struct ChrtArgs {
    pid: Option<u32>,
    policy: Option<u32>,
    priority: Option<u32>,
    command: Vec<Vec<u8>>,
    reset_on_fork: bool,
    all_tasks: bool,
    verbose: bool,
    max_show: bool,
    show_help: bool,
    show_version: bool,
    // Deadline params
    runtime: Option<u64>,
    deadline: Option<u64>,
    period: Option<u64>,
}

fn parse_chrt_args(args: &[Vec<u8>]) -> ChrtArgs {
    let mut result = ChrtArgs {
        pid: None,
        policy: None,
        priority: None,
        command: Vec::new(),
        reset_on_fork: false,
        all_tasks: false,
        verbose: false,
        max_show: false,
        show_help: false,
        show_version: false,
        runtime: None,
        deadline: None,
        period: None,
    };

    let mut i = 0;
    let mut positionals = Vec::new();

    while i < args.len() {
        let arg = &args[i];
        if arg == b"-h" || arg == b"--help" {
            result.show_help = true;
        } else if arg == b"-V" || arg == b"--version" {
            result.show_version = true;
        } else if arg == b"-p" || arg == b"--pid" {
            // PID mode: next arg(s) are [priority] pid
            i += 1;
            if i < args.len() {
                if i + 1 < args.len() && !args[i + 1].starts_with(b"-") {
                    result.priority = parse_u32(&args[i]);
                    i += 1;
                    result.pid = parse_u32(&args[i]);
                } else {
                    result.pid = parse_u32(&args[i]);
                }
            }
        } else if arg == b"-o" || arg == b"--other" {
            result.policy = Some(SCHED_OTHER);
        } else if arg == b"-f" || arg == b"--fifo" {
            result.policy = Some(SCHED_FIFO);
        } else if arg == b"-r" || arg == b"--rr" {
            result.policy = Some(SCHED_RR);
        } else if arg == b"-b" || arg == b"--batch" {
            result.policy = Some(SCHED_BATCH);
        } else if arg == b"-i" || arg == b"--idle" {
            result.policy = Some(SCHED_IDLE);
        } else if arg == b"-d" || arg == b"--deadline" {
            result.policy = Some(SCHED_DEADLINE);
        } else if arg == b"-R" || arg == b"--reset-on-fork" {
            result.reset_on_fork = true;
        } else if arg == b"-a" || arg == b"--all-tasks" {
            result.all_tasks = true;
        } else if arg == b"-v" || arg == b"--verbose" {
            result.verbose = true;
        } else if arg == b"-m" || arg == b"--max" {
            result.max_show = true;
        } else if arg == b"--sched-runtime" {
            i += 1;
            if i < args.len() {
                result.runtime = parse_u64_bytes(&args[i]);
            }
        } else if arg == b"--sched-deadline" {
            i += 1;
            if i < args.len() {
                result.deadline = parse_u64_bytes(&args[i]);
            }
        } else if arg == b"--sched-period" {
            i += 1;
            if i < args.len() {
                result.period = parse_u64_bytes(&args[i]);
            }
        } else if !arg.starts_with(b"-") {
            positionals.push(arg.clone());
        }
        i += 1;
    }

    // If no -p, first positional is priority, rest is command
    if result.pid.is_none() && !positionals.is_empty() {
        result.priority = parse_u32(&positionals[0]);
        result.command = positionals[1..].to_vec();
    }

    result
}

fn cmd_chrt(args: &ChrtArgs) -> i32 {
    if args.show_help {
        print_out(b"Usage: chrt [options] [prio] [pid | cmd [args...]]\n\n");
        print_out(b"Show or change the real-time scheduling attributes of a process.\n\n");
        print_out(b"Policy options:\n");
        print_out(b"  -o, --other         SCHED_OTHER (default)\n");
        print_out(b"  -f, --fifo          SCHED_FIFO\n");
        print_out(b"  -r, --rr            SCHED_RR (round-robin)\n");
        print_out(b"  -b, --batch         SCHED_BATCH\n");
        print_out(b"  -i, --idle          SCHED_IDLE\n");
        print_out(b"  -d, --deadline      SCHED_DEADLINE\n\n");
        print_out(b"Other options:\n");
        print_out(b"  -p, --pid PID       operate on existing PID\n");
        print_out(b"  -R, --reset-on-fork reset scheduling on fork\n");
        print_out(b"  -a, --all-tasks     operate on all threads\n");
        print_out(b"  -m, --max           show min/max priority values\n");
        print_out(b"  -v, --verbose       verbose output\n");
        print_out(b"  -h, --help          display this help\n");
        print_out(b"  -V, --version       display version\n");
        return 0;
    }

    if args.show_version {
        print_out(b"chrt (OurOS util-linux) 1.0.0\n");
        return 0;
    }

    if args.max_show {
        return chrt_show_max();
    }

    if let Some(pid) = args.pid {
        if args.priority.is_some() || args.policy.is_some() {
            chrt_set(pid, args)
        } else {
            chrt_get(pid, args.verbose)
        }
    } else if !args.command.is_empty() {
        chrt_exec(args)
    } else {
        print_err(b"chrt: no target specified\n");
        1
    }
}

fn chrt_show_max() -> i32 {
    let policies: &[(&[u8], u32, i32, i32)] = &[
        (b"SCHED_OTHER", SCHED_OTHER, 0, 0),
        (b"SCHED_FIFO", SCHED_FIFO, 1, 99),
        (b"SCHED_RR", SCHED_RR, 1, 99),
        (b"SCHED_BATCH", SCHED_BATCH, 0, 0),
        (b"SCHED_IDLE", SCHED_IDLE, 0, 0),
        (b"SCHED_DEADLINE", SCHED_DEADLINE, 0, 0),
    ];

    for (name, _, min, max) in policies {
        print_out(name);
        print_out(b" min/max priority\t: ");
        print_out(&format_i32(*min));
        print_out(b"/");
        print_out(&format_i32(*max));
        print_out(b"\n");
    }
    0
}

fn policy_name(policy: u32) -> &'static [u8] {
    match policy {
        SCHED_OTHER => b"SCHED_OTHER",
        SCHED_FIFO => b"SCHED_FIFO",
        SCHED_RR => b"SCHED_RR",
        SCHED_BATCH => b"SCHED_BATCH",
        SCHED_IDLE => b"SCHED_IDLE",
        SCHED_DEADLINE => b"SCHED_DEADLINE",
        _ => b"unknown",
    }
}

fn chrt_get(pid: u32, verbose: bool) -> i32 {
    // In real implementation: sched_getscheduler + sched_getparam
    let current_policy = SCHED_OTHER;
    let current_prio = 0;

    print_out(b"pid ");
    print_out(&format_u64(pid as u64));
    print_out(b"'s current scheduling policy: ");
    print_out(policy_name(current_policy));
    print_out(b"\n");

    print_out(b"pid ");
    print_out(&format_u64(pid as u64));
    print_out(b"'s current scheduling priority: ");
    print_out(&format_u64(current_prio as u64));
    print_out(b"\n");

    0
}

fn chrt_set(pid: u32, args: &ChrtArgs) -> i32 {
    let policy = args.policy.unwrap_or(SCHED_RR);
    let prio = args.priority.unwrap_or(0);

    if args.verbose {
        print_out(b"pid ");
        print_out(&format_u64(pid as u64));
        print_out(b"'s new scheduling policy: ");
        print_out(policy_name(policy));
        print_out(b"\n");
        print_out(b"pid ");
        print_out(&format_u64(pid as u64));
        print_out(b"'s new scheduling priority: ");
        print_out(&format_u64(prio as u64));
        print_out(b"\n");
    }

    // In real implementation: sched_setscheduler(pid, policy, &param)
    0
}

fn chrt_exec(args: &ChrtArgs) -> i32 {
    let policy = args.policy.unwrap_or(SCHED_RR);
    let prio = args.priority.unwrap_or(0);

    if args.verbose {
        print_out(b"Policy: ");
        print_out(policy_name(policy));
        print_out(b", priority: ");
        print_out(&format_u64(prio as u64));
        print_out(b"\n");
    }

    // In real implementation: sched_setscheduler + execvp
    0
}

// ── ionice ─────────────────────────────────────────────────────────────

struct IonicArgs {
    pid: Option<u32>,
    pgid: Option<u32>,
    uid: Option<u32>,
    class: Option<u32>,
    classdata: Option<u32>,
    command: Vec<Vec<u8>>,
    show_help: bool,
    show_version: bool,
}

fn parse_ionice_args(args: &[Vec<u8>]) -> IonicArgs {
    let mut result = IonicArgs {
        pid: None,
        pgid: None,
        uid: None,
        class: None,
        classdata: None,
        command: Vec::new(),
        show_help: false,
        show_version: false,
    };

    let mut i = 0;
    let mut found_command = false;

    while i < args.len() {
        if found_command {
            result.command.push(args[i].clone());
            i += 1;
            continue;
        }

        let arg = &args[i];
        if arg == b"-h" || arg == b"--help" {
            result.show_help = true;
        } else if arg == b"-V" || arg == b"--version" {
            result.show_version = true;
        } else if arg == b"-p" || arg == b"--pid" {
            i += 1;
            if i < args.len() {
                result.pid = parse_u32(&args[i]);
            }
        } else if arg == b"-P" || arg == b"--pgid" {
            i += 1;
            if i < args.len() {
                result.pgid = parse_u32(&args[i]);
            }
        } else if arg == b"-u" || arg == b"--uid" {
            i += 1;
            if i < args.len() {
                result.uid = parse_u32(&args[i]);
            }
        } else if arg == b"-c" || arg == b"--class" {
            i += 1;
            if i < args.len() {
                result.class = parse_ionice_class(&args[i]);
            }
        } else if arg == b"-n" || arg == b"--classdata" {
            i += 1;
            if i < args.len() {
                result.classdata = parse_u32(&args[i]);
            }
        } else if !arg.starts_with(b"-") {
            found_command = true;
            result.command.push(arg.clone());
        }
        i += 1;
    }

    result
}

fn parse_ionice_class(s: &[u8]) -> Option<u32> {
    match s {
        b"0" | b"none" => Some(IOPRIO_CLASS_NONE),
        b"1" | b"realtime" | b"rt" => Some(IOPRIO_CLASS_RT),
        b"2" | b"best-effort" | b"be" => Some(IOPRIO_CLASS_BE),
        b"3" | b"idle" => Some(IOPRIO_CLASS_IDLE),
        _ => None,
    }
}

fn ioclass_name(class: u32) -> &'static [u8] {
    match class {
        IOPRIO_CLASS_NONE => b"none",
        IOPRIO_CLASS_RT => b"realtime",
        IOPRIO_CLASS_BE => b"best-effort",
        IOPRIO_CLASS_IDLE => b"idle",
        _ => b"unknown",
    }
}

fn cmd_ionice(args: &IonicArgs) -> i32 {
    if args.show_help {
        print_out(b"Usage: ionice [options] [-p PID] [cmd [args...]]\n\n");
        print_out(b"Show or change the I/O scheduling class and priority.\n\n");
        print_out(b"Options:\n");
        print_out(b"  -c, --class CLASS    scheduling class (0-3 or name)\n");
        print_out(b"                       0: none, 1: realtime, 2: best-effort, 3: idle\n");
        print_out(b"  -n, --classdata N    scheduling class data (0-7)\n");
        print_out(b"  -p, --pid PID        operate on existing PID\n");
        print_out(b"  -P, --pgid PGID      operate on process group\n");
        print_out(b"  -u, --uid UID        operate on all processes of user\n");
        print_out(b"  -h, --help           display this help\n");
        print_out(b"  -V, --version        display version\n");
        return 0;
    }

    if args.show_version {
        print_out(b"ionice (OurOS util-linux) 1.0.0\n");
        return 0;
    }

    // Determine who to target
    let target_pid = args.pid.or(args.pgid).or(args.uid);

    if args.class.is_none() && args.classdata.is_none() && !args.command.is_empty() {
        // Just run command with default scheduling
        print_out(b"Executing with default I/O scheduling: ");
        for (i, arg) in args.command.iter().enumerate() {
            if i > 0 {
                print_out(b" ");
            }
            print_out(arg);
        }
        print_out(b"\n");
        return 0;
    }

    if let Some(pid) = target_pid {
        if args.class.is_some() || args.classdata.is_some() {
            // Set I/O priority
            let class = args.class.unwrap_or(IOPRIO_CLASS_BE);
            let data = args.classdata.unwrap_or(4);

            if class == IOPRIO_CLASS_RT || class == IOPRIO_CLASS_BE {
                if data >= IOPRIO_MAX_PRIO {
                    print_err(b"ionice: classdata must be 0-7\n");
                    return 1;
                }
            }

            print_out(b"Setting I/O scheduling for PID ");
            print_out(&format_u64(pid as u64));
            print_out(b" to class ");
            print_out(ioclass_name(class));
            print_out(b" priority ");
            print_out(&format_u64(data as u64));
            print_out(b"\n");
        } else {
            // Show I/O priority
            // In real implementation: ioprio_get
            print_out(ioclass_name(IOPRIO_CLASS_BE));
            print_out(b": prio 4\n");
        }
    } else if !args.command.is_empty() {
        let class = args.class.unwrap_or(IOPRIO_CLASS_BE);
        let data = args.classdata.unwrap_or(4);

        print_out(b"Executing with I/O class ");
        print_out(ioclass_name(class));
        print_out(b" priority ");
        print_out(&format_u64(data as u64));
        print_out(b": ");
        for (i, arg) in args.command.iter().enumerate() {
            if i > 0 {
                print_out(b" ");
            }
            print_out(arg);
        }
        print_out(b"\n");
    } else {
        // Show current I/O priority of self
        print_out(ioclass_name(IOPRIO_CLASS_BE));
        print_out(b": prio 4\n");
    }

    0
}

// ── renice ─────────────────────────────────────────────────────────────

struct ReniceArgs {
    priority: Option<i32>,
    pids: Vec<u32>,
    pgids: Vec<u32>,
    users: Vec<Vec<u8>>,
    show_help: bool,
    show_version: bool,
}

fn parse_renice_args(args: &[Vec<u8>]) -> ReniceArgs {
    let mut result = ReniceArgs {
        priority: None,
        pids: Vec::new(),
        pgids: Vec::new(),
        users: Vec::new(),
        show_help: false,
        show_version: false,
    };

    let mut i = 0;
    let mut mode: u8 = b'p'; // default: process

    while i < args.len() {
        let arg = &args[i];
        if arg == b"-h" || arg == b"--help" {
            result.show_help = true;
        } else if arg == b"-V" || arg == b"--version" {
            result.show_version = true;
        } else if arg == b"-n" || arg == b"--priority" {
            i += 1;
            if i < args.len() {
                result.priority = parse_i32_bytes(&args[i]);
            }
        } else if arg == b"-p" || arg == b"--pid" {
            mode = b'p';
        } else if arg == b"-g" || arg == b"--pgrp" {
            mode = b'g';
        } else if arg == b"-u" || arg == b"--user" {
            mode = b'u';
        } else if !arg.starts_with(b"-")
            || (arg.len() > 1 && arg[0] == b'-' && arg[1].is_ascii_digit())
        {
            // Could be priority (first positional) or target
            if result.priority.is_none() {
                if let Some(p) = parse_i32_bytes(arg) {
                    result.priority = Some(p);
                } else {
                    // It's a target
                    add_renice_target(&mut result, mode, arg);
                }
            } else {
                add_renice_target(&mut result, mode, arg);
            }
        }
        i += 1;
    }

    result
}

fn add_renice_target(args: &mut ReniceArgs, mode: u8, value: &[u8]) {
    match mode {
        b'p' => {
            if let Some(pid) = parse_u32(value) {
                args.pids.push(pid);
            }
        }
        b'g' => {
            if let Some(pgid) = parse_u32(value) {
                args.pgids.push(pgid);
            }
        }
        b'u' => {
            args.users.push(value.to_vec());
        }
        _ => {}
    }
}

fn cmd_renice(args: &ReniceArgs) -> i32 {
    if args.show_help {
        print_out(b"Usage: renice [-n] priority [-p|-g|-u] id...\n\n");
        print_out(b"Alter the priority of running processes.\n\n");
        print_out(b"Options:\n");
        print_out(b"  -n, --priority N    specify nice value\n");
        print_out(b"  -p, --pid           interpret IDs as process IDs (default)\n");
        print_out(b"  -g, --pgrp          interpret IDs as process group IDs\n");
        print_out(b"  -u, --user          interpret IDs as user names/IDs\n");
        print_out(b"  -h, --help          display this help\n");
        print_out(b"  -V, --version       display version\n");
        return 0;
    }

    if args.show_version {
        print_out(b"renice (OurOS util-linux) 1.0.0\n");
        return 0;
    }

    let priority = match args.priority {
        Some(p) => {
            if p < NICE_MIN || p > NICE_MAX {
                print_err(b"renice: priority must be between -20 and 19\n");
                return 1;
            }
            p
        }
        None => {
            print_err(b"renice: no priority specified\n");
            return 1;
        }
    };

    if args.pids.is_empty() && args.pgids.is_empty() && args.users.is_empty() {
        print_err(b"renice: no target specified\n");
        return 1;
    }

    let mut errors = 0;

    for &pid in &args.pids {
        // In real implementation: setpriority(PRIO_PROCESS, pid, priority)
        print_out(&format_u64(pid as u64));
        print_out(b" (process ID) old priority 0, new priority ");
        print_out(&format_i32(priority));
        print_out(b"\n");
    }

    for &pgid in &args.pgids {
        print_out(&format_u64(pgid as u64));
        print_out(b" (process group ID) old priority 0, new priority ");
        print_out(&format_i32(priority));
        print_out(b"\n");
    }

    for user in &args.users {
        print_out(user);
        print_out(b" (user ID) old priority 0, new priority ");
        print_out(&format_i32(priority));
        print_out(b"\n");
    }

    errors
}

// ── Utility Functions ──────────────────────────────────────────────────

fn parse_u32(s: &[u8]) -> Option<u32> {
    let s = trim_bytes(s);
    if s.is_empty() {
        return None;
    }
    let mut result: u32 = 0;
    for &b in s {
        match b {
            b'0'..=b'9' => {
                result = result.checked_mul(10)?.checked_add((b - b'0') as u32)?;
            }
            _ => return None,
        }
    }
    Some(result)
}

fn parse_u64_bytes(s: &[u8]) -> Option<u64> {
    let s = trim_bytes(s);
    if s.is_empty() {
        return None;
    }
    let mut result: u64 = 0;
    for &b in s {
        match b {
            b'0'..=b'9' => {
                result = result.checked_mul(10)?.checked_add((b - b'0') as u64)?;
            }
            _ => return None,
        }
    }
    Some(result)
}

fn parse_i32_bytes(s: &[u8]) -> Option<i32> {
    let s = trim_bytes(s);
    if s.is_empty() {
        return None;
    }

    let (negative, digits) = if s[0] == b'-' {
        (true, &s[1..])
    } else if s[0] == b'+' {
        (false, &s[1..])
    } else {
        (false, s)
    };

    let mut result: i32 = 0;
    for &b in digits {
        match b {
            b'0'..=b'9' => {
                result = result.checked_mul(10)?.checked_add((b - b'0') as i32)?;
            }
            _ => return None,
        }
    }

    if negative {
        Some(-result)
    } else {
        Some(result)
    }
}

fn parse_usize(s: &[u8]) -> Option<usize> {
    let s = trim_bytes(s);
    if s.is_empty() {
        return None;
    }
    let mut result: usize = 0;
    for &b in s {
        match b {
            b'0'..=b'9' => {
                result = result.checked_mul(10)?.checked_add((b - b'0') as usize)?;
            }
            _ => return None,
        }
    }
    Some(result)
}

fn format_u64(mut n: u64) -> Vec<u8> {
    if n == 0 {
        return vec![b'0'];
    }
    let mut buf = Vec::new();
    while n > 0 {
        buf.push(b'0' + (n % 10) as u8);
        n /= 10;
    }
    buf.reverse();
    buf
}

fn format_i32(n: i32) -> Vec<u8> {
    if n < 0 {
        let mut buf = vec![b'-'];
        buf.extend_from_slice(&format_u64(n.unsigned_abs() as u64));
        buf
    } else {
        format_u64(n as u64)
    }
}

fn format_usize(n: usize) -> Vec<u8> {
    format_u64(n as u64)
}

fn trim_bytes(s: &[u8]) -> &[u8] {
    let start = s
        .iter()
        .position(|&b| b != b' ' && b != b'\t' && b != b'\r' && b != b'\n')
        .unwrap_or(s.len());
    let end = s
        .iter()
        .rposition(|&b| b != b' ' && b != b'\t' && b != b'\r' && b != b'\n')
        .map(|p| p + 1)
        .unwrap_or(start);
    if start >= end { &[] } else { &s[start..end] }
}

fn print_out(msg: &[u8]) {
    #[cfg(not(test))]
    {
        use std::io::Write;
        let _ = std::io::stdout().write_all(msg);
    }
    #[cfg(test)]
    {
        let _ = msg;
    }
}

fn print_err(msg: &[u8]) {
    #[cfg(not(test))]
    {
        use std::io::Write;
        let _ = std::io::stderr().write_all(msg);
    }
    #[cfg(test)]
    {
        let _ = msg;
    }
}

fn get_args() -> Vec<Vec<u8>> {
    #[cfg(not(test))]
    {
        std::env::args().map(|a| a.into_bytes()).collect()
    }
    #[cfg(test)]
    {
        Vec::new()
    }
}

// ── Entry Point ────────────────────────────────────────────────────────

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let args = get_args();
    if args.is_empty() {
        print_err(b"taskset: unable to determine program name\n");
        return 1;
    }

    let personality = detect_personality(&args[0]);
    let rest: Vec<Vec<u8>> = args.into_iter().skip(1).collect();

    match personality {
        Personality::Taskset => {
            let parsed = parse_taskset_args(&rest);
            cmd_taskset(&parsed)
        }
        Personality::Chrt => {
            let parsed = parse_chrt_args(&rest);
            cmd_chrt(&parsed)
        }
        Personality::Ionice => {
            let parsed = parse_ionice_args(&rest);
            cmd_ionice(&parsed)
        }
        Personality::Renice => {
            let parsed = parse_renice_args(&rest);
            cmd_renice(&parsed)
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Personality Detection ──────────────────────────────────

    #[test]
    fn test_detect_taskset() {
        assert_eq!(detect_personality(b"taskset"), Personality::Taskset);
        assert_eq!(
            detect_personality(b"/usr/bin/taskset"),
            Personality::Taskset
        );
    }

    #[test]
    fn test_detect_chrt() {
        assert_eq!(detect_personality(b"chrt"), Personality::Chrt);
        assert_eq!(detect_personality(b"/usr/bin/chrt"), Personality::Chrt);
    }

    #[test]
    fn test_detect_ionice() {
        assert_eq!(detect_personality(b"ionice"), Personality::Ionice);
    }

    #[test]
    fn test_detect_renice() {
        assert_eq!(detect_personality(b"renice"), Personality::Renice);
    }

    // ── CPU Mask ───────────────────────────────────────────────

    #[test]
    fn test_cpumask_new() {
        let mask = CpuMask::new(64);
        assert!(mask.is_empty());
        assert_eq!(mask.count(), 0);
    }

    #[test]
    fn test_cpumask_set_get() {
        let mut mask = CpuMask::new(64);
        mask.set(0);
        mask.set(3);
        assert!(mask.is_set(0));
        assert!(!mask.is_set(1));
        assert!(mask.is_set(3));
        assert_eq!(mask.count(), 2);
    }

    #[test]
    fn test_cpumask_clear() {
        let mut mask = CpuMask::new(64);
        mask.set(5);
        assert!(mask.is_set(5));
        mask.clear(5);
        assert!(!mask.is_set(5));
    }

    #[test]
    fn test_cpumask_from_hex() {
        let mask = CpuMask::from_hex(b"f").unwrap();
        assert!(mask.is_set(0));
        assert!(mask.is_set(1));
        assert!(mask.is_set(2));
        assert!(mask.is_set(3));
        assert!(!mask.is_set(4));
        assert_eq!(mask.count(), 4);
    }

    #[test]
    fn test_cpumask_from_hex_with_prefix() {
        let mask = CpuMask::from_hex(b"0xff").unwrap();
        assert_eq!(mask.count(), 8);
    }

    #[test]
    fn test_cpumask_from_hex_large() {
        let mask = CpuMask::from_hex(b"ff00").unwrap();
        assert!(!mask.is_set(0));
        assert!(mask.is_set(8));
        assert!(mask.is_set(15));
        assert_eq!(mask.count(), 8);
    }

    #[test]
    fn test_cpumask_from_list_single() {
        let mask = CpuMask::from_list(b"3").unwrap();
        assert!(mask.is_set(3));
        assert_eq!(mask.count(), 1);
    }

    #[test]
    fn test_cpumask_from_list_multiple() {
        let mask = CpuMask::from_list(b"0,2,4").unwrap();
        assert!(mask.is_set(0));
        assert!(!mask.is_set(1));
        assert!(mask.is_set(2));
        assert!(!mask.is_set(3));
        assert!(mask.is_set(4));
        assert_eq!(mask.count(), 3);
    }

    #[test]
    fn test_cpumask_from_list_range() {
        let mask = CpuMask::from_list(b"0-7").unwrap();
        assert_eq!(mask.count(), 8);
        for i in 0..8 {
            assert!(mask.is_set(i));
        }
        assert!(!mask.is_set(8));
    }

    #[test]
    fn test_cpumask_from_list_mixed() {
        let mask = CpuMask::from_list(b"0,2-4,8").unwrap();
        assert!(mask.is_set(0));
        assert!(!mask.is_set(1));
        assert!(mask.is_set(2));
        assert!(mask.is_set(3));
        assert!(mask.is_set(4));
        assert!(!mask.is_set(5));
        assert!(mask.is_set(8));
        assert_eq!(mask.count(), 5);
    }

    #[test]
    fn test_cpumask_to_hex() {
        let mut mask = CpuMask::new(64);
        mask.set(0);
        mask.set(1);
        mask.set(2);
        mask.set(3);
        assert_eq!(&mask.to_hex_string(), b"f");
    }

    #[test]
    fn test_cpumask_to_list() {
        let mut mask = CpuMask::new(64);
        mask.set(0);
        mask.set(1);
        mask.set(2);
        mask.set(3);
        assert_eq!(&mask.to_list_string(), b"0-3");
    }

    #[test]
    fn test_cpumask_to_list_gaps() {
        let mut mask = CpuMask::new(64);
        mask.set(0);
        mask.set(2);
        mask.set(4);
        assert_eq!(&mask.to_list_string(), b"0,2,4");
    }

    #[test]
    fn test_cpumask_to_list_mixed() {
        let mut mask = CpuMask::new(64);
        mask.set(0);
        mask.set(1);
        mask.set(2);
        mask.set(5);
        mask.set(6);
        assert_eq!(&mask.to_list_string(), b"0-2,5-6");
    }

    #[test]
    fn test_cpumask_empty_hex() {
        let mask = CpuMask::new(64);
        assert_eq!(&mask.to_hex_string(), b"0");
    }

    // ── taskset Args ───────────────────────────────────────────

    #[test]
    fn test_taskset_help() {
        let args = parse_taskset_args(&[b"-h".to_vec()]);
        assert!(args.show_help);
    }

    #[test]
    fn test_taskset_version() {
        let args = parse_taskset_args(&[b"-V".to_vec()]);
        assert!(args.show_version);
    }

    #[test]
    fn test_taskset_get_affinity() {
        assert_eq!(taskset_get_affinity(1, false), 0);
        assert_eq!(taskset_get_affinity(1, true), 0);
    }

    #[test]
    fn test_taskset_set_empty_mask() {
        let mask = CpuMask::new(64);
        assert_eq!(taskset_set_affinity(1, &mask, false), 1);
    }

    #[test]
    fn test_taskset_set_valid_mask() {
        let mut mask = CpuMask::new(64);
        mask.set(0);
        assert_eq!(taskset_set_affinity(1, &mask, false), 0);
    }

    // ── chrt Args ──────────────────────────────────────────────

    #[test]
    fn test_chrt_help() {
        let args = parse_chrt_args(&[b"-h".to_vec()]);
        assert!(args.show_help);
    }

    #[test]
    fn test_chrt_policy_flags() {
        let args = parse_chrt_args(&[b"-f".to_vec()]);
        assert_eq!(args.policy, Some(SCHED_FIFO));

        let args = parse_chrt_args(&[b"-r".to_vec()]);
        assert_eq!(args.policy, Some(SCHED_RR));

        let args = parse_chrt_args(&[b"-o".to_vec()]);
        assert_eq!(args.policy, Some(SCHED_OTHER));

        let args = parse_chrt_args(&[b"-b".to_vec()]);
        assert_eq!(args.policy, Some(SCHED_BATCH));

        let args = parse_chrt_args(&[b"-i".to_vec()]);
        assert_eq!(args.policy, Some(SCHED_IDLE));

        let args = parse_chrt_args(&[b"-d".to_vec()]);
        assert_eq!(args.policy, Some(SCHED_DEADLINE));
    }

    #[test]
    fn test_chrt_show_max() {
        assert_eq!(chrt_show_max(), 0);
    }

    #[test]
    fn test_chrt_get() {
        assert_eq!(chrt_get(1, false), 0);
        assert_eq!(chrt_get(1, true), 0);
    }

    #[test]
    fn test_policy_name() {
        assert_eq!(policy_name(SCHED_OTHER), b"SCHED_OTHER");
        assert_eq!(policy_name(SCHED_FIFO), b"SCHED_FIFO");
        assert_eq!(policy_name(SCHED_RR), b"SCHED_RR");
        assert_eq!(policy_name(SCHED_BATCH), b"SCHED_BATCH");
        assert_eq!(policy_name(SCHED_IDLE), b"SCHED_IDLE");
        assert_eq!(policy_name(SCHED_DEADLINE), b"SCHED_DEADLINE");
    }

    // ── ionice Args ────────────────────────────────────────────

    #[test]
    fn test_ionice_help() {
        let args = parse_ionice_args(&[b"-h".to_vec()]);
        assert!(args.show_help);
    }

    #[test]
    fn test_ionice_class_parse() {
        assert_eq!(parse_ionice_class(b"0"), Some(IOPRIO_CLASS_NONE));
        assert_eq!(parse_ionice_class(b"none"), Some(IOPRIO_CLASS_NONE));
        assert_eq!(parse_ionice_class(b"1"), Some(IOPRIO_CLASS_RT));
        assert_eq!(parse_ionice_class(b"realtime"), Some(IOPRIO_CLASS_RT));
        assert_eq!(parse_ionice_class(b"2"), Some(IOPRIO_CLASS_BE));
        assert_eq!(parse_ionice_class(b"best-effort"), Some(IOPRIO_CLASS_BE));
        assert_eq!(parse_ionice_class(b"3"), Some(IOPRIO_CLASS_IDLE));
        assert_eq!(parse_ionice_class(b"idle"), Some(IOPRIO_CLASS_IDLE));
        assert_eq!(parse_ionice_class(b"invalid"), None);
    }

    #[test]
    fn test_ioclass_name() {
        assert_eq!(ioclass_name(IOPRIO_CLASS_NONE), b"none");
        assert_eq!(ioclass_name(IOPRIO_CLASS_RT), b"realtime");
        assert_eq!(ioclass_name(IOPRIO_CLASS_BE), b"best-effort");
        assert_eq!(ioclass_name(IOPRIO_CLASS_IDLE), b"idle");
    }

    #[test]
    fn test_ionice_args_with_class() {
        let args = parse_ionice_args(&[
            b"-c".to_vec(),
            b"2".to_vec(),
            b"-n".to_vec(),
            b"4".to_vec(),
            b"-p".to_vec(),
            b"1234".to_vec(),
        ]);
        assert_eq!(args.class, Some(IOPRIO_CLASS_BE));
        assert_eq!(args.classdata, Some(4));
        assert_eq!(args.pid, Some(1234));
    }

    // ── renice Args ────────────────────────────────────────────

    #[test]
    fn test_renice_help() {
        let args = parse_renice_args(&[b"-h".to_vec()]);
        assert!(args.show_help);
    }

    #[test]
    fn test_renice_no_priority() {
        let args = ReniceArgs {
            priority: None,
            pids: vec![1],
            pgids: Vec::new(),
            users: Vec::new(),
            show_help: false,
            show_version: false,
        };
        assert_eq!(cmd_renice(&args), 1);
    }

    #[test]
    fn test_renice_no_target() {
        let args = ReniceArgs {
            priority: Some(5),
            pids: Vec::new(),
            pgids: Vec::new(),
            users: Vec::new(),
            show_help: false,
            show_version: false,
        };
        assert_eq!(cmd_renice(&args), 1);
    }

    #[test]
    fn test_renice_out_of_range() {
        let args = ReniceArgs {
            priority: Some(-21),
            pids: vec![1],
            pgids: Vec::new(),
            users: Vec::new(),
            show_help: false,
            show_version: false,
        };
        assert_eq!(cmd_renice(&args), 1);
    }

    #[test]
    fn test_renice_valid() {
        let args = ReniceArgs {
            priority: Some(10),
            pids: vec![1234],
            pgids: Vec::new(),
            users: Vec::new(),
            show_help: false,
            show_version: false,
        };
        assert_eq!(cmd_renice(&args), 0);
    }

    // ── Number Parsing ─────────────────────────────────────────

    #[test]
    fn test_parse_u32() {
        assert_eq!(parse_u32(b"0"), Some(0));
        assert_eq!(parse_u32(b"42"), Some(42));
        assert_eq!(parse_u32(b""), None);
        assert_eq!(parse_u32(b"abc"), None);
    }

    #[test]
    fn test_parse_i32() {
        assert_eq!(parse_i32_bytes(b"0"), Some(0));
        assert_eq!(parse_i32_bytes(b"-5"), Some(-5));
        assert_eq!(parse_i32_bytes(b"+10"), Some(10));
        assert_eq!(parse_i32_bytes(b""), None);
    }

    #[test]
    fn test_format_u64() {
        assert_eq!(format_u64(0), b"0");
        assert_eq!(format_u64(42), b"42");
    }

    #[test]
    fn test_format_i32() {
        assert_eq!(format_i32(0), b"0");
        assert_eq!(format_i32(10), b"10");
        assert_eq!(format_i32(-5), b"-5");
    }

    #[test]
    fn test_trim_bytes() {
        assert_eq!(trim_bytes(b"  hi  "), b"hi");
        assert_eq!(trim_bytes(b""), b"" as &[u8]);
    }
}
