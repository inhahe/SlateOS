//! Process/system information pseudo-filesystem (`/proc`).
//!
//! ProcFs is a read-only virtual filesystem that generates content on the fly
//! by querying kernel subsystems.  It provides system information to userspace
//! without adding special-purpose syscalls for every diagnostic need.
//!
//! ## Layout
//!
//! ```text
//! /proc/
//! ├── version        Kernel version string
//! ├── uptime         Uptime in seconds (decimal)
//! ├── meminfo        Memory statistics (key: value format)
//! ├── cpuinfo        CPU topology and features
//! ├── config         Kernel build configuration and enabled features
//! ├── mounts         Mounted filesystems
//! ├── stat           System-wide scheduler statistics
//! ├── filesystems    Available filesystem types
//! ├── cmdline        Kernel command line
//! ├── loadavg        Instantaneous system load
//! ├── cacheinfo      Buffer cache and VFS dcache statistics
//! ├── locks          Advisory file lock information
//! ├── fdinfo         Open file handle listing
//! ├── diskstats      Block device statistics
//! ├── interrupts     APIC timer and IRQ state
//! ├── devices        PCI device listing
//! ├── net            Network interface configuration
//! ├── vmstat         Virtual memory statistics (frames, swap, zram, OOM)
//! ├── buddyinfo      Buddy allocator free blocks per order
//! ├── swaps          Active swap devices with usage and priority
//! ├── fsstats        Per-filesystem debug statistics
//! ├── cas            Content-addressed store statistics
//! ├── integrity      File integrity monitoring statistics
//! ├── fhistory       File version history statistics
//! └── <pid>/         Per-process directories
//!     ├── status     Process name, state, priority, credentials
//!     ├── cmdline    Process command name (null-terminated)
//!     ├── stat       Single-line statistics (pid, name, state, ppid, ...)
//!     ├── maps       Virtual memory areas (PML4, threads)
//!     └── caps       Capability table and credentials
//! ```
//!
//! ## Design
//!
//! Content is generated fresh on every `read_file()` call — there is no
//! caching.  This keeps the implementation simple and ensures data is always
//! current.  The cost is acceptable: procfs reads are infrequent compared to
//! real I/O, and the generation functions are cheap (a few microseconds).
//!
//! Implements the [`FileSystem`] trait.  Write operations return
//! `NotSupported` (this is a read-only filesystem).

#![allow(dead_code)]

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::vfs::{DirEntry, EntryType, FileMeta, FileSystem, FsInfo};

// ---------------------------------------------------------------------------
// ProcFs implementation
// ---------------------------------------------------------------------------

/// Virtual filesystem exposing kernel and process information.
///
/// All content is generated dynamically — no persistent storage.
pub struct ProcFs;

impl ProcFs {
    /// Create a new ProcFs instance.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

/// Names of virtual files in the procfs root.
const ROOT_FILES: &[&str] = &[
    "version",
    "uptime",
    "meminfo",
    "cpuinfo",
    "config",
    "mounts",
    "stat",
    "filesystems",
    "cmdline",
    "loadavg",
    "cpucache",
    "locks",
    "fdinfo",
    "diskstats",
    "partitions",
    "interrupts",
    "devices",
    "net",
    "vmstat",
    "buddyinfo",
    "swaps",
    "fsstats",
    "heapinfo",
    "bcache",
    "cas",
    "integrity",
    "fhistory",
    "quotas",
    "security",
    "pipes",
    "overlays",
    "namespaces",
    "rlimits",
    "audit",
    "snapshots",
    "reclaim",
    "transactions",
    "certmgr",
    "installer",
    "changetrack",
    "fcompress",
    "encryption",
    "dedup",
    "search",
    "tags",
    "usage",
    "health",
    "dirsync",
    "backup",
    "undelete",
    "archives",
    "batch",
    "linkcheck",
    "profile",
    "fspolicy",
    "fsbench",
    "ioprio",
    "atime",
    "prefetch",
    "splice",
    "directio",
    "fstrim",
    "fstune",
    "fontmgr",
    "sparse",
    "readdir_plus",
    "freeze",
    "sealing",
    "recent",
    "fileinfo",
    "fswalk",
    "findex",
    "thumbcache",
    "bookmarks",
    "clipboard",
    "dragdrop",
    "contextmenu",
    "deskicons",
    "fileops",
    "fileselect",
    "filetype",
    "openwith",
    "preview",
    "sidebar",
    "statusbar",
    "templates",
    "toolbar",
    "queryable",
    "immutable",
    "fcomment",
    "rundialog",
    "notifcenter",
    "appregistry",
    "systray",
    "taskbar",
    "startmenu",
    "filepicker",
    "theme",
    "hotkeys",
    "widgets",
    "soundmixer",
    "wallpaper",
    "credentials",
    "power",
    "display",
    "vdesktop",
    "keylayout",
    "screenshot",
    "a11y",
    "ime",
    "netindicator",
    "winsnap",
    "colorpicker",
    "cursorsettings",
    "kbsettings",
    "detailcols",
    "partmgr",
    "locale",
    "useracct",
    "progmgr",
    "scriptlang",
    "osreset",
    "bootcfg",
    "swapcfg",
    "timezone",
    "autostart",
    "schedtune",
    "mmtune",
    "capsettings",
    "vpn",
    "dyndns",
    "loginscreen",
    "appnotify",
    "kernelbuild",
    "wakesensor",
    "netsettings",
    "sysinfo",
    "perfmon",
    "focusassist",
    "storageclean",
    "sysdiag",
    "nightlight",
    "tasksched",
    "envvars",
    "bluetooth",
    "printmgr",
    "screenrec",
    "datausage",
    "mousesettings",
    "touchpad",
    "powerprofile",
    "defaultapps",
    "monitors",
    "fwsettings",
    "updatemgr",
    "notifprefs",
    "fileshare",
    "parental",
    "audiodevice",
    "sessionmgr",
    "crashreport",
    "netproxy",
    "fileversion",
    "devicemgr",
    "location",
    "diskencrypt",
    "pkgmgr",
    "remotedesktop",
    "restorepoint",
    "battery",
    "dictation",
    "screenreader",
    "langpack",
    "spellcheck",
    "screentime",
    "disksmart",
    "magnifier",
    "cloudsync",
    "gestures",
    "soundevents",
    "usbmgr",
    "cliphistory",
    "displaycolor",
    "syslog",
    "inputa11y",
    "driverupdate",
    "netshare",
    "startuprepair",
    "remoteassist",
    "taskmon",
    "printqueue",
    "servicemgr",
    "hwmonitor",
    "appsandbox",
    "gamepadinput",
    "sysrestore",
    "audiomux",
    "netthrottle",
    "dumpanalyzer",
    "memdiag",
    "parentaltime",
    "mediakeys",
    "webcam",
    "speechio",
    "mobilelink",
    "screenlock",
    "appstore",
    "wintiling",
    "peninput",
    "brightness",
    "quicksettings",
    "volumeosd",
    "netdiag",
    "sharesheet",
    "oobe",
    "hdrdisplay",
    "surroundsound",
    "audioeq",
    "screensaver",
    "colortemp",
    "gamemode",
    "dpiscaling",
    "netprofile",
    "apppermissions",
    "kbshortcuts",
    "displayarrange",
    "sysanimations",
    "filevault",
    "mousegestures",
    "fontsettings",
    "notifbadge",
    "lockwallpaper",
    "systemsounds",
    "hotcorners",
    "dynlock",
    "snaplayout",
    "haptfeedback",
    "eyeprotect",
    "pinnedapps",
    "inputmethod",
    "storagesense",
    "autofix",
    "recentsearch",
    "sysmaint",
    "multiclip",
    "focussession",
    "quicknote",
    "colorscheme",
    "appcompat",
    "windowrules",
    "spatialaudio",
    "filetransfer",
    "startupopt",
    "usagetime",
    "voicecontrol",
    "devpair",
    "notifgroup",
    "playmedia",
    "kbmacro",
    "sysresource",
    "faceunlock",
    "usbpolicy",
    "applaunch",
    "sysprofiler",
    "clipsync",
    "netusage",
    "touchscreen",
    "diskquota",
    "appdefaults",
    "policyengine",
    "fontpreview",
    "wifiscan",
    "splitview",
    "iotdevice",
    "prochistory",
    "notiffilter",
    "colorblind",
    "clipaction",
    "energysaver",
    "filerules",
    "secureboot",
    "eventlog",
    "sysevents",
    "logpersist",
    "svcstart",
    "sockactivation",
    "drvmon",
    "reslimit",
    "initproc",
    "syshealth",
    "udriver",
    "hotplug",
    "devpower",
    "vmguest",
    "pciids",
    "upnp",
    "http",
    "ntp",
    "mdns",
    "telnet",
    "tftp",
    "netsyslog",
    "wol",
    "pcap",
    "traceroute",
    "dhcpv6",
    "firewall",
    "igmp",
    "mld",
    "lldp",
    "netstat",
    "ndisc",
    "netcat",
    "iperf",
    "snmp",
    "ftp",
    "smtp",
    "vlan",
    "qos",
    "socks",
    "bridge",
    "systemimage",
    "raidmgr",
    "networkbridge",
    "secureerase",
    "dnssettings",
    "backupsched",
    "displaycal",
    "vpnprofile",
    "diskhealth",
    "recoverypart",
    "userprofile",
    "diskclean",
    "acl",
    "associations",
    "logrotate",
    "powerwake",
    "diskio",
    "sysuptime",
    "netspeed",
    "cpufreq",
    "thermal",
    "swapmon",
    "sysctlfs",
    "cputopo",
    "memlayout",
    "irqbalance",
    "fs_loadavg",
    "kernlog",
    "coredump",
    "fwupdate",
    "timesync",
    "kmod",
    "entropy",
    "iosched",
    "netmon",
    "groupmgr",
    "sysrq",
    "telemetry",
    "fscache",
    "nameservice",
    "oomkiller",
    "blktrace",
    "cgroupfs",
    "secpolicy",
    "procstat",
    "kernparam",
    "tracemon",
    "authbroker",
    "prociso",
    "dmevent",
    "pftrack",
    "ipclog",
    "numastat",
    "shmem",
    "wqstat",
    "slabstat",
    "timerq",
    "fdtable",
    "rcustat",
    "kconsole",
    "signalq",
    "memcg",
    "tlbstat",
    "pagestat",
    "dmastat",
    "compstat",
    "irqstat",
    "epollstat",
    "vmmap",
    "softirq",
    "netfilter",
    "schedclass",
    "cpuidle",
    "futexstat",
    "writeback",
    "iolatency",
    "taskstats",
    "kprobes",
    "netsock",
    "blkqueue",
    "powerstat",
    "inodestat",
    "migstat",
    "pagecache",
    "netdev",
    "cpustat",
    "filelock",
    "pidstat",
    "binfmt",
    "pipestat",
    "sockbuf",
    "schedlat",
    "mempress",
    "cpucache",
    "aiostat",
    "kthread",
    "mmapstat",
    "rqstat",
    "thpstat",
    "cgiostat",
    "bpfstat",
    "pgtable",
    "zramstat",
    "ksmstat",
    "clocksrc",
    "pmcstat",
    "cputhr",
    "ipcns",
    "netqueue",
    "secmod",
    "vmballoon",
    "devfreq",
    "hwrng",
    "acpistat",
    "userfault",
    "ioport",
    "msivec",
    "cpuset",
    "ftrace",
    "kstack",
    "fnotify",
    "netlat",
    "diskstat",
    "taskio",
    "ttystat",
    "swapact",
    "schedwait",
    "ratestat",
    "iomem",
    "vmzone",
    "budstat",
    "cgmem",
    "vmfrag",
    "pidfd",
    "columnview",
    "pathbar",
    "viewstate",
    "properties",
];

/// Files in the `/proc/sys` sysctl tree, keyed by their path *under*
/// `/proc/sys` (e.g. `"kernel/osrelease"`).
///
/// Every value here mirrors real kernel state or a real ABI ceiling — the
/// `kernel/{ostype,osrelease,version,hostname,domainname}` entries are the
/// exact `uname(2)` surface (see [`crate::syscall::linux`]'s `sys_uname`),
/// `kernel/pid_max` is the real PID ceiling, `fs/nr_open` the real per-process
/// fd-table size, and `kernel/random/{uuid,boot_id}` are generated from the
/// kernel CSPRNG per Linux semantics — so nothing is fabricated.  The tree is
/// read-only (procfs only honours writes to `oom_score_adj`).
const SYS_FILES: &[&str] = &[
    "kernel/ostype",
    "kernel/osrelease",
    "kernel/version",
    "kernel/hostname",
    "kernel/domainname",
    "kernel/pid_max",
    "kernel/random/uuid",
    "kernel/random/boot_id",
    "kernel/random/poolsize",
    "kernel/random/entropy_avail",
    "vm/overcommit_memory",
    "fs/nr_open",
];

/// Directories in the `/proc/sys` tree, keyed by their path under `/proc/sys`
/// (`""` denotes `/proc/sys` itself).  Used to answer `stat`/`readdir` on the
/// interior directories of the sysctl tree.
const SYS_DIRS: &[&str] = &["", "kernel", "kernel/random", "vm", "fs"];

/// Names of virtual files inside each `/proc/<pid>/` directory.
const PID_FILES: &[&str] = &[
    "status",
    "cmdline",
    "stat",
    "maps",
    "caps",
    "comm",
    "statm",
    "limits",
    "environ",
    "auxv",
    "mountinfo",
    "mounts",
    "cgroup",
    "cpuset",
    "oom_score",
    "oom_score_adj",
    "schedstat",
    "loginuid",
    "sessionid",
    "io",
];

/// Per-PID symbolic links, served via [`FileSystem::readlink`].
///
/// These mirror Linux's magic links inside `/proc/<pid>/`:
/// - `cwd`  → the process's current working directory.
/// - `root` → the process's filesystem root, reported as `/`.  A process
///   sees its *own* root as `/` regardless of any container jail (a jailed
///   container process's root prefix is invisible to it — that is the point
///   of the jail), so `/` is correct from the process's own perspective,
///   matching what Linux reports for `/proc/self/root` inside a container.
///   (We do not resolve another process's root *as seen by the caller* — a
///   host process reading a container's `/proc/<pid>/root` still gets `/`
///   rather than the container's host-side rootfs path; a minor fidelity gap
///   that never leaks the host mount topology.)
/// - `exe`  → the resolved absolute path of the executable image,
///   captured at spawn/`exec` time (empty until the process has loaded a
///   binary, in which case the link reports `NotFound`).
const PID_LINKS: &[&str] = &["cwd", "root", "exe"];

/// Files exposed inside each `/proc/<pid>/task/<tid>/` thread directory.
///
/// Linux's per-thread directory mirrors most of the per-process file set,
/// but we expose only the files we can render **truthfully per-thread**
/// from the scheduler task alone, with no process/thread field mixing:
///
/// - `comm` — the thread's name (threads may differ from the process and
///   from each other; `prctl(PR_SET_NAME)` is per-thread).
/// - `schedstat` — the thread's own CPU time, run-queue wait, and dispatch
///   count, all from real per-task accounting.
///
/// `stat`/`status` are intentionally omitted for now: their `ppid`,
/// `tgid`, and `num_threads` fields need the owning process's context
/// (a thread tid is not a process-table key), so serving them here would
/// either fabricate those fields or require threading the owner pid
/// through the per-thread generators — tracked as a follow-up in
/// `todo.txt` rather than shipped wrong.
const TASK_FILES: &[&str] = &["comm", "schedstat", "stat", "status"];

// ---------------------------------------------------------------------------
// Content generators
//
// Each function generates the content for one virtual file.  They query
// kernel subsystems and format the result as human-readable text.
// ---------------------------------------------------------------------------

/// `/proc/version` — kernel version and build info.
fn gen_version() -> Vec<u8> {
    // Keep this consistent with any future version syscall.
    let text = "MintOS kernel 0.1.0 (Rust, x86_64, 16 KiB pages)\n".to_string();
    text.into_bytes()
}

/// `/proc/uptime` — system uptime and total idle time, Linux format.
///
/// Follows Linux `fs/proc/uptime.c`: two space-separated fields, each in
/// seconds with centisecond (2-decimal) precision:
///
///   `<uptime_seconds> <idle_seconds>`
///
/// The second field is the sum of idle time across ALL CPUs (so on an N-CPU
/// machine it can be up to N× the uptime). It is sourced honestly from
/// [`crate::cputime`], which performs real per-CPU TSC idle accounting hooked
/// into the live idle path — never fabricated. `uptime::ProcessUptime` and
/// strict two-field parsers (`sscanf "%lf %lf"`) rely on both fields existing.
fn gen_uptime() -> Vec<u8> {
    let ns = crate::hpet::elapsed_ns();
    let secs = ns / 1_000_000_000;
    let centis = (ns % 1_000_000_000) / 10_000_000;

    // Total idle across all CPUs (summed), from real per-CPU TSC accounting.
    let idle_ns = crate::cputime::aggregate_stats().idle_ns;
    let idle_secs = idle_ns / 1_000_000_000;
    let idle_centis = (idle_ns % 1_000_000_000) / 10_000_000;

    let text = format!("{secs}.{centis:02} {idle_secs}.{idle_centis:02}\n");
    text.into_bytes()
}

/// `/proc/meminfo` — memory statistics in `key: value` format.
///
/// Modelled after Linux's `/proc/meminfo` but with our own field names
/// reflecting our memory subsystem (16 KiB frames, zero-page pool, slab heap).
fn gen_meminfo() -> Vec<u8> {
    let info = crate::mm::memory_info();
    let mut s = String::with_capacity(512);

    // Total / free / used in KiB (matching Linux convention).
    let total_kib = info.total_bytes / 1024;
    let free_kib = info.free_bytes / 1024;
    let used_kib = info.used_bytes / 1024;

    s.push_str(&format!("MemTotal:       {total_kib} kB\n"));
    s.push_str(&format!("MemFree:        {free_kib} kB\n"));
    s.push_str(&format!("MemUsed:        {used_kib} kB\n"));
    s.push_str(&format!("Frames:         {} total, {} free\n",
        info.total_frames, info.free_frames));

    // Zero-page pool.
    s.push_str(&format!("ZeroPool:       {} pages\n", info.zero_pool_count));
    s.push_str(&format!("ZeroPoolHits:   {}\n", info.zero_pool_hits));
    s.push_str(&format!("ZeroPoolMisses: {}\n", info.zero_pool_misses));

    // Heap allocator.
    s.push_str(&format!("HeapSlabAllocs: {}\n", info.heap_slab_allocs));
    s.push_str(&format!("HeapSlabFrees:  {}\n", info.heap_slab_frees));
    s.push_str(&format!("HeapLargeAllocs:{}\n", info.heap_large_allocs));
    s.push_str(&format!("HeapAllocFails: {}\n", info.heap_alloc_failures));

    // Swap.
    let swap_total_kib = info.swap_total_bytes / 1024;
    let swap_used_kib = info.swap_used_bytes / 1024;
    s.push_str(&format!("SwapTotal:      {swap_total_kib} kB\n"));
    s.push_str(&format!("SwapUsed:       {swap_used_kib} kB\n"));
    s.push_str(&format!("SwapDevices:    {}\n", info.swap_device_count));

    // OOM.
    s.push_str(&format!("OomEvents:      {}\n", info.oom_events));
    s.push_str(&format!("OomKills:       {}\n", info.oom_kills));

    // kswapd.
    s.push_str(&format!("KswapdRunning:  {}\n", info.kswapd_running));
    s.push_str(&format!("KswapdCycles:   {}\n", info.kswapd_reclaim_cycles));
    s.push_str(&format!("KswapdReclaimed:{}\n", info.kswapd_total_reclaimed));

    s.into_bytes()
}

/// Append the Linux-style `flags` line for the given CPU feature set.
///
/// The key uses a trailing tab + colon to match Linux's `%s\t: %s`
/// formatting so that tools splitting on `:` recover the value cleanly.
fn push_cpu_flags(s: &mut String, f: &crate::cpu::CpuFeatures) {
    s.push_str("flags\t\t:");
    // Baseline x86_64 features that are architecturally guaranteed.
    s.push_str(" fpu vme de pse tsc msr pae mce cx8 apic sep mtrr pge mca cmov");
    s.push_str(" pat pse36 clflush mmx fxsr");
    if f.sse       { s.push_str(" sse"); }
    if f.sse2      { s.push_str(" sse2"); }
    s.push_str(" ht syscall nx lm");
    if f.sse3      { s.push_str(" pni"); }
    if f.ssse3     { s.push_str(" ssse3"); }
    if f.sse4_1    { s.push_str(" sse4_1"); }
    if f.sse4_2    { s.push_str(" sse4_2"); }
    if f.popcnt    { s.push_str(" popcnt"); }
    if f.aes_ni    { s.push_str(" aes"); }
    if f.xsave     { s.push_str(" xsave"); }
    if f.avx       { s.push_str(" avx"); }
    if f.f16c      { s.push_str(" f16c"); }
    if f.rdrand    { s.push_str(" rdrand"); }
    if f.fxsr      { s.push_str(" fxsr_opt"); }
    if f.page_1g   { s.push_str(" pdpe1gb"); }
    if f.rdtscp    { s.push_str(" rdtscp"); }
    if f.bmi1      { s.push_str(" bmi1"); }
    if f.avx2      { s.push_str(" avx2"); }
    if f.bmi2      { s.push_str(" bmi2"); }
    if f.rdseed    { s.push_str(" rdseed"); }
    if f.sha       { s.push_str(" sha_ni"); }
    if f.avx512f   { s.push_str(" avx512f"); }
    if f.vaes      { s.push_str(" vaes"); }
    if f.rdpid     { s.push_str(" rdpid"); }
    s.push('\n');
}

/// `/proc/cpuinfo` — per-processor topology and features (Linux format).
///
/// Emits one Linux-style block per online logical CPU, each beginning with a
/// `processor\t: N` line.  This matches what build tools (`grep -c ^processor`),
/// glibc, and native utilities (lscpu, hwinfo) expect; the previous custom
/// format (a header block plus `acpi_id`/`apic_id` keys) was miscounted as an
/// extra CPU by block-counting parsers and omitted keys consumers look for.
fn gen_cpuinfo() -> Vec<u8> {
    let processors = crate::acpi::processors();
    // List only enabled (online) processors; fall back to a single synthetic
    // BSP entry when no MADT was parsed.
    let mut apics: Vec<u8> = processors
        .iter()
        .filter(|p| p.enabled)
        .map(|p| p.apic_id)
        .collect();
    if apics.is_empty() {
        apics.push(0);
    }
    let count = apics.len();

    // Identity, shared across all logical CPUs (we model a single package).
    let vendor = crate::cpu::vendor_string();
    let vendor_str = core::str::from_utf8(&vendor).unwrap_or("unknown");
    let (family, model, stepping) = crate::cpu::cpu_family_model_stepping();
    let brand = crate::cpu::brand_string();
    let brand_str = core::str::from_utf8(&brand)
        .unwrap_or("")
        .trim_matches(|c: char| c == '\0' || c == ' ');
    let model_name = if brand_str.is_empty() {
        "Unknown CPU"
    } else {
        brand_str
    };

    // Clock from the calibrated TSC frequency (Hz → MHz with 3 decimals).
    let tsc_freq = crate::bench::tsc_freq();
    let (mhz_int, mhz_frac) = if tsc_freq > 0 {
        (tsc_freq / 1_000_000, (tsc_freq % 1_000_000) / 1000)
    } else {
        (0, 0)
    };
    // BogoMIPS: classic 2× clock approximation.
    let (bogo_int, bogo_frac) = (mhz_int.saturating_mul(2), mhz_frac);

    // Last-level cache size (largest detected cache), reported in KB.
    let cache_kb = crate::cpu::cache_topology()
        .iter()
        .map(|c| c.size)
        .max()
        .unwrap_or(0)
        / 1024;
    let clflush = crate::cpu::cache_line_size();

    let mut s = String::with_capacity(1024);
    for (i, &apic_id) in apics.iter().enumerate() {
        s.push_str(&format!("processor\t: {i}\n"));
        s.push_str(&format!("vendor_id\t: {vendor_str}\n"));
        s.push_str(&format!("cpu family\t: {family}\n"));
        s.push_str(&format!("model\t\t: {model}\n"));
        s.push_str(&format!("model name\t: {model_name}\n"));
        s.push_str(&format!("stepping\t: {stepping}\n"));
        s.push_str(&format!("cpu MHz\t\t: {mhz_int}.{mhz_frac:03}\n"));
        if cache_kb > 0 {
            s.push_str(&format!("cache size\t: {cache_kb} KB\n"));
        }
        s.push_str("physical id\t: 0\n");
        s.push_str(&format!("siblings\t: {count}\n"));
        s.push_str(&format!("core id\t\t: {i}\n"));
        s.push_str(&format!("cpu cores\t: {count}\n"));
        s.push_str(&format!("apicid\t\t: {apic_id}\n"));
        s.push_str(&format!("initial apicid\t: {apic_id}\n"));
        s.push_str("fpu\t\t: yes\n");
        s.push_str("fpu_exception\t: yes\n");
        s.push_str("wp\t\t: yes\n");
        if let Some(f) = crate::cpu::features() {
            push_cpu_flags(&mut s, f);
        }
        s.push_str(&format!("bogomips\t: {bogo_int}.{bogo_frac:03}\n"));
        s.push_str(&format!("clflush size\t: {clflush}\n"));
        s.push_str(&format!("cache_alignment\t: {clflush}\n"));
        s.push_str("power management:\n");
        s.push('\n');
    }

    s.into_bytes()
}

/// `/proc/config` — kernel build configuration and enabled features.
///
/// Reports architecture, page size, subsystem limits, and which filesystem
/// and network features are compiled in.  Uses real constants from the
/// codebase where available, hardcoded values for private constants.
fn gen_config() -> Vec<u8> {
    let mut s = String::with_capacity(512);

    s.push_str("# Kernel Configuration\n");
    s.push_str("ARCH=x86_64\n");
    s.push_str(&format!("PAGE_SIZE={}\n", crate::mm::frame::FRAME_SIZE));
    s.push_str(&format!("MAX_CPUS={}\n", crate::sched::priority_rr::MAX_CPUS));
    s.push_str("PREEMPTION=yes\n");

    // Memory subsystems.
    s.push_str("SWAP=yes\n");
    s.push_str("ZRAM=yes\n");

    // Filesystems.
    s.push_str("EXT4=yes\n");
    s.push_str("FAT=yes\n");
    s.push_str("ISO9660=yes\n");
    s.push_str("MEMFS=yes\n");
    s.push_str("PROCFS=yes\n");
    s.push_str("DEVFS=yes\n");
    s.push_str("SYSFS=yes\n");

    // Drivers.
    s.push_str("VIRTIO_BLK=yes\n");
    s.push_str("VIRTIO_NET=yes\n");

    // Networking.
    s.push_str("TCP=yes\n");
    s.push_str("UDP=yes\n");
    s.push_str("DHCP=yes\n");
    s.push_str("DNS=yes\n");

    // Subsystem limits.
    // cache::MAX_ENTRIES is private (2048), hardcoded here.
    s.push_str("BUFFER_CACHE_SECTORS=2048\n");
    s.push_str(&format!("VFS_DCACHE_SIZE={}\n", super::vfs::VFS_DCACHE_SIZE));

    s.into_bytes()
}

/// `/proc/mounts` — mounted filesystems.
///
/// Format matches Linux `show_vfsmnt` (`fs/proc_namespace.c`):
/// `source mount_point fstype options dump pass` per line, where `dump`
/// and `pass` are always `0`.  The source is `none` (we track no backing
/// device); the mount point and fstype are escaped with the same
/// `mangle()`-equivalent that `mountinfo` uses (see [`mangle_mount_field`])
/// so whitespace in a mount point does not corrupt the space-separated
/// layout for `getmntent`/`mount(8)`.
fn gen_mounts() -> Vec<u8> {
    // Reading `/proc/mounts` from inside a container must show the container's
    // own mounts, not the host's global table (same info-leak/correctness
    // reasoning as `/proc/<pid>/mountinfo`). Resolve the caller's view.
    let global = crate::fs::Vfs::mounts_full();
    if let Some(view) =
        crate::ipc::namespace::mount_view_for(crate::sched::current_task_id())
    {
        return render_container_mounts(&view, &global);
    }
    render_global_mounts(&global)
}

/// Render the global VFS mount table in the `/proc/mounts` line format
/// (`source mount_point fstype options 0 0`).
fn render_global_mounts(
    mounts: &[(String, String, crate::fs::vfs::MountOptions)],
) -> Vec<u8> {
    let mut s = String::with_capacity(256);
    for (path, fs_type, options) in mounts {
        let opts = options.to_string();
        let mount_point = mangle_mount_field(path);
        let fstype = mangle_mount_field(fs_type);
        s.push_str(&format!("none {mount_point} {fstype} {opts} 0 0\n"));
    }
    s.into_bytes()
}

/// Render a *container* process's own mount view in the `/proc/mounts` line
/// format.  Companion to [`render_container_mountinfo`]: same view (rootfs at
/// guest `/`, then each volume/tmpfs), fstype resolved from the backing host
/// mount, `source` hidden (`none`) so host paths are not leaked, and the
/// `rw`/`ro` option taken from the container's own read-only view.
fn render_container_mounts(
    view: &[crate::ipc::namespace::MountViewEntry],
    global: &[(String, String, crate::fs::vfs::MountOptions)],
) -> Vec<u8> {
    let mut s = String::with_capacity(view.len().saturating_mul(64).max(16));
    for entry in view {
        let mount_point = mangle_mount_field(&entry.guest_path);
        let fstype = mangle_mount_field(fstype_for_host_path(&entry.host_target, global));
        let opts = if entry.read_only { "ro" } else { "rw" };
        s.push_str(&format!("none {mount_point} {fstype} {opts} 0 0\n"));
    }
    s.into_bytes()
}

/// `/proc/<pid>/mounts` — the per-process view of [`gen_mounts`].
///
/// Linux exposes `/proc/<pid>/mounts` (and `/proc/self/mounts`) as the
/// mount-namespace-local table; ours mirrors that so a container process reads
/// its own mounts.  Gated on process existence like the other per-PID files.
fn gen_pid_mounts(task_id: u64) -> KernelResult<Vec<u8>> {
    if crate::proc::pcb::state(task_id).is_none() {
        return Err(KernelError::NotFound);
    }
    let global = crate::fs::Vfs::mounts_full();
    if let Some(view) = crate::ipc::namespace::mount_view_for(task_id) {
        return Ok(render_container_mounts(&view, &global));
    }
    Ok(render_global_mounts(&global))
}

/// `/proc/stat` — system-wide kernel/scheduler statistics in Linux format.
///
/// Follows Linux `fs/proc/stat.c` `show_stat()`. All fields are backed by
/// honest data sources — fields the kernel does not yet track are reported as
/// zero rather than fabricated:
///
/// - `cpu`/`cpuN` jiffy columns come from [`crate::cputime`], which performs
///   real per-CPU TSC-precision accounting hooked into the live ISR, idle, and
///   softirq paths. We track a four-way split (system / idle / irq / softirq),
///   so the `user`, `nice`, `iowait`, `steal`, `guest`, and `guest_nice`
///   columns are honestly zero — we do not yet separate user-vs-kernel CPU time.
/// - `intr` total is the real hardware-IRQ count; per-IRQ breakdown is omitted
///   (we do not yet keep a per-vector histogram here).
/// - `ctxt` is the sum of every task's `schedule_count` (real dispatch count).
/// - `btime` is the wall-clock boot epoch from [`crate::timekeeping`].
/// - `processes` is the cumulative fork/create count from [`crate::proc::pcb`].
/// - `procs_running` / `procs_blocked` are live scheduler state counts.
/// - `softirq` total is the real softirq dispatch count.
fn gen_stat() -> Vec<u8> {
    use crate::sched::task::TaskState;

    // Linux reports CPU time in USER_HZ jiffies. We use a 100 Hz tick, so one
    // jiffy is 10 ms = 10_000_000 ns. Genuinely-untracked columns stay zero.
    const NS_PER_JIFFY: u64 = 10_000_000;

    // Emit one `user nice system idle iowait irq softirq steal guest guest_nice`
    // row from a CpuTimeStats sample. We track only the system/idle/irq/softirq
    // split, so the remaining columns are honestly zero.
    fn push_cpu_row(s: &mut String, label: &str, st: &crate::cputime::CpuTimeStats) {
        let system = st.system_ns / NS_PER_JIFFY;
        let idle = st.idle_ns / NS_PER_JIFFY;
        let irq = st.irq_ns / NS_PER_JIFFY;
        let softirq = st.softirq_ns / NS_PER_JIFFY;
        // user nice system idle iowait irq softirq steal guest guest_nice
        s.push_str(&format!(
            "{label} 0 0 {system} {idle} 0 {irq} {softirq} 0 0 0\n"
        ));
    }

    let mut s = String::with_capacity(512);

    // Aggregate line uses the label "cpu" followed by TWO spaces (Linux quirk).
    let agg = crate::cputime::aggregate_stats();
    push_cpu_row(&mut s, "cpu ", &agg);

    // Per-CPU lines: "cpuN" with a single space.
    for (cpu, st) in crate::cputime::all_cpu_stats() {
        push_cpu_row(&mut s, &format!("cpu{cpu}"), &st);
    }

    // Hardware interrupt total. Per-IRQ breakdown is omitted (not tracked here).
    s.push_str(&format!("intr {}\n", agg.irq_count));

    // Context switches: sum of real per-task dispatch counts.
    let tasks = crate::sched::task_list();
    let ctxt: u64 = tasks.iter().map(|t| t.schedule_count).sum();
    s.push_str(&format!("ctxt {ctxt}\n"));

    // Boot wall-clock time (seconds since the Unix epoch).
    s.push_str(&format!("btime {}\n", crate::timekeeping::boot_time_epoch_secs()));

    // Cumulative processes created (forks) since boot.
    s.push_str(&format!("processes {}\n", crate::proc::pcb::processes_created()));

    // Live scheduler state: runnable vs. blocked.
    let mut procs_running = 0u64;
    let mut procs_blocked = 0u64;
    for t in &tasks {
        match t.state {
            TaskState::Running | TaskState::Ready => {
                procs_running = procs_running.saturating_add(1);
            }
            TaskState::Blocked => procs_blocked = procs_blocked.saturating_add(1),
            TaskState::Suspended | TaskState::Dead => {}
        }
    }
    s.push_str(&format!("procs_running {procs_running}\n"));
    s.push_str(&format!("procs_blocked {procs_blocked}\n"));

    // Softirq total dispatch count. Per-type breakdown omitted (not tracked here).
    s.push_str(&format!("softirq {}\n", agg.softirq_count));

    s.into_bytes()
}

/// `/proc/filesystems` — list of available filesystem types.
///
/// Format follows Linux: `nodev <type>` for virtual filesystems,
/// plain `<type>` for disk-backed ones.
fn gen_filesystems() -> Vec<u8> {
    let mut s = String::with_capacity(256);

    // Virtual filesystems (no backing block device).
    s.push_str("nodev\tmemfs\n");
    s.push_str("nodev\tprocfs\n");
    s.push_str("nodev\tdevfs\n");
    s.push_str("nodev\tsysfs\n");

    // Disk-backed filesystems.
    s.push_str("\text4\n");
    s.push_str("\tfat\n");
    s.push_str("\tiso9660\n");

    s.into_bytes()
}

/// `/proc/cmdline` — kernel command line.
///
/// Reports a synthetic command line reflecting the boot configuration.
/// In the future, this could read actual bootloader-provided arguments.
fn gen_cmdline() -> Vec<u8> {
    // Build a synthetic cmdline from boot state.
    let cpu_count = crate::acpi::processor_count();
    let text = format!(
        "kernel=mintos cpus={cpu_count} pages=16k\n"
    );
    text.into_bytes()
}

/// `/proc/loadavg` — system load average approximation.
///
/// Reports the number of runnable (ready + running) tasks as an
/// instantaneous load metric.  True exponentially-weighted load
/// averages (1/5/15 min) would require periodic sampling in the
/// scheduler; for now, the snapshot is useful for monitoring.
fn gen_loadavg() -> Vec<u8> {
    let tasks = crate::sched::task_list();

    use crate::sched::task::TaskState;

    // Linux /proc/loadavg fields (fs/proc/loadavg.c show_loadavg):
    //   "<load1> <load5> <load15> <runnable>/<total> <last_pid>\n"
    //
    // The three load figures are the scheduler's 1/5/15-minute EWMAs in
    // Linux fixed-point form, formatted as <int>.<2-digit-frac> exactly as
    // Linux does (LOAD_INT / LOAD_FRAC).  Previously this stuffed the
    // instantaneous runnable count into all three slots, which is
    // meaningless to any consumer (uptime/top/w) expecting time-averaged
    // load — now they get genuine moving averages.
    let (l1, l5, l15) = crate::sched::load_averages_fixed();

    let runnable = tasks.iter()
        .filter(|t| matches!(t.state, TaskState::Running | TaskState::Ready))
        .count();
    let total = tasks.len();
    let last_pid = tasks.iter().map(|t| t.id).max().unwrap_or(0);

    let text = format!(
        "{}.{:02} {}.{:02} {}.{:02} {runnable}/{total} {last_pid}\n",
        crate::sched::load_int(l1), crate::sched::load_frac(l1),
        crate::sched::load_int(l5), crate::sched::load_frac(l5),
        crate::sched::load_int(l15), crate::sched::load_frac(l15),
    );
    text.into_bytes()
}

/// `/proc/cacheinfo` — buffer cache statistics.
#[allow(clippy::arithmetic_side_effects)]
fn gen_cacheinfo() -> Vec<u8> {
    let stats = super::cache::stats();
    let hit_rate = if stats.reads > 0 {
        (stats.hits as f64 / stats.reads as f64) * 100.0
    } else {
        0.0
    };

    // VFS path resolution cache (dcache) stats.
    let (dcache_hits, dcache_misses, dcache_valid) = super::vfs::Vfs::dcache_stats();
    let dcache_hit_rate = {
        let total = dcache_hits.saturating_add(dcache_misses);
        if total > 0 {
            (dcache_hits as f64 / total as f64) * 100.0
        } else {
            0.0
        }
    };

    let text = format!(
        "--- buffer cache ---\n\
         reads:        {}\n\
         hits:         {}\n\
         misses:       {}\n\
         hit_rate:     {:.1}%\n\
         writes:       {}\n\
         writebacks:   {}\n\
         readaheads:   {}\n\
         entries_used: {}/{}\n\
         entries_dirty:{}\n\
         --- vfs dcache ---\n\
         dcache_hits:  {}\n\
         dcache_misses:{}\n\
         dcache_valid: {}/{}\n\
         dcache_rate:  {:.1}%\n",
        stats.reads,
        stats.hits,
        stats.misses,
        hit_rate,
        stats.writes,
        stats.writebacks,
        stats.readaheads,
        stats.entries_used,
        stats.capacity,
        stats.entries_dirty,
        dcache_hits,
        dcache_misses,
        dcache_valid,
        super::vfs::VFS_DCACHE_SIZE,
        dcache_hit_rate,
    );
    text.into_bytes()
}

/// `/proc/locks` — advisory file lock information.
fn gen_locks() -> Vec<u8> {
    // Query the lock table directly via Vfs internal.
    // We can use lock_query for individual paths, but for a full dump
    // we need to access the table.  Use a simpler approach: just report
    // that the lock subsystem is active.
    let mut text = String::from("LOCK  TYPE       OWNER    PATH\n");

    // Access the global lock table through a helper on Vfs.
    let lock_info = super::vfs::lock_table_dump();
    if lock_info.is_empty() {
        text.push_str("(no active locks)\n");
    } else {
        for (path, lock_type, owner) in &lock_info {
            let type_str = match lock_type {
                super::vfs::LockType::Shared => "SHARED   ",
                super::vfs::LockType::Exclusive => "EXCLUSIVE",
            };
            text.push_str(&format!("FLOCK {} {:>8}  {}\n", type_str, owner, path));
        }
    }
    text.into_bytes()
}

/// `/proc/diskstats` — block device statistics.
fn gen_diskstats() -> Vec<u8> {
    let devices = crate::blkdev::list_devices_full();
    let cache_stats = super::cache::stats();

    let mut text = String::from("DEVICE     SECTORS      SIZE         RO    CACHE\n");

    if devices.is_empty() {
        text.push_str("(no block devices)\n");
    } else {
        for dev in &devices {
            // Calculate size from sector count.
            let bytes = dev.sector_count.saturating_mul(dev.sector_size as u64);
            let size_str = if bytes >= 1_073_741_824 {
                format!("{} GiB", bytes / 1_073_741_824)
            } else if bytes >= 1_048_576 {
                format!("{} MiB", bytes / 1_048_576)
            } else if bytes >= 1024 {
                format!("{} KiB", bytes / 1024)
            } else {
                format!("{} B", bytes)
            };

            let ro_str = if dev.read_only { "yes" } else { "no" };

            text.push_str(&format!(
                "{:<10} {:<12} {:<12} {:<5} {}/{}\n",
                dev.name,
                dev.sector_count,
                size_str,
                ro_str,
                cache_stats.entries_used,
                cache_stats.capacity,
            ));
        }
    }

    // Cache summary.
    let hit_rate = if cache_stats.reads > 0 {
        cache_stats.hits.saturating_mul(100) / cache_stats.reads
    } else {
        0
    };
    text.push_str(&format!(
        "\nBuffer cache: {} hits / {} reads ({}% hit rate), {} readaheads\n",
        cache_stats.hits, cache_stats.reads, hit_rate, cache_stats.readaheads,
    ));

    // Device I/O activity tracking.
    let io = crate::blkdev::io_stats();
    let idle_secs = if io.last_io_tick > 0 {
        let elapsed = crate::apic::tick_count().saturating_sub(io.last_io_tick);
        elapsed / 100 // ~100 Hz timer
    } else {
        0
    };
    text.push_str(&format!(
        "Device I/O: {} reads, {} writes, idle {} sec\n",
        io.total_reads, io.total_writes, idle_secs,
    ));

    text.into_bytes()
}

/// `/proc/partitions` — block device partitions.
///
/// Matches Linux format: `major minor #blocks name`.
/// Since our OS doesn't yet support partitions, each device is listed
/// as a whole-disk entry with major 254 (virtio).
fn gen_partitions() -> Vec<u8> {
    let devices = crate::blkdev::list_devices_full();

    let mut text = String::from("major minor  #blocks  name\n\n");

    for (i, dev) in devices.iter().enumerate() {
        // Calculate size in 1 KiB blocks (Linux convention).
        let kib_blocks = dev.sector_count
            .saturating_mul(dev.sector_size as u64)
            / 1024;
        text.push_str(&format!(
            " 254    {:>4}  {:>8}  {}\n",
            i, kib_blocks, dev.name,
        ));
    }

    text.into_bytes()
}

/// `/proc/fdinfo` — open file handle information.
fn gen_fdinfo() -> Vec<u8> {
    let handles = super::handle::list_handles();
    let mut text = "HANDLE  FLAGS  OFFSET       SIZE         PATH\n".to_string();

    if handles.is_empty() {
        text.push_str("(no open handles)\n");
    } else {
        for h in &handles {
            // Decode flags into a compact string.
            let mut flags_str = String::new();
            if h.flags & 0x01 != 0 { flags_str.push('R'); }
            if h.flags & 0x02 != 0 { flags_str.push('W'); }
            if h.flags & 0x04 != 0 { flags_str.push('C'); }
            if h.flags & 0x08 != 0 { flags_str.push('T'); }
            if h.flags & 0x10 != 0 { flags_str.push('A'); }
            if flags_str.is_empty() { flags_str.push('-'); }

            text.push_str(&format!(
                "{:<7} {:<5} {:<12} {:<12} {}\n",
                h.id, flags_str, h.offset, h.size, h.path,
            ));
        }
    }

    text.push_str(&format!("\nTotal: {} open handles\n", handles.len()));
    text.into_bytes()
}

/// `/proc/interrupts` — interrupt statistics and IOAPIC IRQ state.
///
/// Reports APIC timer tick count, ISR latency measurements, and
/// per-IRQ pending state for standard x86 IRQ lines.
fn gen_interrupts() -> Vec<u8> {
    let mut text = String::with_capacity(512);

    // APIC timer statistics.
    let ticks = crate::apic::tick_count();
    text.push_str(&format!("APIC timer ticks: {ticks}\n"));

    // ISR latency measurements (if sampling was active).
    if let Some(isr) = crate::apic::isr_measurement_results() {
        text.push_str(&format!(
            "ISR latency:  min={} max={} mean={} cycles ({} samples)\n",
            isr.min_cycles, isr.max_cycles, isr.mean_cycles, isr.count,
        ));
    } else {
        text.push_str("ISR latency:  (no measurements)\n");
    }

    // Per-IRQ pending state from IOAPIC.
    text.push_str("\nIRQ  PENDING  DESCRIPTION\n");
    let irq_descs: &[(u32, &str)] = &[
        (0, "PIT timer / HPET"),
        (1, "Keyboard (PS/2)"),
        (2, "Cascade (PIC2)"),
        (3, "COM2 / Serial"),
        (4, "COM1 / Serial"),
        (6, "Floppy disk"),
        (8, "RTC / CMOS"),
        (9, "ACPI SCI"),
        (11, "PCI / AHCI"),
        (12, "PS/2 mouse"),
        (14, "Primary ATA"),
        (15, "Secondary ATA"),
    ];

    for &(irq, desc) in irq_descs {
        let pending = if crate::ioapic::irq_is_pending(irq) { "yes" } else { "no " };
        text.push_str(&format!("{:<4} {:<8} {}\n", irq, pending, desc));
    }

    text.into_bytes()
}

/// `/proc/devices` — PCI device listing.
///
/// Scans PCI bus 0 and reports all discovered devices with their
/// bus/device/function address, class/subclass codes, and vendor:device IDs.
fn gen_devices() -> Vec<u8> {
    let mut text = String::from("BUS  DEV  FN   CLASS:SUB  VENDOR:DEVICE\n");

    let devices = crate::pci::scan_bus0();
    if devices.is_empty() {
        text.push_str("(no PCI devices found)\n");
    } else {
        for dev in &devices {
            text.push_str(&format!(
                "{:02x}   {:02x}   {:02x}   {:02x}:{:02x}      {:04x}:{:04x}\n",
                dev.address.bus, dev.address.device, dev.address.function,
                dev.class, dev.subclass, dev.vendor_id, dev.device_id,
            ));
        }
        text.push_str(&format!("\n{} devices total\n", devices.len()));
    }

    text.into_bytes()
}

/// `/proc/net` — network interface information.
///
/// Reports the primary network interface's MAC, IP, netmask, gateway,
/// and DNS configuration.  Uses `interface::info()` to get all fields
/// in a single consistent snapshot.
fn gen_net() -> Vec<u8> {
    let mut text = String::with_capacity(256);

    // Get a consistent snapshot of all interface state.
    let ni = crate::net::interface::info();

    let up_str = if ni.up { "UP" } else { "DOWN" };
    text.push_str(&format!("Interface: eth0  ({})\n", up_str));
    // MacAddress is a newtype around [u8; 6]; access via .0[i].
    text.push_str(&format!("  MAC:     {}\n", ni.mac)); // Display impl formats as hex
    text.push_str(&format!("  IPv4:    {}\n", ni.ip));
    text.push_str(&format!("  Netmask: {}\n", ni.subnet_mask));
    text.push_str(&format!("  Gateway: {}\n", ni.gateway));
    text.push_str(&format!("  DNS:     {}\n", ni.dns));

    text.into_bytes()
}

/// `/proc/vmstat` — virtual memory statistics.
///
/// Summarizes page fault handling, swap activity, and frame allocator
/// state.  Useful for diagnosing memory pressure and swap storms.
fn gen_vmstat() -> Vec<u8> {
    let info = crate::mm::memory_info();

    let mut s = String::with_capacity(512);

    // Frame allocator.
    s.push_str(&format!("nr_free_frames {}\n", info.free_frames));
    s.push_str(&format!("nr_total_frames {}\n", info.total_frames));

    // Zero page pool.
    s.push_str(&format!("nr_zero_pool {}\n", info.zero_pool_count));
    s.push_str(&format!("zero_pool_hits {}\n", info.zero_pool_hits));
    s.push_str(&format!("zero_pool_misses {}\n", info.zero_pool_misses));

    // Heap allocator.
    s.push_str(&format!("heap_slab_allocs {}\n", info.heap_slab_allocs));
    s.push_str(&format!("heap_slab_frees {}\n", info.heap_slab_frees));
    s.push_str(&format!("heap_large_allocs {}\n", info.heap_large_allocs));
    s.push_str(&format!("heap_alloc_failures {}\n", info.heap_alloc_failures));

    // Swap.
    let swap_free = crate::mm::swap::free_slots();
    let swap_used = crate::mm::swap::used_slots();
    s.push_str(&format!("swap_free_slots {swap_free}\n"));
    s.push_str(&format!("swap_used_slots {swap_used}\n"));

    // Compression.
    let comp = crate::mm::swap::compression_stats();
    s.push_str(&format!("zram_compressed_bytes {}\n", comp.compressed_bytes));
    s.push_str(&format!("zram_uncompressed_bytes {}\n", comp.uncompressed_bytes));
    s.push_str(&format!("zram_compressed_pages {}\n", comp.compressed_count));
    s.push_str(&format!("zram_uncompressed_pages {}\n", comp.uncompressed_count));
    if comp.uncompressed_bytes > 0 {
        s.push_str(&format!("zram_ratio_pct {}\n", comp.ratio_percent()));
        s.push_str(&format!("zram_bytes_saved {}\n", comp.bytes_saved()));
    }

    // kswapd.
    s.push_str(&format!("kswapd_running {}\n", if info.kswapd_running { 1 } else { 0 }));
    s.push_str(&format!("kswapd_reclaim_cycles {}\n", info.kswapd_reclaim_cycles));
    s.push_str(&format!("kswapd_total_reclaimed {}\n", info.kswapd_total_reclaimed));

    // OOM.
    s.push_str(&format!("oom_events {}\n", info.oom_events));
    s.push_str(&format!("oom_kills {}\n", info.oom_kills));

    s.into_bytes()
}

/// `/proc/buddyinfo` — buddy allocator free block counts per order.
///
/// Each line shows how many free blocks exist at each order level.
/// Order 0 = 1 frame (16 KiB), order 1 = 2 frames (32 KiB), etc.
/// This is essential for diagnosing memory fragmentation.
fn gen_buddyinfo() -> Vec<u8> {
    match crate::mm::frame::stats() {
        Some(stats) => {
            // Linux fs/proc layout (mm/vmstat.c frag_show):
            //   "Node <id>, zone <name> <free_order0> <free_order1> ... <free_orderN>"
            // one line per memory zone, each column the number of free blocks
            // of that buddy order.  We expose a single Node 0 / zone Normal with
            // our 16 KiB base page and MAX_ORDER=10 (11 columns, orders 0..=10).
            //
            // Matches Linux's column spacing ("Node %d, zone %8s " then "%6lu "
            // per order) so that tools parsing /proc/buddyinfo by whitespace
            // (e.g. procps, fragmentation monitors) read it correctly.  Counts
            // come straight from the buddy allocator's per-order free lists
            // (mm::frame::stats().order_counts) — never fabricated.  The old
            // format appended non-Linux "# Order sizes"/"# Total free frames"
            // comment lines that would corrupt any strict parser; those are
            // dropped (the same totals are available via /proc/meminfo).
            let mut s = String::with_capacity(128);
            s.push_str(&format!("Node 0, zone {:>8} ", "Normal"));
            for count in &stats.order_counts {
                s.push_str(&format!("{count:>6} "));
            }
            s.push('\n');
            s.into_bytes()
        }
        None => b"(frame allocator not initialized)\n".to_vec(),
    }
}

/// `/proc/fsstats` — per-filesystem debug statistics.
///
/// Iterates all mounted filesystems and calls their `debug_stats()` method,
/// concatenating the results.  Useful for monitoring filesystem internals
/// (extent counts, inode usage, cache states, etc.) in a single read.
fn gen_fsstats() -> Vec<u8> {
    let mounts = crate::fs::Vfs::mounts();
    let mut s = String::with_capacity(512);

    for (mount_path, fs_type) in &mounts {
        s.push_str(&format!("--- {} ({}) ---\n", mount_path, fs_type));
        match crate::fs::Vfs::debug_stats(mount_path) {
            Ok(stats) if !stats.is_empty() => {
                s.push_str(&stats);
                if !stats.ends_with('\n') {
                    s.push('\n');
                }
            }
            Ok(_) => {
                s.push_str("(no stats)\n");
            }
            Err(_) => {
                s.push_str("(unavailable)\n");
            }
        }
    }

    if mounts.is_empty() {
        s.push_str("(no filesystems mounted)\n");
    }

    s.into_bytes()
}

/// `/proc/heapinfo` — kernel heap allocator statistics.
///
/// Shows slab allocator and large-allocation counters, refill
/// count, and failure count.  Useful for diagnosing memory
/// allocation patterns and detecting heap pressure.
#[allow(clippy::arithmetic_side_effects)]
fn gen_heapinfo() -> Vec<u8> {
    let stats = crate::mm::heap::stats();
    let mut s = String::with_capacity(512);

    s.push_str("Kernel Heap Statistics\n");
    s.push_str("---------------------\n");

    // Slab allocator stats (small allocations, per-CPU fast path).
    let slab_active = stats.slab_allocs.saturating_sub(stats.slab_frees);
    s.push_str(&format!(
        "slab_allocs:    {}\n\
         slab_frees:     {}\n\
         slab_active:    {} (allocs - frees)\n\
         slab_refills:   {}\n",
        stats.slab_allocs, stats.slab_frees, slab_active, stats.slab_refills,
    ));

    // Large allocation stats (buddy allocator path, >512 bytes).
    let large_active = stats.large_allocs.saturating_sub(stats.large_frees);
    s.push_str(&format!(
        "large_allocs:   {}\n\
         large_frees:    {}\n\
         large_active:   {} (allocs - frees)\n",
        stats.large_allocs, stats.large_frees, large_active,
    ));

    // Failure and total stats.
    let total_allocs = stats.slab_allocs.saturating_add(stats.large_allocs);
    let total_frees = stats.slab_frees.saturating_add(stats.large_frees);
    s.push_str(&format!(
        "total_allocs:   {}\n\
         total_frees:    {}\n\
         alloc_failures: {}\n",
        total_allocs, total_frees, stats.alloc_failures,
    ));

    s.into_bytes()
}

/// `/proc/bcache` — buffer cache statistics.
///
/// Shows hit/miss rates, dirty/clean entries, read-ahead stats,
/// and overall cache utilization.
fn gen_bcache() -> Vec<u8> {
    let stats = super::cache::stats();

    let mut s = String::with_capacity(512);
    s.push_str("Buffer Cache Statistics\n");
    s.push_str("----------------------\n");

    // Hit rate calculation.
    let total_io = stats.reads;
    let hit_rate = if total_io > 0 {
        (stats.hits * 100) / total_io
    } else {
        0
    };

    s.push_str(&format!(
        "reads:        {}\n\
         hits:         {} ({}%)\n\
         misses:       {}\n\
         writes:       {}\n\
         writebacks:   {}\n\
         readaheads:   {}\n\
         exp_flushes:  {}\n",
        stats.reads,
        stats.hits, hit_rate,
        stats.misses,
        stats.writes,
        stats.writebacks,
        stats.readaheads,
        stats.expired_flushes,
    ));

    s.push_str(&format!(
        "entries_used: {}/{}\n\
         entries_dirty:{}/{}\n",
        stats.entries_used, stats.capacity,
        stats.entries_dirty, stats.capacity,
    ));

    // Utilization percentage.
    let util = if stats.capacity > 0 {
        (stats.entries_used * 100) / stats.capacity
    } else {
        0
    };
    let dirty_pct = if stats.capacity > 0 {
        (stats.entries_dirty * 100) / stats.capacity
    } else {
        0
    };
    s.push_str(&format!(
        "utilization:  {}%\n\
         dirty_pct:    {}%\n",
        util, dirty_pct,
    ));

    s.into_bytes()
}

/// `/proc/swaps` — active swap devices, Linux-compatible format.
///
/// Shows each swap device's type, capacity, usage, and priority.
fn gen_swaps() -> Vec<u8> {
    let devices = crate::mm::swap::list_devices();

    let mut s = String::with_capacity(256);
    // Header matching Linux's /proc/swaps format.
    s.push_str("Filename\t\t\tType\t\tSize\tUsed\tPriority\n");

    if devices.is_empty() {
        // No swap devices.
        return s.into_bytes();
    }

    for dev in &devices {
        // Size/used in KiB (1 slot = 1 frame = 16 KiB).
        let size_kib = (dev.total_slots as u64).saturating_mul(16);
        let used_kib = (dev.used_slots as u64).saturating_mul(16);
        s.push_str(&format!(
            "{}\t\t\t{}\t\t{}\t{}\t{}\n",
            dev.name, dev.device_type, size_kib, used_kib, dev.priority
        ));
    }

    s.into_bytes()
}

/// `/proc/cas` — Content-addressed store statistics.
///
/// Shows blob count, total bytes, deduplication hits, GC stats,
/// and capacity.
fn gen_cas() -> Vec<u8> {
    let st = super::cas::stats();

    let mut s = String::with_capacity(512);
    s.push_str("Content-Addressed Store\n");
    s.push_str("----------------------\n");

    let util_pct = if st.max_bytes > 0 {
        (st.total_bytes * 100) / st.max_bytes
    } else {
        0
    };

    s.push_str(&format!(
        "blob_count:         {}\n\
         total_bytes:        {} ({} / {} = {}%)\n\
         total_refs:         {}\n\
         dedup_hits:         {}\n\
         gc_collected:       {}\n\
         integrity_failures: {}\n",
        st.blob_count,
        st.total_bytes, st.total_bytes, st.max_bytes, util_pct,
        st.total_refs,
        st.dedup_hits,
        st.gc_collected,
        st.integrity_failures,
    ));

    s.into_bytes()
}

/// `/proc/integrity` — File integrity monitoring statistics.
///
/// Shows baseline entry count, configuration, and operation counts.
fn gen_integrity() -> Vec<u8> {
    let st = super::integrity::stats();

    let mut s = String::with_capacity(512);
    s.push_str("File Integrity Monitor\n");
    s.push_str("---------------------\n");

    s.push_str(&format!(
        "baseline_entries:    {}\n\
         max_entries:         {}\n\
         max_file_size:       {}\n\
         baseline_operations: {}\n\
         verify_operations:   {}\n",
        st.baseline_entries,
        st.max_entries,
        st.max_file_size,
        st.baseline_count,
        st.verify_count,
    ));

    if st.baseline_timestamp > 0 {
        let secs = st.baseline_timestamp / 1_000_000_000;
        s.push_str(&format!("last_baseline:       {}s after boot\n", secs));
    } else {
        s.push_str("last_baseline:       never\n");
    }

    s.into_bytes()
}

/// `/proc/fhistory` — File version history statistics.
///
/// Shows tracked file count, total versions, eviction stats,
/// and operation counters.
fn gen_fhistory() -> Vec<u8> {
    let st = super::history::stats();

    let mut s = String::with_capacity(512);
    s.push_str("File Version History\n");
    s.push_str("--------------------\n");

    s.push_str(&format!(
        "enabled:            {}\n\
         auto_version:       {}\n\
         tracked_files:      {}\n\
         total_versions:     {}\n\
         evicted_versions:   {}\n\
         record_operations:  {}\n\
         restore_operations: {}\n",
        if st.enabled { "yes" } else { "no" },
        if st.auto_version { "yes" } else { "no" },
        st.tracked_files,
        st.total_versions,
        st.evicted_versions,
        st.record_count,
        st.restore_count,
    ));

    s.into_bytes()
}

/// `/proc/quotas` — Filesystem quota status.
///
/// Shows global quota enforcement status and per-subject usage/limits.
fn gen_quotas() -> Vec<u8> {
    let st = super::quota::stats();
    let all = super::quota::list_all();

    let mut s = String::with_capacity(1024);
    s.push_str("Filesystem Quotas\n");
    s.push_str("-----------------\n");
    s.push_str(&format!(
        "enforcement: {}\n\
         entries:     {}\n\
         user_quotas: {}\n\
         group_quotas:{}\n\
         over_soft:   {}\n\
         over_hard:   {}\n",
        if st.enabled { "yes" } else { "no" },
        st.entries,
        st.user_quotas,
        st.group_quotas,
        st.over_soft,
        st.over_hard,
    ));

    if !all.is_empty() {
        s.push_str("\nSubject      Bytes Used   Soft Limit   Hard Limit   Files  Status\n");
        for info in &all {
            let subj = match info.subject {
                super::quota::QuotaSubject::User(uid) => format!("user:{}", uid),
                super::quota::QuotaSubject::Group(gid) => format!("group:{}", gid),
            };
            let status = if info.over_hard_bytes || info.over_hard_inodes {
                "OVER_HARD"
            } else if info.over_soft_bytes || info.over_soft_inodes {
                "over_soft"
            } else {
                "ok"
            };
            s.push_str(&format!("{:<12} {:>12} {:>12} {:>12} {:>6} {}\n",
                subj,
                super::quota::format_bytes(info.usage.bytes_used),
                if info.limits.soft_bytes > 0 {
                    super::quota::format_bytes(info.limits.soft_bytes)
                } else {
                    String::from("-")
                },
                if info.limits.hard_bytes > 0 {
                    super::quota::format_bytes(info.limits.hard_bytes)
                } else {
                    String::from("-")
                },
                info.usage.inodes_used,
                status,
            ));
        }
    }

    s.into_bytes()
}

/// `/proc/security` — Security posture summary.
///
/// Consolidates capability system status, IOMMU protection,
/// namespace isolation, file tags, audit trail, and pending
/// capability requests into a single overview.
fn gen_security() -> Vec<u8> {
    let mut s = String::with_capacity(1024);
    s.push_str("Security Posture\n");
    s.push_str("================\n\n");

    // --- IOMMU ---
    s.push_str("[IOMMU / DMA Protection]\n");
    let iommu_available = crate::iommu::is_available();
    s.push_str(&format!(
        "  status:             {}\n",
        if iommu_available { "active" } else { "not detected" }
    ));
    if iommu_available {
        s.push_str(&format!(
            "  vendor:             {:?}\n\
               units:              {}\n",
            crate::iommu::vendor(),
            crate::iommu::unit_count(),
        ));
        let remap = crate::iommu_remap::stats();
        s.push_str(&format!(
            "  dma_remapping:      {}\n\
               active_domains:     {}\n\
               mapped_pages:       {}\n\
               dma_faults:         {}\n",
            if remap.active { "enabled" } else { "disabled" },
            remap.active_domains,
            remap.total_mapped_pages,
            remap.total_faults,
        ));
    }
    s.push('\n');

    // --- CET (Control-flow Enforcement) ---
    let cet = crate::cet::status();
    s.push_str("[Control-flow Enforcement (CET)]\n");
    s.push_str(&format!(
        "  shadow_stack_hw:    {}\n\
           ibt_hw:             {}\n\
           supervisor_shstk:   {}\n\
           supervisor_ibt:     {}\n\
           cp_exceptions:      {}\n",
        if cet.hw_shstk { "supported" } else { "not available" },
        if cet.hw_ibt { "supported" } else { "not available" },
        if cet.supervisor_shstk { "active" } else { "inactive" },
        if cet.supervisor_ibt { "active" } else { "inactive" },
        cet.cp_exceptions,
    ));
    s.push('\n');

    // --- Capability Audit ---
    let audit = crate::cap::audit::stats();
    s.push_str("[Capability Audit]\n");
    s.push_str(&format!(
        "  auditing:           {}\n\
           total_events:       {}\n\
           grants:             {}\n\
           denials:            {}\n\
           revocations:        {}\n\
           ring_entries:       {} / 128\n",
        if audit.enabled { "enabled" } else { "disabled" },
        audit.total_events,
        audit.total_grants,
        audit.total_denials,
        audit.total_revokes,
        audit.ring_entries,
    ));
    s.push('\n');

    // --- Capability Groups ---
    let group_count = crate::cap::groups::count();
    s.push_str("[Capability Groups]\n");
    s.push_str(&format!("  defined_groups:     {}\n", group_count));
    // List groups briefly.
    let groups = crate::cap::groups::list();
    for (id, name, member_count, _max, enabled) in &groups {
        s.push_str(&format!(
            "  group[{}]:           {} (members: {}, {})\n",
            id,
            name,
            member_count,
            if *enabled { "active" } else { "disabled" },
        ));
    }
    s.push('\n');

    // --- File Tags ---
    let file_tag_count = crate::cap::file_tags::count();
    s.push_str("[File Capability Tags]\n");
    s.push_str(&format!("  tagged_paths:       {}\n", file_tag_count));
    s.push('\n');

    // --- Capability Requests ---
    let pending = crate::cap::request::pending_count();
    s.push_str("[Capability Requests]\n");
    s.push_str(&format!("  pending_requests:   {}\n", pending));
    s.push('\n');

    // --- Process Namespaces ---
    let ns_count = crate::ipc::namespace::active_count();
    s.push_str("[Process Namespaces]\n");
    s.push_str(&format!("  active_namespaces:  {}\n", ns_count));
    s.push('\n');

    // --- Overall Assessment ---
    s.push_str("[Assessment]\n");
    let mut issues: u32 = 0;
    if !iommu_available {
        s.push_str("  WARNING: No IOMMU — DMA attacks possible from PCI devices\n");
        issues += 1;
    }
    if !audit.enabled {
        s.push_str("  WARNING: Capability auditing disabled\n");
        issues += 1;
    }
    if !cet.supervisor_shstk && cet.hw_shstk {
        s.push_str("  NOTE: CET shadow stacks available but not enabled\n");
    }
    if !cet.hw_shstk {
        s.push_str("  INFO: Hardware CET not available (pre-11th gen or QEMU)\n");
    }
    if audit.total_denials > 0 {
        s.push_str(&format!(
            "  NOTE: {} capability denial(s) recorded — review audit log\n",
            audit.total_denials,
        ));
    }
    if issues == 0 {
        s.push_str("  All security subsystems operational\n");
    }

    s.into_bytes()
}

/// `/proc/<pid>/status` — per-task status in Linux `/proc/<pid>/status`
/// format (key/value lines, tab-separated).
///
/// Uses the field names and ordering from Linux `fs/proc/array.c`
/// `proc_pid_status()` so key-based parsers (`ps`, `htop`, glibc, WINE)
/// read it correctly.  Values are populated from the scheduler task and
/// the process control block; fields the native kernel does not track are
/// reported as the Linux default (0 / unset), which is exactly what a real
/// kernel reports for a task with no such activity.  The `State:` mapping
/// and `Tgid`/`Pid` values are kept consistent with [`gen_pid_stat`] and
/// the `getpid`/`getuid`/`getpgid` syscalls so the files never disagree.
fn gen_pid_status(task_id: u64) -> KernelResult<Vec<u8>> {
    let tasks = crate::sched::task_list();
    let task = tasks.iter().find(|t| t.id == task_id)
        .ok_or(KernelError::NotFound)?;
    Ok(build_pid_status(task, task_id))
}

/// `/proc/<pid>/task/<tid>/status` — per-thread status.
///
/// Same key/value layout as [`gen_pid_status`], but the thread-specific
/// fields (Name, State, Pid, context switches) come from the thread's
/// scheduler task `tid`, while the process-wide fields (Tgid, PPid, Umask,
/// Uid/Gid/Groups, Vm*, Threads) come from the owning process `proc_id`.
/// This matches Linux's `task/<tid>/status` (Pid != Tgid for non-leader
/// threads) and is strictly more correct than serving `gen_pid_status(tid)`
/// (which would key the process-wide fields off the non-process tid).
fn gen_thread_status(proc_id: u64, tid: u64) -> KernelResult<Vec<u8>> {
    let tasks = crate::sched::task_list();
    let task = tasks.iter().find(|t| t.id == tid)
        .ok_or(KernelError::NotFound)?;
    Ok(build_pid_status(task, proc_id))
}

/// Format a bitmap (`u64` mask, sized to `nbits` significant bits) the way the
/// Linux kernel's `%*pb` (`bitmap_string`) conversion does — used for the
/// `/proc/<pid>/status` `Cpus_allowed:` and `Mems_allowed:` hex bitmaps.  The
/// bitmap is printed in comma-separated 32-bit groups most-significant-first;
/// the top (possibly partial) group is zero-padded to `ceil(top_bits / 4)` hex
/// digits and every lower group to 8.  Examples: `(0xff, 8) -> "ff"`,
/// `(1, 12) -> "001"`, `(u64::MAX, 64) -> "ffffffff,ffffffff"`,
/// `(all_40, 40) -> "ff,ffffffff"`, `(1, 1) -> "1"`, `(3, 2) -> "3"`.  Pure (no
/// locks), so it is unit-tested directly with synthetic masks in `self_test`.
fn format_bitmap_hex(mask: u64, nbits_in: usize) -> String {
    use core::fmt::Write as _;
    // Our mask is a u64, so cap the width at 64; size to at least one bit
    // because Linux always emits a group (the bitmap width is >= 1).
    let nbits = nbits_in.clamp(1, 64);
    // Drop any stray bits beyond the sized width, as the kernel does.
    let mask = if nbits >= 64 {
        mask
    } else {
        mask & (1u64 << nbits).wrapping_sub(1)
    };
    // Bits in the most-significant chunk: nbits mod 32, or a full 32 when
    // nbits is a positive multiple of 32.
    let top_bits = match nbits % 32 {
        0 => 32,
        r => r,
    };
    let top_width = top_bits.div_ceil(4);
    let nchunks = nbits.div_ceil(32); // 1 or 2 for a u64 mask
    let mut s = String::new();
    let mut chunk = nchunks;
    let mut first = true;
    while chunk > 0 {
        chunk = chunk.saturating_sub(1);
        let shift = (chunk.saturating_mul(32)) as u32;
        let group = ((mask >> shift) & 0xffff_ffff) as u32;
        let width = if first { top_width } else { 8 };
        let _ = write!(s, "{group:0width$x}");
        if chunk > 0 {
            s.push(',');
        }
        first = false;
    }
    s
}

/// Format a bitmap as Linux's range list (used for `Cpus_allowed_list:` and
/// `Mems_allowed_list:`), e.g. `(0xff, 8) -> "0-7"`, `(0b1101, 8) -> "0,2-3"`,
/// `(1, 8) -> "0"`, `(0, 8) -> ""`, `(1, 1) -> "0"`.  Pure; unit-tested in
/// `self_test`.
fn format_bitmap_list(mask: u64, nbits_in: usize) -> String {
    use core::fmt::Write as _;
    let nbits = nbits_in.clamp(0, 64);
    let mut s = String::new();
    let mut cpu = 0usize;
    let mut first = true;
    while cpu < nbits {
        if (mask >> cpu) & 1 == 1 {
            let start = cpu;
            // Extend the run while the next bit is also set and in range.
            while cpu.saturating_add(1) < nbits && (mask >> cpu.saturating_add(1)) & 1 == 1 {
                cpu = cpu.saturating_add(1);
            }
            if !first {
                s.push(',');
            }
            first = false;
            if start == cpu {
                let _ = write!(s, "{start}");
            } else {
                let _ = write!(s, "{start}-{cpu}");
            }
        }
        cpu = cpu.saturating_add(1);
    }
    s
}

/// Build a Linux `/proc/<pid>/status` body.
///
/// Thread-specific fields come from `task` (Name, State, Pid = `task.id`,
/// context switches); process-wide fields come from `proc_id` (Tgid, PPid,
/// Umask, Uid/Gid/Groups, Vm*, Threads).  For a process's own
/// `/proc/<pid>/status`, the caller passes `proc_id == task.id` so Pid ==
/// Tgid and the two id sources coincide; for a thread's `task/<tid>/status`
/// they differ (`task.id == tid`, `proc_id == owning pid`), so Pid is the
/// thread id while Tgid is the process id — exactly as Linux reports.
fn build_pid_status(task: &crate::sched::TaskInfo, proc_id: u64) -> Vec<u8> {
    use crate::sched::task::TaskState;
    use core::fmt::Write as _;

    // Linux's `Name:` is sourced from get_task_comm() — the 16-byte `comm`
    // field — so it is truncated to TASK_COMM_LEN-1 (15 bytes), exactly like
    // /proc/<pid>/comm and /proc/<pid>/stat field 2.  Use the shared helper so
    // all three surfaces agree (a tool cross-referencing Name: against comm
    // must see the same string).
    let full_name = core::str::from_utf8(task.name.get(..task.name_len).unwrap_or(&[]))
        .unwrap_or("???");
    let name = comm_truncate(full_name);

    // Linux `State:` is "<char> (<word>)".  Mirror exactly the single-char
    // mapping used by /proc/<pid>/stat (see build_pid_stat) so the two files
    // never disagree about a task's state.
    let state = match task.state {
        TaskState::Running | TaskState::Ready => "R (running)", // Ready == runnable
        TaskState::Blocked => "S (sleeping)",
        TaskState::Suspended => "T (stopped)",
        TaskState::Dead => "Z (zombie)",
    };

    let ppid = crate::proc::pcb::parent(proc_id).unwrap_or(0);
    let num_threads = crate::proc::pcb::get_threads(proc_id).map_or(1, |t| t.len());
    let creds = crate::proc::pcb::get_credentials(proc_id);
    let (uid, gid) = creds.as_ref().map_or((0, 0), |c| (c.uid, c.gid));

    // Field names and order follow Linux fs/proc/array.c proc_pid_status()
    // closely enough for key-based parsers (ps, htop, glibc, WINE).  Values
    // the native kernel does not track are reported as the Linux default,
    // exactly as a real kernel reports for a task with no such activity —
    // these are genuinely-zero values, not placeholders.  All field
    // separators are a single tab, matching real /proc/<pid>/status.
    let mut s = String::with_capacity(512);
    // `write!` into a String is infallible; the Result is ignored on purpose.
    let _ = writeln!(s, "Name:\t{name}");
    // Umask: per-process file-creation mask, octal.  Bare scheduler tasks
    // (no PCB) inherit the kernel default 022.  Process-wide → proc_id.
    let umask = crate::proc::pcb::get_umask(proc_id).unwrap_or(0o022);
    let _ = writeln!(s, "Umask:\t{umask:04o}");
    let _ = writeln!(s, "State:\t{state}");
    // Tgid is the thread-group (process) id; Pid is this thread's id.  For a
    // single-threaded process / the leader they coincide, matching getpid()
    // == gettid(); for a non-leader thread Tgid == owning pid while Pid ==
    // tid, exactly as Linux reports under task/<tid>/.
    let _ = writeln!(s, "Tgid:\t{proc_id}");
    let _ = writeln!(s, "Ngid:\t0");
    let _ = writeln!(s, "Pid:\t{}", task.id);
    let _ = writeln!(s, "PPid:\t{ppid}");
    let _ = writeln!(s, "TracerPid:\t0"); // no ptrace tracer tracking yet
    // Linux prints four credential columns (real, effective, saved-set,
    // filesystem).  Our credential model holds a single uid/gid, so all four
    // columns carry the same value — consistent with getuid/geteuid/
    // getresuid all returning the same id.
    let _ = writeln!(s, "Uid:\t{uid}\t{uid}\t{uid}\t{uid}");
    let _ = writeln!(s, "Gid:\t{gid}\t{gid}\t{gid}\t{gid}");
    // Groups: space-separated supplementary GIDs (may be empty).
    s.push_str("Groups:\t");
    if let Some(c) = creds.as_ref() {
        for (i, g) in c.groups.iter().enumerate() {
            if i > 0 {
                s.push(' ');
            }
            let _ = write!(s, "{g}");
        }
    }
    s.push('\n');
    // Memory: only processes with an address-space charge carry these.  A
    // bare scheduler task (kernel thread) omits them, exactly as Linux omits
    // the Vm* lines for tasks with no mm.  Derive the size from the SAME
    // 4 KiB ABI-page accounting as /proc/<pid>/statm so the two files agree
    // exactly (Linux keeps VmSize == statm.size * pagesize): pages =
    // ceil(bytes / 4096), VmSize_kB = pages * 4.  VmRSS mirrors VmSize
    // because we do not track resident pages separately — an upper bound,
    // which is the safe direction for callers (see gen_pid_statm).  Threads
    // share the owning process's address space, so this is process-wide.
    if let Some(as_bytes) = crate::proc::pcb::linux_as_used(proc_id) {
        if as_bytes > 0 {
            const ABI_PAGE_SIZE: u64 = 4096;
            let kb = as_bytes.div_ceil(ABI_PAGE_SIZE).saturating_mul(4);
            let _ = writeln!(s, "VmSize:\t{kb} kB");
            let _ = writeln!(s, "VmRSS:\t{kb} kB");
        }
    }
    let _ = writeln!(s, "Threads:\t{num_threads}");
    // NoNewPrivs (PR_SET_NO_NEW_PRIVS): 1 once a task has irreversibly opted
    // out of privilege-gaining execs, else 0.  systemd, WINE, and container
    // runtimes read this to confirm a sandbox took effect.  Real per-process
    // state; a bare task with no PCB has set no such flag, so 0 is truthful.
    // Linux prints this line for every task (process-wide → proc_id).
    let no_new_privs = crate::proc::pcb::get_no_new_privs(proc_id).unwrap_or(0);
    let _ = writeln!(s, "NoNewPrivs:\t{no_new_privs}");
    // Seccomp mode, matching Linux's SECCOMP_MODE_* encoding: 0 = disabled,
    // 1 = strict, 2 = filter.  Our scfilter is an allow/deny syscall filter
    // (the filter-mode semantics), and we have no strict mode, so an installed
    // filter maps to 2 and its absence to 0.  seccomp-aware tools read this.
    let seccomp = if crate::scfilter::has_filter(proc_id) { 2 } else { 0 };
    let _ = writeln!(s, "Seccomp:\t{seccomp}");
    // Cpus_allowed / Cpus_allowed_list: the task's CPU affinity, sized to the
    // online CPU count.  Affinity is a per-thread property in Linux, so this
    // keys off `task.id` (the thread id), not `proc_id`.  taskset, numactl and
    // htop read these.  A task absent from the scheduler snapshot (e.g. a bare
    // task) has no stored mask; default to "all online CPUs allowed", which is
    // a freshly-created task's affinity.
    let ncpus = crate::smp::cpu_count();
    let affinity = crate::sched::get_cpu_affinity(task.id).unwrap_or_else(|| {
        if ncpus >= 64 {
            u64::MAX
        } else {
            (1u64 << ncpus).wrapping_sub(1)
        }
    });
    let _ = writeln!(s, "Cpus_allowed:\t{}", format_bitmap_hex(affinity, ncpus));
    let _ = writeln!(
        s,
        "Cpus_allowed_list:\t{}",
        format_bitmap_list(affinity, ncpus)
    );
    // NUMA memory-node affinity.  We do not track a per-process mempolicy /
    // cpuset mems mask, so the truthful value is "all online nodes allowed" —
    // exactly Linux's default for a process under no cpuset restriction.  This
    // is process-wide, sourced from the online node count (not invented data).
    // numactl reads these.  node_count() <= MAX_NODES (8), so the mask always
    // fits in a single small chunk.
    let nnodes = crate::numa::node_count();
    let node_mask = if nnodes >= 64 {
        u64::MAX
    } else {
        (1u64 << nnodes).wrapping_sub(1)
    };
    let _ = writeln!(s, "Mems_allowed:\t{}", format_bitmap_hex(node_mask, nnodes));
    let _ = writeln!(
        s,
        "Mems_allowed_list:\t{}",
        format_bitmap_list(node_mask, nnodes)
    );
    // Context switches: the scheduler now tracks the voluntary/involuntary
    // split per task (charged at the switch point — voluntary when the task
    // yields/blocks/self-suspends, involuntary when it is preempted by the
    // timer).  Per-thread → from `task`.
    let _ = writeln!(s, "voluntary_ctxt_switches:\t{}", task.nvcsw);
    let _ = writeln!(s, "nonvoluntary_ctxt_switches:\t{}", task.nivcsw);

    s.into_bytes()
}

/// `/proc/<pid>/cmdline` — full command line, Linux-exact format.
///
/// Linux emits the process's argv as a sequence of NUL-terminated
/// strings concatenated together (each argument followed by a `\0`,
/// including the last).  We serve this from the persistent `proc_argv`
/// snapshot captured at spawn (see `pcb::set_initial_args`).
///
/// Fallbacks, in order:
///   1. persistent argv snapshot (the normal case for spawned programs);
///   2. the process name as a single argument (kernel-spawned tasks and
///      any process started without an explicit argv) — matches the
///      effect of Linux's single-arg cmdline;
///   3. the scheduler task name for bare tasks with no PCB.
///
/// Known limitation: like every other consumer of the snapshot, this
/// does not reflect a process rewriting its own `argv[]` at runtime
/// (`setproctitle`); it reports the argv as captured at spawn.
fn gen_pid_cmdline(task_id: u64) -> KernelResult<Vec<u8>> {
    // 1. Full argv from the persistent snapshot.
    if let Some(argv) = crate::proc::pcb::get_proc_argv(task_id) {
        if !argv.is_empty() {
            // Sum lengths + one NUL per argument for an exact allocation.
            let cap = argv.iter().map(|a| a.len().saturating_add(1)).sum();
            let mut data = Vec::with_capacity(cap);
            for arg in &argv {
                data.extend_from_slice(arg);
                data.push(0); // NUL terminator after each arg, Linux-style.
            }
            return Ok(data);
        }
        // argv snapshot empty (spawned without args): fall through to
        // the process-name single-argument form below.
    }

    // 2. Process name as a single NUL-terminated argument.
    if let Some(name) = crate::proc::pcb::name(task_id) {
        let mut data = name.into_bytes();
        data.push(0);
        return Ok(data);
    }

    // 3. Fall back to task name from the scheduler.
    let tasks = crate::sched::task_list();
    let task = tasks.iter().find(|t| t.id == task_id)
        .ok_or(KernelError::NotFound)?;

    let name = core::str::from_utf8(task.name.get(..task.name_len).unwrap_or(&[]))
        .unwrap_or("???");
    let mut data = name.as_bytes().to_vec();
    data.push(0);
    Ok(data)
}

/// `/proc/<pid>/environ` — process environment, Linux-exact format.
///
/// Like `cmdline`, Linux emits the environment as NUL-terminated
/// `KEY=value` strings concatenated together (each entry followed by a
/// `\0`).  Served from the persistent `proc_envp` snapshot captured at
/// spawn.
///
/// Returns an empty file (not an error) for a process that was spawned
/// without an environment — Linux's `/proc/<pid>/environ` is likewise
/// empty for such processes.  Returns `NotFound` only if the task id
/// resolves to no process at all (a bare scheduler task carries no
/// environment).
fn gen_pid_environ(task_id: u64) -> KernelResult<Vec<u8>> {
    let envp = crate::proc::pcb::get_proc_envp(task_id)
        .ok_or(KernelError::NotFound)?;
    let cap = envp.iter().map(|e| e.len().saturating_add(1)).sum();
    let mut data = Vec::with_capacity(cap);
    for entry in &envp {
        data.extend_from_slice(entry);
        data.push(0); // NUL terminator after each entry.
    }
    Ok(data)
}

/// `/proc/<pid>/auxv` — the process's ELF auxiliary vector.
///
/// On Linux this is the raw `Elf64_auxv_t` byte stream the kernel placed
/// on the process's initial stack: `(a_type, a_val)` `u64` pairs ending
/// in an `AT_NULL` terminator.  glibc's `getauxval(3)` reads it as a
/// fallback when `prctl(PR_GET_AUXV)` is unavailable, and tools like
/// `LD_SHOW_AUXV`, `ldd`, and `pmap -X` parse it.
///
/// Only **Linux-ABI** processes have an auxv (built and persisted by
/// `proc::linux_stack` at spawn/exec — see `pcb::linux_saved_auxv`).  A
/// *native* process has none by design (design-decision #4: it gets
/// argv/envp from `SYS_PROCESS_GET_ARGS` and never has a SysV stack), so
/// the honest answer is an empty file — which is also what a real Linux
/// kernel returns for a process whose auxv is unavailable.  Gated on
/// process existence: an unknown pid yields `NotFound`.
fn gen_pid_auxv(task_id: u64) -> KernelResult<Vec<u8>> {
    // Confirm the process exists (so an unknown pid is NotFound, not an
    // empty file) by probing a field every live process has.
    if crate::proc::pcb::get_proc_envp(task_id).is_none() {
        return Err(KernelError::NotFound);
    }
    Ok(crate::proc::pcb::linux_saved_auxv(task_id).unwrap_or_default())
}

/// `/proc/<pid>/stat` — single-line task statistics (Linux-compatible format).
///
/// Emits the full 52-field `/proc/[pid]/stat` line in the exact field
/// order defined by Linux `fs/proc/array.c` / `proc(5)`, so positional
/// parsers (`ps`, `top`, `htop`, glibc `sysconf`, WINE's `server/process.c`)
/// read it correctly.  The fields we have real data for are populated from
/// the scheduler task and the process control block; fields the native
/// kernel does not track (per-field fault counters, signal masks, code/stack
/// segment bounds, etc.) are reported as `0`, which is the same thing a real
/// Linux kernel does for a freshly-created task with no such activity —
/// these are genuinely-zero values, not placeholders.
///
/// Time fields (`utime`) are in clock ticks at `USER_HZ = 100`; the native
/// timer runs at [`crate::apic::TICK_RATE_HZ`] `= 100`, so `total_ticks`
/// already matches the ABI unit one-to-one (no rescale needed).  Memory
/// sizes use the Linux ABI page size of 4096 bytes (see [`gen_pid_statm`]),
/// not the native 16 KiB frame size.
fn gen_pid_stat(task_id: u64) -> KernelResult<Vec<u8>> {
    let tasks = crate::sched::task_list();
    let task = tasks.iter().find(|t| t.id == task_id)
        .ok_or(KernelError::NotFound)?;
    Ok(build_pid_stat(task, task_id))
}

/// `/proc/<pid>/task/<tid>/stat` — per-thread task statistics.
///
/// Same 52-field layout as [`gen_pid_stat`], but the thread-specific fields
/// (pid=field 1, comm, state, utime, priority, exit_code) come from the
/// thread's scheduler task `tid`, while the process-wide fields (ppid,
/// pgrp/session, num_threads, vsize, rss, rsslim) come from the owning
/// process `proc_id`.  This is what Linux's `task/<tid>/stat` reports and is
/// strictly more correct than serving `gen_pid_stat(tid)` (which would key
/// the process-wide fields off the non-process tid).
fn gen_thread_stat(proc_id: u64, tid: u64) -> KernelResult<Vec<u8>> {
    let tasks = crate::sched::task_list();
    let task = tasks.iter().find(|t| t.id == tid)
        .ok_or(KernelError::NotFound)?;
    Ok(build_pid_stat(task, proc_id))
}

/// Length of Linux's `comm` field minus the trailing NUL: `TASK_COMM_LEN - 1`.
/// `comm` is a fixed 16-byte field in the kernel, so the visible name is at
/// most 15 bytes.  Both `/proc/<pid>/comm` and the `(comm)` token in
/// `/proc/<pid>/stat` (field 2) must obey this limit, or strict parsers that
/// size their buffers to `TASK_COMM_LEN` overflow and the two files disagree.
const TASK_COMM_LEN_MINUS_1: usize = 15;

/// Truncate a task name to Linux's `comm` length (`TASK_COMM_LEN - 1` = 15
/// bytes), cutting on a UTF-8 char boundary so a multibyte sequence is never
/// split.  Shared by `gen_pid_comm` and `build_pid_stat` so the two
/// procfs surfaces always agree on the truncated name.
fn comm_truncate(name: &str) -> &str {
    if name.len() <= TASK_COMM_LEN_MINUS_1 {
        return name;
    }
    let mut end = TASK_COMM_LEN_MINUS_1;
    while end > 0 && !name.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    name.get(..end).unwrap_or("")
}

/// Build a Linux 52-field stat line.
///
/// Thread-specific fields come from `task` (field 1 = `task.id`, plus comm,
/// state, utime, priority, and exit code); process-wide fields come from
/// `proc_id` (ppid, pgrp/session, num_threads, vsize, rss, rsslim).  For a
/// process's own `/proc/<pid>/stat`, the caller passes `proc_id == task.id`
/// so the two id sources coincide; for a thread's `task/<tid>/stat` they
/// differ (`task.id == tid`, `proc_id == owning pid`).
fn build_pid_stat(task: &crate::sched::TaskInfo, proc_id: u64) -> Vec<u8> {
    use crate::sched::task::TaskState;

    // Field 2 (`comm`) must match `/proc/<pid>/comm` exactly, including the
    // 15-byte truncation — otherwise parsers that split on the last `)` and
    // size buffers to TASK_COMM_LEN disagree between the two files.
    let full_name = core::str::from_utf8(task.name.get(..task.name_len).unwrap_or(&[]))
        .unwrap_or("???");
    let name = comm_truncate(full_name);

    let state_char = match task.state {
        TaskState::Running => 'R',
        TaskState::Ready => 'R',    // runnable = R in Linux
        TaskState::Blocked => 'S',  // sleeping
        TaskState::Suspended => 'T', // stopped
        TaskState::Dead => 'Z',     // zombie
    };

    let ppid = crate::proc::pcb::parent(proc_id).unwrap_or(0);
    let num_threads = crate::proc::pcb::get_threads(proc_id)
        .map_or(1, |t| t.len());

    // utime/stime (fields 14/15): user and system CPU time in clock
    // ticks.  USER_HZ == TICK_RATE_HZ == 100, so the raw timer-tick
    // counts are already in ABI units.  These are this task's own
    // tick-sampled user/kernel split (Linux `account_user_time`/
    // `account_system_time` model); `user_ticks + sys_ticks ==
    // total_ticks`.  For a single-threaded process this equals the
    // process total; the thread-group-leader sum for multi-threaded
    // processes is the same TD14 follow-up as getrusage's children path.
    let utime = task.user_ticks;
    let stime = task.sys_ticks;

    // cutime/cstime (fields 16/17): user/system CPU time of this process's
    // reaped descendants, in clock ticks.  Process-wide, so keyed off
    // `proc_id` (a bare kernel thread with no PCB reports 0).  Credited at
    // wait/reap from each reaped child's (utime+cutime, stime+cstime),
    // mirroring Linux's `signal->cutime`/`cstime`.
    let (cutime, cstime) = crate::proc::pcb::process_child_ticks(proc_id);

    // minflt/majflt (fields 10/12): minor/major page faults charged to this
    // task.  Mirrors the utime/stime treatment above — a per-task value
    // (TaskInfo), which equals the process total for a single-threaded
    // process; the thread-group sum is the same TD14 follow-up.  cminflt/
    // cmajflt (fields 11/13): faults of reaped descendants, process-wide
    // (keyed off `proc_id`; a bare kernel thread reports 0).
    let minflt = task.min_flt;
    let majflt = task.maj_flt;
    let (cminflt, cmajflt) = crate::proc::pcb::process_child_faults(proc_id);

    // Virtual size (bytes) and resident pages, in Linux ABI page units.
    // Bare scheduler tasks (no process / address-space charge) report 0,
    // exactly as Linux does for kernel threads.  Threads share the owning
    // process's address space, so this is a process-wide charge.
    const ABI_PAGE_SIZE: u64 = 4096;
    let vsize = crate::proc::pcb::linux_as_used(proc_id).unwrap_or(0);
    let rss_pages = vsize / ABI_PAGE_SIZE;

    // rsslim: RLIMIT_RSS hard limit (resource index 5).  Default unlimited.
    let rsslim = crate::proc::pcb::get_rlimit(proc_id, 5)
        .map_or(crate::proc::pcb::RLIM_INFINITY, |(_soft, hard)| hard);

    // exit_code is only meaningful once the task is a zombie; Linux encodes
    // it the same way `wait()` does (status << 8).  We store the raw exit
    // code, so shift it into the wait-status high byte to match the ABI.
    // This is per-task (the thread's own exit status).
    let exit_code = crate::proc::pcb::exit_code(task.id)
        .map_or(0i64, |c| (i64::from(c) & 0xff) << 8);

    // priority: Linux reports the kernel-internal priority; for normal tasks
    // this is in the 0..39 range.  nice is 0 (native scheduler has no nice).
    let priority = i64::from(task.priority);

    // pgrp (field 5) and session (field 6).  We don't track process groups
    // or sessions as distinct objects; our model is "every process is its
    // own group and session leader", which is exactly what sys_getpgid /
    // sys_getsid / sys_getpgrp report (pgid == sid == pid).  These are
    // process-wide, so they key off `proc_id`.  Bare scheduler tasks
    // (kernel threads, no PCB) have no group/session — getpgid returns
    // ESRCH for them — so they report 0/0, matching Linux's kernel-thread
    // convention.
    let pgrp_sid = if crate::proc::pcb::state(proc_id).is_some() {
        proc_id
    } else {
        0
    };

    // starttime (field 22): the boot-relative tick when this task was
    // created, in clock ticks at USER_HZ.  The native timer ticks at
    // TICK_RATE_HZ == USER_HZ == 100, so the captured tick count is already
    // in ABI units (no rescale).  ps/top/htop subtract this from system
    // uptime to compute process age and to normalise CPU% over the
    // process's lifetime, so reporting the real value (rather than 0) makes
    // those columns correct.  This is a thread/task property, so it comes
    // from `task`, not `proc_id`.
    let starttime = task.start_tick;

    // processor (field 39): the CPU number this task last executed on.
    // `top -1` / htop's "P" column read this to show per-task CPU placement.
    // We snapshot last_cpu on every dispatch, so this is a real, current
    // value rather than a 0 stub.  Thread/task property → from `task`.
    let processor = task.last_cpu;

    // Field order matches proc(5) / Linux fs/proc/array.c do_task_stat().
    // 1:pid 2:comm 3:state 4:ppid 5:pgrp 6:session 7:tty_nr 8:tpgid 9:flags
    // 10:minflt 11:cminflt 12:majflt 13:cmajflt 14:utime 15:stime 16:cutime
    // 17:cstime 18:priority 19:nice 20:num_threads 21:itrealvalue
    // 22:starttime 23:vsize 24:rss 25:rsslim 26:startcode 27:endcode
    // 28:startstack 29:kstkesp 30:kstkeip 31:signal 32:blocked 33:sigignore
    // 34:sigcatch 35:wchan 36:nswap 37:cnswap 38:exit_signal 39:processor
    // 40:rt_priority 41:policy 42:delayacct_blkio_ticks 43:guest_time
    // 44:cguest_time 45:start_data 46:end_data 47:start_brk 48:arg_start
    // 49:arg_end 50:env_start 51:env_end 52:exit_code
    // One space between every field, terminated by a single newline.
    // Placeholders left-to-right: pid comm state ppid pgrp session
    // <tty_nr/tpgid/flags=0/-1/0> <minflt..cmajflt=0> utime stime
    // cutime cstime priority nice=0 num_threads itrealvalue=0
    // starttime vsize rss rsslim <startcode..wchan=0> <nswap/cnswap=0>
    // exit_signal=17 processor <rt_priority..env_end=0> exit_code.
    let text = format!(
        "{} ({}) {} {} {} {} 0 -1 0 {} {} {} {} {} {} {} {} {} 0 {} 0 {} {} {} {} \
         0 0 0 0 0 0 0 0 0 0 0 0 17 {} 0 0 0 0 0 0 0 0 0 0 0 0 {}\n",
        task.id, name, state_char, ppid, pgrp_sid, pgrp_sid,
        minflt, cminflt, majflt, cmajflt,
        utime, stime, cutime, cstime, priority, num_threads, starttime,
        vsize, rss_pages, rsslim,
        processor,
        exit_code,
    );
    text.into_bytes()
}

/// Render a process's VMA list as Linux `/proc/<pid>/maps` lines.
///
/// Pure helper (no PCB / lock access) so it can be unit-tested with
/// synthetic VMAs in kernel context.  Each line matches Linux's
/// `fs/proc/task_mmu.c` format closely enough for parsers (`pmap`,
/// debuggers, language runtimes):
///
/// ```text
/// start-end perms offset dev inode pathname
/// ```
///
/// - `start`/`end`: lowercase hex, no `0x` prefix, zero-padded to a minimum
///   of 8 digits (Linux's `%08lx` / `seq_put_hex_ll(.., 8)`); addresses wider
///   than 32 bits print their natural width.
/// - `perms`: exactly four chars `r`/`w`/`x`/(`p`|`s`).  We map `r` from
///   PRESENT *and* USER_ACCESSIBLE (a mapped, user-accessible page is
///   readable), `w` from WRITABLE, `x` from the absence of NO_EXECUTE.  A
///   `PROT_NONE` VMA carries no USER_ACCESSIBLE, so it renders `---p` like a
///   guard (design-decisions §32).  All our VMAs are private mappings, so the
///   fourth char is always `p`.  Guard VMAs are never backed and carry no
///   access rights → `---p`.
/// - `offset`/`dev`/`inode`: we do not track file-backed mappings yet, so
///   these are `0`/`00:00`/`0`, exactly as Linux reports for anonymous
///   mappings.
/// - `pathname`: a bracketed tag identifying the region kind (`[stack]`,
///   `[guard]`, anonymous = empty, fixed = `[fixed]`), mirroring Linux's
///   `[stack]`/`[heap]` pseudo-paths.
fn render_maps(vmas: &[crate::mm::vma::Vma]) -> Vec<u8> {
    use crate::mm::page_table::PageFlags;
    use crate::mm::vma::VmaKind;
    use core::fmt::Write as _;

    let mut text = String::with_capacity(vmas.len().saturating_mul(64).max(16));
    for vma in vmas {
        let f = vma.flags;
        // Guard pages are never mapped (no PRESENT): report no access.
        let is_guard = matches!(vma.kind, VmaKind::Guard);
        // 'r' requires both PRESENT and USER_ACCESSIBLE: a PROT_NONE region is
        // PRESENT but not user-accessible, so it must render '-' (design-
        // decisions §32), exactly like a guard.
        let r = if !is_guard
            && f.contains(PageFlags::PRESENT)
            && f.contains(PageFlags::USER_ACCESSIBLE)
        {
            'r'
        } else {
            '-'
        };
        let w = if !is_guard && f.contains(PageFlags::WRITABLE) { 'w' } else { '-' };
        let x = if !is_guard && !f.contains(PageFlags::NO_EXECUTE) { 'x' } else { '-' };
        // All current VMAs are private mappings.
        let pathname = match vma.kind {
            VmaKind::Stack => "[stack]",
            VmaKind::Guard => "[guard]",
            VmaKind::Fixed => "[fixed]",
            VmaKind::Brk => "[heap]",
            // A file-backed private mapping; Linux would show the backing
            // pathname here, but we don't cache the path in the VMA, so the
            // region renders unnamed (like an anonymous mapping).
            VmaKind::FileBacked { .. } => "",
            VmaKind::Anonymous => "",
        };
        // `write!` into a String is infallible; the Result is ignored.
        // Linux zero-pads start/end to a minimum of 8 hex digits
        // (fs/proc/task_mmu.c show_vma_header_prefix -> seq_put_hex_ll(.., 8));
        // addresses wider than 32 bits print their natural width.  Match that
        // with `{:08x}` so sub-4 GiB addresses align byte-for-byte with Linux.
        let _ = writeln!(
            text,
            "{:08x}-{:08x} {r}{w}{x}p 00000000 00:00 0 {pathname}",
            vma.start, vma.end
        );
    }
    text.into_bytes()
}

/// `/proc/<pid>/maps` — virtual memory area listing, Linux-format.
///
/// Lists the process's registered VMAs (both committed and demand-paged
/// anonymous/stack regions, guard pages, fixed mappings) one per line in
/// Linux `/proc/<pid>/maps` format; see [`render_maps`] for the exact
/// layout.  Every `mmap` region — committed (default) and lazy
/// (`MAP_LAZY`) — registers a VMA, so this stays consistent with the
/// RLIMIT_AS accounting behind `/proc/<pid>/status` VmSize and statm.
/// Bare scheduler tasks (kernel threads with no PCB) carry no VMAs and
/// return `NotFound`, matching how every other process-only per-PID file
/// behaves.  A process that has registered no VMAs yet yields an empty
/// file, exactly as Linux does for a task with an empty address space.
fn gen_pid_maps(task_id: u64) -> KernelResult<Vec<u8>> {
    // VMAs are only tracked for processes, not bare scheduler tasks.
    let vmas = crate::proc::pcb::list_vmas(task_id)
        .ok_or(KernelError::NotFound)?;
    Ok(render_maps(&vmas))
}

/// Escape a mount-table field the way Linux's `mangle()` does.
///
/// `/proc/mounts` and `/proc/<pid>/mountinfo` are space-separated, so the
/// four bytes that would otherwise corrupt the layout — space, tab,
/// newline and backslash — are rendered as 3-digit octal escapes
/// (`\040`, `\011`, `\012`, `\134`).  This mirrors `seq_escape(m, s,
/// " \t\n\\")` in `fs/proc_namespace.c` (`mangle()`), so mount points
/// containing whitespace round-trip correctly through `findmnt`,
/// `mount(8)` and glibc's `getmntent`.  Our filesystem allows every byte
/// except `/` and NUL in path components, so a mount point genuinely can
/// contain a space — without this, such a mount would be misparsed.
///
/// Iterating over `char`s (not bytes) keeps multi-byte UTF-8 sequences
/// intact; the four escaped characters are all ASCII (`< 0x80`) and so
/// never appear as UTF-8 continuation bytes.
fn mangle_mount_field(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            ' ' => out.push_str("\\040"),
            '\t' => out.push_str("\\011"),
            '\n' => out.push_str("\\012"),
            '\\' => out.push_str("\\134"),
            other => out.push(other),
        }
    }
    out
}

/// Render the mount table as Linux `/proc/<pid>/mountinfo` lines.
///
/// Pure helper (no VFS / lock access) so it can be unit-tested with a
/// synthetic mount list in kernel context.  Each line matches the
/// 11-field layout from Linux `fs/proc_namespace.c`
/// (`show_mountinfo`) closely enough for parsers (`findmnt`, glibc's
/// `__mount_proc`, systemd):
///
/// ```text
/// mount_id parent_id major:minor root mount_point options - fstype source super_options
/// ```
///
/// We have no mount namespaces, so every process sees the same global
/// mount table — `/proc/<pid>/mountinfo` is identical for all PIDs,
/// which is accurate rather than fabricated.  Field choices:
///
/// - `mount_id`: a stable small integer per mount (`base + index`).  Linux
///   assigns these from a global counter; the exact values are opaque to
///   parsers, which only require uniqueness and stability within one read.
/// - `parent_id`: the root mount's id for every entry.  We do not model a
///   mount tree, so all mounts are reported as children of the root mount;
///   the root reports itself.  Parsers that build a tree tolerate this.
/// - `major:minor`: `0:<index+1>`.  We have no block-device numbers, and
///   Linux uses `0:N` minors for anonymous/virtual filesystems anyway.
/// - `root`: always `/` (we mount whole filesystems, never subtrees).
/// - optional fields: none, so the separator `-` follows the options
///   directly (a valid, common case in real `mountinfo`).
/// - `source`: `none` (we do not track backing devices), matching what we
///   already emit in `/proc/mounts`.
/// - per-mount and super options are the same `MountOptions` string.
fn render_mountinfo(mounts: &[(String, String, crate::fs::vfs::MountOptions)]) -> Vec<u8> {
    use core::fmt::Write as _;

    /// Base for synthetic mount ids.  Linux ids are arbitrary positive
    /// integers; starting above a small reserved range keeps them clearly
    /// distinct from the parent-id we report for the root mount.
    const MOUNT_ID_BASE: usize = 20;
    let root_id = MOUNT_ID_BASE;

    let mut text = String::with_capacity(mounts.len().saturating_mul(96).max(16));
    for (i, (path, fs_type, options)) in mounts.iter().enumerate() {
        let mount_id = MOUNT_ID_BASE.saturating_add(i);
        let minor = i.saturating_add(1);
        let opts = options.to_string();
        // Linux mangles the mount-point and fstype fields (see
        // `mangle_mount_field`); the options string is composed of
        // comma-separated flag tokens with no whitespace, so it is emitted
        // verbatim, matching `show_mnt_opts`.
        let mount_point = mangle_mount_field(path);
        let fstype = mangle_mount_field(fs_type);
        // 11 fields, optional-field section empty (separator `-` follows
        // the per-mount options directly).
        let _ = writeln!(
            text,
            "{mount_id} {root_id} 0:{minor} / {mount_point} {opts} - {fstype} none {opts}",
        );
    }
    text.into_bytes()
}

/// True iff mount point `mount_path` covers host path `host` — either an
/// exact match or a proper parent directory (boundary-aware so `/data` does
/// not spuriously cover `/database`).  The root mount `/` covers everything.
fn mount_path_covers(mount_path: &str, host: &str) -> bool {
    if mount_path == "/" {
        return true;
    }
    if host == mount_path {
        return true;
    }
    host.strip_prefix(mount_path)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// Resolve the filesystem type serving host path `host` from the global mount
/// table: the longest mount-point prefix that covers it wins.  Returns `none`
/// if nothing covers it (unreachable in practice — `/` always covers).
fn fstype_for_host_path<'a>(
    host: &str,
    global: &'a [(String, String, crate::fs::vfs::MountOptions)],
) -> &'a str {
    let mut best: Option<(&str, usize)> = None;
    for (path, fs_type, _) in global {
        if mount_path_covers(path, host) {
            let len = path.len();
            if best.is_none_or(|(_, best_len)| len > best_len) {
                best = Some((fs_type.as_str(), len));
            }
        }
    }
    best.map_or("none", |(t, _)| t)
}

/// Render a *container* process's own mount view as `/proc/<pid>/mountinfo`.
///
/// A jailed process must see its container's mounts — the rootfs at `/` and
/// each volume/tmpfs at its guest path — not the host's global mount table
/// (which would both be wrong and leak the host mount topology into the
/// container).  Each entry's filesystem type is resolved from the real host
/// mount backing it ([`fstype_for_host_path`]); the `source` field is reported
/// as `none` so host backing paths are not leaked into the container.  The
/// read-only flag comes from the container's own view (a `:ro` volume, or any
/// rootfs path under a `--read-only` root), so the `rw`/`ro` option matches
/// what a write would actually do inside the container.
fn render_container_mountinfo(
    view: &[crate::ipc::namespace::MountViewEntry],
    global: &[(String, String, crate::fs::vfs::MountOptions)],
) -> Vec<u8> {
    use core::fmt::Write as _;

    const MOUNT_ID_BASE: usize = 20;
    let root_id = MOUNT_ID_BASE;

    let mut text = String::with_capacity(view.len().saturating_mul(96).max(16));
    for (i, entry) in view.iter().enumerate() {
        let mount_id = MOUNT_ID_BASE.saturating_add(i);
        let minor = i.saturating_add(1);
        let fstype = mangle_mount_field(fstype_for_host_path(&entry.host_target, global));
        let mount_point = mangle_mount_field(&entry.guest_path);
        let opts = if entry.read_only { "ro" } else { "rw" };
        let _ = writeln!(
            text,
            "{mount_id} {root_id} 0:{minor} / {mount_point} {opts} - {fstype} none {opts}",
        );
    }
    text.into_bytes()
}

/// `/proc/<pid>/mountinfo` — per-process mount table, Linux-format.
///
/// For an ordinary (unjailed) process this is the global mount table (the
/// data behind `/proc/mounts`), rendered in the richer `mountinfo` layout that
/// `findmnt`, glibc and systemd parse; see [`render_mountinfo`].  A **jailed
/// (container) process** instead sees *its own* mount view — the container
/// rootfs at `/` plus its volume/tmpfs mounts — via [`render_container_mountinfo`],
/// so a process inside a container does not observe the host's mount topology.
/// Gated on process existence so bare scheduler tasks (kernel threads with no
/// PCB) return `NotFound`, matching every other process-only per-PID file.
fn gen_pid_mountinfo(task_id: u64) -> KernelResult<Vec<u8>> {
    // Process-only file: a bare scheduler task has no process record.
    if crate::proc::pcb::state(task_id).is_none() {
        return Err(KernelError::NotFound);
    }
    let mounts = crate::fs::Vfs::mounts_full();
    // A container (jailed) process sees its own mount view, not the host's.
    if let Some(view) = crate::ipc::namespace::mount_view_for(task_id) {
        return Ok(render_container_mountinfo(&view, &mounts));
    }
    Ok(render_mountinfo(&mounts))
}

/// Render a process's cgroup membership as a Linux `/proc/<pid>/cgroup`
/// line (cgroup v2 unified hierarchy).
///
/// Pure helper (no cgroupfs lock access) so it can be unit-tested with a
/// synthetic group list in kernel context.  We run a cgroup v2 unified
/// hierarchy, so the file is a single line:
///
/// ```text
/// 0::<path>
/// ```
///
/// where hierarchy id is `0`, the controller field is empty (v2 lists
/// controllers in `cgroup.controllers`, not here), and `<path>` is the
/// group's path relative to the cgroup mount.  A process not explicitly
/// assigned to any group lives in the root cgroup `/`, which is exactly
/// what Linux reports.  `systemd`, container runtimes, and
/// `systemd-detect-virt` parse this line.
/// Resolve a process's cgroup path in the unified (v2) hierarchy.
///
/// cgroup membership is keyed by the 32-bit PID the cgroup API uses.  A
/// process not explicitly assigned to any group lives in the root cgroup
/// `/`, matching Linux.  Shared by [`render_cgroup`] (which prefixes the
/// `0::` hierarchy field) and [`render_cpuset`] (which reports the bare
/// path), so both files always agree on a process's membership.
fn cgroup_path_for(pid: u64, groups: &[crate::fs::cgroupfs::Cgroup]) -> &str {
    u32::try_from(pid)
        .ok()
        .and_then(|p| groups.iter().find(|g| g.processes.contains(&p)))
        .map_or("/", |g| g.path.as_str())
}

fn render_cgroup(pid: u64, groups: &[crate::fs::cgroupfs::Cgroup]) -> Vec<u8> {
    use core::fmt::Write as _;

    let path = cgroup_path_for(pid, groups);

    let mut text = String::with_capacity(path.len().saturating_add(4));
    // `writeln!` into a String is infallible; the Result is ignored.
    let _ = writeln!(text, "0::{path}");
    text.into_bytes()
}

/// `/proc/<pid>/cgroup` — the process's cgroup membership, Linux-format.
///
/// Reflects the real cgroup the process belongs to in our cgroup v2
/// subsystem (see [`render_cgroup`]); processes that were never assigned
/// to a group report the root cgroup `/`.  Gated on process existence so
/// bare scheduler tasks (kernel threads with no PCB) return `NotFound`,
/// matching every other process-only per-PID file.
fn gen_pid_cgroup(task_id: u64) -> KernelResult<Vec<u8>> {
    // Process-only file: a bare scheduler task has no process record.
    if crate::proc::pcb::state(task_id).is_none() {
        return Err(KernelError::NotFound);
    }
    let groups = crate::fs::cgroupfs::list_groups();
    Ok(render_cgroup(task_id, &groups))
}

/// Render `/proc/<pid>/cpuset` — the process's cpuset cgroup path.
///
/// Pure helper (no cgroupfs lock access) so it can be unit-tested with a
/// synthetic group list in kernel context.  In a cgroup v2 unified
/// hierarchy the cpuset controller shares the single cgroup tree, so a
/// process's cpuset path *is* its cgroup path: a single newline-terminated
/// line such as `/` or `/app.slice` (no `0::` prefix — this legacy file
/// predates the unified-hierarchy line format).  `numactl`/`libnuma` read
/// this to discover which NUMA-affine cpuset a task is confined to; with no
/// explicit assignment the task lives in the root cpuset `/`.
fn render_cpuset(pid: u64, groups: &[crate::fs::cgroupfs::Cgroup]) -> Vec<u8> {
    use core::fmt::Write as _;

    let path = cgroup_path_for(pid, groups);

    let mut text = String::with_capacity(path.len().saturating_add(1));
    // `writeln!` into a String is infallible; the Result is ignored.
    let _ = writeln!(text, "{path}");
    text.into_bytes()
}

/// `/proc/<pid>/cpuset` — the process's cpuset cgroup path, Linux-format.
///
/// Reflects the same cgroup membership as [`render_cgroup`] (we run a
/// unified v2 hierarchy, so cpuset and cgroup paths coincide); see
/// [`render_cpuset`] for the exact format.  Gated on process existence so
/// bare scheduler tasks (kernel threads with no PCB) return `NotFound`,
/// matching every other process-only per-PID file.
fn gen_pid_cpuset(task_id: u64) -> KernelResult<Vec<u8>> {
    // Process-only file: a bare scheduler task has no process record.
    if crate::proc::pcb::state(task_id).is_none() {
        return Err(KernelError::NotFound);
    }
    let groups = crate::fs::cgroupfs::list_groups();
    Ok(render_cpuset(task_id, &groups))
}

/// Render `/proc/<pid>/oom_score` — the current OOM badness, Linux-format.
///
/// Pure helper (no oomkiller lock access) so it can be unit-tested in
/// kernel context.  Linux's `oom_score` is a single integer clamped to
/// `0..=1000` that folds the user adjustment into the base badness; the
/// OOM killer ranks victims by `(score + adj).max(0)` (see
/// [`crate::fs::oomkiller`]), so we report exactly that, capped at the
/// Linux ceiling of 1000.
fn render_oom_score(base: i32, adj: i32) -> Vec<u8> {
    let eff = base.saturating_add(adj).clamp(0, 1000);
    format!("{eff}\n").into_bytes()
}

/// Render `/proc/<pid>/oom_score_adj` — the user OOM adjustment.
///
/// Pure helper.  Linux's `oom_score_adj` is a single integer in
/// `-1000..=1000`; we report the stored adjustment verbatim.
fn render_oom_score_adj(adj: i32) -> Vec<u8> {
    format!("{adj}\n").into_bytes()
}

/// `/proc/<pid>/oom_score` — current OOM-killer badness for the process.
///
/// Reflects the live oomkiller score when the process is registered;
/// processes the OOM killer has never scored report `0`, matching the
/// Linux default for a task with no accumulated badness.  Gated on
/// process existence so bare scheduler tasks return `NotFound`.
fn gen_pid_oom_score(task_id: u64) -> KernelResult<Vec<u8>> {
    if crate::proc::pcb::state(task_id).is_none() {
        return Err(KernelError::NotFound);
    }
    let (base, adj) = u32::try_from(task_id)
        .ok()
        .and_then(crate::fs::oomkiller::get_score)
        .map_or((0, 0), |s| (s.score, s.adj));
    Ok(render_oom_score(base, adj))
}

/// `/proc/<pid>/oom_score_adj` — user OOM adjustment for the process.
///
/// Writable: see [`set_pid_oom_score_adj`] / the `ProcFs::write_file`
/// override.  Unregistered processes report the Linux default of `0`.
/// Gated on process existence.
fn gen_pid_oom_score_adj(task_id: u64) -> KernelResult<Vec<u8>> {
    if crate::proc::pcb::state(task_id).is_none() {
        return Err(KernelError::NotFound);
    }
    let adj = u32::try_from(task_id)
        .ok()
        .and_then(crate::fs::oomkiller::get_score)
        .map_or(0, |s| s.adj);
    Ok(render_oom_score_adj(adj))
}

/// Parse the value written to `/proc/<pid>/oom_score_adj`.
///
/// Pure helper (unit-testable).  Linux accepts an ASCII decimal integer,
/// tolerating surrounding whitespace and a trailing newline (the shell's
/// `echo -1000 > .../oom_score_adj`), and rejects anything outside the
/// `-1000..=1000` range.  We mirror that: trim ASCII whitespace, parse a
/// signed integer, and bound-check.  An empty or malformed write is
/// `InvalidArgument` (Linux returns `-EINVAL`).
fn parse_oom_score_adj(data: &[u8]) -> KernelResult<i32> {
    let text = core::str::from_utf8(data)
        .map_err(|_| KernelError::InvalidArgument)?;
    let trimmed = text.trim();
    let value: i32 = trimmed.parse().map_err(|_| KernelError::InvalidArgument)?;
    if !(-1000..=1000).contains(&value) {
        return Err(KernelError::InvalidArgument);
    }
    Ok(value)
}

/// Apply a write to `/proc/<pid>/oom_score_adj`.
///
/// Gated on process existence (NotFound for bare scheduler tasks), then
/// validates the value via [`parse_oom_score_adj`] and stores it through
/// [`crate::fs::oomkiller::adjust_score`].  A process the OOM subsystem
/// has not registered yet returns `NotFound` from `adjust_score` — we have
/// nowhere to persist an adjustment for an untracked process, which is the
/// truthful outcome rather than silently dropping the write.
fn set_pid_oom_score_adj(task_id: u64, data: &[u8]) -> KernelResult<()> {
    if crate::proc::pcb::state(task_id).is_none() {
        return Err(KernelError::NotFound);
    }
    let adj = parse_oom_score_adj(data)?;
    let pid = u32::try_from(task_id).map_err(|_| KernelError::InvalidArgument)?;
    crate::fs::oomkiller::adjust_score(pid, adj)
}

/// Render `/proc/<pid>/schedstat` — scheduler statistics, Linux-format.
///
/// Pure helper (no scheduler lock access) so it can be unit-tested in
/// kernel context.  Linux's `/proc/<pid>/schedstat` is three
/// space-separated integers on one line (`fs/proc/base.c`
/// `proc_pid_schedstat`):
///
/// ```text
/// <cpu_time_ns> <run_delay_ns> <timeslices>
/// ```
///
/// 1. time spent running on the CPU, in nanoseconds;
/// 2. time spent waiting on a run queue, in nanoseconds;
/// 3. number of times the task was scheduled (timeslices run).
///
/// All three come from real per-task accounting (TSC cycles, run-queue
/// wait ticks, and the dispatch counter); none are placeholders.
fn render_schedstat(cpu_ns: u64, run_delay_ns: u64, timeslices: u64) -> Vec<u8> {
    format!("{cpu_ns} {run_delay_ns} {timeslices}\n").into_bytes()
}

/// `/proc/<pid>/schedstat` — per-task scheduler statistics.
///
/// Available for any live scheduler task (including kernel threads with
/// no PCB), matching Linux which serves `schedstat` for every task.
/// CPU time uses the TSC-based `total_cycles` accounting (nanosecond
/// precision, consistent with but finer than the 10 ms `utime` ticks in
/// `/proc/<pid>/stat`); run delay uses the cumulative run-queue wait
/// ticks at `USER_HZ = TICK_RATE_HZ = 100` (10 ms/tick).
fn gen_pid_schedstat(task_id: u64) -> KernelResult<Vec<u8>> {
    /// Nanoseconds per scheduler tick (USER_HZ == TICK_RATE_HZ == 100).
    const NS_PER_TICK: u64 = 1_000_000_000 / 100;

    let tasks = crate::sched::task_list();
    let task = tasks.iter().find(|t| t.id == task_id)
        .ok_or(KernelError::NotFound)?;

    let cpu_ns = crate::bench::cycles_to_ns(task.total_cycles);
    let run_delay_ns = task.total_wait_ticks.saturating_mul(NS_PER_TICK);
    Ok(render_schedstat(cpu_ns, run_delay_ns, task.schedule_count))
}

/// Render `/proc/<pid>/io` from a process's I/O counters.
///
/// Pure helper (unit-testable).  Emits Linux's exact seven-line
/// `key: value\n` layout (`fs/proc/base.c::proc_tgid_io_accounting`):
///
/// ```text
/// rchar: <bytes read via read-family syscalls>
/// wchar: <bytes written via write-family syscalls>
/// syscr: <number of read syscalls>
/// syscw: <number of write syscalls>
/// read_bytes: 0
/// write_bytes: 0
/// cancelled_write_bytes: 0
/// ```
///
/// The last three counters are storage-layer attribution Linux fills
/// from the block layer (`task_io_account_read` and friends).  We have
/// no per-process block-layer accounting, so we report them as 0 rather
/// than fabricate values — consistent with the project's "never invent
/// data in procfs" rule.  `rchar`/`wchar`/`syscr`/`syscw` are real,
/// tracked at the syscall boundary by `pcb::account_io_{read,write}`.
fn render_pid_io(rchar: u64, wchar: u64, syscr: u64, syscw: u64) -> Vec<u8> {
    format!(
        "rchar: {rchar}\n\
         wchar: {wchar}\n\
         syscr: {syscr}\n\
         syscw: {syscw}\n\
         read_bytes: 0\n\
         write_bytes: 0\n\
         cancelled_write_bytes: 0\n"
    )
    .into_bytes()
}

/// `/proc/<pid>/io` — per-process I/O byte accounting.
///
/// Consumed by `iotop`, `pidstat -d`, and monitoring agents.  Gated on
/// process existence (NotFound for a bare scheduler task with no PCB).
/// Reads the four honestly-tracked counters via [`crate::proc::pcb::
/// io_counters`]; the three storage-layer fields render as 0 (see
/// [`render_pid_io`]).
///
/// Limitation: the `rchar`/`wchar`/`syscr`/`syscw` counters are folded
/// in only on the **Linux-ABI** read/write syscall path (see
/// `syscall::linux::account_io_syscall`).  A process using the native
/// ABI exclusively will read all-zero counters here.  This is honest —
/// we genuinely do not yet track native-ABI byte transfers — not a
/// fabricated value; native accounting is a documented follow-up in
/// todo.txt.
fn gen_pid_io(task_id: u64) -> KernelResult<Vec<u8>> {
    let (rchar, wchar, syscr, syscw) =
        crate::proc::pcb::io_counters(task_id).ok_or(KernelError::NotFound)?;
    Ok(render_pid_io(rchar, wchar, syscr, syscw))
}

/// Render the symlink target for `/proc/<pid>/fd/<n>` from its fd-table
/// entry, mirroring Linux's magic fd links.
///
/// Every target is derived from the real fd entry — a regular file
/// resolves to its VFS path, a pipe to `pipe:[id]`, and anonymous kernel
/// objects (eventfd / pidfd / memfd) to Linux's `anon_inode:[type]`
/// labels.  The console maps to `/dev/console`.  Nothing here is
/// fabricated: if a File handle can no longer be resolved (it raced a
/// close), we report `anon_inode:[file]` rather than inventing a path.
fn fd_link_target(entry: &crate::proc::linux_fd::FdEntry) -> String {
    use crate::proc::linux_fd::HandleKind;
    match entry.kind {
        HandleKind::Console => String::from("/dev/console"),
        HandleKind::File => crate::fs::handle::handle_path(entry.raw_handle)
            .unwrap_or_else(|_| String::from("anon_inode:[file]")),
        HandleKind::Pipe => format!("pipe:[{}]", entry.raw_handle),
        HandleKind::EventFd => String::from("anon_inode:[eventfd]"),
        HandleKind::PidFd => String::from("anon_inode:[pidfd]"),
        HandleKind::MemFd => String::from("anon_inode:[memfd]"),
        HandleKind::Epoll => String::from("anon_inode:[eventpoll]"),
        HandleKind::SignalFd => String::from("anon_inode:[signalfd]"),
        HandleKind::Timerfd => String::from("anon_inode:[timerfd]"),
        HandleKind::Inotify => String::from("anon_inode:inotify"),
        // ALSA PCM is a real device node, so /proc/self/fd/N resolves to its
        // /dev/snd path.  The direction (playback `p` vs capture `c`) is
        // recorded on the instance object; a stale handle falls back to the
        // playback node name.
        HandleKind::AlsaPcm => {
            let capture = crate::ipc::alsa_pcm::is_capture(
                crate::ipc::alsa_pcm::AlsaPcmHandle::from_raw(entry.raw_handle),
            )
            .unwrap_or(false);
            if capture {
                String::from("/dev/snd/pcmC0D0c")
            } else {
                String::from("/dev/snd/pcmC0D0p")
            }
        }
        // ALSA control device is a real device node at a fixed path.
        HandleKind::AlsaControl => String::from("/dev/snd/controlC0"),
        // DRM card / render node is a real device node under /dev/dri; the
        // render-node flag is recorded on the instance object, and a stale
        // handle falls back to the card node name.
        HandleKind::DrmCard => {
            let render = crate::drm::card_fd::is_render_node(
                crate::drm::card_fd::DrmCardHandle::from_raw(entry.raw_handle),
            )
            .unwrap_or(false);
            if render {
                String::from("/dev/dri/renderD128")
            } else {
                String::from("/dev/dri/card0")
            }
        }
    }
}

/// Render the body of `/proc/<pid>/fdinfo/<n>` from an fd's `pos` and
/// `flags`, mirroring Linux's `fs/proc/fd.c::seq_show`.
///
/// Linux emits `pos:\t<f_pos>\nflags:\t0<octal f_flags>\n` followed by
/// `mnt_id:` and `ino:`.  We emit only the two fields we genuinely track
/// — the open-file offset and the Linux status flags — and deliberately
/// omit `mnt_id`/`ino` rather than fabricate them: we have no per-fd
/// mount id and no stable inode number to report, and inventing zeros
/// would be dishonest (the "never invent data in procfs" rule).  Tools
/// that read fdinfo (lsof, CRIU) look up `pos:`/`flags:` by key, so the
/// omission does not break them.  The `flags` value is printed with a
/// leading `0` then octal exactly as Linux does (`0%o`).
fn render_pid_fdinfo(pos: u64, flags: u32) -> Vec<u8> {
    format!("pos:\t{pos}\nflags:\t0{flags:o}\n").into_bytes()
}

/// Compute the displayed `flags` for an fd entry the way Linux's
/// `fs/proc/fd.c` does: take the file's status flags, clear the stored
/// `O_CLOEXEC` bit, then re-add it iff the descriptor's `FD_CLOEXEC` is
/// set.  This keeps the reported close-on-exec state authoritative on
/// the descriptor flag, not on a possibly-stale status-flag copy.
fn fdinfo_flags(entry: &crate::proc::linux_fd::FdEntry) -> u32 {
    use crate::proc::linux_fd::{FD_CLOEXEC, O_CLOEXEC};
    let mut flags = entry.status_flags & !O_CLOEXEC;
    if entry.fd_flags & FD_CLOEXEC != 0 {
        flags |= O_CLOEXEC;
    }
    flags
}

/// Render `/proc/<pid>/fdinfo/<n>` directly from an fd-table entry.
///
/// The `pos:` value is the open-file description's current offset for a
/// regular file (via [`crate::fs::handle::current_offset`]); for objects
/// with no seek position (pipes, console, eventfd/pidfd/memfd) it is `0`,
/// which is the truthful answer Linux also reports for those.
///
/// Taking the entry by reference lets callers that already hold one (e.g.
/// `readdir`, which enumerated the whole table via `linux_fd_list`) avoid
/// a redundant `PROCESS_TABLE` re-lookup per fd.
fn fdinfo_from_entry(entry: &crate::proc::linux_fd::FdEntry) -> Vec<u8> {
    use crate::proc::linux_fd::HandleKind;
    let pos = match entry.kind {
        // Only seekable file objects carry a meaningful offset; for
        // everything else f_pos is 0 (and we don't invent one).
        HandleKind::File => crate::fs::handle::current_offset(entry.raw_handle).unwrap_or(0),
        _ => 0,
    };
    render_pid_fdinfo(pos, fdinfo_flags(entry))
}

/// Build `/proc/<pid>/fdinfo/<n>` for a real, open fd, looking the entry
/// up by `(pid, fd)`.  Returns `NotFound` if the pid has no kernel-visible
/// fd table or the fd is not open.
fn gen_pid_fdinfo(pid: u64, fd: i32) -> KernelResult<Vec<u8>> {
    let entry = crate::proc::pcb::linux_fd_lookup(pid, fd).ok_or(KernelError::NotFound)?;
    Ok(fdinfo_from_entry(&entry))
}

/// The audit "unset" sentinel — `(uid_t)-1` / `(unsigned)-1`.
///
/// Linux reports this for `loginuid`/`sessionid` on any task the audit
/// subsystem has not bound to a login session.  We do not track a kernel
/// audit login session, so every process is genuinely unset — this value
/// is the correct, truthful answer, not a placeholder.
const AUDIT_UNSET: u32 = u32::MAX;

/// Render an audit uid/session value as Linux does: the decimal integer
/// with **no trailing newline** (`proc_loginuid_read` / `proc_sessionid_read`
/// use a bare `scnprintf("%u")`).  Pure helper, unit-testable.
fn render_audit_id(value: u32) -> Vec<u8> {
    format!("{value}").into_bytes()
}

/// `/proc/<pid>/loginuid` — audit login UID.
///
/// Reports the audit "unset" sentinel (`u32::MAX`) because we do not
/// track a kernel audit login session; this matches Linux for a process
/// outside any login session.  `systemd-logind` and `pam_loginuid` read
/// this.  Gated on process existence (NotFound for bare scheduler tasks).
fn gen_pid_loginuid(task_id: u64) -> KernelResult<Vec<u8>> {
    if crate::proc::pcb::state(task_id).is_none() {
        return Err(KernelError::NotFound);
    }
    Ok(render_audit_id(AUDIT_UNSET))
}

/// `/proc/<pid>/sessionid` — audit session ID.
///
/// Reports the audit "unset" sentinel (`u32::MAX`); see
/// [`gen_pid_loginuid`].  Gated on process existence.
fn gen_pid_sessionid(task_id: u64) -> KernelResult<Vec<u8>> {
    if crate::proc::pcb::state(task_id).is_none() {
        return Err(KernelError::NotFound);
    }
    Ok(render_audit_id(AUDIT_UNSET))
}

/// `/proc/<pid>/caps` — capability table listing.
///
/// Shows the count and types of capabilities granted to this process,
/// plus the process credentials (UID/GID).
fn gen_pid_caps(task_id: u64) -> KernelResult<Vec<u8>> {
    use crate::cap::{ResourceType, Rights};
    use crate::proc::pcb;

    let cap_count = pcb::cap_count(task_id)
        .ok_or(KernelError::NotFound)?;

    let mut text = format!("Capabilities: {} total\n", cap_count);

    if cap_count == 0 {
        text.push_str("(no capabilities granted)\n");
    } else {
        // Probe well-known resource types with READ rights to show which
        // kinds of capabilities this process holds.
        let probes: &[(ResourceType, &str)] = &[
            (ResourceType::Process, "Process"),
            (ResourceType::Thread, "Thread"),
            (ResourceType::Channel, "Channel"),
            (ResourceType::Pipe, "Pipe"),
            (ResourceType::SharedMemory, "SharedMem"),
            (ResourceType::File, "File"),
            (ResourceType::Socket, "Socket"),
            (ResourceType::PortIo, "PortIO"),
            (ResourceType::DeviceIrq, "DevIRQ"),
            (ResourceType::IoScheduler, "IoSched"),
        ];

        for &(rt, label) in probes {
            if pcb::has_capability_type(task_id, rt, Rights::READ) {
                text.push_str(&format!("  {}: yes\n", label));
            }
        }
    }

    // Credentials.
    if let Some(creds) = pcb::get_credentials(task_id) {
        text.push_str(&format!("\nUID: {} GID: {}\n", creds.uid, creds.gid));
        if creds.is_root() {
            text.push_str("Privilege: root (all capabilities implied)\n");
        }
    }

    Ok(text.into_bytes())
}

/// `/proc/<pid>/comm` — the command name (Linux-exact format).
///
/// Linux's `/proc/<pid>/comm` is the task's `comm` field: the command
/// name with no path, truncated to `TASK_COMM_LEN - 1 == 15` bytes,
/// followed by a single newline.  Many tools and language runtimes
/// (glibc's `pthread_getname_np`, Go's runtime, `ps`, `htop`) read this
/// exact shape, so we match it precisely rather than emitting our
/// richer status formatting.
fn gen_pid_comm(task_id: u64) -> KernelResult<Vec<u8>> {
    // `comm` reflects the scheduler task name (set by exec / prctl
    // PR_SET_NAME), which is what Linux's `comm` tracks — not the full
    // process name.  Fall back to the process name only if there is no
    // scheduler task (e.g. a process record without a live task).
    let tasks = crate::sched::task_list();
    let name: String = if let Some(task) = tasks.iter().find(|t| t.id == task_id) {
        core::str::from_utf8(task.name.get(..task.name_len).unwrap_or(&[]))
            .unwrap_or("???")
            .to_string()
    } else if let Some(proc_name) = crate::proc::pcb::name(task_id) {
        proc_name
    } else {
        return Err(KernelError::NotFound);
    };

    // Linux truncates `comm` to TASK_COMM_LEN - 1 = 15 bytes.  Use the
    // shared helper so `/proc/<pid>/comm` and `/proc/<pid>/stat` field 2
    // truncate identically (char-boundary safe).
    let truncated = comm_truncate(&name);

    let mut data = truncated.as_bytes().to_vec();
    data.push(b'\n');
    Ok(data)
}

/// `/proc/<pid>/statm` — memory usage in pages (Linux-compatible).
///
/// Linux emits seven space-separated integers, each a count of pages of
/// `getpagesize()` bytes:
///   `size resident shared text lib data dt`
///
/// CRITICAL — page-size unit: statm counts are in units of the page
/// size *visible to userspace* via `sysconf(_SC_PAGESIZE)`.  Our Linux
/// ABI advertises **4096** (4 KiB) to Linux programs even though the
/// native kernel uses 16 KiB frames — see
/// `kernel/src/syscall/linux.rs`'s `ABI_PAGE_SIZE` (the same 4 KiB unit
/// `mmap`/`mprotect`/`msync`/`mremap` already use at the boundary).
/// We MUST report statm in that same 4 KiB unit: a Linux app reading
/// statm multiplies each count by `getpagesize()` (4096) to recover
/// bytes, so dividing by the 16 KiB frame size here would make every
/// process appear to use a quarter of its true address space.
///
/// We track a single Linux address-space charge per process
/// (`linux_as_used`, the sum of Linux-ABI `mmap` sizes) rather than a
/// full VMA breakdown, so we report:
/// - `size` = total address-space charge / 4 KiB
/// - `resident` = same value (we do not track RSS separately; this is an
///   upper bound, which is the safe direction for callers that read
///   statm — they treat it as "at most this much")
/// - `shared`, `text`, `lib`, `data`, `dt` = 0 (we do not yet attribute
///   the charge to those categories; `lib` and `dt` are always 0 on
///   modern Linux anyway)
///
/// When per-VMA accounting lands this becomes a richer breakdown; the
/// page-unit contract stays the same.
fn gen_pid_statm(task_id: u64) -> KernelResult<Vec<u8>> {
    // statm only applies to processes (which carry the AS charge), not
    // bare scheduler tasks.
    let as_bytes = crate::proc::pcb::linux_as_used(task_id)
        .ok_or(KernelError::NotFound)?;
    // Linux ABI page size (sysconf(_SC_PAGESIZE)), NOT the kernel's
    // 16 KiB frame size — see the doc comment above.
    const ABI_PAGE_SIZE: u64 = 4096;
    // ABI_PAGE_SIZE is a non-zero compile-time constant, so this
    // division is always safe; round up so a partial page still counts
    // as one.
    let pages = as_bytes.div_ceil(ABI_PAGE_SIZE);
    // size resident shared text lib data dt
    let text = format!("{pages} {pages} 0 0 0 0 0\n");
    Ok(text.into_bytes())
}

/// `/proc/<pid>/limits` — resource limits table (Linux-compatible).
///
/// Reproduces Linux's `/proc/<pid>/limits` column layout exactly so
/// tools that scrape it (systemd, container runtimes, `ulimit -a`
/// fallbacks) parse correctly.  Values come from the per-process
/// `rlimits` table; if the pid has no live PCB we fall back to the
/// compiled-in [`DEFAULT_RLIMITS`] so the file is never empty for a
/// valid task id.
fn gen_pid_limits(task_id: u64) -> KernelResult<Vec<u8>> {
    use crate::proc::pcb::{self, DEFAULT_RLIMITS, NUM_RLIMITS, RLIM_INFINITY};

    // Linux row labels + units, indexed by RLIMIT_* resource number.
    const ROWS: [(&str, &str); 16] = [
        ("Max cpu time", "seconds"),
        ("Max file size", "bytes"),
        ("Max data size", "bytes"),
        ("Max stack size", "bytes"),
        ("Max core file size", "bytes"),
        ("Max resident set", "bytes"),
        ("Max processes", "processes"),
        ("Max open files", "files"),
        ("Max locked memory", "bytes"),
        ("Max address space", "bytes"),
        ("Max file locks", "locks"),
        ("Max pending signals", "signals"),
        ("Max msgqueue size", "bytes"),
        ("Max nice priority", ""),
        ("Max realtime priority", ""),
        ("Max realtime timeout", "us"),
    ];

    // Compile-time guard: the row-label table and the per-process defaults
    // table must each cover exactly NUM_RLIMITS resources.  The loop below
    // indexes ROWS[resource] and DEFAULT_RLIMITS[resource] over
    // 0..NUM_RLIMITS with indexing_slicing allowed; if a new RLIMIT_* is ever
    // added and NUM_RLIMITS bumped without extending these arrays in lockstep,
    // that indexing would panic at runtime (a DoS in this kernel's threat
    // model).  These asserts turn that latent runtime panic into a build
    // failure instead, justifying the #[allow] annotations below.
    const _: () = assert!(ROWS.len() == NUM_RLIMITS as usize);
    const _: () = assert!(DEFAULT_RLIMITS.len() == NUM_RLIMITS as usize);

    // Validate the task id resolves to *something* (process or task) so
    // a bogus pid yields NotFound rather than a default table.
    if pcb::state(task_id).is_none() {
        let tasks = crate::sched::task_list();
        if !tasks.iter().any(|t| t.id == task_id) {
            return Err(KernelError::NotFound);
        }
    }

    let mut s = String::with_capacity(1024);
    // Header — column widths match util-linux / kernel fs/proc/base.c.
    s.push_str(&format!(
        "{:<25}{:<21}{:<21}{:<11}\n",
        "Limit", "Soft Limit", "Hard Limit", "Units"
    ));

    let fmt_val = |v: u64| -> String {
        if v == RLIM_INFINITY {
            String::from("unlimited")
        } else {
            format!("{v}")
        }
    };

    for resource in 0..NUM_RLIMITS {
        let (soft, hard) = pcb::get_rlimit(task_id, resource)
            .unwrap_or_else(|| {
                // Bare task without a PCB: report the system defaults.
                #[allow(clippy::indexing_slicing)]
                DEFAULT_RLIMITS[resource as usize]
            });
        #[allow(clippy::indexing_slicing)]
        let (label, units) = ROWS[resource as usize];
        s.push_str(&format!(
            "{:<25}{:<21}{:<21}{:<11}\n",
            label,
            fmt_val(soft),
            fmt_val(hard),
            units,
        ));
    }

    Ok(s.into_bytes())
}

/// Generate content for a per-PID virtual file.
fn generate_pid(task_id: u64, file_name: &str) -> KernelResult<Vec<u8>> {
    match file_name {
        "status" => gen_pid_status(task_id),
        "cmdline" => gen_pid_cmdline(task_id),
        "stat" => gen_pid_stat(task_id),
        "maps" => gen_pid_maps(task_id),
        "caps" => gen_pid_caps(task_id),
        "comm" => gen_pid_comm(task_id),
        "statm" => gen_pid_statm(task_id),
        "limits" => gen_pid_limits(task_id),
        "environ" => gen_pid_environ(task_id),
        "auxv" => gen_pid_auxv(task_id),
        "mountinfo" => gen_pid_mountinfo(task_id),
        "mounts" => gen_pid_mounts(task_id),
        "cgroup" => gen_pid_cgroup(task_id),
        "cpuset" => gen_pid_cpuset(task_id),
        "oom_score" => gen_pid_oom_score(task_id),
        "oom_score_adj" => gen_pid_oom_score_adj(task_id),
        "schedstat" => gen_pid_schedstat(task_id),
        "loginuid" => gen_pid_loginuid(task_id),
        "sessionid" => gen_pid_sessionid(task_id),
        "io" => gen_pid_io(task_id),
        _ => Err(KernelError::NotFound),
    }
}

/// Generate the contents of a `/proc/<pid>/task/<tid>/<file>` thread file.
///
/// `pid` is the owning process id; `tid` is the scheduler task id of the
/// thread.  Only the files in [`TASK_FILES`] are served.  `comm` and
/// `schedstat` are rendered purely from the scheduler task (the underlying
/// `gen_pid_*` helpers key on the task id, not the process id), so passing
/// the thread's tid yields that thread's own data with no process/thread
/// field mixing.  `stat` and `status` need both ids: thread-specific fields
/// come from `tid` while the process-wide fields (ppid, num_threads, vsize,
/// Tgid, credentials, …) come from `pid` — see [`gen_thread_stat`] and
/// [`gen_thread_status`].  The caller must already have verified the tid
/// belongs to the process via [`thread_belongs`].
fn generate_task(pid: u64, tid: u64, file_name: &str) -> KernelResult<Vec<u8>> {
    match file_name {
        "comm" => gen_pid_comm(tid),
        "schedstat" => gen_pid_schedstat(tid),
        "stat" => gen_thread_stat(pid, tid),
        "status" => gen_thread_status(pid, tid),
        _ => Err(KernelError::NotFound),
    }
}

/// Generate `/proc/pipes` — active named pipes.
fn gen_pipes() -> Vec<u8> {
    let pipes = crate::fs::pipe::list();
    let mut s = String::with_capacity(512);
    s.push_str(&format!("Active pipes: {}\n\n", pipes.len()));
    if !pipes.is_empty() {
        s.push_str(&format!("{:<30} {:>8} {:>8} {:>4} {:>4} {:>12} {:>12}\n",
            "Path", "Capacity", "Buffered", "R", "W", "BytesIn", "BytesOut"));
        for p in &pipes {
            s.push_str(&format!("{:<30} {:>8} {:>8} {:>4} {:>4} {:>12} {:>12}\n",
                p.path, p.capacity, p.buffered, p.readers, p.writers,
                p.bytes_written, p.bytes_read));
        }
    }
    s.into_bytes()
}

/// Generate `/proc/overlays` — active overlay mounts.
fn gen_overlays() -> Vec<u8> {
    let overlays = crate::fs::overlay::list();
    let mut s = String::with_capacity(512);
    s.push_str(&format!("Active overlays: {}\n\n", overlays.len()));
    for (id, ov) in &overlays {
        s.push_str(&format!("overlay {} ({}):\n", id, ov.name));
        s.push_str(&format!("  lower:      {}\n", ov.lower_path));
        s.push_str(&format!("  upper:      {}\n", ov.upper_path));
        s.push_str(&format!("  whiteouts:  {}\n", ov.whiteout_count));
        s.push_str(&format!("  opaque:     {}\n", ov.opaque_dir_count));
        s.push_str(&format!("  reads:      {}\n", ov.reads));
        s.push_str(&format!("  writes:     {}\n", ov.writes));
        s.push_str(&format!("  copyups:    {}\n", ov.copyups));
        s.push('\n');
    }
    s.into_bytes()
}

/// Generate `/proc/namespaces` — active mount namespaces.
fn gen_namespaces() -> Vec<u8> {
    let nss = crate::fs::mount_ns::list();
    let mut s = String::with_capacity(512);
    s.push_str(&format!("Mount namespaces: {}\n\n", nss.len()));
    for ns in &nss {
        let parent = ns.parent.map(|p| format!("{}", p)).unwrap_or_else(|| String::from("none"));
        s.push_str(&format!("ns {} ({}):\n", ns.id, ns.name));
        s.push_str(&format!("  parent:     {}\n", parent));
        s.push_str(&format!("  mounts:     {}\n", ns.mount_count));
        s.push_str(&format!("  refcount:   {}\n", ns.refcount));
        s.push_str(&format!("  nested:     {}\n", ns.allow_nested));
        s.push('\n');
    }
    s.into_bytes()
}

/// Generate `/proc/rlimits` — resource limits.
fn gen_rlimits() -> Vec<u8> {
    use crate::fs::rlimit;
    let defaults = rlimit::get_defaults();
    let overrides = rlimit::list_overrides();
    let mut s = String::with_capacity(512);

    s.push_str("Global defaults:\n");
    s.push_str(&format!("  nofile:  soft={} hard={}\n",
        rlimit::Rlimit::format_value(defaults.nofile.soft),
        rlimit::Rlimit::format_value(defaults.nofile.hard)));
    s.push_str(&format!("  fsize:   soft={} hard={}\n",
        rlimit::Rlimit::format_value(defaults.fsize.soft),
        rlimit::Rlimit::format_value(defaults.fsize.hard)));
    s.push_str(&format!("  locks:   soft={} hard={}\n",
        rlimit::Rlimit::format_value(defaults.locks.soft),
        rlimit::Rlimit::format_value(defaults.locks.hard)));

    if !overrides.is_empty() {
        s.push_str(&format!("\nPer-UID overrides ({}):\n", overrides.len()));
        for (uid, set) in &overrides {
            s.push_str(&format!("  uid {}:\n", uid));
            s.push_str(&format!("    nofile: soft={} hard={}\n",
                rlimit::Rlimit::format_value(set.nofile.soft),
                rlimit::Rlimit::format_value(set.nofile.hard)));
            s.push_str(&format!("    fsize:  soft={} hard={}\n",
                rlimit::Rlimit::format_value(set.fsize.soft),
                rlimit::Rlimit::format_value(set.fsize.hard)));
            s.push_str(&format!("    locks:  soft={} hard={}\n",
                rlimit::Rlimit::format_value(set.locks.soft),
                rlimit::Rlimit::format_value(set.locks.hard)));
        }
    } else {
        s.push_str("\nNo per-UID overrides.\n");
    }
    s.into_bytes()
}

/// Generate `/proc/audit` — filesystem audit status.
fn gen_audit() -> Vec<u8> {
    use crate::fs::audit;
    let st = audit::stats();
    let rules = audit::list_rules();
    let mut s = String::with_capacity(512);

    s.push_str(&format!("Filesystem audit: {}\n\n", if st.enabled { "enabled" } else { "disabled" }));
    s.push_str(&format!("  buffer:       {}/{} entries\n", st.buffer_used, st.buffer_size));
    s.push_str(&format!("  total events: {}\n", st.total_events));
    s.push_str(&format!("  dropped:      {}\n", st.dropped_events));
    s.push_str(&format!("  rules:        {}\n\n", st.rules_count));

    if !rules.is_empty() {
        s.push_str("Rules:\n");
        for r in &rules {
            let uid_str = r.uid.map(|u| format!("{}", u)).unwrap_or_else(|| String::from("*"));
            let prefix = if r.path_prefix.is_empty() { "(all)" } else { &r.path_prefix };
            s.push_str(&format!("  rule {}: path={} mask=0x{:X} uid={} failures={} enabled={}\n",
                r.id, prefix, r.mask.0, uid_str, r.failures_only, r.enabled));
        }
    }
    s.into_bytes()
}

fn gen_snapshots() -> Vec<u8> {
    use crate::fs::snapshot;
    let snaps = snapshot::list();
    let mut s = String::with_capacity(512);

    s.push_str(&format!("Filesystem snapshots: {}\n\n", snaps.len()));

    if !snaps.is_empty() {
        s.push_str(&format!("{:>4}  {:20}  {:30}  {:>8}  {:>12}  {}\n",
            "ID", "NAME", "PATH", "FILES", "BYTES", "PARENT"));
        for snap in &snaps {
            let parent_str = snap.parent
                .map(|p| format!("{}", p.0))
                .unwrap_or_else(|| String::from("-"));
            s.push_str(&format!("{:>4}  {:20}  {:30}  {:>8}  {:>12}  {}\n",
                snap.id.0, snap.name, snap.root_path,
                snap.file_count, snap.total_bytes, parent_str));
        }
    }

    s.into_bytes()
}

fn gen_reclaim() -> Vec<u8> {
    use crate::fs::reclaim;
    let s = reclaim::stats();
    let (hi, lo) = reclaim::watermarks();
    let p = reclaim::phases();
    let mut out = String::with_capacity(512);

    out.push_str(&format!("Space reclamation: {}\n\n", if reclaim::is_enabled() { "enabled" } else { "disabled" }));
    out.push_str(&format!("  watermarks:   high={}% low={}%\n", hi, lo));
    out.push_str(&format!("  triggers:     {}\n", s.trigger_count));
    out.push_str(&format!("  total freed:  {} bytes\n", s.total_bytes_freed));
    out.push_str(&format!("  CAS blobs:    {}\n", s.total_cas_blobs));
    out.push_str(&format!("  tmp files:    {}\n", s.total_tmpwatch_files));
    out.push_str(&format!("  trash items:  {}\n", s.total_trash_items));
    out.push_str(&format!("  journal ents: {}\n", s.total_journal_entries));
    out.push_str(&format!("  active:       {}\n\n", s.active));
    out.push_str(&format!("  phases: cache={} cas={} tmp={} trash={} journal={}\n",
        p.cache, p.cas_gc, p.tmpwatch, p.trash, p.journal));

    out.into_bytes()
}

fn gen_transactions() -> Vec<u8> {
    use crate::fs::transaction;
    let txns = transaction::list();
    let active = transaction::active_count();
    let mut out = String::with_capacity(512);

    out.push_str(&format!("Filesystem transactions: {} total, {} active\n\n", txns.len(), active));

    if txns.is_empty() {
        out.push_str("(no transactions)\n");
    } else {
        out.push_str(&format!("{:<6} {:<12} {:<6} {}\n", "ID", "STATE", "OPS", "LABEL"));
        for t in &txns {
            let state = match t.state {
                transaction::TxState::Active => "active",
                transaction::TxState::Committed => "committed",
                transaction::TxState::RolledBack => "rolled-back",
                transaction::TxState::Dirty => "DIRTY",
            };
            out.push_str(&format!("{:<6} {:<12} {:<6} {}\n", t.id.0, state, t.ops_count, t.label));
        }
    }

    out.into_bytes()
}

/// Generate `/proc/certmgr` — certificate store status.
fn gen_certmgr() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (total, roots, servers, requests, ops) = super::certmgr::stats();

    out.push_str("Certificate Manager\n");
    out.push_str("===================\n\n");
    out.push_str(&format!("Total certs:   {}\n", total));
    out.push_str(&format!("Root CAs:      {}\n", roots));
    out.push_str(&format!("Server certs:  {}\n", servers));
    out.push_str(&format!("ACME requests: {}\n", requests));
    out.push_str(&format!("Threshold:     {} days\n", super::certmgr::renewal_threshold()));
    out.push_str(&format!("Operations:    {}\n", ops));

    let certs = super::certmgr::list_certs();
    if !certs.is_empty() {
        out.push_str(&format!("\n{:<6} {:<28} {:<12} {:<10} {:<10} {}\n",
            "ID", "CN", "TYPE", "SOURCE", "STATUS", "AUTO"));
        for c in &certs {
            let ct = match c.cert_type {
                super::certmgr::CertType::Root => "root",
                super::certmgr::CertType::Intermediate => "inter",
                super::certmgr::CertType::Server => "server",
                super::certmgr::CertType::Client => "client",
                super::certmgr::CertType::CodeSigning => "code",
                super::certmgr::CertType::SelfSigned => "self",
            };
            let src = match c.source {
                super::certmgr::CertSource::System => "system",
                super::certmgr::CertSource::UserImported => "user",
                super::certmgr::CertSource::LetsEncrypt => "LE",
                super::certmgr::CertSource::Acme => "acme",
                super::certmgr::CertSource::Generated => "gen",
            };
            let st = match c.status {
                super::certmgr::CertStatus::Valid => "valid",
                super::certmgr::CertStatus::Expired => "expired",
                super::certmgr::CertStatus::Revoked => "revoked",
                super::certmgr::CertStatus::NotYetValid => "future",
                super::certmgr::CertStatus::Untrusted => "untrusted",
                super::certmgr::CertStatus::Disabled => "disabled",
            };
            let auto = if c.auto_renew { "yes" } else { "no" };
            out.push_str(&format!("{:<6} {:<28} {:<12} {:<10} {:<10} {}\n",
                c.id, c.common_name, ct, src, st, auto));
        }
    }

    out.into_bytes()
}

/// Generate `/proc/installer` — installation wizard status.
fn gen_installer() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (total, complete, failed, ops) = super::installer::stats();

    out.push_str("Installation Wizard\n");
    out.push_str("===================\n\n");
    out.push_str(&format!("Sessions:   {}\n", total));
    out.push_str(&format!("Complete:   {}\n", complete));
    out.push_str(&format!("Failed:     {}\n", failed));
    out.push_str(&format!("Operations: {}\n", ops));

    let sessions = super::installer::list_sessions();
    if !sessions.is_empty() {
        out.push_str(&format!("\n{:<6} {:<12} {:<14} {:<4}% {}\n",
            "ID", "MODE", "PHASE", "PCT", "STATUS"));
        for s in &sessions {
            let mode = match s.mode {
                super::installer::InstallMode::Easy => "easy",
                super::installer::InstallMode::Manual => "manual",
                super::installer::InstallMode::Unattended => "unattended",
            };
            let phase = match s.phase {
                super::installer::InstallPhase::NotStarted => "not-started",
                super::installer::InstallPhase::PreInstall => "pre-install",
                super::installer::InstallPhase::Partitioning => "partitioning",
                super::installer::InstallPhase::Copying => "copying",
                super::installer::InstallPhase::Bootloader => "bootloader",
                super::installer::InstallPhase::PendingReboot => "reboot",
                super::installer::InstallPhase::FirstBoot => "first-boot",
                super::installer::InstallPhase::Complete => "complete",
                super::installer::InstallPhase::Failed => "FAILED",
            };
            out.push_str(&format!("{:<6} {:<12} {:<14} {:<4} {}\n",
                s.id, mode, phase, s.progress_pct, s.status_message));
        }
    }

    out.into_bytes()
}

fn gen_changetrack() -> Vec<u8> {
    use crate::fs::changetrack;
    let cursors = changetrack::list();
    let mut out = String::with_capacity(512);

    out.push_str(&format!("Change tracking cursors: {}\n\n", cursors.len()));

    if cursors.is_empty() {
        out.push_str("(no cursors registered)\n");
    } else {
        out.push_str(&format!("{:<20} {:<10} {:<10}\n", "NAME", "LAST_SEQ", "ADVANCES"));
        for c in &cursors {
            out.push_str(&format!("{:<20} {:<10} {:<10}\n", c.name, c.last_seq, c.advance_count));
        }
    }

    out.into_bytes()
}

fn gen_fcompress() -> Vec<u8> {
    use crate::fs::fcompress;
    let s = fcompress::stats();
    let mut out = String::with_capacity(512);

    out.push_str(&format!("Transparent compression: {}\n", if fcompress::is_enabled() { "enabled" } else { "disabled" }));
    out.push_str(&format!("  default algorithm: {}\n", fcompress::default_algorithm().name()));
    out.push_str(&format!("  min file size:     {} bytes\n\n", fcompress::min_size()));
    out.push_str(&format!("  files compressed:  {}\n", s.files_compressed));
    out.push_str(&format!("  files decompressed:{}\n", s.files_decompressed));
    out.push_str(&format!("  files skipped:     {}\n", s.files_skipped));
    out.push_str(&format!("  bytes original:    {}\n", s.bytes_original));
    out.push_str(&format!("  bytes stored:      {}\n", s.bytes_stored));
    out.push_str(&format!("  bytes delivered:   {}\n\n", s.bytes_delivered));

    let rules = fcompress::list_rules();
    out.push_str(&format!("  rules: {}\n", rules.len()));
    for r in &rules {
        let exts = if r.extensions.is_empty() {
            alloc::string::String::from("*")
        } else {
            r.extensions.join(",")
        };
        out.push_str(&format!("    {} -> {} (ext: {})\n", r.path_prefix, r.algorithm.name(), exts));
    }

    out.into_bytes()
}

fn gen_encryption() -> Vec<u8> {
    use crate::fs::encrypt;
    let (enc, dec, keys) = encrypt::stats();
    let key_list = encrypt::list_keys();
    let mut out = String::with_capacity(512);

    out.push_str("File encryption: ChaCha20 + HMAC-SHA256\n\n");
    out.push_str(&format!("  keys stored:      {}\n", keys));
    out.push_str(&format!("  files encrypted:  {}\n", enc));
    out.push_str(&format!("  files decrypted:  {}\n\n", dec));

    if !key_list.is_empty() {
        out.push_str("  Key names:\n");
        for k in &key_list {
            out.push_str(&format!("    {}\n", k.name));
        }
    }

    out.into_bytes()
}

fn gen_dedup() -> Vec<u8> {
    use crate::fs::dedup;
    let s = dedup::stats();
    let mut out = String::with_capacity(512);

    out.push_str(&format!("Deduplication: {}\n\n", if dedup::is_enabled() { "enabled" } else { "disabled" }));
    out.push_str(&format!("  scans run:       {}\n", s.scans_run));
    out.push_str(&format!("  total files:     {}\n", s.total_files));
    out.push_str(&format!("  dup groups:      {}\n", s.total_groups));
    out.push_str(&format!("  dup files:       {}\n", s.total_duplicates));
    out.push_str(&format!("  potential savings:{} bytes\n", s.total_savings));
    out.push_str(&format!("  active:          {}\n", s.active));

    out.into_bytes()
}

/// Generate `/proc/search` — file search engine statistics.
fn gen_search() -> Vec<u8> {
    use crate::fs::search;
    let (searches, results) = search::stats();
    let mut out = String::with_capacity(256);

    out.push_str("File Search Engine\n\n");
    out.push_str(&format!("  total searches:  {}\n", searches));
    out.push_str(&format!("  total results:   {}\n", results));
    if searches > 0 {
        out.push_str(&format!("  avg results:     {}\n", results / searches));
    }

    out.into_bytes()
}

/// Generate `/proc/tags` — file tagging system statistics.
fn gen_tags() -> Vec<u8> {
    use crate::fs::tags;
    let s = tags::stats();
    let mut out = String::with_capacity(512);

    out.push_str(&format!("File Tagging: {}\n\n", if tags::is_enabled() { "enabled" } else { "disabled" }));
    out.push_str(&format!("  unique tags:     {}\n", s.unique_tags));
    out.push_str(&format!("  tagged files:    {}\n", s.tagged_files));
    out.push_str(&format!("  associations:    {}\n", s.total_associations));
    out.push_str(&format!("  adds:            {}\n", s.adds));
    out.push_str(&format!("  removes:         {}\n", s.removes));
    out.push_str(&format!("  searches:        {}\n", s.searches));
    out.push_str(&format!("  index built:     {}\n", s.index_built));

    // List known tags if index is built.
    let all_tags = tags::list_tags();
    if !all_tags.is_empty() {
        out.push_str("\nKnown Tags:\n");
        for (tag, count) in &all_tags {
            out.push_str(&format!("  {:20} {} file(s)\n", tag, count));
        }
    }

    out.into_bytes()
}

/// Generate `/proc/usage` — disk usage analyzer statistics.
fn gen_usage() -> Vec<u8> {
    use crate::fs::usage;
    let mut out = String::with_capacity(1024);

    out.push_str(&format!("Disk Usage Analyzer ({} analyses run)\n\n", usage::analyses_run()));

    if let Some(report) = usage::last_report() {
        out.push_str(&format!("Last analysis: {}\n", report.root));
        out.push_str(&format!("  total size:   {}\n", usage::format_size(report.total_size)));
        out.push_str(&format!("  files:        {}\n", report.file_count));
        out.push_str(&format!("  directories:  {}\n", report.dir_count));
        out.push_str(&format!("  avg file:     {}\n", usage::format_size(report.avg_file_size)));
        out.push_str(&format!("  median file:  {}\n", usage::format_size(report.median_file_size)));

        if !report.top_dirs.is_empty() {
            out.push_str("\nTop Directories:\n");
            for d in report.top_dirs.iter().take(10) {
                out.push_str(&format!("  {:>10} {}\n", usage::format_size(d.size), d.path));
            }
        }

        if !report.by_extension.is_empty() {
            out.push_str("\nBy Extension:\n");
            for e in report.by_extension.iter().take(10) {
                out.push_str(&format!(
                    "  .{:8} {:>10} ({} files)\n",
                    e.extension,
                    usage::format_size(e.total_size),
                    e.count
                ));
            }
        }

        out.push_str("\nAge Distribution:\n");
        out.push_str(&format!("  <1 day:  {} files, {}\n", report.by_age.last_day.count, usage::format_size(report.by_age.last_day.size)));
        out.push_str(&format!("  <1 week: {} files, {}\n", report.by_age.last_week.count, usage::format_size(report.by_age.last_week.size)));
        out.push_str(&format!("  <1 month:{} files, {}\n", report.by_age.last_month.count, usage::format_size(report.by_age.last_month.size)));
        out.push_str(&format!("  <1 year: {} files, {}\n", report.by_age.last_year.count, usage::format_size(report.by_age.last_year.size)));
        out.push_str(&format!("  >1 year: {} files, {}\n", report.by_age.older.count, usage::format_size(report.by_age.older.size)));

        out.push_str("\nWasted Space:\n");
        out.push_str(&format!("  empty files:  {}\n", report.wasted.empty_files));
        out.push_str(&format!("  tiny files:   {} ({})\n", report.wasted.tiny_files, usage::format_size(report.wasted.tiny_size)));
        out.push_str(&format!("  dup names:    {}\n", report.wasted.duplicate_names));
    } else {
        out.push_str("(no analysis cached; run `diskuse` to analyze)\n");
    }

    out.into_bytes()
}

/// Generate `/proc/health` — filesystem health status.
fn gen_health() -> Vec<u8> {
    use crate::fs::health;
    let mut out = String::with_capacity(1024);

    out.push_str(&format!("Filesystem Health ({} checks run)\n\n", health::checks_run()));

    if let Some(report) = health::last_report() {
        out.push_str(&format!("Overall: {}\n", report.status.name()));
        out.push_str(&format!("  healthy:  {}\n", report.healthy));
        out.push_str(&format!("  warnings: {}\n", report.warnings));
        out.push_str(&format!("  critical: {}\n", report.critical));
        out.push_str("\nChecks:\n");
        for c in &report.checks {
            let icon = match c.status {
                health::HealthStatus::Healthy => "+",
                health::HealthStatus::Warning => "!",
                health::HealthStatus::Critical => "X",
            };
            out.push_str(&format!("  [{}] {:14} {}\n", icon, c.name, c.message));
            if let Some(ref rec) = c.recommendation {
                out.push_str(&format!("      -> {}\n", rec));
            }
        }
    } else {
        out.push_str("(no health check cached; run `fshealth` to check)\n");
    }

    out.into_bytes()
}

/// Generate `/proc/dirsync` — directory sync statistics.
fn gen_dirsync() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (comparisons, syncs) = super::dirsync::stats();

    out.push_str("Directory Sync Statistics\n");
    out.push_str("========================\n\n");
    out.push_str(&format!("Comparisons performed: {}\n", comparisons));
    out.push_str(&format!("Syncs performed:       {}\n", syncs));

    out.into_bytes()
}

/// Generate `/proc/backup` — backup engine statistics.
fn gen_backup() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (backups, restores, bytes) = super::backup::stats();

    out.push_str("Backup Statistics\n");
    out.push_str("=================\n\n");
    out.push_str(&format!("Backups created:  {}\n", backups));
    out.push_str(&format!("Restores done:    {}\n", restores));
    out.push_str(&format!("Bytes backed up:  {}\n", bytes));

    out.into_bytes()
}

/// Generate `/proc/undelete` — file recovery statistics.
fn gen_undelete() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (scans, recoveries, bytes) = super::undelete::stats();

    out.push_str("Undelete Statistics\n");
    out.push_str("===================\n\n");
    out.push_str(&format!("Scans performed: {}\n", scans));
    out.push_str(&format!("Recoveries:      {}\n", recoveries));
    out.push_str(&format!("Bytes recovered: {}\n", bytes));

    out.into_bytes()
}

/// Generate `/proc/archives` — archive manager statistics.
fn gen_archives() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (lists, extracts, creates) = super::archive::stats();

    out.push_str("Archive Manager Statistics\n");
    out.push_str("=========================\n\n");
    out.push_str(&format!("Listings:    {}\n", lists));
    out.push_str(&format!("Extractions: {}\n", extracts));
    out.push_str(&format!("Creations:   {}\n", creates));
    out.push_str("\nSupported formats: ZIP, TAR, CPIO, AR, RAR5, 7z\n");

    out.into_bytes()
}

/// Generate `/proc/batch` — batch operation statistics.
fn gen_batch() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (renames, copies, moves, deletes) = super::batch::stats();

    out.push_str("Batch Operation Statistics\n");
    out.push_str("=========================\n\n");
    out.push_str(&format!("Rename ops: {}\n", renames));
    out.push_str(&format!("Copy ops:   {}\n", copies));
    out.push_str(&format!("Move ops:   {}\n", moves));
    out.push_str(&format!("Delete ops: {}\n", deletes));

    out.into_bytes()
}

/// Generate `/proc/linkcheck` — link analysis statistics.
fn gen_linkcheck() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (checks, broken) = super::linkcheck::stats();

    out.push_str("Link Check Statistics\n");
    out.push_str("=====================\n\n");
    out.push_str(&format!("Checks performed:  {}\n", checks));
    out.push_str(&format!("Broken links found: {}\n", broken));

    out.into_bytes()
}

/// Generate `/proc/profile` — filesystem I/O profiling statistics.
fn gen_profile() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (total_ops, total_bytes, enabled) = super::profile::stats();

    out.push_str("Filesystem I/O Profile\n");
    out.push_str("======================\n\n");
    out.push_str(&format!("Status:      {}\n", if enabled { "enabled" } else { "disabled" }));
    out.push_str(&format!("Total ops:   {}\n", total_ops));
    out.push_str(&format!("Total bytes: {}\n", total_bytes));

    if enabled && total_ops > 0 {
        let rpt = super::profile::report();
        out.push_str(&format!("Duration:    {} ms\n\n", rpt.duration_ns / 1_000_000));

        out.push_str("Per-Operation Breakdown\n");
        out.push_str("-----------------------\n");
        for (kind, stats) in &rpt.ops {
            out.push_str(&format!(
                "  {:10} count={:<8} bytes={:<12} avg={:<8}ns min={:<8}ns max={}ns\n",
                kind.label(), stats.count, stats.bytes,
                stats.avg_ns(), stats.min_ns, stats.max_ns,
            ));
            if stats.bytes > 0 {
                let bps = stats.throughput_bps();
                if bps > 1_000_000 {
                    out.push_str(&format!("             throughput: {} MB/s\n", bps / 1_000_000));
                } else if bps > 1_000 {
                    out.push_str(&format!("             throughput: {} KB/s\n", bps / 1_000));
                } else {
                    out.push_str(&format!("             throughput: {} B/s\n", bps));
                }
            }
        }

        if !rpt.hot_paths.is_empty() {
            out.push_str("\nHot Paths (most accessed)\n");
            out.push_str("-------------------------\n");
            for (path, count) in &rpt.hot_paths {
                out.push_str(&format!("  {:6} {}\n", count, path));
            }
        }
    }

    out.into_bytes()
}

/// Generate `/proc/fspolicy` — filesystem policy engine status.
fn gen_fspolicy() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let profile = super::policy::current_profile();
    let stats = super::policy::stats();

    out.push_str("Filesystem Policy Engine\n");
    out.push_str("========================\n\n");
    out.push_str(&format!("Active profile:    {}\n",
        match profile {
            Some(p) => p.label(),
            None => "custom (manually tuned)",
        }));
    out.push_str(&format!("Profiles applied:  {}\n", stats.profiles_applied));
    out.push_str(&format!("Settings changed:  {}\n", stats.settings_changed));
    out.push_str(&format!("Settings queried:  {}\n\n", stats.settings_queried));

    out.push_str("Current Settings\n");
    out.push_str("----------------\n");
    let settings = super::policy::list_settings();
    for s in &settings {
        out.push_str(&format!("  {:28} = {:8}  # {}\n", s.key, s.value, s.description));
    }

    out.push_str("\nProfile Presets\n");
    out.push_str("---------------\n");
    out.push_str(&format!("  {:28} {:>8} {:>8} {:>8} {:>8}\n",
        "SETTING", "Desktop", "Server", "Dev", "Gaming"));
    for s in &settings {
        out.push_str(&format!("  {:28} {:>8} {:>8} {:>8} {:>8}\n",
            s.key, s.presets[0], s.presets[1], s.presets[2], s.presets[3]));
    }

    out.into_bytes()
}

/// Generate `/proc/fsbench` — filesystem benchmark results.
fn gen_fsbench() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (runs, last_ns) = super::bench::stats();

    out.push_str("Filesystem Benchmarks\n");
    out.push_str("=====================\n\n");
    out.push_str(&format!("Suites run:     {}\n", runs));
    out.push_str(&format!("Last suite:     {} ms\n\n", last_ns / 1_000_000));
    out.push_str("Targets (from design spec):\n");
    out.push_str("  Path lookup:      500 ns/component (cached)\n");
    out.push_str("  Metadata cycle:   10,000 ns (create+stat+delete)\n");
    out.push_str("  File open:        5,000 ns (cached path)\n");
    out.push_str("  Small read (4K):  2,000 ns\n\n");
    out.push_str("Run `fsbench all` in kshell for full results.\n");

    out.into_bytes()
}

/// Generate `/proc/ioprio` — I/O priority assignments.
fn gen_ioprio() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (sets, gets, active) = super::ioprio::stats();

    out.push_str("I/O Priority Management\n");
    out.push_str("=======================\n\n");
    out.push_str(&format!("Active entries: {}/{}\n", active, 256));
    out.push_str(&format!("Set calls:      {}\n", sets));
    out.push_str(&format!("Get calls:      {}\n\n", gets));

    let all = super::ioprio::list_all();
    if all.is_empty() {
        out.push_str("No explicit I/O priorities set (all tasks use default: best-effort:4)\n");
    } else {
        out.push_str(&format!("{:>6} {:>12} {:>6}\n", "TASK", "CLASS", "LEVEL"));
        for (tid, prio) in &all {
            out.push_str(&format!("{:>6} {:>12} {:>6}\n",
                tid, prio.class.label(), prio.level));
        }
    }

    out.into_bytes()
}

/// Generate `/proc/atime` — access time policy status.
fn gen_atime() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let policy = super::atime::global_policy();
    let st = super::atime::stats();
    let overrides = super::atime::list_overrides();

    out.push_str("Access Time (atime) Policy\n");
    out.push_str("==========================\n\n");
    out.push_str(&format!("Global policy: {}\n", policy.label()));
    out.push_str(&format!("Checks:        {}\n", st.checks));
    out.push_str(&format!("Updates:       {}\n", st.updates));
    out.push_str(&format!("Skipped:       {}\n", st.skipped));
    if st.checks > 0 {
        let skip_pct = (st.skipped * 100) / st.checks;
        out.push_str(&format!("Skip rate:     {}%\n", skip_pct));
    }

    if !overrides.is_empty() {
        out.push_str("\nPer-mount overrides:\n");
        for ovr in &overrides {
            out.push_str(&format!("  {:20} → {}\n", ovr.mount_path, ovr.policy.label()));
        }
    }

    out.push_str("\nAvailable policies: always, relatime, noatime, lazyday\n");

    out.into_bytes()
}

/// Generate `/proc/prefetch` — file prefetch/advisory status.
fn gen_prefetch() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (advises, prefetches, bytes, active) = super::prefetch::stats();

    out.push_str("File Prefetch / Access Advisory\n");
    out.push_str("===============================\n\n");
    out.push_str(&format!("Active advice entries: {}/{}\n", active, 256));
    out.push_str(&format!("Advise calls:         {}\n", advises));
    out.push_str(&format!("Prefetch calls:       {}\n", prefetches));
    out.push_str(&format!("Bytes prefetched:     {}\n\n", bytes));

    let entries = super::prefetch::list_active();
    if entries.is_empty() {
        out.push_str("No active advice entries.\n");
    } else {
        out.push_str(&format!("{:40} {}\n", "PATH", "ADVICE"));
        for (path, advice) in &entries {
            out.push_str(&format!("{:40} {}\n", path, advice.label()));
        }
    }

    out.into_bytes()
}

/// Generate `/proc/splice` — zero-copy I/O transfer statistics.
fn gen_splice() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let s = super::splice::stats();
    let total_ops = s.splice_ops + s.sendfile_ops + s.copy_range_ops + s.tee_ops;
    let total_bytes = s.splice_bytes + s.sendfile_bytes + s.copy_range_bytes + s.tee_bytes;

    out.push_str("Zero-Copy I/O Transfer (splice)\n");
    out.push_str("===============================\n\n");
    out.push_str(&format!("{:20} {:>10} {:>12}\n", "OPERATION", "OPS", "BYTES"));
    out.push_str(&format!("{:20} {:>10} {:>12}\n", "splice", s.splice_ops, s.splice_bytes));
    out.push_str(&format!("{:20} {:>10} {:>12}\n", "sendfile", s.sendfile_ops, s.sendfile_bytes));
    out.push_str(&format!("{:20} {:>10} {:>12}\n", "copy_file_range", s.copy_range_ops, s.copy_range_bytes));
    out.push_str(&format!("{:20} {:>10} {:>12}\n", "tee", s.tee_ops, s.tee_bytes));
    out.push_str(&format!("{:20} {:>10} {:>12}\n", "TOTAL", total_ops, total_bytes));
    out.push_str(&format!("\nErrors: {}\n", s.errors));

    out.into_bytes()
}

/// Generate `/proc/directio` — direct I/O statistics and registered paths.
fn gen_directio() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (reads, writes, rbytes, wbytes, unaligned, invalidations, path_count) =
        super::directio::stats();

    out.push_str("Direct I/O (Cache Bypass)\n");
    out.push_str("========================\n\n");
    out.push_str(&format!("Read ops:       {:>10}  ({} bytes)\n", reads, rbytes));
    out.push_str(&format!("Write ops:      {:>10}  ({} bytes)\n", writes, wbytes));
    out.push_str(&format!("Unaligned ops:  {:>10}\n", unaligned));
    out.push_str(&format!("Cache inv.:     {:>10}\n", invalidations));
    out.push_str(&format!("Registered paths: {}/{}\n\n", path_count, 128));

    let paths = super::directio::list_paths();
    if paths.is_empty() {
        out.push_str("No registered direct-I/O paths.\n");
    } else {
        out.push_str("Registered paths:\n");
        for p in &paths {
            out.push_str(&format!("  {}\n", p));
        }
    }

    out.into_bytes()
}

/// Generate `/proc/fstrim` — SSD TRIM/discard status.
fn gen_fstrim() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (trims, bytes, queued, coalesced, overflows, pending, last_flush) =
        super::fstrim::stats();
    let mode = super::fstrim::get_mode();

    out.push_str("Filesystem TRIM/DISCARD\n");
    out.push_str("======================\n\n");
    out.push_str(&format!("Mode:             {}\n", mode.label()));
    out.push_str(&format!("Pending ranges:   {}\n", pending));
    out.push_str(&format!("Total TRIMs:      {}\n", trims));
    out.push_str(&format!("Bytes trimmed:    {}\n", bytes));
    out.push_str(&format!("Ranges queued:    {}\n", queued));
    out.push_str(&format!("Coalesced:        {}\n", coalesced));
    out.push_str(&format!("Queue overflows:  {}\n", overflows));
    out.push_str(&format!("Last flush (ns):  {}\n", last_flush));

    let summary = super::fstrim::pending_summary();
    if !summary.is_empty() {
        out.push_str(&format!("\n{:20} {:>8} {:>12}\n", "DEVICE", "RANGES", "BYTES"));
        for (dev, count, bytes) in &summary {
            out.push_str(&format!("{:20} {:>8} {:>12}\n", dev, count, bytes));
        }
    }

    out.into_bytes()
}

/// Generate `/proc/fstune` — filesystem tuning profiles and parameters.
fn gen_fstune() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (profile_count, tradeoff_count, applied_count, ops) = super::fstune::stats();

    out.push_str("Filesystem Tuning\n");
    out.push_str("=================\n\n");
    out.push_str(&format!("Profiles:      {}\n", profile_count));
    out.push_str(&format!("Applied:       {}\n", applied_count));
    out.push_str(&format!("Tradeoffs:     {}\n", tradeoff_count));
    out.push_str(&format!("Operations:    {}\n", ops));

    let profiles = super::fstune::list_profiles();
    if !profiles.is_empty() {
        out.push_str(&format!("\n{:<20} {:<8} {:<12} {:<10} {:<10} {}\n",
            "NAME", "FS", "WORKLOAD", "BLOCK", "JOURNAL", "APPLIED"));
        for p in &profiles {
            let fs = match p.fs_type {
                super::fstune::FsType::Ext4 => "ext4",
                super::fstune::FsType::Btrfs => "btrfs",
                super::fstune::FsType::Xfs => "xfs",
                super::fstune::FsType::F2fs => "f2fs",
                super::fstune::FsType::Fat32 => "fat32",
            };
            let wl = match p.workload {
                super::fstune::WorkloadType::Desktop => "desktop",
                super::fstune::WorkloadType::Database => "database",
                super::fstune::WorkloadType::Server => "server",
                super::fstune::WorkloadType::Development => "dev",
                super::fstune::WorkloadType::Gaming => "gaming",
            };
            let jm = match p.journal_mode {
                super::fstune::JournalMode::Ordered => "ordered",
                super::fstune::JournalMode::Journal => "journal",
                super::fstune::JournalMode::Writeback => "writeback",
                super::fstune::JournalMode::Off => "off",
            };
            let app = if p.applied { "yes" } else { "no" };
            out.push_str(&format!("{:<20} {:<8} {:<12} {:<10} {:<10} {}\n",
                p.name, fs, wl, p.block_size, jm, app));
        }
    }

    out.into_bytes()
}

/// Generate `/proc/fontmgr` — font registry status.
fn gen_fontmgr() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (total, families, system, ops) = super::fontmgr::stats();
    let defs = super::fontmgr::default_fonts();
    let rs = super::fontmgr::render_settings();

    out.push_str("Font Manager\n");
    out.push_str("============\n\n");
    out.push_str(&format!("Total fonts:  {}\n", total));
    out.push_str(&format!("Families:     {}\n", families));
    out.push_str(&format!("System fonts: {}\n", system));
    out.push_str(&format!("Operations:   {}\n", ops));
    out.push_str("\nDefaults:\n");
    out.push_str(&format!("  UI:         {}\n", defs.ui));
    out.push_str(&format!("  Document:   {}\n", defs.document));
    out.push_str(&format!("  Monospace:  {}\n", defs.monospace));
    out.push_str(&format!("  Titlebar:   {}\n", defs.titlebar));
    out.push_str(&format!("  Fallback:   {}\n", defs.fallback));
    out.push_str("\nRendering:\n");
    out.push_str(&format!("  Size:       {} pt\n", rs.global_size_pt));
    out.push_str(&format!("  Hinting:    {:?}\n", rs.hint_mode));
    out.push_str(&format!("  Antialias:  {:?}\n", rs.antialias));
    out.push_str(&format!("  Subpixel:   {:?}\n", rs.subpixel_order));
    out.push_str(&format!("  DPI:        {}\n", rs.dpi));

    let fonts = super::fontmgr::list_fonts(None);
    if !fonts.is_empty() {
        out.push_str(&format!("\n{:<6} {:<20} {:<12} {:<10} {:<8} {}\n",
            "ID", "FAMILY", "STYLE", "FMT", "GLYPHS", "SYS"));
        for f in &fonts {
            let st = match f.style {
                super::fontmgr::FontStyle::Regular => "regular",
                super::fontmgr::FontStyle::Bold => "bold",
                super::fontmgr::FontStyle::Italic => "italic",
                super::fontmgr::FontStyle::BoldItalic => "bold-italic",
                super::fontmgr::FontStyle::Light => "light",
                super::fontmgr::FontStyle::Medium => "medium",
                super::fontmgr::FontStyle::SemiBold => "semibold",
                super::fontmgr::FontStyle::ExtraBold => "extrabold",
                super::fontmgr::FontStyle::Thin => "thin",
            };
            let fmt = match f.format { super::fontmgr::FontFormat::TrueType => "ttf", super::fontmgr::FontFormat::OpenType => "otf", super::fontmgr::FontFormat::Woff => "woff", super::fontmgr::FontFormat::Woff2 => "woff2", super::fontmgr::FontFormat::Bitmap => "bmp" };
            out.push_str(&format!("{:<6} {:<20} {:<12} {:<10} {:<8} {}\n",
                f.id, f.family, st, fmt, f.glyph_count, if f.system { "yes" } else { "no" }));
        }
    }

    out.into_bytes()
}

/// Generate `/proc/sparse` — sparse file management status.
fn gen_sparse() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (punches, punch_bytes, zeros, collapses, inserts, maps, tracked) =
        super::sparse::stats();

    out.push_str("Sparse File Management\n");
    out.push_str("======================\n\n");
    out.push_str(&format!("Tracked files:    {}/{}\n", tracked, 256));
    out.push_str(&format!("Punch holes:      {} ({} bytes)\n", punches, punch_bytes));
    out.push_str(&format!("Zero ranges:      {}\n", zeros));
    out.push_str(&format!("Collapse ranges:  {}\n", collapses));
    out.push_str(&format!("Insert ranges:    {}\n", inserts));
    out.push_str(&format!("Map queries:      {}\n\n", maps));

    let files = super::sparse::list_tracked();
    if files.is_empty() {
        out.push_str("No tracked sparse files.\n");
    } else {
        out.push_str(&format!("{:40} {:>6}\n", "PATH", "HOLES"));
        for (path, holes) in &files {
            out.push_str(&format!("{:40} {:>6}\n", path, holes));
        }
    }

    out.into_bytes()
}

/// Generate `/proc/readdir_plus` — enhanced listing statistics.
fn gen_readdir_plus() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (calls, entries, fetched, errors) = super::readdir_plus::stats();

    out.push_str("Enhanced Directory Listing (readdir+)\n");
    out.push_str("=====================================\n\n");
    out.push_str(&format!("Calls:            {}\n", calls));
    out.push_str(&format!("Entries returned:  {}\n", entries));
    out.push_str(&format!("Metadata fetched:  {}\n", fetched));
    out.push_str(&format!("Metadata errors:   {}\n", errors));
    if calls > 0 {
        out.push_str(&format!("Avg entries/call: {:.1}\n", entries as f64 / calls as f64));
    }

    out.into_bytes()
}

/// Generate `/proc/freeze` — filesystem freeze/thaw status.
fn gen_freeze() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (freezes, thaws, auto_thaws, blocked, frozen) = super::freeze::stats();

    out.push_str("Filesystem Freeze/Thaw\n");
    out.push_str("======================\n\n");
    out.push_str(&format!("Currently frozen: {}/{}\n", frozen, 16));
    out.push_str(&format!("Freeze ops:       {}\n", freezes));
    out.push_str(&format!("Thaw ops:         {}\n", thaws));
    out.push_str(&format!("Auto-thaws:       {}\n", auto_thaws));
    out.push_str(&format!("Blocked writes:   {}\n\n", blocked));

    let list = super::freeze::list_frozen();
    if list.is_empty() {
        out.push_str("No frozen filesystems.\n");
    } else {
        out.push_str(&format!("{:20} {:>5} {:>12} {:>12} {:>8} {}\n",
            "MOUNTPOINT", "LEVEL", "DURATION", "UNTIL_THAW", "BLOCKED", "REASON"));
        for entry in &list {
            let dur_s = entry.frozen_duration_ns / 1_000_000_000;
            let until_s = entry.time_until_thaw_ns / 1_000_000_000;
            out.push_str(&format!("{:20} {:>5} {:>10}s {:>10}s {:>8} {}\n",
                entry.mountpoint, entry.freeze_level,
                dur_s, until_s, entry.blocked_writes, entry.reason));
        }
    }

    out.into_bytes()
}

/// Generate `/proc/sealing` — file sealing status.
fn gen_sealing() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (seal_ops, check_ops, denied, sealed_count) = super::sealing::stats();

    out.push_str("File Sealing\n");
    out.push_str("============\n\n");
    out.push_str(&format!("Sealed files:    {}/{}\n", sealed_count, 512));
    out.push_str(&format!("Seal operations: {}\n", seal_ops));
    out.push_str(&format!("Seal checks:     {}\n", check_ops));
    out.push_str(&format!("Denied ops:      {}\n\n", denied));

    let files = super::sealing::list_sealed();
    if files.is_empty() {
        out.push_str("No sealed files.\n");
    } else {
        out.push_str(&format!("{:40} {}\n", "PATH", "SEALS"));
        for (path, flags) in &files {
            out.push_str(&format!("{:40} {}\n", path, flags.label()));
        }
    }

    out.into_bytes()
}

fn gen_recent() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (recorded, queried, evicted, excluded, count, enabled) = super::recent::stats();

    out.push_str("Recent Files Tracking\n");
    out.push_str("=====================\n\n");
    out.push_str(&format!("Status:          {}\n", if enabled { "enabled" } else { "disabled" }));
    out.push_str(&format!("Tracked entries: {}/{}\n", count, 1024));
    out.push_str(&format!("Recorded:        {}\n", recorded));
    out.push_str(&format!("Queried:         {}\n", queried));
    out.push_str(&format!("Evicted:         {}\n", evicted));
    out.push_str(&format!("Excluded:        {}\n\n", excluded));

    let retention_ns = super::recent::get_retention_ns();
    let retention_days = retention_ns / (24 * 60 * 60 * 1_000_000_000);
    out.push_str(&format!("Retention:       {} days\n\n", retention_days));

    let entries = super::recent::most_recent(20);
    if entries.is_empty() {
        out.push_str("No recent files.\n");
    } else {
        out.push_str(&format!("{:40} {:8} {:>5} {}\n", "PATH", "TYPE", "COUNT", "SOURCE"));
        for e in &entries {
            out.push_str(&format!(
                "{:40} {:8} {:>5} {}\n",
                e.path, e.access_type.label(), e.access_count, e.source,
            ));
        }
    }

    out.into_bytes()
}

fn gen_fileinfo() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (extractions, fields, errors) = super::fileinfo::stats();

    out.push_str("File Info Metadata Extraction\n");
    out.push_str("=============================\n\n");
    out.push_str(&format!("Extractions: {}\n", extractions));
    out.push_str(&format!("Fields:      {}\n", fields));
    out.push_str(&format!("Errors:      {}\n\n", errors));

    out.push_str("Supported formats:\n");
    out.push_str("  audio/mpeg    — MP3 (ID3v1, ID3v2, MPEG frame)\n");
    out.push_str("  audio/wav     — WAV (RIFF/PCM headers)\n");
    out.push_str("  image/jpeg    — JPEG (EXIF, SOF dimensions)\n");
    out.push_str("  image/png     — PNG (IHDR, tEXt chunks)\n");
    out.push_str("  image/gif     — GIF (dimensions, version)\n");
    out.push_str("  image/bmp     — BMP (dimensions, bit depth)\n");
    out.push_str("  application/pdf — PDF (version, linearized)\n");
    out.push_str("  application/x-elf — ELF (class, machine, type)\n");

    out.into_bytes()
}

fn gen_fswalk() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (walks, entries, errors) = super::fswalk::stats();

    out.push_str("Filesystem Walk Engine\n");
    out.push_str("======================\n\n");
    out.push_str(&format!("Total walks:    {}\n", walks));
    out.push_str(&format!("Entries walked: {}\n", entries));
    out.push_str(&format!("Errors:         {}\n\n", errors));

    out.push_str("Traversal modes: DepthFirst, BreadthFirst\n");
    out.push_str("Filters:         All, FilesOnly, DirsOnly, SymlinksOnly\n");
    out.push_str(&format!("Max queue:       {} pending dirs\n", 8192));
    out.push_str(&format!("Max results:     {}\n", 65536));
    out.push_str(&format!("Default depth:   {}\n", 64));
    out.push_str("Default excl:    /proc, /sys, /dev\n");

    out.into_bytes()
}

fn gen_findex() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (builds, index_ops, queries, indexed, fields) = super::findex::stats();

    out.push_str("File Metadata Index\n");
    out.push_str("===================\n\n");
    out.push_str(&format!("Indexed files: {}/{}\n", indexed, 16384));
    out.push_str(&format!("Known fields:  {}/{}\n", fields, 256));
    out.push_str(&format!("Builds:        {}\n", builds));
    out.push_str(&format!("Index ops:     {}\n", index_ops));
    out.push_str(&format!("Queries:       {}\n\n", queries));

    let known = super::findex::known_fields();
    if !known.is_empty() {
        out.push_str("Known field names:\n");
        for (name, label) in &known {
            out.push_str(&format!("  {:30} {}\n", name, label));
        }
    }

    out.into_bytes()
}

fn gen_thumbcache() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (hits, misses, stores, evicts, count, mem) = super::thumbcache::stats();
    let hit_rate = if hits + misses > 0 {
        (hits * 100) / (hits + misses)
    } else {
        0
    };

    out.push_str("Thumbnail Cache\n");
    out.push_str("===============\n\n");
    out.push_str(&format!("Cached:     {}/{}\n", count, 2048));
    out.push_str(&format!("Memory:     {} / {} bytes\n", mem, 16 * 1024 * 1024));
    out.push_str(&format!("Hit rate:   {}% ({} hits, {} misses)\n", hit_rate, hits, misses));
    out.push_str(&format!("Stores:     {}\n", stores));
    out.push_str(&format!("Evictions:  {}\n", evicts));

    out.into_bytes()
}

fn gen_bookmarks() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (resolves, adds, count) = super::bookmarks::stats();

    out.push_str("Filesystem Bookmarks\n");
    out.push_str("====================\n\n");
    out.push_str(&format!("Bookmarks: {}/{}\n", count, 128));
    out.push_str(&format!("Resolves:  {}\n", resolves));
    out.push_str(&format!("Adds:      {}\n\n", adds));

    let bookmarks = super::bookmarks::list_visible();
    if !bookmarks.is_empty() {
        out.push_str(&format!("{:12} {:8} {:30} {}\n", "NAME", "CAT", "PATH", "LABEL"));
        for bm in &bookmarks {
            out.push_str(&format!("{:12} {:8} {:30} {}\n",
                bm.name, bm.category.label(), bm.path, bm.label));
        }
    }

    out.into_bytes()
}

fn gen_clipboard() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (copies, pastes, total_bytes, seq, hist_count, watchers) = super::clipboard::stats();

    out.push_str("Clipboard\n");
    out.push_str("=========\n\n");
    out.push_str(&format!("Sequence:     {}\n", seq));
    out.push_str(&format!("Copies:       {}\n", copies));
    out.push_str(&format!("Pastes:       {}\n", pastes));
    out.push_str(&format!("Total bytes:  {}\n", total_bytes));
    out.push_str(&format!("History:      {}/{}\n", hist_count, 32));
    out.push_str(&format!("Watchers:     {}/{}\n\n", watchers, 16));

    let formats = super::clipboard::available_formats();
    if formats.is_empty() {
        out.push_str("Clipboard is empty.\n");
    } else {
        out.push_str("Current formats:\n");
        for f in &formats {
            out.push_str(&format!("  {}\n", f.mime()));
        }
    }

    out.into_bytes()
}

fn gen_dragdrop() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (drags, drops, cancels, total_bytes, zone_count) = super::dragdrop::stats();

    out.push_str("Drag and Drop\n");
    out.push_str("=============\n\n");
    out.push_str(&format!("Drags:        {}\n", drags));
    out.push_str(&format!("Drops:        {}\n", drops));
    out.push_str(&format!("Cancels:      {}\n", cancels));
    out.push_str(&format!("Total bytes:  {}\n", total_bytes));
    out.push_str(&format!("Drop zones:   {}/{}\n\n", zone_count, 256));

    let active = super::dragdrop::is_dragging();
    out.push_str(&format!("Active drag:  {}\n", if active { "yes" } else { "no" }));

    if let Some(session) = super::dragdrop::current_session() {
        out.push_str(&format!("  Source:     {}\n", session.source));
        out.push_str(&format!("  Formats:    {}\n", session.offered_formats.len()));
        out.push_str(&format!("  Cursor:     ({}, {})\n", session.cursor.0, session.cursor.1));
    }

    out.into_bytes()
}

fn gen_fileops() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (total, completed, cancelled, bytes_moved) = super::fileops::stats();

    out.push_str("File Operations\n");
    out.push_str("===============\n\n");
    out.push_str(&format!("Total ops:    {}\n", total));
    out.push_str(&format!("Completed:    {}\n", completed));
    out.push_str(&format!("Cancelled:    {}\n", cancelled));
    out.push_str(&format!("Bytes moved:  {}\n\n", bytes_moved));

    let ops = super::fileops::list_ops();
    if !ops.is_empty() {
        out.push_str(&format!("{:6} {:6} {:10} {}\n", "ID", "KIND", "STATE", "LABEL"));
        for (id, kind, state, label) in &ops {
            let state_str = match state {
                super::fileops::OpState::Queued => "queued",
                super::fileops::OpState::Running => "running",
                super::fileops::OpState::Paused => "paused",
                super::fileops::OpState::Completed => "done",
                super::fileops::OpState::Cancelled => "cancelled",
                super::fileops::OpState::Undoing => "undoing",
            };
            out.push_str(&format!("{:6} {:6} {:10} {}\n", id, kind.label(), state_str, label));
        }
    }

    out.into_bytes()
}

fn gen_preview() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (generate_calls, cache_hits, failures, total_bytes) = super::preview::stats();

    out.push_str("Preview Generation\n");
    out.push_str("==================\n\n");
    out.push_str(&format!("Generate calls: {}\n", generate_calls));
    out.push_str(&format!("Cache hits:     {}\n", cache_hits));
    out.push_str(&format!("Failures:       {}\n", failures));
    out.push_str(&format!("Bytes generated:{}\n\n", total_bytes));

    let generators = super::preview::list_generators();
    if !generators.is_empty() {
        out.push_str("Custom generators:\n");
        for g in &generators {
            out.push_str(&format!("  {} ({}): {}\n",
                g.id, g.app_name,
                g.mime_types.join(", ")));
        }
    }

    out.into_bytes()
}

fn gen_templates() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (count, creates, total_bytes) = super::templates::stats();

    out.push_str("File Templates\n");
    out.push_str("==============\n\n");
    out.push_str(&format!("Templates:   {}/{}\n", count, 256));
    out.push_str(&format!("Creates:     {}\n", creates));
    out.push_str(&format!("Bytes:       {}\n\n", total_bytes));

    let templates = super::templates::list();
    if !templates.is_empty() {
        out.push_str(&format!("{:6} {:12} {:24} {:8} {}\n", "ID", "CATEGORY", "NAME", "EXT", "SOURCE"));
        for t in &templates {
            out.push_str(&format!("{:6} {:12} {:24} {:8} {}\n",
                t.id, t.category.label(), t.name, t.extension, t.source));
        }
    }

    out.into_bytes()
}

fn gen_toolbar() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (build_count, action_count) = super::toolbar::stats();

    out.push_str("File Explorer Toolbar\n");
    out.push_str("=====================\n\n");
    out.push_str(&format!("Builds:      {}\n", build_count));
    out.push_str(&format!("Actions:     {}\n\n", action_count));

    // Show default toolbar layout.
    let ctx = super::toolbar::ToolbarContext::default();
    let layout = super::toolbar::build(&ctx);
    out.push_str(&format!("Default buttons: {}\n\n", layout.buttons.len()));
    out.push_str(&format!("{:16} {:12} {:8} {:8} {}\n",
        "ACTION", "SECTION", "ENABLED", "TOGGLE", "LABEL"));
    for btn in &layout.buttons {
        let sec = format!("{:?}", btn.section);
        out.push_str(&format!("{:16} {:12} {:8} {:8} {}\n",
            btn.action, sec, btn.enabled, btn.toggled, btn.label));
    }

    out.into_bytes()
}

fn gen_queryable() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (files, total_attrs, sets, gets, queries, indexes) = super::queryable::stats();

    out.push_str("Queryable File Metadata (BFS-inspired)\n");
    out.push_str("======================================\n\n");
    out.push_str(&format!("Files:       {}/{}\n", files, 65536));
    out.push_str(&format!("Attributes:  {}\n", total_attrs));
    out.push_str(&format!("Indexes:     {}/{}\n", indexes, 1024));
    out.push_str(&format!("Set ops:     {}\n", sets));
    out.push_str(&format!("Get ops:     {}\n", gets));
    out.push_str(&format!("Queries:     {}\n\n", queries));

    let indexed = super::queryable::list_indexes();
    if !indexed.is_empty() {
        out.push_str("Indexed attributes:\n");
        for name in &indexed {
            out.push_str(&format!("  {}\n", name));
        }
        out.push('\n');
    }

    let schemas = super::queryable::list_schemas();
    if !schemas.is_empty() {
        out.push_str(&format!("Schemas: {}\n", schemas.len()));
        out.push_str(&format!("{:30} {:8} {:8} {}\n", "NAME", "TYPE", "INDEXED", "DESCRIPTION"));
        for s in &schemas {
            let idx = if s.indexed { "yes" } else { "no" };
            out.push_str(&format!("{:30} {:8} {:8} {}\n", s.name, s.value_type, idx, s.description));
        }
    }

    out.into_bytes()
}

fn gen_immutable() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (flagged, set_ops, check_ops) = super::immutable::stats();

    out.push_str("Immutable / Append-Only File Flags\n");
    out.push_str("==================================\n\n");
    out.push_str(&format!("Flagged files: {}/{}\n", flagged, 65536));
    out.push_str(&format!("Set ops:       {}\n", set_ops));
    out.push_str(&format!("Check ops:     {}\n\n", check_ops));

    let flagged_files = super::immutable::list_flagged();
    if !flagged_files.is_empty() {
        out.push_str(&format!("{:40} {}\n", "PATH", "FLAGS"));
        for (path, flags) in &flagged_files {
            out.push_str(&format!("{:40} {}\n", path, super::immutable::flags_to_string(*flags)));
        }
    }

    out.into_bytes()
}

fn gen_fcomment() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (comment_count, set_ops, get_ops, search_ops) = super::fcomment::stats();

    out.push_str("File Comments\n");
    out.push_str("=============\n\n");
    out.push_str(&format!("Comments:    {}/{}\n", comment_count, 65536));
    out.push_str(&format!("Set ops:     {}\n", set_ops));
    out.push_str(&format!("Get ops:     {}\n", get_ops));
    out.push_str(&format!("Search ops:  {}\n\n", search_ops));

    let all = super::fcomment::list(None);
    if !all.is_empty() {
        out.push_str(&format!("{:40} {:8} {}\n", "PATH", "LENGTH", "PREVIEW"));
        for (path, comment) in &all {
            let preview: String = comment.chars().take(40).collect();
            let preview = preview.replace('\n', " ");
            out.push_str(&format!("{:40} {:8} {}\n", path, comment.len(), preview));
        }
    }

    out.into_bytes()
}

fn gen_rundialog() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (recent_count, alias_count, cache_count, bookmark_count, runs, completions) =
        super::rundialog::stats();

    out.push_str("Run Dialog (Ctrl+R)\n");
    out.push_str("===================\n\n");
    out.push_str(&format!("Recent:      {}/{}\n", recent_count, 256));
    out.push_str(&format!("Aliases:     {}/{}\n", alias_count, 512));
    out.push_str(&format!("PATH cache:  {}\n", cache_count));
    out.push_str(&format!("Bookmarks:   {}/{}\n", bookmark_count, 64));
    out.push_str(&format!("Run ops:     {}\n", runs));
    out.push_str(&format!("Completions: {}\n\n", completions));

    let recent = super::rundialog::recent(10);
    if !recent.is_empty() {
        out.push_str("Recent commands:\n");
        for cmd in &recent {
            out.push_str(&format!("  {} (x{}) → {}\n",
                cmd.command, cmd.run_count, cmd.resolved_path));
        }
        out.push('\n');
    }

    let aliases = super::rundialog::list_aliases();
    if !aliases.is_empty() {
        out.push_str("Aliases:\n");
        for (name, target) in &aliases {
            out.push_str(&format!("  {} → {}\n", name, target));
        }
    }

    out.into_bytes()
}

fn gen_notifcenter() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (total, unread_n, muted, sends, dismisses) = super::notifcenter::stats();

    out.push_str("Notification Center\n");
    out.push_str("===================\n\n");
    out.push_str(&format!("Total:       {}/{}\n", total, 1024));
    out.push_str(&format!("Unread:      {}\n", unread_n));
    out.push_str(&format!("Muted apps:  {}\n", muted));
    out.push_str(&format!("Send ops:    {}\n", sends));
    out.push_str(&format!("Dismiss ops: {}\n\n", dismisses));

    let summaries = super::notifcenter::app_summaries();
    if !summaries.is_empty() {
        out.push_str(&format!("{:20} {:6} {:6} {:6}\n", "APP", "TOTAL", "UNREAD", "MUTED"));
        for s in &summaries {
            let muted_s = if s.muted { "yes" } else { "no" };
            out.push_str(&format!("{:20} {:6} {:6} {:6}\n", s.app, s.total, s.unread, muted_s));
        }
    }

    out.into_bytes()
}

fn gen_appregistry() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (app_count, mime_count, register_ops, lookup_ops) = super::appregistry::stats();

    out.push_str("Application Registry\n");
    out.push_str("====================\n\n");
    out.push_str(&format!("Apps:         {}/{}\n", app_count, 4096));
    out.push_str(&format!("MIME types:   {}\n", mime_count));
    out.push_str(&format!("Register ops: {}\n", register_ops));
    out.push_str(&format!("Lookup ops:   {}\n\n", lookup_ops));

    let tree = super::appregistry::menu_tree();
    if !tree.is_empty() {
        for (cat, entries) in &tree {
            out.push_str(&format!("[{}]\n", cat.label()));
            for entry in entries {
                out.push_str(&format!("  {} ({})\n", entry.name, entry.exec_path));
            }
        }
    }

    out.into_bytes()
}

fn gen_systray() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (icon_count, override_count, add_ops, click_ops) = super::systray::stats();

    out.push_str("System Tray\n");
    out.push_str("===========\n\n");
    out.push_str(&format!("Icons:     {}/{}\n", icon_count, 128));
    out.push_str(&format!("Overrides: {}/{}\n", override_count, 256));
    out.push_str(&format!("Add ops:   {}\n", add_ops));
    out.push_str(&format!("Click ops: {}\n\n", click_ops));

    let visible = super::systray::visible_icons();
    if !visible.is_empty() {
        out.push_str(&format!("{:20} {:20} {:8} {}\n", "ID", "TOOLTIP", "ORDER", "BADGE"));
        for icon in &visible {
            let badge = icon.badge.as_deref().unwrap_or("-");
            out.push_str(&format!("{:20} {:20} {:8} {}\n",
                icon.id, icon.tooltip, icon.order, badge));
        }
    }

    let overrides = super::systray::list_overrides();
    if !overrides.is_empty() {
        out.push_str("\nOverrides:\n");
        for (app_id, ov) in &overrides {
            let ov_str = match ov {
                super::systray::TrayOverride::Default => "default",
                super::systray::TrayOverride::AlwaysStartInTray => "always-tray",
                super::systray::TrayOverride::AlwaysStartInTaskbar => "always-taskbar",
                super::systray::TrayOverride::NoTrayIcon => "no-tray",
                super::systray::TrayOverride::TrayOnly => "tray-only",
            };
            out.push_str(&format!("  {:30} {}\n", app_id, ov_str));
        }
    }

    out.into_bytes()
}

fn gen_taskbar() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (pinned_n, running_n, window_n, pin_ops, win_ops) = super::taskbar::stats();
    let cfg = super::taskbar::config();

    out.push_str("Taskbar\n");
    out.push_str("=======\n\n");
    out.push_str(&format!("Pinned:     {}/{}\n", pinned_n, 64));
    out.push_str(&format!("Running:    {}/{}\n", running_n, 256));
    out.push_str(&format!("Windows:    {}\n", window_n));
    out.push_str(&format!("Pin ops:    {}\n", pin_ops));
    out.push_str(&format!("Window ops: {}\n\n", win_ops));

    let pos = match cfg.position {
        super::taskbar::TaskbarPosition::Bottom => "bottom",
        super::taskbar::TaskbarPosition::Top => "top",
        super::taskbar::TaskbarPosition::Left => "left",
        super::taskbar::TaskbarPosition::Right => "right",
    };
    out.push_str(&format!("Position:   {}\n", pos));
    out.push_str(&format!("Names:      {}\n", if cfg.show_names { "yes" } else { "no" }));
    out.push_str(&format!("Grouping:   {}\n", if cfg.group_windows { "yes" } else { "no" }));
    out.push_str(&format!("Auto-hide:  {}\n", if cfg.auto_hide { "yes" } else { "no" }));
    out.push_str(&format!("Small icons:{}\n\n", if cfg.small_icons { " yes" } else { " no" }));

    let pinned = super::taskbar::pinned_apps();
    if !pinned.is_empty() {
        out.push_str("Pinned:\n");
        for p in &pinned {
            out.push_str(&format!("  [{}] {} ({})\n", p.position, p.name, p.app_id));
        }
        out.push('\n');
    }

    let running = super::taskbar::running_apps();
    if !running.is_empty() {
        out.push_str("Running:\n");
        for e in &running {
            let state = match e.state {
                super::taskbar::EntryState::Normal => "",
                super::taskbar::EntryState::Attention => " [!]",
                super::taskbar::EntryState::NotResponding => " [NR]",
                super::taskbar::EntryState::Loading => " [...]",
            };
            out.push_str(&format!("  {} ({} windows){}\n", e.name, e.windows.len(), state));
        }
    }

    out.into_bytes()
}

fn gen_startmenu() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (fav_n, ql_n, recent_n, open_ops, search_ops, launch_ops) = super::startmenu::stats();

    out.push_str("Start Menu\n");
    out.push_str("==========\n\n");
    out.push_str(&format!("Favorites:   {}/{}\n", fav_n, 32));
    out.push_str(&format!("Quick links: {}/{}\n", ql_n, 16));
    out.push_str(&format!("Recent apps: {}/{}\n", recent_n, 20));
    out.push_str(&format!("Open ops:    {}\n", open_ops));
    out.push_str(&format!("Search ops:  {}\n", search_ops));
    out.push_str(&format!("Launch ops:  {}\n\n", launch_ops));

    let favs = super::startmenu::favorites();
    if !favs.is_empty() {
        out.push_str("Favorites:\n");
        for f in &favs {
            out.push_str(&format!("  [{}] {} ({})\n", f.position, f.name, f.app_id));
        }
        out.push('\n');
    }

    let links = super::startmenu::quick_links();
    if !links.is_empty() {
        out.push_str("Quick Links:\n");
        for ql in &links {
            out.push_str(&format!("  {} ({})\n", ql.label, ql.app_id));
        }
        out.push('\n');
    }

    let recent = super::startmenu::recent_apps();
    if !recent.is_empty() {
        out.push_str("Recent:\n");
        for r in &recent {
            out.push_str(&format!("  {} (x{}) — {}\n", r.name, r.launch_count, r.app_id));
        }
    }

    out.into_bytes()
}

fn gen_filepicker() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (active, total, bm_n, recent_n, open_ops, nav_ops) = super::filepicker::stats();

    out.push_str("File Picker\n");
    out.push_str("===========\n\n");
    out.push_str(&format!("Active dialogs: {}\n", active));
    out.push_str(&format!("Total dialogs:  {}\n", total));
    out.push_str(&format!("Bookmarks:      {}\n", bm_n));
    out.push_str(&format!("Recent dirs:    {}\n", recent_n));
    out.push_str(&format!("Open ops:       {}\n", open_ops));
    out.push_str(&format!("Navigate ops:   {}\n\n", nav_ops));

    let bookmarks = super::filepicker::bookmarks();
    if !bookmarks.is_empty() {
        out.push_str("Bookmarks:\n");
        for bm in &bookmarks {
            out.push_str(&format!("  {} → {}\n", bm.label, bm.path));
        }
        out.push('\n');
    }

    let recent = super::filepicker::recent_dirs();
    if !recent.is_empty() {
        out.push_str("Recent directories:\n");
        for d in recent.iter().take(10) {
            out.push_str(&format!("  {}\n", d));
        }
    }

    out.into_bytes()
}

fn gen_theme() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (mode, custom_n, override_n, queries, changes) = super::theme::stats();

    out.push_str("Desktop Theme\n");
    out.push_str("=============\n\n");
    out.push_str(&format!("Mode:           {}\n", mode.label()));
    out.push_str(&format!("Custom themes:  {}/{}\n", custom_n, 64));
    out.push_str(&format!("Overrides:      {}/{}\n", override_n, 128));
    out.push_str(&format!("Accent:         {}\n", super::theme::accent().to_hex()));
    out.push_str(&format!("Query ops:      {}\n", queries));
    out.push_str(&format!("Change ops:     {}\n\n", changes));

    let overrides = super::theme::list_overrides();
    if !overrides.is_empty() {
        out.push_str("Active overrides:\n");
        for (role, color) in &overrides {
            out.push_str(&format!("  {:20} {}\n", role.label(), color.to_hex()));
        }
        out.push('\n');
    }

    let custom = super::theme::list_custom();
    if !custom.is_empty() {
        out.push_str("Custom themes:\n");
        for name in &custom {
            out.push_str(&format!("  {}\n", name));
        }
    }

    out.into_bytes()
}

fn gen_hotkeys() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (total, enabled, dispatches, hits) = super::hotkeys::stats();

    out.push_str("Hotkeys\n");
    out.push_str("=======\n\n");
    out.push_str(&format!("Bindings: {}/{}\n", total, 512));
    out.push_str(&format!("Enabled:  {}\n", enabled));
    out.push_str(&format!("Dispatch: {}\n", dispatches));
    out.push_str(&format!("Hits:     {}\n\n", hits));

    let bindings = super::hotkeys::list_enabled();
    if !bindings.is_empty() {
        out.push_str(&format!("{:24} {:30} {}\n", "COMBO", "ACTION", "DESC"));
        for h in &bindings {
            let action_str = h.actions.first()
                .map_or(String::from("-"), |a| a.label());
            let def = if h.is_default { " [default]" } else { "" };
            out.push_str(&format!("{:24} {:30} {}{}\n",
                h.combo.display(), action_str, h.description, def));
        }
    }

    out.into_bytes()
}

fn gen_widgets() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (widget_count, type_count, adds, refreshes) = super::widgets::stats();

    out.push_str("Desktop Widgets\n");
    out.push_str("===============\n\n");
    out.push_str(&format!("Active:   {}/{}\n", widget_count, 64));
    out.push_str(&format!("Types:    {}/{}\n", type_count, 128));
    out.push_str(&format!("Adds:     {}\n", adds));
    out.push_str(&format!("Refresh:  {}\n\n", refreshes));

    let widgets = super::widgets::active_widgets();
    if !widgets.is_empty() {
        out.push_str(&format!("{:6} {:16} {:20} {:10} {:10} {}\n",
            "ID", "KIND", "TITLE", "POS", "SIZE", "VISIBLE"));
        for w in &widgets {
            let pos = format!("{},{}", w.x, w.y);
            let size = format!("{}x{}", w.width, w.height);
            let vis = if w.visible { "yes" } else { "hidden" };
            out.push_str(&format!("{:<6} {:16} {:20} {:10} {:10} {}\n",
                w.id, w.kind.label(), w.title, pos, size, vis));
        }
    }

    out.into_bytes()
}

fn gen_soundmixer() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (streams, apps, devices, vol_changes, total_streams) = super::soundmixer::stats();
    let master = super::soundmixer::master_volume();
    let muted = super::soundmixer::master_muted();

    out.push_str("Sound Mixer\n");
    out.push_str("===========\n\n");
    out.push_str(&format!("Master:   {}%{}\n", master, if muted { " (MUTED)" } else { "" }));
    out.push_str(&format!("Ducking:  {}\n", super::soundmixer::ducking_policy().label()));
    out.push_str(&format!("Devices:  {}/{}\n", devices, 32));
    out.push_str(&format!("Apps:     {}/{}\n", apps, 128));
    out.push_str(&format!("Streams:  {}/{}\n", streams, 256));
    out.push_str(&format!("Vol chg:  {}\n", vol_changes));
    out.push_str(&format!("Created:  {}\n\n", total_streams));

    let app_list = super::soundmixer::app_entries();
    if !app_list.is_empty() {
        out.push_str(&format!("{:20} {:20} {:6} {:6} {:8} {}\n",
            "APP_ID", "NAME", "VOL", "MUTED", "STREAMS", "PLAYING"));
        for a in &app_list {
            out.push_str(&format!("{:20} {:20} {:6} {:6} {:8} {}\n",
                a.app_id, a.app_name,
                format!("{}%", a.volume),
                if a.muted { "yes" } else { "no" },
                a.stream_count,
                if a.playing { "YES" } else { "-" }));
        }
    }

    out.into_bytes()
}

fn gen_wallpaper() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let cfg = super::wallpaper::current();
    let (slide_count, hist_count, sets, advances) = super::wallpaper::stats();

    out.push_str("Desktop Wallpaper\n");
    out.push_str("=================\n\n");
    out.push_str(&format!("Kind:       {}\n", cfg.kind.label()));
    out.push_str(&format!("Image:      {}\n", if cfg.image_path.is_empty() { "(none)" } else { &cfg.image_path }));
    out.push_str(&format!("Fit:        {}\n", cfg.fit_mode.label()));
    out.push_str(&format!("BG Color:   {}\n", cfg.background_color));
    out.push_str(&format!("Login:      {}\n", if cfg.use_for_login { "same as desktop" } else { "separate" }));
    out.push_str(&format!("Random:     boot={} daily={}\n", cfg.random_on_boot, cfg.change_daily));
    out.push_str(&format!("Slideshow:  {} images, {}s interval, {}\n",
        slide_count, cfg.slideshow_interval_secs,
        if cfg.slideshow_running { "running" } else { "paused" }));
    out.push_str(&format!("History:    {}/{}\n", hist_count, 64));
    out.push_str(&format!("Sets:       {}\n", sets));
    out.push_str(&format!("Advances:   {}\n", advances));

    out.into_bytes()
}

fn gen_credentials() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (cred_count, autofill_count, stores, retrieves) = super::credentials::stats();
    let unlocked = super::credentials::is_unlocked();

    out.push_str("Credential Store\n");
    out.push_str("================\n\n");
    out.push_str(&format!("Status:    {}\n", if unlocked { "UNLOCKED" } else { "LOCKED" }));
    out.push_str(&format!("Stored:    {}/{}\n", cred_count, 4096));
    out.push_str(&format!("Autofill:  {}/{}\n", autofill_count, 1024));
    out.push_str(&format!("Stores:    {}\n", stores));
    out.push_str(&format!("Retrieves: {}\n\n", retrieves));

    // Only show summaries (no secrets).
    let creds = super::credentials::list_all();
    if !creds.is_empty() {
        out.push_str(&format!("{:16} {:24} {:20} {:10} {}\n",
            "APP", "SERVICE", "USER", "KIND", "EXPIRED"));
        for c in creds.iter().take(30) {
            out.push_str(&format!("{:16} {:24} {:20} {:10} {}\n",
                c.app_id, c.service, c.username, c.kind.label(),
                if c.expired { "YES" } else { "-" }));
        }
    }

    out.into_bytes()
}

fn gen_power() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let cfg = super::power::config();
    let bat = super::power::battery_status();
    let (events, idles, screen_off, bat_present) = super::power::stats();

    out.push_str("Power Management\n");
    out.push_str("================\n\n");
    out.push_str(&format!("Profile:      {}\n", cfg.profile.label()));
    out.push_str(&format!("Power btn:    {}\n", cfg.power_button_action.label()));
    out.push_str(&format!("Lid close:    {}\n", cfg.lid_close_action.label()));
    out.push_str(&format!("Screen off:   {}min\n", cfg.screen_off_minutes));
    out.push_str(&format!("Sleep after:  {}min\n", cfg.sleep_minutes));
    out.push_str(&format!("Screen:       {}\n", if screen_off { "OFF" } else { "ON" }));
    out.push_str(&format!("Events:       {}\n", events));
    out.push_str(&format!("Idle checks:  {}\n\n", idles));

    if bat_present {
        out.push_str(&format!("Battery:      {}%{}\n", bat.percent,
            if bat.charging { " (charging)" } else { "" }));
        out.push_str(&format!("Minutes left: {}\n",
            if bat.minutes_left < 0 { String::from("unknown") }
            else { format!("{}", bat.minutes_left) }));
        out.push_str(&format!("Health:       {}%\n", bat.health));
        out.push_str(&format!("Source:       {}\n", bat.source.label()));
        out.push_str(&format!("Low bat:      {}% → {}\n",
            cfg.low_battery_percent, cfg.low_battery_action.label()));
        out.push_str(&format!("Critical:     {}min → {}\n",
            cfg.critical_battery_minutes, cfg.critical_battery_action.label()));
    } else {
        out.push_str("Battery:      not present\n");
    }

    out.into_bytes()
}

fn gen_display() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (monitor_count, mode_changes) = super::display::stats();

    out.push_str("Display Settings\n");
    out.push_str("================\n\n");
    out.push_str(&format!("Monitors:      {}\n", monitor_count));
    out.push_str(&format!("Mode changes:  {}\n\n", mode_changes));

    let monitors = super::display::list_monitors();
    if !monitors.is_empty() {
        for m in &monitors {
            let active = if let Some(mode) = m.modes.get(m.active_mode) {
                format!("{}x{}@{}Hz", mode.width, mode.height, mode.refresh_hz)
            } else {
                String::from("(none)")
            };
            let orient = match m.orientation {
                super::display::Orientation::Landscape => "landscape",
                super::display::Orientation::Portrait => "portrait",
                super::display::Orientation::LandscapeFlipped => "landscape-flip",
                super::display::Orientation::PortraitFlipped => "portrait-flip",
            };
            out.push_str(&format!("{}{}: {} — {} scale={}% orient={} pos=({},{}) {}\n",
                if m.primary { "*" } else { " " },
                m.id, m.name, active, m.scale_percent, orient,
                m.pos_x, m.pos_y,
                if m.enabled { "ON" } else { "OFF" }));
        }
    }

    if let Some(p) = super::display::pending_change() {
        out.push_str(&format!("\nPending change: monitor={} revert in {}s\n",
            p.monitor_id, p.timeout_secs));
    }

    out.into_bytes()
}

fn gen_vdesktop() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (dc, wc, pc, switches, moves) = super::vdesktop::stats();

    out.push_str("Virtual Desktops\n");
    out.push_str("================\n\n");
    out.push_str(&format!("Desktops:   {}\n", dc));
    out.push_str(&format!("Windows:    {}\n", wc));
    out.push_str(&format!("Pinned:     {}\n", pc));
    out.push_str(&format!("Switches:   {}\n", switches));
    out.push_str(&format!("Moves:      {}\n", moves));
    out.push_str(&format!("Current:    {}\n", super::vdesktop::current()));
    out.push_str(&format!("Animation:  {}\n", super::vdesktop::animation().label()));
    out.push_str(&format!("Wrap:       {}\n\n", super::vdesktop::wrap_around()));

    let desktops = super::vdesktop::list();
    for d in &desktops {
        out.push_str(&format!("{}{}: {} ({} windows){}\n",
            if d.active { "*" } else { " " },
            d.id, d.name, d.windows.len(),
            if d.wallpaper.is_empty() { String::new() }
            else { format!(" wp={}", d.wallpaper) }));
    }

    out.into_bytes()
}

fn gen_keylayout() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (lc, rc, tc, sc) = super::keylayout::stats();

    out.push_str("Keyboard Layouts\n");
    out.push_str("================\n\n");
    out.push_str(&format!("Layouts:     {}\n", lc));
    out.push_str(&format!("Remaps:      {}\n", rc));
    out.push_str(&format!("Translates:  {}\n", tc));
    out.push_str(&format!("Switches:    {}\n", sc));
    out.push_str(&format!("Active:      {}\n\n", {
        let a = super::keylayout::active();
        if a.is_empty() { String::from("(none)") } else { a }
    }));

    let layouts = super::keylayout::list_layouts();
    for (name, desc, builtin) in &layouts {
        out.push_str(&format!("{}{}: {}{}\n",
            if *name == super::keylayout::active() { "*" } else { " " },
            name, desc,
            if *builtin { " [built-in]" } else { "" }));
    }

    out.into_bytes()
}

fn gen_screenshot() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (hc, cc) = super::screenshot::stats();
    let cfg = super::screenshot::config();

    out.push_str("Screenshot\n");
    out.push_str("==========\n\n");
    out.push_str(&format!("History:    {}\n", hc));
    out.push_str(&format!("Captures:   {}\n", cc));
    out.push_str(&format!("Save dir:   {}\n", if cfg.save_dir.is_empty() { "(default)" } else { &cfg.save_dir }));
    out.push_str(&format!("Format:     {}\n", cfg.format.label()));
    out.push_str(&format!("Cursor:     {}\n", cfg.include_cursor));
    out.push_str(&format!("Clipboard:  {}\n", cfg.copy_to_clipboard));
    out.push_str(&format!("Delay:      {}s\n\n", cfg.delay_seconds));

    let shots = super::screenshot::recent(10);
    for s in &shots {
        out.push_str(&format!("#{}: {} {}x{} {}\n",
            s.id, s.kind.label(), s.width, s.height, s.path));
    }

    out.into_bytes()
}

fn gen_a11y() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (tc, ec, ic, ac) = super::a11y::stats();
    let cfg = super::a11y::config();

    out.push_str("Accessibility\n");
    out.push_str("=============\n\n");
    out.push_str(&format!("Tools:        {}\n", tc));
    out.push_str(&format!("Elements:     {}\n", ec));
    out.push_str(&format!("Injections:   {}\n", ic));
    out.push_str(&format!("Announcements:{}\n\n", ac));
    out.push_str(&format!("High contrast: {}\n", cfg.high_contrast));
    out.push_str(&format!("Reduce motion: {}\n", cfg.reduce_motion));
    out.push_str(&format!("Screen reader: {}\n", cfg.screen_reader_active));
    out.push_str(&format!("Font scale:    {}%\n", cfg.font_scale));
    out.push_str(&format!("Sticky keys:   {}\n", cfg.sticky_keys));
    out.push_str(&format!("Mouse keys:    {}\n", cfg.mouse_keys));
    out.push_str(&format!("Cursor scale:  {}%\n", cfg.cursor_scale));
    out.push_str(&format!("Captions:      {}\n\n", cfg.captions));

    let tools = super::a11y::list_tools();
    for t in &tools {
        out.push_str(&format!("  #{}: {} [{}]{}\n",
            t.id, t.name, t.kind.label(),
            if t.active { "" } else { " (inactive)" }));
    }

    out.into_bytes()
}

fn gen_ime() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (mc, ec, cc, kc) = super::ime::stats();

    out.push_str("Input Methods\n");
    out.push_str("=============\n\n");
    out.push_str(&format!("Methods:  {}\n", mc));
    out.push_str(&format!("Emoji:    {}\n", ec));
    out.push_str(&format!("Commits:  {}\n", cc));
    out.push_str(&format!("Keys:     {}\n", kc));
    out.push_str(&format!("Active:   {} [{}]\n\n",
        { let a = super::ime::active(); if a.is_empty() { String::from("(none)") } else { a } },
        super::ime::active_indicator()));

    let methods = super::ime::list_methods();
    for m in &methods {
        out.push_str(&format!("{}{}: {} ({}){}\n",
            if m.id == super::ime::active() { "*" } else { " " },
            m.id, m.name, m.language,
            if m.builtin { " [built-in]" } else { "" }));
    }

    let comp = super::ime::composition();
    if comp.active {
        out.push_str(&format!("\nComposing: '{}' ({} candidates)\n",
            comp.buffer, comp.candidates.len()));
    }

    out.into_bytes()
}

fn gen_netindicator() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (ic, wc, pc, sc, cc) = super::netindicator::stats();
    let (cs, desc) = super::netindicator::connection_summary();

    out.push_str("Network Status\n");
    out.push_str("==============\n\n");
    out.push_str(&format!("Status:     {} — {}\n", cs.label(), desc));
    out.push_str(&format!("Interfaces: {}\n", ic));
    out.push_str(&format!("WiFi nets:  {}\n", wc));
    out.push_str(&format!("Profiles:   {}\n", pc));
    out.push_str(&format!("Scans:      {}\n", sc));
    out.push_str(&format!("Connects:   {}\n", cc));
    out.push_str(&format!("Airplane:   {}\n\n", super::netindicator::airplane_mode()));

    let ifaces = super::netindicator::list_interfaces();
    for i in &ifaces {
        out.push_str(&format!("{}: {} [{}] {}{}\n",
            i.name, i.iface_type.label(), i.state.label(),
            if i.ipv4.is_empty() { String::new() } else { format!("ip={} ", i.ipv4) },
            if i.iface_type == super::netindicator::InterfaceType::Wifi && !i.ssid.is_empty() {
                format!("ssid={} signal={}%", i.ssid, i.signal)
            } else { String::new() }));
    }

    out.into_bytes()
}

fn gen_winsnap() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (snapped, layout_count, snap_ops) = super::winsnap::stats();
    let cfg = super::winsnap::config();

    out.push_str("Window Snapping\n");
    out.push_str("===============\n\n");
    out.push_str(&format!("Enabled:       {}\n", cfg.enabled));
    out.push_str(&format!("Edge distance: {}px\n", cfg.edge_distance));
    out.push_str(&format!("Preview:       {}\n", cfg.show_preview));
    out.push_str(&format!("Animation:     {}ms\n", cfg.animation_ms));
    out.push_str(&format!("Corner snap:   {}\n", cfg.corner_snap));
    out.push_str(&format!("Thirds:        {}\n", cfg.thirds));
    out.push_str(&format!("Snapped wins:  {}\n", snapped));
    out.push_str(&format!("Snap ops:      {}\n", snap_ops));
    out.push_str(&format!("Layouts:       {}\n\n", layout_count));

    let layouts = super::winsnap::list_layouts();
    for l in &layouts {
        out.push_str(&format!("{}: {} ({} zones)\n", l.name, l.description, l.zones.len()));
        for z in &l.zones {
            out.push_str(&format!("  {} ({},{} {}x{})\n",
                z.name, z.x_pct, z.y_pct, z.w_pct, z.h_pct));
        }
    }

    out.into_bytes()
}

fn gen_colorpicker() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (active, pal_count, recent_count, picks, samples) = super::colorpicker::stats();

    out.push_str("Color Picker\n");
    out.push_str("============\n\n");
    out.push_str(&format!("Active pickers: {}\n", active));
    out.push_str(&format!("Palettes:       {}\n", pal_count));
    out.push_str(&format!("Recent colors:  {}\n", recent_count));
    out.push_str(&format!("Picks:          {}\n", picks));
    out.push_str(&format!("Samples:        {}\n\n", samples));

    let palettes = super::colorpicker::list_palettes();
    for p in &palettes {
        out.push_str(&format!("{}: {} colors\n", p.name, p.colors.len()));
    }

    let recent = super::colorpicker::recent_colors();
    if !recent.is_empty() {
        out.push_str("\nRecent:\n");
        for c in &recent {
            out.push_str(&format!("  {}\n", c.to_hex()));
        }
    }

    out.into_bytes()
}

fn gen_cursorsettings() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let cfg = super::cursorsettings::config();
    let (theme_count, changes) = super::cursorsettings::stats();

    out.push_str("Cursor Settings\n");
    out.push_str("===============\n\n");
    out.push_str(&format!("Active theme:     {}\n", cfg.active_theme));
    out.push_str(&format!("Cursor size:      {}px\n", cfg.cursor_size));
    out.push_str(&format!("Speed:            {}\n", cfg.speed));
    out.push_str(&format!("Accel profile:    {}\n", cfg.accel_profile.label()));
    out.push_str(&format!("Button layout:    {}\n", cfg.button_layout.label()));
    out.push_str(&format!("Double-click:     {}ms\n", cfg.double_click_ms));
    out.push_str(&format!("Scroll speed:     {}\n", cfg.scroll_speed));
    out.push_str(&format!("Natural scroll:   {}\n", cfg.natural_scroll));
    out.push_str(&format!("Trail:            {}{}\n",
        cfg.show_trail,
        if cfg.show_trail { alloc::format!(" (len={})", cfg.trail_length) } else { String::new() }));
    out.push_str(&format!("Locate on Ctrl:   {}\n", cfg.locate_on_ctrl));
    out.push_str(&format!("Hide while typing:{}\n", cfg.hide_while_typing));
    out.push_str(&format!("Themes:           {}\n", theme_count));
    out.push_str(&format!("Changes:          {}\n\n", changes));

    let themes = super::cursorsettings::list_themes();
    for t in &themes {
        out.push_str(&format!("{}: {} ({}px, {} cursors{})\n",
            t.name, t.description, t.base_size, t.cursors.len(),
            if t.builtin { ", builtin" } else { "" }));
    }

    out.into_bytes()
}

fn gen_kbsettings() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let cfg = super::kbsettings::config();
    let (prof_count, override_count, changes) = super::kbsettings::stats();

    out.push_str("Keyboard Settings\n");
    out.push_str("=================\n\n");
    out.push_str(&format!("Preset:          {}\n", cfg.preset.label()));
    out.push_str(&format!("Repeat delay:    {}ms\n", cfg.repeat_delay_ms));
    out.push_str(&format!("Repeat rate:     {}ms\n", cfg.repeat_rate_ms));
    out.push_str(&format!("NumLock boot:    {}\n", cfg.numlock_boot.label()));
    out.push_str(&format!("CapsLock toggle: {}\n", cfg.caps_lock_toggle));
    out.push_str(&format!("Sticky Keys:     {}{}\n", cfg.sticky_keys,
        if cfg.sticky_lock_on_double { " (lock on double)" } else { "" }));
    out.push_str(&format!("Filter Keys:     {}{}\n", cfg.filter_keys,
        if cfg.filter_keys { alloc::format!(" (hold={}ms debounce={}ms)", cfg.filter_min_hold_ms, cfg.filter_debounce_ms) } else { String::new() }));
    out.push_str(&format!("Toggle sounds:   {}\n", cfg.toggle_keys_sound));
    out.push_str(&format!("Bounce Keys:     {}{}\n", cfg.bounce_keys,
        if cfg.bounce_keys { alloc::format!(" ({}ms)", cfg.bounce_ms) } else { String::new() }));
    out.push_str(&format!("Compose key:     {}\n", cfg.compose_key));
    out.push_str(&format!("Ctrl+Alt=AltGr:  {}\n", cfg.ctrl_alt_as_altgr));
    out.push_str(&format!("Profiles:        {}\n", prof_count));
    out.push_str(&format!("Overrides:       {}\n", override_count));
    out.push_str(&format!("Changes:         {}\n\n", changes));

    let profiles = super::kbsettings::list_profiles();
    for p in &profiles {
        out.push_str(&format!("{}: {} delay={}ms rate={}ms{}\n",
            p.name, p.preset.label(), p.repeat_delay_ms, p.repeat_rate_ms,
            if p.active { " [active]" } else { "" }));
    }

    out.into_bytes()
}

fn gen_detailcols() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (col_count, bind_count, user_count, queries) = super::detailcols::stats();

    out.push_str("Detail Columns\n");
    out.push_str("==============\n\n");
    out.push_str(&format!("Columns:    {}\n", col_count));
    out.push_str(&format!("Bindings:   {}\n", bind_count));
    out.push_str(&format!("User prefs: {}\n", user_count));
    out.push_str(&format!("Queries:    {}\n\n", queries));

    let bindings = super::detailcols::list_bindings();
    for b in &bindings {
        out.push_str(&format!("{} → {} columns\n", b.mime_pattern, b.column_ids.len()));
    }

    out.into_bytes()
}

fn gen_partmgr() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (disk_count, part_count, ops) = super::partmgr::stats();

    out.push_str("Partition Manager\n");
    out.push_str("=================\n\n");
    out.push_str(&format!("Disks:      {}\n", disk_count));
    out.push_str(&format!("Partitions: {}\n", part_count));
    out.push_str(&format!("Operations: {}\n", ops));
    out.push_str(&format!("Confirm:    {}\n\n", super::partmgr::confirmation_required()));

    let disks = super::partmgr::list_disks();
    for d in &disks {
        let gb = d.size_bytes / (1024 * 1024 * 1024);
        out.push_str(&format!("{}: {} {} {}GB [{}]{}{}\n",
            d.name, d.model, d.serial, gb, d.table_type.label(),
            if d.removable { " removable" } else { "" },
            if d.read_only { " ro" } else { "" }));
        let parts = super::partmgr::list_partitions(d.id);
        for p in &parts {
            let mb = p.size_bytes / (1024 * 1024);
            out.push_str(&format!("  #{}: {} {}MB {} [{}]{}\n",
                p.number, p.label, mb, p.fs_type.label(),
                p.flags.iter().map(|f| f.label()).collect::<Vec<_>>().join(","),
                if p.mount_point.is_empty() { String::new() }
                else { alloc::format!(" → {}", p.mount_point) }));
        }
    }

    out.into_bytes()
}

fn gen_locale() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let cfg = super::locale::config();
    let (lang_count, tz_count, changes) = super::locale::stats();

    out.push_str("Locale Settings\n");
    out.push_str("===============\n\n");
    out.push_str(&format!("Language:      {}\n", cfg.language));
    out.push_str(&format!("Fallback:      {}\n", cfg.fallback_language));
    out.push_str(&format!("Region format: {}\n", cfg.region_format));
    out.push_str(&format!("Numbers:       {}\n", cfg.number_format.label()));
    out.push_str(&format!("Currency:      {}{}\n", cfg.currency_symbol,
        if cfg.currency_before { " (before)" } else { " (after)" }));
    out.push_str(&format!("Date:          {} ({})\n", cfg.date_order.label(), cfg.date_separator.label()));
    out.push_str(&format!("Time:          {}\n", cfg.time_format.label()));
    out.push_str(&format!("First day:     {}\n", cfg.first_day.label()));
    out.push_str(&format!("Measurement:   {}\n", cfg.measurement.label()));
    out.push_str(&format!("Timezone:      {} (UTC{:+})\n", cfg.timezone, super::locale::timezone_offset_minutes() as f32 / 60.0));
    out.push_str(&format!("Paper:         {}\n", if cfg.paper_a4 { "A4" } else { "Letter" }));
    out.push_str(&format!("Languages:     {}\n", lang_count));
    out.push_str(&format!("Timezones:     {}\n", tz_count));
    out.push_str(&format!("Changes:       {}\n", changes));

    out.into_bytes()
}

fn gen_useracct() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (user_count, group_count, session_count, login_count) = super::useracct::stats();

    out.push_str("User Accounts\n");
    out.push_str("=============\n\n");
    out.push_str(&format!("Users:    {}\n", user_count));
    out.push_str(&format!("Groups:   {}\n", group_count));
    out.push_str(&format!("Sessions: {}\n", session_count));
    out.push_str(&format!("Logins:   {}\n", login_count));

    let users = super::useracct::list_users();
    if !users.is_empty() {
        out.push_str("\nUsers:\n");
        for u in &users {
            let type_str = match u.account_type {
                super::useracct::AccountType::Administrator => "admin",
                super::useracct::AccountType::Standard => "standard",
                super::useracct::AccountType::Guest => "guest",
                super::useracct::AccountType::System => "system",
            };
            let status = if u.locked {
                "locked"
            } else if !u.enabled {
                "disabled"
            } else {
                "active"
            };
            out.push_str(&format!(
                "  {} (uid={}, type={}, status={})\n",
                u.username, u.uid, type_str, status
            ));
        }
    }

    let groups = super::useracct::list_groups();
    if !groups.is_empty() {
        out.push_str("\nGroups:\n");
        for g in &groups {
            let kind = if g.system_group { "system" } else { "user" };
            out.push_str(&format!("  {} (gid={}, {})\n", g.name, g.gid, kind));
        }
    }

    out.into_bytes()
}

fn gen_progmgr() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (total, installed, snaps, ops) = super::progmgr::stats();

    out.push_str("Program Manager\n");
    out.push_str("===============\n\n");
    out.push_str(&format!("Programs:   {} ({} installed)\n", total, installed));
    out.push_str(&format!("Snapshots:  {}\n", snaps));
    out.push_str(&format!("Operations: {}\n", ops));

    let progs = super::progmgr::list_programs();
    if !progs.is_empty() {
        out.push_str("\nPrograms:\n");
        for p in &progs {
            let prio = match p.priority {
                super::progmgr::PriorityLevel::Idle => "idle",
                super::progmgr::PriorityLevel::BelowNormal => "below",
                super::progmgr::PriorityLevel::Normal => "normal",
                super::progmgr::PriorityLevel::AboveNormal => "above",
                super::progmgr::PriorityLevel::High => "high",
                super::progmgr::PriorityLevel::Realtime => "rt",
            };
            let status = if p.installed { "installed" } else { "removed" };
            out.push_str(&format!(
                "  {} v{} [{}] prio={} caps={} snaps={}\n",
                p.name, p.version, status, prio, p.capabilities.len(), p.snapshots.len()
            ));
        }
    }

    out.into_bytes()
}

fn gen_scriptlang() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (engine_count, ctx_count, evals, _changes) = super::scriptlang::stats();

    out.push_str("Scripting Languages\n");
    out.push_str("===================\n\n");
    out.push_str(&format!("Engines:  {}\n", engine_count));
    out.push_str(&format!("Contexts: {}\n", ctx_count));
    out.push_str(&format!("Evals:    {}\n", evals));

    let engines = super::scriptlang::list_engines();
    if !engines.is_empty() {
        out.push_str("\nEngines:\n");
        for e in &engines {
            let etype = match e.engine_type {
                super::scriptlang::EngineType::Interpreted => "interp",
                super::scriptlang::EngineType::Jit => "jit",
                super::scriptlang::EngineType::Wasm => "wasm",
                super::scriptlang::EngineType::Shell => "shell",
                super::scriptlang::EngineType::Dsl => "dsl",
                super::scriptlang::EngineType::Compiled => "compiled",
            };
            let sandbox = match e.sandbox {
                super::scriptlang::SandboxLevel::None => "none",
                super::scriptlang::SandboxLevel::Basic => "basic",
                super::scriptlang::SandboxLevel::Strict => "strict",
                super::scriptlang::SandboxLevel::Capability => "caps",
            };
            let status = if e.enabled { "on" } else { "off" };
            out.push_str(&format!(
                "  {} v{} [{}] type={} sandbox={} exts={}\n",
                e.name, e.version, status, etype, sandbox,
                e.extensions.len()
            ));
        }
    }

    out.into_bytes()
}

fn gen_osreset() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (cps, plans, problems, ops) = super::osreset::stats();
    let status = match super::osreset::status() {
        super::osreset::ResetStatus::Idle => "idle",
        super::osreset::ResetStatus::Scanning => "scanning",
        super::osreset::ResetStatus::Checkpointing => "checkpointing",
        super::osreset::ResetStatus::Planning => "planning",
        super::osreset::ResetStatus::Executing => "executing",
        super::osreset::ResetStatus::Repairing => "repairing",
    };

    out.push_str("OS Reset / Repair\n");
    out.push_str("=================\n\n");
    out.push_str(&format!("Status:       {}\n", status));
    out.push_str(&format!("Checkpoints:  {}\n", cps));
    out.push_str(&format!("Plans:        {}\n", plans));
    out.push_str(&format!("Problems:     {}\n", problems));
    out.push_str(&format!("Operations:   {}\n", ops));

    let checkpoints = super::osreset::list_checkpoints();
    if !checkpoints.is_empty() {
        out.push_str("\nCheckpoints:\n");
        for c in &checkpoints {
            let valid = if c.valid { "valid" } else { "invalid" };
            out.push_str(&format!("  id={} '{}' [{}]\n", c.id, c.name, valid));
        }
    }

    out.into_bytes()
}

fn gen_bootcfg() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (entry_count, event_count, boots, _changes) = super::bootcfg::stats();
    let cfg = super::bootcfg::get_config();

    let loader = match cfg.loader_type {
        super::bootcfg::BootloaderType::Grub2 => "GRUB2",
        super::bootcfg::BootloaderType::SystemdBoot => "systemd-boot",
        super::bootcfg::BootloaderType::CustomUefi => "Custom UEFI",
        super::bootcfg::BootloaderType::DirectUefi => "Direct UEFI",
    };
    let console = match cfg.console_mode {
        super::bootcfg::ConsoleMode::Text => "text",
        super::bootcfg::ConsoleMode::Graphical => "graphical",
        super::bootcfg::ConsoleMode::Verbose => "verbose",
        super::bootcfg::ConsoleMode::Silent => "silent",
    };

    out.push_str("Boot Configuration\n");
    out.push_str("==================\n\n");
    out.push_str(&format!("Loader:    {}\n", loader));
    out.push_str(&format!("Timeout:   {}s\n", cfg.timeout_secs));
    out.push_str(&format!("Console:   {}\n", console));
    out.push_str(&format!("Activity:  {}\n", cfg.show_boot_activity));
    out.push_str(&format!("Entries:   {}\n", entry_count));
    out.push_str(&format!("Boot log:  {} events ({} total)\n", event_count, boots));

    let entries = super::bootcfg::list_entries();
    if !entries.is_empty() {
        out.push_str("\nEntries:\n");
        for e in &entries {
            let def = if e.is_default { " [DEFAULT]" } else { "" };
            let hid = if e.hidden { " (hidden)" } else { "" };
            out.push_str(&format!("  #{} {} — {}{}{}\n", e.position, e.name, e.kernel_path, def, hid));
        }
    }

    out.into_bytes()
}

fn gen_swapcfg() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (area_count, active_count, total_bytes, ops) = super::swapcfg::stats();
    let cfg = super::swapcfg::get_config();
    let usage = super::swapcfg::usage();

    out.push_str("Swap Configuration\n");
    out.push_str("==================\n\n");
    out.push_str(&format!("Enabled:      {}\n", cfg.enabled));
    out.push_str(&format!("Swappiness:   {}\n", cfg.swappiness));
    out.push_str(&format!("Min free:     {} bytes\n", cfg.min_free_bytes));
    out.push_str(&format!("zswap:        {} ({})\n", cfg.zswap_enabled, cfg.zswap_algorithm));
    out.push_str(&format!("Areas:        {} ({} active)\n", area_count, active_count));
    out.push_str(&format!("Total:        {} bytes\n", total_bytes));
    out.push_str(&format!("Used:         {} / {} bytes\n", usage.used_bytes, usage.total_bytes));
    out.push_str(&format!("Operations:   {}\n", ops));

    let areas = super::swapcfg::list_swaps();
    if !areas.is_empty() {
        out.push_str("\nAreas:\n");
        for a in &areas {
            let stype = match a.swap_type {
                super::swapcfg::SwapType::File => "file",
                super::swapcfg::SwapType::Partition => "partition",
                super::swapcfg::SwapType::Compressed => "compressed",
            };
            let status = if a.active { "active" } else { "inactive" };
            out.push_str(&format!(
                "  id={} {} [{}] {} prio={} {}/{} bytes\n",
                a.id, a.path, stype, status, a.priority.0, a.used_bytes, a.size_bytes
            ));
        }
    }

    out.into_bytes()
}

/// Generate `/proc/timezone` — timezone and NTP status.
fn gen_timezone() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (tz_count, ntp_count, ntp_on, ops) = super::timezone::stats();
    let current = super::timezone::current_timezone();
    let (tf, df, ws, sec, date) = super::timezone::format_settings();
    let ntp_st = super::timezone::ntp_status();

    out.push_str("Timezone & Clock\n");
    out.push_str("================\n\n");
    out.push_str(&format!("Current:       {}\n", current));
    out.push_str(&format!("Time format:   {:?}\n", tf));
    out.push_str(&format!("Date format:   {:?}\n", df));
    out.push_str(&format!("Week start:    {:?}\n", ws));
    out.push_str(&format!("Show seconds:  {}\n", sec));
    out.push_str(&format!("Show date:     {}\n", date));
    out.push_str(&format!("NTP enabled:   {}\n", ntp_on));
    out.push_str(&format!("NTP status:    {:?}\n", ntp_st));
    out.push_str(&format!("NTP servers:   {}\n", ntp_count));
    out.push_str(&format!("Timezones:     {}\n", tz_count));
    out.push_str(&format!("Operations:    {}\n", ops));

    let servers = super::timezone::list_ntp_servers();
    if !servers.is_empty() {
        out.push_str(&format!("\n{:<30} {:<6} {:<10} {}\n", "SERVER", "PORT", "ENABLED", "OFFSET_US"));
        for s in &servers {
            out.push_str(&format!("{:<30} {:<6} {:<10} {}\n",
                s.hostname, s.port, s.enabled, s.offset_us));
        }
    }

    out.into_bytes()
}

fn gen_autostart() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (total, enabled, system, ops) = super::autostart::stats();

    out.push_str("Autostart Items\n");
    out.push_str("===============\n\n");
    out.push_str(&format!("Total items:   {}\n", total));
    out.push_str(&format!("Enabled:       {}\n", enabled));
    out.push_str(&format!("System:        {}\n", system));
    out.push_str(&format!("Operations:    {}\n", ops));

    let items = super::autostart::list_items();
    if !items.is_empty() {
        out.push_str(&format!("\n{:<4} {:<20} {:<16} {:<10} {:<8} {:<6} {}\n",
            "ID", "NAME", "PHASE", "CONDITION", "ENABLED", "ORDER", "COMMAND"));
        for it in &items {
            out.push_str(&format!("{:<4} {:<20} {:<16} {:<10} {:<8} {:<6} {}\n",
                it.id,
                it.name,
                format!("{:?}", it.phase),
                format!("{:?}", it.condition),
                it.enabled,
                it.order,
                it.command));
        }
    }

    out.into_bytes()
}

fn gen_schedtune() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (total, active_count, tradeoff_count, ops) = super::schedtune::stats();

    out.push_str("Scheduler Tuning\n");
    out.push_str("================\n\n");
    out.push_str(&format!("Profiles:      {}\n", total));
    out.push_str(&format!("Active:        {}\n", active_count));
    out.push_str(&format!("Tradeoffs:     {}\n", tradeoff_count));
    out.push_str(&format!("Operations:    {}\n", ops));

    if let Ok(a) = super::schedtune::active_profile() {
        out.push_str(&format!("\nActive: {} ({:?})\n", a.name, a.workload));
        out.push_str(&format!("  Model:       {:?}\n", a.model));
        out.push_str(&format!("  Preempt:     {:?}\n", a.preempt));
        out.push_str(&format!("  Timeslice:   {} us\n", a.timeslice_us));
        out.push_str(&format!("  Latency:     {} us\n", a.target_latency_us));
        out.push_str(&format!("  Interactive: {}\n", a.interactive_boost));
        out.push_str(&format!("  Balance:     {:?}\n", a.balance_strategy));
    }

    let profiles = super::schedtune::list_profiles();
    if !profiles.is_empty() {
        out.push_str(&format!("\n{:<4} {:<25} {:<12} {:<18} {:<8} {}\n",
            "ID", "NAME", "WORKLOAD", "MODEL", "ACTIVE", "PREEMPT"));
        for p in &profiles {
            out.push_str(&format!("{:<4} {:<25} {:<12} {:<18} {:<8} {:?}\n",
                p.id, p.name,
                format!("{:?}", p.workload),
                format!("{:?}", p.model),
                p.active,
                p.preempt));
        }
    }

    out.into_bytes()
}

fn gen_mmtune() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (total, active_count, tradeoff_count, ops) = super::mmtune::stats();

    out.push_str("Memory Management Tuning\n");
    out.push_str("========================\n\n");
    out.push_str(&format!("Profiles:      {}\n", total));
    out.push_str(&format!("Active:        {}\n", active_count));
    out.push_str(&format!("Tradeoffs:     {}\n", tradeoff_count));
    out.push_str(&format!("Operations:    {}\n", ops));

    if let Ok(a) = super::mmtune::active_profile() {
        out.push_str(&format!("\nActive: {} ({:?})\n", a.name, a.workload));
        out.push_str(&format!("  Allocator:   {:?}\n", a.alloc_model));
        out.push_str(&format!("  Reclaim:     {:?}\n", a.reclaim));
        out.push_str(&format!("  Overcommit:  {:?} ({}%)\n", a.overcommit, a.overcommit_ratio));
        out.push_str(&format!("  Huge pages:  {:?}\n", a.huge_pages));
        out.push_str(&format!("  Compaction:  {:?}\n", a.compact_level));
        out.push_str(&format!("  Swappiness:  {}\n", a.swappiness));
        out.push_str(&format!("  Dirty ratio: {}/{}\n", a.dirty_ratio, a.dirty_bg_ratio));
        out.push_str(&format!("  ZRAM:        {}\n", a.zram_enabled));
    }

    let profiles = super::mmtune::list_profiles();
    if !profiles.is_empty() {
        out.push_str(&format!("\n{:<4} {:<25} {:<12} {:<12} {:<12} {}\n",
            "ID", "NAME", "WORKLOAD", "ALLOCATOR", "RECLAIM", "ACTIVE"));
        for p in &profiles {
            out.push_str(&format!("{:<4} {:<25} {:<12} {:<12} {:<12} {}\n",
                p.id, p.name,
                format!("{:?}", p.workload),
                format!("{:?}", p.alloc_model),
                format!("{:?}", p.reclaim),
                p.active));
        }
    }

    out.into_bytes()
}

fn gen_capsettings() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (groups, users, programs, paths, ops) = super::capsettings::stats();

    out.push_str("Capability Settings\n");
    out.push_str("===================\n\n");
    out.push_str(&format!("Groups:        {}\n", groups));
    out.push_str(&format!("Users:         {}\n", users));
    out.push_str(&format!("Programs:      {}\n", programs));
    out.push_str(&format!("Path reqs:     {}\n", paths));
    out.push_str(&format!("Operations:    {}\n", ops));

    let group_list = super::capsettings::list_groups();
    if !group_list.is_empty() {
        out.push_str(&format!("\n{:<4} {:<20} {:<8} {}\n", "ID", "NAME", "CAPS", "BUILTIN"));
        for g in &group_list {
            out.push_str(&format!("{:<4} {:<20} {:<8} {}\n",
                g.id, g.name, g.caps.len(), g.builtin));
        }
    }

    out.into_bytes()
}

fn gen_vpn() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (total, connected, tp_count, ops) = super::vpn::conn_stats();
    let s = super::vpn::status();

    out.push_str("VPN Management\n");
    out.push_str("==============\n\n");
    out.push_str(&format!("Profiles:      {}\n", total));
    out.push_str(&format!("Connected:     {}\n", connected));
    out.push_str(&format!("State:         {:?}\n", s.state));
    out.push_str(&format!("Third-party:   {}\n", tp_count));
    out.push_str(&format!("Operations:    {}\n", ops));

    if connected {
        out.push_str(&format!("\nServer:  {}\n", s.connected_server));
        out.push_str(&format!("VPN IP:  {}\n", s.vpn_ip));
        out.push_str(&format!("Uptime:  {} s\n", s.uptime_s));
        out.push_str(&format!("Sent:    {} bytes\n", s.bytes_sent));
        out.push_str(&format!("Recv:    {} bytes\n", s.bytes_received));
    }

    let profiles = super::vpn::list_profiles();
    if !profiles.is_empty() {
        out.push_str(&format!("\n{:<4} {:<20} {:<12} {:<20} {:<6} {}\n",
            "ID", "NAME", "PROTOCOL", "SERVER", "PORT", "AUTO"));
        for p in &profiles {
            out.push_str(&format!("{:<4} {:<20} {:<12} {:<20} {:<6} {}\n",
                p.id, p.name,
                format!("{:?}", p.protocol),
                p.server, p.port, p.auto_connect));
        }
    }

    out.into_bytes()
}

fn gen_dyndns() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (entry_count, forward_count, router_detected, ops) = super::dyndns::stats();

    out.push_str("Dynamic DNS & Port Forwarding\n");
    out.push_str("=============================\n\n");
    out.push_str(&format!("DDNS entries:  {}\n", entry_count));
    out.push_str(&format!("Forwards:      {}\n", forward_count));
    out.push_str(&format!("Router:        {}\n", if router_detected { "detected" } else { "none" }));
    out.push_str(&format!("Operations:    {}\n", ops));

    if let Some(ri) = super::dyndns::router_info() {
        out.push_str(&format!("\nRouter: {} ({})\n", ri.ip, ri.model));
        out.push_str(&format!("  External IP: {}\n", ri.external_ip));
        out.push_str(&format!("  UPnP:        {}\n", ri.upnp_available));
        out.push_str(&format!("  NAT-PMP:     {}\n", ri.natpmp_available));
    }

    let entries = super::dyndns::list_entries();
    if !entries.is_empty() {
        out.push_str(&format!("\n{:<4} {:<15} {:<10} {:<25} {:<10} {}\n",
            "ID", "NAME", "PROVIDER", "HOSTNAME", "STATUS", "IP"));
        for e in &entries {
            out.push_str(&format!("{:<4} {:<15} {:<10} {:<25} {:<10} {}\n",
                e.id, e.name,
                format!("{:?}", e.provider),
                e.hostname,
                format!("{:?}", e.status),
                e.last_ip));
        }
    }

    out.into_bytes()
}

fn gen_loginscreen() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (synced, changes, ops) = super::loginscreen::stats();
    let cfg = super::loginscreen::config();

    out.push_str("Login Screen\n");
    out.push_str("============\n\n");
    out.push_str(&format!("Background:    {:?}\n", cfg.background_mode));
    out.push_str(&format!("Synced:        {}\n", synced));
    out.push_str(&format!("Fit:           {:?}\n", cfg.fit_mode));
    out.push_str(&format!("Blur:          {}\n", cfg.blur_amount));
    out.push_str(&format!("Clock:         {:?}\n", cfg.clock_position));
    out.push_str(&format!("User list:     {:?}\n", cfg.user_list));
    out.push_str(&format!("Lock timeout:  {} s\n", cfg.lock_timeout_s));
    out.push_str(&format!("Changes:       {}\n", changes));
    out.push_str(&format!("Operations:    {}\n", ops));

    out.into_bytes()
}

fn gen_appnotify() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (app_count, sound_count, type_count, ops) = super::appnotify::stats();

    out.push_str("App Notification Settings\n");
    out.push_str("=========================\n\n");
    out.push_str(&format!("Registered apps:     {}\n", app_count));
    out.push_str(&format!("System sounds:       {}\n", sound_count));
    out.push_str(&format!("Notification types:  {}\n", type_count));
    out.push_str(&format!("Operations:          {}\n", ops));

    let apps = super::appnotify::list_apps();
    if !apps.is_empty() {
        out.push_str("\nApps:\n");
        for app in &apps {
            let status = if app.enabled { "on" } else { "off" };
            out.push_str(&format!("  {} ({}) [{}] types={}\n",
                app.display_name, app.app_id, status,
                app.notification_types.len()));
        }
    }

    out.into_bytes()
}

fn gen_kernelbuild() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (comp_count, built, changed, ops) = super::kernelbuild::stats();

    out.push_str("Kernel Build Configuration\n");
    out.push_str("==========================\n\n");
    out.push_str(&format!("Components:     {}\n", comp_count));
    out.push_str(&format!("Up to date:     {}\n", built));
    out.push_str(&format!("Source changed: {}\n", changed));
    out.push_str(&format!("Operations:     {}\n", ops));

    let comps = super::kernelbuild::list_components();
    if !comps.is_empty() {
        out.push_str("\nComponents:\n");
        for c in &comps {
            out.push_str(&format!("  {} ({:?}) [{:?}] params={}\n",
                c.name, c.comp_type, c.status, c.params.len()));
        }
    }

    out.into_bytes()
}

fn gen_wakesensor() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (global, cam, mic, events, ops) = super::wakesensor::stats();

    out.push_str("Wake Sensors\n");
    out.push_str("============\n\n");
    out.push_str(&format!("Global enabled: {}\n", global));
    out.push_str(&format!("Camera:         {}\n", if cam { "on" } else { "off" }));
    out.push_str(&format!("Microphone:     {}\n", if mic { "on" } else { "off" }));
    out.push_str(&format!("Wake events:    {}\n", events));
    out.push_str(&format!("Operations:     {}\n", ops));

    out.into_bytes()
}

fn gen_netsettings() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (ifaces, connected, saved, ops) = super::netsettings::stats();
    let ri = super::netsettings::router_info();

    out.push_str("Network Settings\n");
    out.push_str("================\n\n");
    out.push_str(&format!("Hostname:    {}\n", super::netsettings::hostname()));
    out.push_str(&format!("Interfaces:  {} ({} connected)\n", ifaces, connected));
    out.push_str(&format!("Saved WiFi:  {}\n", saved));
    out.push_str(&format!("Gateway:     {} {}\n", ri.gateway_ip,
        if ri.reachable { "(reachable)" } else { "(unreachable)" }));
    if !ri.external_ipv4.is_empty() {
        out.push_str(&format!("External IP: {}\n", ri.external_ipv4));
    }
    out.push_str(&format!("Operations:  {}\n", ops));

    let interfaces = super::netsettings::list_interfaces();
    if !interfaces.is_empty() {
        out.push_str("\nInterfaces:\n");
        for i in &interfaces {
            out.push_str(&format!("  {} ({:?}) {:?} {}\n",
                i.name, i.iface_type, i.link_state, i.ipv4.address));
        }
    }

    out.into_bytes()
}

fn gen_sysinfo() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let os = super::sysinfo::os_info();
    let cpu = super::sysinfo::cpu_info();
    let mem = super::sysinfo::memory_info();
    let kp = super::sysinfo::kernel_params();

    out.push_str("System Information\n");
    out.push_str("==================\n\n");
    out.push_str(&format!("OS:         {} {} ({})\n", os.name, os.version, os.codename));
    out.push_str(&format!("Arch:       {}\n", os.arch));
    out.push_str(&format!("Kernel:     {}\n", os.kernel_version));
    out.push_str(&format!("CPU:        {} ({} cores / {} threads)\n", cpu.model, cpu.cores, cpu.threads));
    out.push_str(&format!("Memory:     {} {} @ {} MT/s\n", mem.mem_type, mem.dimm_count, mem.speed_mts));
    out.push_str(&format!("Page size:  {} B\n", kp.page_size));
    out.push_str(&format!("Scheduler:  {}\n", kp.sched_model));
    out.push_str(&format!("Storage:    {} devices\n", super::sysinfo::storage_info().len()));
    out.push_str(&format!("GPUs:       {}\n", super::sysinfo::gpu_info().len()));

    out.into_bytes()
}

fn gen_perfmon() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (cpu_n, mem_n, disk_n, net_n, alerts_n, ops) = super::perfmon::stats();
    let cfg = super::perfmon::get_config();

    out.push_str("Performance Monitor\n");
    out.push_str("===================\n\n");
    out.push_str(&format!("Interval:    {} ms\n", cfg.sample_interval_ms));
    out.push_str(&format!("CPU samples: {} ({})\n", cpu_n, if cfg.cpu_enabled { "on" } else { "off" }));
    out.push_str(&format!("Mem samples: {} ({})\n", mem_n, if cfg.mem_enabled { "on" } else { "off" }));
    out.push_str(&format!("Disk samples:{} ({})\n", disk_n, if cfg.disk_enabled { "on" } else { "off" }));
    out.push_str(&format!("Net samples: {} ({})\n", net_n, if cfg.net_enabled { "on" } else { "off" }));
    out.push_str(&format!("Alerts:      {}\n", alerts_n));
    out.push_str(&format!("Operations:  {}\n", ops));

    if let Some(cpu) = super::perfmon::cpu_latest() {
        out.push_str(&format!("\nLatest CPU: {}% ({} MHz, {} procs, {} threads)\n",
            cpu.usage_pct, cpu.freq_mhz, cpu.process_count, cpu.thread_count));
    }

    out.into_bytes()
}

fn gen_focusassist() -> Vec<u8> {
    use alloc::format;
    let mut out = String::from("=== Focus Assist ===\n");

    let (profile_count, schedule_count, active, missed, sessions, ops) =
        super::focusassist::stats();

    out.push_str(&format!("status: {}\n", if active { "active" } else { "off" }));
    out.push_str(&format!("profiles: {}\n", profile_count));
    out.push_str(&format!("schedules: {}\n", schedule_count));
    out.push_str(&format!("missed_this_session: {}\n", missed));
    out.push_str(&format!("total_sessions: {}\n", sessions));
    out.push_str(&format!("ops: {}\n", ops));

    if let Some(profile) = super::focusassist::active_profile() {
        out.push_str(&format!("\nactive_profile: {} (id={})\n", profile.name, profile.id));
        out.push_str(&format!("mode: {:?}\n", profile.mode));
        out.push_str(&format!("priority_apps: {}\n", profile.priority_apps.len()));
        if let Some(ref reply) = profile.auto_reply {
            out.push_str(&format!("auto_reply: {}\n", reply));
        }
    }

    let profiles = super::focusassist::list_profiles();
    if !profiles.is_empty() {
        out.push_str("\nprofiles:\n");
        for p in &profiles {
            out.push_str(&format!("  id={} name={:?} mode={:?} enabled={} builtin={}\n",
                p.id, p.name, p.mode, p.enabled, p.builtin));
        }
    }

    let schedules = super::focusassist::list_schedules();
    if !schedules.is_empty() {
        out.push_str("\nschedules:\n");
        for s in &schedules {
            out.push_str(&format!("  id={} name={:?} {:02}:{:02}-{:02}:{:02} enabled={} profile={}\n",
                s.id, s.name, s.start_hour, s.start_minute,
                s.end_hour, s.end_minute, s.enabled, s.profile_id));
        }
    }

    out.into_bytes()
}

fn gen_storageclean() -> Vec<u8> {
    use alloc::format;
    let mut out = String::from("=== Storage Cleanup ===\n");

    let (items, freed, scans, cleans, ops) = super::storageclean::stats();
    out.push_str(&format!("cached_items: {}\n", items));
    out.push_str(&format!("total_freed: {} ({})\n", freed,
        super::storageclean::format_size(freed)));
    out.push_str(&format!("scans: {}\n", scans));
    out.push_str(&format!("cleanups: {}\n", cleans));
    out.push_str(&format!("ops: {}\n", ops));

    if let Ok(cfg) = super::storageclean::config() {
        out.push_str(&format!("\nauto_enabled: {}\n", cfg.auto_enabled));
        out.push_str(&format!("auto_threshold: {}%\n", cfg.auto_clean_threshold_pct));
        out.push_str(&format!("large_threshold: {}\n",
            super::storageclean::format_size(cfg.large_file_threshold)));
        out.push_str(&format!("old_download_days: {}\n", cfg.old_download_days));
        out.push_str(&format!("log_retention_days: {}\n", cfg.log_retention_days));
    }

    if let Some(report) = super::storageclean::last_report() {
        out.push_str("\nlast_scan:\n");
        out.push_str(&format!("  reclaimable: {} ({})\n",
            report.total_reclaimable_bytes,
            super::storageclean::format_size(report.total_reclaimable_bytes)));
        out.push_str(&format!("  items: {}\n", report.total_items));
        out.push_str(&format!("  duration: {} us\n", report.scan_duration_us));
        for cat in &report.categories {
            out.push_str(&format!("  {}: {} items, {}\n",
                cat.category.label(), cat.item_count,
                super::storageclean::format_size(cat.total_bytes)));
        }
    }

    out.into_bytes()
}

fn gen_sysdiag() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (issue_count, total_runs, total_issues_found, history_count, ops) =
        super::sysdiag::stats();

    out.push_str(&format!("issue_count: {}\n", issue_count));
    out.push_str(&format!("total_runs: {}\n", total_runs));
    out.push_str(&format!("total_issues_found: {}\n", total_issues_found));
    out.push_str(&format!("history_count: {}\n", history_count));
    out.push_str(&format!("ops: {}\n", ops));

    // Show quick check results
    let issues = super::sysdiag::quick_check();
    if issues.is_empty() {
        out.push_str("status: healthy\n");
    } else {
        out.push_str(&format!("status: {} issue(s)\n", issues.len()));
        for issue in &issues {
            out.push_str(&format!(
                "  [{:?}] {}: {}\n",
                issue.severity, issue.category.label(), issue.title
            ));
        }
    }

    // Show recent history
    let hist = super::sysdiag::history();
    if !hist.is_empty() {
        out.push_str("history:\n");
        for (ts, cats, issues_n, sev) in hist.iter().take(8) {
            out.push_str(&format!(
                "  ts={} categories={} issues={} max_severity={}\n",
                ts, cats, issues_n, sev
            ));
        }
    }

    out.into_bytes()
}

fn gen_nightlight() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (enabled, current_temp, toggle_count, check_count, ops) =
        super::nightlight::stats();

    out.push_str(&format!("enabled: {}\n", enabled));
    out.push_str(&format!("current_temp: {}K\n", current_temp));
    out.push_str(&format!("state: {}\n", super::nightlight::current_state().label()));
    out.push_str(&format!("toggle_count: {}\n", toggle_count));
    out.push_str(&format!("check_count: {}\n", check_count));
    out.push_str(&format!("ops: {}\n", ops));

    if let Ok(cfg) = super::nightlight::config() {
        out.push_str(&format!("schedule_mode: {}\n", cfg.schedule_mode.label()));
        out.push_str(&format!("night_temp: {}K\n", cfg.night_temp));
        out.push_str(&format!("day_temp: {}K\n", cfg.day_temp));
        out.push_str(&format!("start_time: {:02}:{:02}\n",
            cfg.start_time.hour, cfg.start_time.minute));
        out.push_str(&format!("end_time: {:02}:{:02}\n",
            cfg.end_time.hour, cfg.end_time.minute));
        out.push_str(&format!("transition_minutes: {}\n", cfg.transition_minutes));
        out.push_str(&format!("disable_on_battery: {}\n", cfg.disable_on_battery));
        if let Some(loc) = &cfg.location {
            out.push_str(&format!("location: lat={} lon={}\n",
                loc.latitude, loc.longitude));
        }
    }

    let (r, g, b) = super::nightlight::temp_to_rgb(current_temp);
    out.push_str(&format!("rgb: {},{},{}\n", r, g, b));

    out.into_bytes()
}

fn gen_tasksched() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (task_count, total_runs, total_failures, hist_count, ops) =
        super::tasksched::stats();

    out.push_str(&format!("task_count: {}\n", task_count));
    out.push_str(&format!("total_runs: {}\n", total_runs));
    out.push_str(&format!("total_failures: {}\n", total_failures));
    out.push_str(&format!("history_entries: {}\n", hist_count));
    out.push_str(&format!("ops: {}\n", ops));

    let tasks = super::tasksched::list_tasks();
    for task in &tasks {
        out.push_str(&format!("task {}: {} [{}] {} {:02}:{:02} runs={} {}\n",
            task.id, task.name, task.schedule_type.label(),
            task.status.label(), task.hour, task.minute,
            task.run_count, task.command));
    }

    if let Some((id, name, h, m)) = super::tasksched::next_due() {
        out.push_str(&format!("next_due: {} ({}) at {:02}:{:02}\n", name, id, h, m));
    }

    out.into_bytes()
}

fn gen_envvars() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (sys_count, user_count, total_uv, ops) = super::envvars::stats();
    out.push_str(&format!("system_vars: {}\n", sys_count));
    out.push_str(&format!("user_count: {}\n", user_count));
    out.push_str(&format!("total_user_vars: {}\n", total_uv));
    out.push_str(&format!("ops: {}\n", ops));

    let sys_vars = super::envvars::list_system();
    for v in &sys_vars {
        let ro = if v.read_only { " [ro]" } else { "" };
        out.push_str(&format!("{}={}{}\n", v.name, v.value, ro));
    }

    out.into_bytes()
}

fn gen_bluetooth() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (dev_count, connected, scan_count, pair_count, ops) = super::bluetooth::stats();
    out.push_str(&format!("enabled: {}\n", super::bluetooth::is_enabled()));
    out.push_str(&format!("devices: {}\n", dev_count));
    out.push_str(&format!("connected: {}\n", connected));
    out.push_str(&format!("scans: {}\n", scan_count));
    out.push_str(&format!("pairs: {}\n", pair_count));
    out.push_str(&format!("ops: {}\n", ops));

    if let Ok(cfg) = super::bluetooth::config() {
        out.push_str(&format!("adapter: {} [{}]\n", cfg.adapter_name, cfg.adapter_state.label()));
        out.push_str(&format!("bt_version: {}\n", cfg.bt_version));
        out.push_str(&format!("discoverable: {}\n", cfg.discoverable));
        out.push_str(&format!("auto_connect: {}\n", cfg.auto_connect));
    }

    let devices = super::bluetooth::list_devices();
    for dev in &devices {
        let bat = dev.battery_pct.map_or(String::new(), |b| format!(" bat={}%", b));
        out.push_str(&format!("{} {} [{}] {} {}{}\n",
            dev.address, dev.name, dev.device_type.label(),
            dev.state.label(), dev.device_type.icon(), bat));
    }

    out.into_bytes()
}

fn gen_printmgr() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (printer_count, pending, total_pages, hist_count, ops) = super::printmgr::stats();
    out.push_str(&format!("printers: {}\n", printer_count));
    out.push_str(&format!("pending_jobs: {}\n", pending));
    out.push_str(&format!("total_pages: {}\n", total_pages));
    out.push_str(&format!("history: {}\n", hist_count));
    out.push_str(&format!("ops: {}\n", ops));

    if let Some(def) = super::printmgr::default_printer() {
        out.push_str(&format!("default: {} ({})\n", def.name, def.model));
    }

    let printers = super::printmgr::list_printers();
    for p in &printers {
        let def = if p.is_default { " *" } else { "" };
        out.push_str(&format!("{}: {} [{}] {} jobs={} pages={}{}\n",
            p.id, p.name, p.printer_type.label(),
            p.status.label(), p.total_jobs, p.total_pages, def));
    }

    out.into_bytes()
}

fn gen_screenrec() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (count, active, total, total_sec, total_bytes, ops) = super::screenrec::stats();
    out.push_str(&format!("recordings: {}\n", count));
    out.push_str(&format!("active: {}\n", active));
    out.push_str(&format!("is_recording: {}\n", super::screenrec::is_recording()));
    out.push_str(&format!("total_recordings: {}\n", total));
    out.push_str(&format!("total_seconds: {}\n", total_sec));
    out.push_str(&format!("total_bytes: {}\n", total_bytes));
    out.push_str(&format!("ops: {}\n", ops));

    if let Ok(cfg) = super::screenrec::get_config() {
        out.push_str(&format!("format: {}\n", cfg.format.label()));
        out.push_str(&format!("audio: {}\n", cfg.audio.label()));
        out.push_str(&format!("quality: {}\n", cfg.quality.label()));
        out.push_str(&format!("fps: {}\n", cfg.fps));
        out.push_str(&format!("cursor: {}\n", cfg.show_cursor));
        out.push_str(&format!("output_dir: {}\n", cfg.output_dir));
    }

    out.into_bytes()
}

fn gen_datausage() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (apps, daily, rx, tx, limits, ops) = super::datausage::stats();
    out.push_str(&format!("apps_tracked: {}\n", apps));
    out.push_str(&format!("daily_records: {}\n", daily));
    out.push_str(&format!("total_rx: {}\n", rx));
    out.push_str(&format!("total_tx: {}\n", tx));
    out.push_str(&format!("total_rx_human: {}\n", super::datausage::format_bytes(rx)));
    out.push_str(&format!("total_tx_human: {}\n", super::datausage::format_bytes(tx)));
    out.push_str(&format!("limits: {}\n", limits));
    out.push_str(&format!("metered: {}\n", super::datausage::metered_status().label()));
    out.push_str(&format!("restrict_background: {}\n", super::datausage::should_restrict_background()));
    out.push_str(&format!("ops: {}\n", ops));

    let top_apps = super::datausage::app_usage();
    if !top_apps.is_empty() {
        out.push_str("top_apps:\n");
        for app in top_apps.iter().take(10) {
            out.push_str(&format!("  {}: rx={} tx={} total={}\n",
                app.app_id,
                super::datausage::format_bytes(app.rx_bytes),
                super::datausage::format_bytes(app.tx_bytes),
                super::datausage::format_bytes(app.total_bytes())));
        }
    }

    out.into_bytes()
}

fn gen_mousesettings() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (devices, speed, accel, left, natural, ops) = super::mousesettings::stats();
    out.push_str(&format!("devices: {}\n", devices));
    out.push_str(&format!("speed: {}\n", speed));
    out.push_str(&format!("accel_profile: {}\n", accel));
    out.push_str(&format!("left_handed: {}\n", left));
    out.push_str(&format!("natural_scroll: {}\n", natural));
    out.push_str(&format!("ops: {}\n", ops));

    if let Ok(cfg) = super::mousesettings::config() {
        out.push_str(&format!("accel_factor: {}\n", cfg.accel_factor));
        out.push_str(&format!("scroll_speed: {}\n", cfg.scroll_speed));
        out.push_str(&format!("scroll_method: {}\n", cfg.scroll_method.label()));
        out.push_str(&format!("double_click_ms: {}\n", cfg.double_click_ms));
        out.push_str(&format!("scroll_lines: {}\n", cfg.scroll_lines));
        out.push_str(&format!("middle_click_paste: {}\n", cfg.middle_click_paste));
    }

    out.into_bytes()
}

fn gen_touchpad() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (devices, gestures, tap, natural, sens, ops) = super::touchpad::stats();
    out.push_str(&format!("devices: {}\n", devices));
    out.push_str(&format!("gestures: {}\n", gestures));
    out.push_str(&format!("tap_to_click: {}\n", tap));
    out.push_str(&format!("natural_scroll: {}\n", natural));
    out.push_str(&format!("sensitivity: {}\n", sens));
    out.push_str(&format!("ops: {}\n", ops));

    if let Ok(cfg) = super::touchpad::config() {
        out.push_str(&format!("enabled: {}\n", cfg.enabled));
        out.push_str(&format!("scroll_method: {}\n", cfg.scroll_method.label()));
        out.push_str(&format!("click_method: {}\n", cfg.click_method.label()));
        out.push_str(&format!("disable_while_typing: {}\n", cfg.disable_while_typing));
        out.push_str(&format!("palm_rejection: {}\n", cfg.palm_rejection));
    }

    out.into_bytes()
}

fn gen_powerprofile() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (count, active, switches, batt_pct, batt_state, ops) = super::powerprofile::stats();
    out.push_str(&format!("profiles: {}\n", count));
    out.push_str(&format!("active: {}\n", active));
    out.push_str(&format!("switches: {}\n", switches));
    out.push_str(&format!("battery_pct: {}\n", batt_pct));
    out.push_str(&format!("battery_state: {}\n", batt_state));
    out.push_str(&format!("reduce_background: {}\n", super::powerprofile::should_reduce_background()));
    out.push_str(&format!("disable_animations: {}\n", super::powerprofile::should_disable_animations()));
    out.push_str(&format!("cpu_governor: {}\n", super::powerprofile::active_cpu_governor().label()));
    out.push_str(&format!("ops: {}\n", ops));

    out.into_bytes()
}

fn gen_defaultapps() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (types, cats, overrides, ops) = super::defaultapps::stats();
    out.push_str(&format!("type_mappings: {}\n", types));
    out.push_str(&format!("category_defaults: {}\n", cats));
    out.push_str(&format!("user_overrides: {}\n", overrides));
    out.push_str(&format!("ops: {}\n", ops));

    let cat_defaults = super::defaultapps::list_category_defaults(0);
    if !cat_defaults.is_empty() {
        out.push_str("categories:\n");
        for cd in &cat_defaults {
            out.push_str(&format!("  {}: {}\n", cd.category.label(), cd.app_id));
        }
    }

    out.into_bytes()
}

fn gen_monitors() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (total, enabled, mode, primary_id, ops) = super::monitors::stats();
    out.push_str(&format!("monitors: {}\n", total));
    out.push_str(&format!("enabled: {}\n", enabled));
    out.push_str(&format!("layout_mode: {}\n", mode));
    out.push_str(&format!("primary_id: {}\n", primary_id));
    out.push_str(&format!("ops: {}\n", ops));

    let (bx, by, bw, bh) = super::monitors::desktop_bounds();
    out.push_str(&format!("desktop: {}x{} at ({},{})\n", bw, bh, bx, by));

    let monitors = super::monitors::list_monitors();
    for m in &monitors {
        let primary = if m.primary { " [primary]" } else { "" };
        let enabled_str = if m.enabled { "" } else { " [disabled]" };
        out.push_str(&format!("{}: {} {}x{}@{}Hz pos=({},{}) scale={}% {}{}{}\n",
            m.id, m.name, m.width, m.height, m.refresh_hz,
            m.x, m.y, m.scale_pct,
            m.connector.label(), primary, enabled_str));
    }

    out.into_bytes()
}

fn gen_fwsettings() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (rules, apps, blocked, allowed, enabled, ops) = super::fwsettings::stats();
    out.push_str(&format!("enabled: {}\n", enabled));
    out.push_str(&format!("zone: {}\n", super::fwsettings::active_zone().label()));
    out.push_str(&format!("rules: {}\n", rules));
    out.push_str(&format!("app_permissions: {}\n", apps));
    out.push_str(&format!("total_blocked: {}\n", blocked));
    out.push_str(&format!("total_allowed: {}\n", allowed));
    out.push_str(&format!("ops: {}\n", ops));

    out.into_bytes()
}

fn gen_updatemgr() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (pending, history, version, channel, auto, ops) = super::updatemgr::stats();
    out.push_str(&format!("os_version: {}\n", version));
    out.push_str(&format!("channel: {}\n", channel));
    out.push_str(&format!("auto_check: {}\n", auto));
    out.push_str(&format!("pending_updates: {}\n", pending));
    out.push_str(&format!("history: {}\n", history));
    out.push_str(&format!("pending_size: {}\n", super::updatemgr::format_update_size(super::updatemgr::pending_size())));
    out.push_str(&format!("ops: {}\n", ops));

    let (crit, imp, rec, opt) = super::updatemgr::pending_count();
    if crit + imp + rec + opt > 0 {
        out.push_str(&format!("critical: {}\n", crit));
        out.push_str(&format!("important: {}\n", imp));
        out.push_str(&format!("recommended: {}\n", rec));
        out.push_str(&format!("optional: {}\n", opt));
    }

    out.into_bytes()
}

fn gen_notifprefs() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (app_count, sounds, position, dismiss, ops) = super::notifprefs::stats();
    out.push_str(&format!("app_prefs: {}\n", app_count));
    out.push_str(&format!("sounds: {}\n", sounds));
    out.push_str(&format!("position: {}\n", position));
    out.push_str(&format!("dismiss_timeout: {}\n", dismiss));
    out.push_str(&format!("ops: {}\n", ops));

    let prefs = super::notifprefs::list_app_prefs();
    if !prefs.is_empty() {
        out.push_str("apps:\n");
        for p in &prefs {
            out.push_str(&format!("  {}: enabled={} banner={} sound={} priority={} total={}\n",
                p.app_id, p.enabled, p.banner_style.label(),
                p.sound, p.priority.label(), p.total_count));
        }
    }

    out.into_bytes()
}

fn gen_fileshare() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (local, remote, enabled, connected, ops) = super::fileshare::stats();
    out.push_str(&format!("sharing_enabled: {}\n", enabled));
    out.push_str(&format!("hostname: {}\n", super::fileshare::hostname()));
    out.push_str(&format!("local_shares: {}\n", local));
    out.push_str(&format!("remote_shares: {}\n", remote));
    out.push_str(&format!("connected_remotes: {}\n", connected));
    out.push_str(&format!("ops: {}\n", ops));

    let shares = super::fileshare::list_shares();
    for s in &shares {
        let en = if s.enabled { "" } else { " [disabled]" };
        out.push_str(&format!("{}: {} → {} [{}] {}{}\n",
            s.id, s.name, s.path, s.protocol.label(), s.access.label(), en));
    }

    out.into_bytes()
}

fn gen_parental() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (profile_count, total_blocked, ops) = super::parental::stats();
    out.push_str(&format!("profiles: {}\n", profile_count));
    out.push_str(&format!("total_blocked: {}\n", total_blocked));
    out.push_str(&format!("ops: {}\n", ops));

    let profiles = super::parental::list_profiles();
    for p in &profiles {
        let en = if p.enabled { "enabled" } else { "disabled" };
        out.push_str(&format!("{}: uid={} filter={} apps={} [{}]\n",
            p.name, p.uid, p.filter_level.label(), p.app_mode.label(), en));
    }

    out.into_bytes()
}

fn gen_audiodevice() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (device_count, output_count, input_count, default_out, default_in, ops) =
        super::audiodevice::stats();
    out.push_str(&format!("devices: {}\n", device_count));
    out.push_str(&format!("output_devices: {}\n", output_count));
    out.push_str(&format!("input_devices: {}\n", input_count));
    out.push_str(&format!("default_output_id: {}\n", default_out));
    out.push_str(&format!("default_input_id: {}\n", default_in));
    out.push_str(&format!("ops: {}\n", ops));

    let devices = super::audiodevice::list_devices();
    for d in &devices {
        let muted = if d.muted { " [muted]" } else { "" };
        let def = if d.is_default { " *" } else { "" };
        out.push_str(&format!("{}: {} ({}) vol={}{}{}\n",
            d.id, d.name, d.direction.label(), d.volume, muted, def));
    }

    out.into_bytes()
}

fn gen_sessionmgr() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (session_count, login_count, lock_count, active_uid, ops) = super::sessionmgr::stats();
    out.push_str(&format!("sessions: {}\n", session_count));
    out.push_str(&format!("total_logins: {}\n", login_count));
    out.push_str(&format!("total_locks: {}\n", lock_count));
    out.push_str(&format!("active_uid: {}\n", active_uid));
    out.push_str(&format!("ops: {}\n", ops));

    let sessions = super::sessionmgr::list_sessions();
    for s in &sessions {
        let active = if s.is_active { " *active" } else { "" };
        out.push_str(&format!("{}: {} ({}) {} [{}]{}\n",
            s.id, s.username, s.uid, s.session_type.label(),
            s.state.label(), active));
    }

    out.into_bytes()
}

fn gen_crashreport() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (report_count, total_crashes, fatal_count, enabled, ops) = super::crashreport::stats();
    out.push_str(&format!("enabled: {}\n", enabled));
    out.push_str(&format!("reports: {}\n", report_count));
    out.push_str(&format!("total_crashes: {}\n", total_crashes));
    out.push_str(&format!("fatal_crashes: {}\n", fatal_count));
    out.push_str(&format!("ops: {}\n", ops));

    let reports = super::crashreport::list_reports();
    for r in reports.iter().take(20) {
        out.push_str(&format!("{}: pid={} {} {} [{}]\n",
            r.id, r.pid, r.process_name, r.signal.label(),
            r.severity.label()));
    }

    out.into_bytes()
}

fn gen_netproxy() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (proxy_count, bypass_count, mode_label, app_overrides, ops) = super::netproxy::stats();
    out.push_str(&format!("mode: {}\n", mode_label));
    out.push_str(&format!("proxies: {}\n", proxy_count));
    out.push_str(&format!("bypass_rules: {}\n", bypass_count));
    out.push_str(&format!("app_overrides: {}\n", app_overrides));
    out.push_str(&format!("ops: {}\n", ops));

    let proxies = super::netproxy::list_proxies();
    for p in &proxies {
        let en = if p.enabled { "" } else { " [disabled]" };
        out.push_str(&format!("{}: {}:{}{}\n", p.protocol.label(), p.host, p.port, en));
    }

    out.into_bytes()
}

fn gen_fileversion() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (ver_count, file_count, captured, restored, watch_count, ops) = super::fileversion::stats();
    out.push_str(&format!("versions: {}\n", ver_count));
    out.push_str(&format!("versioned_files: {}\n", file_count));
    out.push_str(&format!("total_captured: {}\n", captured));
    out.push_str(&format!("total_restored: {}\n", restored));
    out.push_str(&format!("watched_paths: {}\n", watch_count));
    out.push_str(&format!("ops: {}\n", ops));

    let watches = super::fileversion::list_watches();
    for w in &watches {
        out.push_str(&format!("{}: {}\n", w.path, w.policy.label()));
    }

    out.into_bytes()
}

fn gen_devicemgr() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (total, ok, no_drv, hotplug, ops) = super::devicemgr::stats();
    out.push_str(&format!("devices: {}\n", total));
    out.push_str(&format!("ok: {}\n", ok));
    out.push_str(&format!("no_driver: {}\n", no_drv));
    out.push_str(&format!("hotplug_events: {}\n", hotplug));
    out.push_str(&format!("ops: {}\n", ops));

    let devices = super::devicemgr::list_devices();
    for d in &devices {
        let drv = if d.driver.is_empty() { "no driver" } else { &d.driver };
        out.push_str(&format!("{}: {} [{}] {} ({})\n",
            d.id, d.name, d.bus.label(), d.status.label(), drv));
    }

    out.into_bytes()
}

fn gen_location() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (enabled, perm_count, requests, denied, hist_len, ops) = super::location::stats();
    out.push_str(&format!("enabled: {}\n", enabled));
    out.push_str(&format!("app_permissions: {}\n", perm_count));
    out.push_str(&format!("total_requests: {}\n", requests));
    out.push_str(&format!("total_denied: {}\n", denied));
    out.push_str(&format!("history_entries: {}\n", hist_len));
    out.push_str(&format!("ops: {}\n", ops));

    if let Some(fix) = super::location::current_location() {
        let _lat = fix.latitude_ud as f64 / 1_000_000.0;
        let _lon = fix.longitude_ud as f64 / 1_000_000.0;
        // Use integer division to show approximate coordinates without float formatting.
        let lat_deg = fix.latitude_ud / 1_000_000;
        let lat_frac = (fix.latitude_ud % 1_000_000).unsigned_abs();
        let lon_deg = fix.longitude_ud / 1_000_000;
        let lon_frac = (fix.longitude_ud % 1_000_000).unsigned_abs();
        out.push_str(&format!("current: {}.{:06},{}.{:06} ±{}m ({})\n",
            lat_deg, lat_frac, lon_deg, lon_frac, fix.accuracy_m, fix.source.label()));
    }

    out.into_bytes()
}

fn gen_diskencrypt() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (total, encrypted, unlocked, failed, ops) = super::diskencrypt::stats();
    out.push_str(&format!("volumes: {}\n", total));
    out.push_str(&format!("encrypted: {}\n", encrypted));
    out.push_str(&format!("unlocked: {}\n", unlocked));
    out.push_str(&format!("failed_unlocks: {}\n", failed));
    out.push_str(&format!("ops: {}\n", ops));

    let vols = super::diskencrypt::list_volumes();
    for v in &vols {
        out.push_str(&format!("{}: {} ({}) {} [{}]\n",
            v.id, v.label, v.device, v.algorithm.label(), v.status.label()));
    }

    out.into_bytes()
}

fn gen_pkgmgr() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (installed, available, upgradeable, repos, ops) = super::pkgmgr::stats();
    out.push_str(&format!("installed: {}\n", installed));
    out.push_str(&format!("available: {}\n", available));
    out.push_str(&format!("upgradeable: {}\n", upgradeable));
    out.push_str(&format!("repositories: {}\n", repos));
    out.push_str(&format!("ops: {}\n", ops));

    let packages = super::pkgmgr::list_installed();
    for p in &packages {
        let up = if p.status == super::pkgmgr::PkgStatus::Upgradeable {
            format!(" → {}", p.available_version)
        } else {
            String::new()
        };
        out.push_str(&format!("{} {} [{}]{}\n", p.name, p.version, p.section.label(), up));
    }

    out.into_bytes()
}

fn gen_remotedesktop() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (active, total, enabled, port, ops) = super::remotedesktop::stats();
    out.push_str(&format!("enabled: {}\n", enabled));
    out.push_str(&format!("port: {}\n", port));
    out.push_str(&format!("active_sessions: {}\n", active));
    out.push_str(&format!("total_connections: {}\n", total));
    out.push_str(&format!("ops: {}\n", ops));

    let sessions = super::remotedesktop::list_sessions();
    for s in &sessions {
        out.push_str(&format!("{}: {} {} [{}] {}\n",
            s.id, s.direction.label(), s.remote_host,
            s.state.label(), s.protocol.label()));
    }

    out.into_bytes()
}

fn gen_restorepoint() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (count, created, restored, space, ops) = super::restorepoint::stats();
    out.push_str(&format!("points: {}\n", count));
    out.push_str(&format!("total_created: {}\n", created));
    out.push_str(&format!("total_restored: {}\n", restored));
    out.push_str(&format!("space_bytes: {}\n", space));
    out.push_str(&format!("ops: {}\n", ops));

    let points = super::restorepoint::list_points();
    for p in points.iter().take(10) {
        out.push_str(&format!("{}: {} [{}] {}\n",
            p.id, p.description, p.restore_type.label(), p.status.label()));
    }

    out.into_bytes()
}

fn gen_battery() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (count, pct, state_label, cycles, alerts, ops) = super::battery::stats();
    out.push_str(&format!("sources: {}\n", count));
    out.push_str(&format!("charge_pct: {}\n", pct));
    out.push_str(&format!("state: {}\n", state_label));
    out.push_str(&format!("cycle_count: {}\n", cycles));
    out.push_str(&format!("alerts: {}\n", alerts));
    out.push_str(&format!("ops: {}\n", ops));

    let sources = super::battery::list_sources();
    for s in &sources {
        out.push_str(&format!("{}: {} ({}) {}% [{}]\n",
            s.id, s.name, s.source_type.label(), s.charge_pct, s.state.label()));
    }

    out.into_bytes()
}

fn gen_dictation() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (state_label, lang_code, transcriptions, words, vocab, ops) = super::dictation::stats();
    out.push_str(&format!("state: {}\n", state_label));
    out.push_str(&format!("language: {}\n", lang_code));
    out.push_str(&format!("transcriptions: {}\n", transcriptions));
    out.push_str(&format!("total_words: {}\n", words));
    out.push_str(&format!("custom_vocab: {}\n", vocab));
    out.push_str(&format!("ops: {}\n", ops));

    out.into_bytes()
}

fn gen_screenreader() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (elements, announcements, queue_len, enabled, rate_label, ops) = super::screenreader::stats();
    out.push_str(&format!("enabled: {}\n", enabled));
    out.push_str(&format!("speech_rate: {}\n", rate_label));
    out.push_str(&format!("elements: {}\n", elements));
    out.push_str(&format!("announcements: {}\n", announcements));
    out.push_str(&format!("queue_len: {}\n", queue_len));
    out.push_str(&format!("ops: {}\n", ops));

    out.into_bytes()
}

fn gen_langpack() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (pack_count, installed_count, system_lang, lookups, misses, ops) = super::langpack::stats();
    out.push_str(&format!("packs: {}\n", pack_count));
    out.push_str(&format!("installed: {}\n", installed_count));
    out.push_str(&format!("system_language: {}\n", system_lang));
    out.push_str(&format!("lookups: {}\n", lookups));
    out.push_str(&format!("misses: {}\n", misses));
    out.push_str(&format!("ops: {}\n", ops));

    out.into_bytes()
}

fn gen_spellcheck() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (dicts, personal, checks, misspellings, corrections, ops) = super::spellcheck::stats();
    out.push_str(&format!("dictionaries: {}\n", dicts));
    out.push_str(&format!("personal_words: {}\n", personal));
    out.push_str(&format!("total_checks: {}\n", checks));
    out.push_str(&format!("misspellings: {}\n", misspellings));
    out.push_str(&format!("corrections: {}\n", corrections));
    out.push_str(&format!("ops: {}\n", ops));

    out.into_bytes()
}

fn gen_screentime() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (app_count, active_secs, idle_secs, switches, focus_events, ops) = super::screentime::stats();
    out.push_str(&format!("tracked_apps: {}\n", app_count));
    out.push_str(&format!("active_secs_today: {}\n", active_secs));
    out.push_str(&format!("idle_secs_today: {}\n", idle_secs));
    out.push_str(&format!("switches_today: {}\n", switches));
    out.push_str(&format!("total_focus_events: {}\n", focus_events));
    out.push_str(&format!("ops: {}\n", ops));

    out.into_bytes()
}

fn gen_disksmart() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (drives, good, warn, checks, alerts, ops) = super::disksmart::stats();
    out.push_str(&format!("drives: {}\n", drives));
    out.push_str(&format!("healthy: {}\n", good));
    out.push_str(&format!("warnings: {}\n", warn));
    out.push_str(&format!("total_checks: {}\n", checks));
    out.push_str(&format!("total_alerts: {}\n", alerts));
    out.push_str(&format!("ops: {}\n", ops));

    out.into_bytes()
}

fn gen_magnifier() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (enabled, zoom, mode, filter, changes, ops) = super::magnifier::stats();
    out.push_str(&format!("enabled: {}\n", enabled));
    out.push_str(&format!("zoom_pct: {}\n", zoom));
    out.push_str(&format!("mode: {}\n", mode));
    out.push_str(&format!("color_filter: {}\n", filter));
    out.push_str(&format!("zoom_changes: {}\n", changes));
    out.push_str(&format!("ops: {}\n", ops));

    out.into_bytes()
}

fn gen_cloudsync() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (accounts, syncs, conflicts, active, ops) = super::cloudsync::stats();
    out.push_str(&format!("accounts: {}\n", accounts));
    out.push_str(&format!("active: {}\n", active));
    out.push_str(&format!("total_syncs: {}\n", syncs));
    out.push_str(&format!("conflicts: {}\n", conflicts));
    out.push_str(&format!("ops: {}\n", ops));

    out.into_bytes()
}

fn gen_gestures() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (mappings, total, enabled, natural, ops) = super::gestures::stats();
    out.push_str(&format!("enabled: {}\n", enabled));
    out.push_str(&format!("mappings: {}\n", mappings));
    out.push_str(&format!("total_gestures: {}\n", total));
    out.push_str(&format!("natural_scroll: {}\n", natural));
    out.push_str(&format!("ops: {}\n", ops));

    out.into_bytes()
}

fn gen_soundevents() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (schemes, mappings, played, enabled, muted, ops) = super::soundevents::stats();
    out.push_str(&format!("enabled: {}\n", enabled));
    out.push_str(&format!("muted: {}\n", muted));
    out.push_str(&format!("schemes: {}\n", schemes));
    out.push_str(&format!("mappings: {}\n", mappings));
    out.push_str(&format!("total_played: {}\n", played));
    out.push_str(&format!("ops: {}\n", ops));

    out.into_bytes()
}

fn gen_usbmgr() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (count, connects, disconnects, safe_removes, power, ops) = super::usbmgr::stats();
    out.push_str(&format!("devices: {}\n", count));
    out.push_str(&format!("total_connects: {}\n", connects));
    out.push_str(&format!("total_disconnects: {}\n", disconnects));
    out.push_str(&format!("safe_removes: {}\n", safe_removes));
    out.push_str(&format!("power_draw_ma: {}\n", power));
    out.push_str(&format!("ops: {}\n", ops));

    out.into_bytes()
}

fn gen_cliphistory() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (entries, pinned, copies, pastes, size, ops) = super::cliphistory::stats();
    out.push_str(&format!("entries: {}\n", entries));
    out.push_str(&format!("pinned: {}\n", pinned));
    out.push_str(&format!("total_copies: {}\n", copies));
    out.push_str(&format!("total_pastes: {}\n", pastes));
    out.push_str(&format!("size_bytes: {}\n", size));
    out.push_str(&format!("ops: {}\n", ops));

    out.into_bytes()
}

fn gen_displaycolor() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (profiles, displays, calibrated, cals, ops) = super::displaycolor::stats();
    out.push_str(&format!("profiles: {}\n", profiles));
    out.push_str(&format!("displays: {}\n", displays));
    out.push_str(&format!("calibrated: {}\n", calibrated));
    out.push_str(&format!("total_calibrations: {}\n", cals));
    out.push_str(&format!("ops: {}\n", ops));

    out.into_bytes()
}

fn gen_syslog() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (entries, total, dropped, errors, crits, ops) = super::syslog::stats();
    out.push_str(&format!("entries: {}\n", entries));
    out.push_str(&format!("total_logged: {}\n", total));
    out.push_str(&format!("dropped: {}\n", dropped));
    out.push_str(&format!("errors: {}\n", errors));
    out.push_str(&format!("critical: {}\n", crits));
    out.push_str(&format!("ops: {}\n", ops));

    out.into_bytes()
}

fn gen_inputa11y() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (sticky, filter, toggle, mouse, keys, filtered, ops) = super::inputa11y::stats();
    out.push_str(&format!("sticky_keys: {}\n", sticky));
    out.push_str(&format!("filter_keys: {}\n", filter));
    out.push_str(&format!("toggle_keys: {}\n", toggle));
    out.push_str(&format!("mouse_keys: {}\n", mouse));
    out.push_str(&format!("total_keys: {}\n", keys));
    out.push_str(&format!("filtered: {}\n", filtered));
    out.push_str(&format!("ops: {}\n", ops));

    out.into_bytes()
}

fn gen_driverupdate() -> Vec<u8> {
    use crate::fs::driverupdate;
    let (driver_count, update_count, total_updates, total_rollbacks, ops) = driverupdate::stats();
    let mut out = String::from("driver_count: ");
    out.push_str(&format!("{}\n", driver_count));
    out.push_str(&format!("updates_available: {}\n", update_count));
    out.push_str(&format!("total_updates: {}\n", total_updates));
    out.push_str(&format!("total_rollbacks: {}\n", total_rollbacks));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_netshare() -> Vec<u8> {
    use crate::fs::netshare;
    let (share_count, connected_count, total_mounts, total_errors, ops) = netshare::stats();
    let mut out = String::from("share_count: ");
    out.push_str(&format!("{}\n", share_count));
    out.push_str(&format!("connected: {}\n", connected_count));
    out.push_str(&format!("total_mounts: {}\n", total_mounts));
    out.push_str(&format!("total_errors: {}\n", total_errors));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_startuprepair() -> Vec<u8> {
    use crate::fs::startuprepair;
    let (session_count, total_checks, total_repairs, failed_boots, ops) = startuprepair::stats();
    let mut out = String::from("session_count: ");
    out.push_str(&format!("{}\n", session_count));
    out.push_str(&format!("total_checks: {}\n", total_checks));
    out.push_str(&format!("total_repairs: {}\n", total_repairs));
    out.push_str(&format!("failed_boots: {}\n", failed_boots));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_remoteassist() -> Vec<u8> {
    use crate::fs::remoteassist;
    let (active_sessions, total_sessions, total_files, ops) = remoteassist::stats();
    let mut out = String::from("active_sessions: ");
    out.push_str(&format!("{}\n", active_sessions));
    out.push_str(&format!("total_sessions: {}\n", total_sessions));
    out.push_str(&format!("total_files: {}\n", total_files));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_taskmon() -> Vec<u8> {
    use crate::fs::taskmon;
    let (task_count, total_created, total_killed, total_suspended, ops) = taskmon::stats();
    let mut out = String::from("task_count: ");
    out.push_str(&format!("{}\n", task_count));
    out.push_str(&format!("total_created: {}\n", total_created));
    out.push_str(&format!("total_killed: {}\n", total_killed));
    out.push_str(&format!("total_suspended: {}\n", total_suspended));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_printqueue() -> Vec<u8> {
    use crate::fs::printqueue;
    let (printer_count, total_jobs, total_pages, active_jobs, ops) = printqueue::stats();
    let mut out = String::from("printer_count: ");
    out.push_str(&format!("{}\n", printer_count));
    out.push_str(&format!("total_jobs: {}\n", total_jobs));
    out.push_str(&format!("total_pages: {}\n", total_pages));
    out.push_str(&format!("active_jobs: {}\n", active_jobs));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_servicemgr() -> Vec<u8> {
    use crate::fs::servicemgr;
    let (total_count, running_count, total_starts, total_stops, total_failures, ops) = servicemgr::stats();
    let mut out = String::from("service_count: ");
    out.push_str(&format!("{}\n", total_count));
    out.push_str(&format!("running: {}\n", running_count));
    out.push_str(&format!("total_starts: {}\n", total_starts));
    out.push_str(&format!("total_stops: {}\n", total_stops));
    out.push_str(&format!("total_failures: {}\n", total_failures));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_hwmonitor() -> Vec<u8> {
    use crate::fs::hwmonitor;
    let (sensor_count, component_count, total_readings, total_alerts, ops) = hwmonitor::stats();
    let mut out = String::from("sensor_count: ");
    out.push_str(&format!("{}\n", sensor_count));
    out.push_str(&format!("component_count: {}\n", component_count));
    out.push_str(&format!("total_readings: {}\n", total_readings));
    out.push_str(&format!("total_alerts: {}\n", total_alerts));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_appsandbox() -> Vec<u8> {
    use crate::fs::appsandbox;
    let (sandbox_count, total_created, total_checks, total_denied, ops) = appsandbox::stats();
    let mut out = String::from("sandbox_count: ");
    out.push_str(&format!("{}\n", sandbox_count));
    out.push_str(&format!("total_created: {}\n", total_created));
    out.push_str(&format!("total_checks: {}\n", total_checks));
    out.push_str(&format!("total_denied: {}\n", total_denied));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_gamepadinput() -> Vec<u8> {
    use crate::fs::gamepadinput;
    let (gamepad_count, connected_count, total_connected, total_inputs, ops) = gamepadinput::stats();
    let mut out = String::from("gamepad_count: ");
    out.push_str(&format!("{}\n", gamepad_count));
    out.push_str(&format!("connected: {}\n", connected_count));
    out.push_str(&format!("total_connected: {}\n", total_connected));
    out.push_str(&format!("total_inputs: {}\n", total_inputs));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_sysrestore() -> Vec<u8> {
    use crate::fs::sysrestore;
    let (snapshot_count, total_created, total_restored, total_rotated, ops) = sysrestore::stats();
    let mut out = String::from("snapshot_count: ");
    out.push_str(&format!("{}\n", snapshot_count));
    out.push_str(&format!("total_created: {}\n", total_created));
    out.push_str(&format!("total_restored: {}\n", total_restored));
    out.push_str(&format!("total_rotated: {}\n", total_rotated));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_audiomux() -> Vec<u8> {
    use crate::fs::audiomux;
    let (output_count, stream_count, total_created, total_reroutes, ops) = audiomux::stats();
    let mut out = String::from("output_count: ");
    out.push_str(&format!("{}\n", output_count));
    out.push_str(&format!("stream_count: {}\n", stream_count));
    out.push_str(&format!("total_streams_created: {}\n", total_created));
    out.push_str(&format!("total_reroutes: {}\n", total_reroutes));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_netthrottle() -> Vec<u8> {
    use crate::fs::netthrottle;
    let (rule_count, total_throttled, total_blocked, enabled, ops) = netthrottle::stats();
    let mut out = String::from("rule_count: ");
    out.push_str(&format!("{}\n", rule_count));
    out.push_str(&format!("total_throttled: {}\n", total_throttled));
    out.push_str(&format!("total_blocked: {}\n", total_blocked));
    out.push_str(&format!("enabled: {}\n", enabled));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_dumpanalyzer() -> Vec<u8> {
    use crate::fs::dumpanalyzer;
    let (count, total, kernel, app, ops) = dumpanalyzer::stats();
    let mut out = String::from("analysis_count: ");
    out.push_str(&format!("{}\n", count));
    out.push_str(&format!("total_analyzed: {}\n", total));
    out.push_str(&format!("kernel_crashes: {}\n", kernel));
    out.push_str(&format!("app_crashes: {}\n", app));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_memdiag() -> Vec<u8> {
    use crate::fs::memdiag;
    let (test_count, total_tests, correctable, uncorrectable, ops) = memdiag::stats();
    let mut out = String::from("test_count: ");
    out.push_str(&format!("{}\n", test_count));
    out.push_str(&format!("total_tests: {}\n", total_tests));
    out.push_str(&format!("correctable_errors: {}\n", correctable));
    out.push_str(&format!("uncorrectable_errors: {}\n", uncorrectable));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_parentaltime() -> Vec<u8> {
    use crate::fs::parentaltime;
    let (count, enforcements, warnings, ops) = parentaltime::stats();
    let mut out = String::from("config_count: ");
    out.push_str(&format!("{}\n", count));
    out.push_str(&format!("total_enforcements: {}\n", enforcements));
    out.push_str(&format!("total_warnings: {}\n", warnings));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_mediakeys() -> Vec<u8> {
    use crate::fs::mediakeys;
    let (count, total, keys, active_id, ops) = mediakeys::stats();
    let mut out = String::from("session_count: ");
    out.push_str(&format!("{}\n", count));
    out.push_str(&format!("total_sessions: {}\n", total));
    out.push_str(&format!("total_key_events: {}\n", keys));
    out.push_str(&format!("active_session_id: {}\n", active_id));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_webcam() -> Vec<u8> {
    use crate::fs::webcam;
    let (cams, streams, total, denied, ops) = webcam::stats();
    let mut out = String::from("camera_count: ");
    out.push_str(&format!("{}\n", cams));
    out.push_str(&format!("active_streams: {}\n", streams));
    out.push_str(&format!("total_streams: {}\n", total));
    out.push_str(&format!("total_denied: {}\n", denied));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_speechio() -> Vec<u8> {
    use crate::fs::speechio;
    let (voices, spoken, recognized, tts_on, ops) = speechio::stats();
    let mut out = String::from("voice_count: ");
    out.push_str(&format!("{}\n", voices));
    out.push_str(&format!("total_spoken: {}\n", spoken));
    out.push_str(&format!("total_recognized: {}\n", recognized));
    out.push_str(&format!("tts_enabled: {}\n", tts_on));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_mobilelink() -> Vec<u8> {
    use crate::fs::mobilelink;
    let (devs, paired, notifs, msgs, transfers, ops) = mobilelink::stats();
    let mut out = String::from("device_count: ");
    out.push_str(&format!("{}\n", devs));
    out.push_str(&format!("total_paired: {}\n", paired));
    out.push_str(&format!("total_notifications: {}\n", notifs));
    out.push_str(&format!("total_messages: {}\n", msgs));
    out.push_str(&format!("total_transfers: {}\n", transfers));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_screenlock() -> Vec<u8> {
    use crate::fs::screenlock;
    let (locks, unlocks, failed, lockouts, ops) = screenlock::stats();
    let mut out = String::from("total_locks: ");
    out.push_str(&format!("{}\n", locks));
    out.push_str(&format!("total_unlocks: {}\n", unlocks));
    out.push_str(&format!("total_failed: {}\n", failed));
    out.push_str(&format!("total_lockouts: {}\n", lockouts));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_appstore() -> Vec<u8> {
    use crate::fs::appstore;
    let (apps, installed, installs, updates, ops) = appstore::stats();
    let mut out = String::from("app_count: ");
    out.push_str(&format!("{}\n", apps));
    out.push_str(&format!("installed: {}\n", installed));
    out.push_str(&format!("total_installs: {}\n", installs));
    out.push_str(&format!("total_updates: {}\n", updates));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_wintiling() -> Vec<u8> {
    use crate::fs::wintiling;
    let (ws, wins, tiles, retiles, ops) = wintiling::stats();
    let mut out = String::from("workspace_count: ");
    out.push_str(&format!("{}\n", ws));
    out.push_str(&format!("window_count: {}\n", wins));
    out.push_str(&format!("total_tiles: {}\n", tiles));
    out.push_str(&format!("total_retiles: {}\n", retiles));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_peninput() -> Vec<u8> {
    use crate::fs::peninput;
    let (pens, events, strokes, ops) = peninput::stats();
    let mut out = String::from("pen_count: ");
    out.push_str(&format!("{}\n", pens));
    out.push_str(&format!("total_events: {}\n", events));
    out.push_str(&format!("total_strokes: {}\n", strokes));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_brightness() -> Vec<u8> {
    use crate::fs::brightness;
    let (displays, adjustments, auto, ops) = brightness::stats();
    let mut out = String::from("display_count: ");
    out.push_str(&format!("{}\n", displays));
    out.push_str(&format!("total_adjustments: {}\n", adjustments));
    out.push_str(&format!("total_auto: {}\n", auto));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_quicksettings() -> Vec<u8> {
    use crate::fs::quicksettings;
    let (tiles, toggles, adjustments, ops) = quicksettings::stats();
    let mut out = String::from("tile_count: ");
    out.push_str(&format!("{}\n", tiles));
    out.push_str(&format!("total_toggles: {}\n", toggles));
    out.push_str(&format!("total_adjustments: {}\n", adjustments));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_volumeosd() -> Vec<u8> {
    use crate::fs::volumeosd;
    let (total, enabled, position, ops) = volumeosd::stats();
    let mut out = String::from("total_shown: ");
    out.push_str(&format!("{}\n", total));
    out.push_str(&format!("enabled: {}\n", enabled));
    out.push_str(&format!("position: {}\n", position));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_netdiag() -> Vec<u8> {
    use crate::fs::netdiag;
    let (results, pings, traces, lookups, ops) = netdiag::stats();
    let mut out = String::from("result_count: ");
    out.push_str(&format!("{}\n", results));
    out.push_str(&format!("total_pings: {}\n", pings));
    out.push_str(&format!("total_traces: {}\n", traces));
    out.push_str(&format!("total_lookups: {}\n", lookups));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_sharesheet() -> Vec<u8> {
    use alloc::format;
    use super::sharesheet;
    let (target_count, total_shares, ops) = sharesheet::stats();
    let mut out = String::from("target_count: ");
    out.push_str(&format!("{}\n", target_count));
    out.push_str(&format!("total_shares: {}\n", total_shares));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_oobe() -> Vec<u8> {
    use alloc::format;
    use super::oobe;
    let (step, completed, skipped, ops) = oobe::stats();
    let mut out = String::from("current_step: ");
    out.push_str(&format!("{}\n", step));
    out.push_str(&format!("completed: {}\n", completed));
    out.push_str(&format!("skipped_count: {}\n", skipped));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_hdrdisplay() -> Vec<u8> {
    use alloc::format;
    use super::hdrdisplay;
    let (display_count, hdr_enabled, total_switches, ops) = hdrdisplay::stats();
    let mut out = String::from("display_count: ");
    out.push_str(&format!("{}\n", display_count));
    out.push_str(&format!("hdr_enabled_count: {}\n", hdr_enabled));
    out.push_str(&format!("total_switches: {}\n", total_switches));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_surroundsound() -> Vec<u8> {
    use alloc::format;
    use super::surroundsound;
    let (config_count, total_configs, total_cals, ops) = surroundsound::stats();
    let mut out = String::from("config_count: ");
    out.push_str(&format!("{}\n", config_count));
    out.push_str(&format!("total_configs: {}\n", total_configs));
    out.push_str(&format!("total_calibrations: {}\n", total_cals));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_audioeq() -> Vec<u8> {
    use alloc::format;
    use super::audioeq;
    let (config_count, total_adj, total_presets, ops) = audioeq::stats();
    let mut out = String::from("config_count: ");
    out.push_str(&format!("{}\n", config_count));
    out.push_str(&format!("total_adjustments: {}\n", total_adj));
    out.push_str(&format!("total_preset_changes: {}\n", total_presets));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_screensaver() -> Vec<u8> {
    use alloc::format;
    use super::screensaver;
    let (saver_count, total_acts, total_deacts, ops) = screensaver::stats();
    let mut out = String::from("saver_count: ");
    out.push_str(&format!("{}\n", saver_count));
    out.push_str(&format!("total_activations: {}\n", total_acts));
    out.push_str(&format!("total_deactivations: {}\n", total_deacts));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_colortemp() -> Vec<u8> {
    use alloc::format;
    use super::colortemp;
    let (profile_count, active_id, total_adj, ops) = colortemp::stats();
    let mut out = String::from("profile_count: ");
    out.push_str(&format!("{}\n", profile_count));
    out.push_str(&format!("active_profile_id: {}\n", active_id));
    out.push_str(&format!("total_adjustments: {}\n", total_adj));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_gamemode() -> Vec<u8> {
    use alloc::format;
    use super::gamemode;
    let (game_count, total_acts, active, ops) = gamemode::stats();
    let mut out = String::from("game_count: ");
    out.push_str(&format!("{}\n", game_count));
    out.push_str(&format!("total_activations: {}\n", total_acts));
    out.push_str(&format!("active: {}\n", active));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_dpiscaling() -> Vec<u8> {
    use alloc::format;
    use super::dpiscaling;
    let (display_count, override_count, total_changes, ops) = dpiscaling::stats();
    let mut out = String::from("display_count: ");
    out.push_str(&format!("{}\n", display_count));
    out.push_str(&format!("override_count: {}\n", override_count));
    out.push_str(&format!("total_changes: {}\n", total_changes));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_netprofile() -> Vec<u8> {
    use alloc::format;
    use super::netprofile;
    let (profile_count, active_id, total_switches, ops) = netprofile::stats();
    let mut out = String::from("profile_count: ");
    out.push_str(&format!("{}\n", profile_count));
    out.push_str(&format!("active_profile_id: {}\n", active_id));
    out.push_str(&format!("total_switches: {}\n", total_switches));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_apppermissions() -> Vec<u8> {
    use alloc::format;
    use super::apppermissions;
    let (entries, checks, grants, denials, ops) = apppermissions::stats();
    let mut out = String::from("entry_count: ");
    out.push_str(&format!("{}\n", entries));
    out.push_str(&format!("total_checks: {}\n", checks));
    out.push_str(&format!("total_grants: {}\n", grants));
    out.push_str(&format!("total_denials: {}\n", denials));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_kbshortcuts() -> Vec<u8> {
    use alloc::format;
    use super::kbshortcuts;
    let (count, binds, triggers, ops) = kbshortcuts::stats();
    let mut out = String::from("shortcut_count: ");
    out.push_str(&format!("{}\n", count));
    out.push_str(&format!("total_binds: {}\n", binds));
    out.push_str(&format!("total_triggers: {}\n", triggers));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_displayarrange() -> Vec<u8> {
    use alloc::format;
    use super::displayarrange;
    let (count, topo, rearrangements, ops) = displayarrange::stats();
    let mut out = String::from("display_count: ");
    out.push_str(&format!("{}\n", count));
    out.push_str(&format!("topology: {}\n", topo));
    out.push_str(&format!("total_rearrangements: {}\n", rearrangements));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_sysanimations() -> Vec<u8> {
    use alloc::format;
    use super::sysanimations;
    let (count, enabled, changes, ops) = sysanimations::stats();
    let mut out = String::from("animation_count: ");
    out.push_str(&format!("{}\n", count));
    out.push_str(&format!("enabled_count: {}\n", enabled));
    out.push_str(&format!("total_changes: {}\n", changes));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_filevault() -> Vec<u8> {
    use alloc::format;
    use super::filevault;
    let (count, unlocked, unlocks, failed, ops) = filevault::stats();
    let mut out = String::from("vault_count: ");
    out.push_str(&format!("{}\n", count));
    out.push_str(&format!("unlocked_count: {}\n", unlocked));
    out.push_str(&format!("total_unlocks: {}\n", unlocks));
    out.push_str(&format!("total_failed_auths: {}\n", failed));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_mousegestures() -> Vec<u8> {
    use alloc::format;
    use super::mousegestures;
    let (count, gestures, recognized, ops) = mousegestures::stats();
    let mut out = String::from("binding_count: ");
    out.push_str(&format!("{}\n", count));
    out.push_str(&format!("total_gestures: {}\n", gestures));
    out.push_str(&format!("total_recognized: {}\n", recognized));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_fontsettings() -> Vec<u8> {
    use alloc::format;
    use super::fontsettings;
    let (changes, ops) = fontsettings::stats();
    let cfg = fontsettings::get_config();
    let mut out = String::from("antialiasing: ");
    out.push_str(cfg.as_ref().map_or("N/A", |c| c.antialiasing.label()));
    out.push('\n');
    out.push_str(&format!("hinting: {}\n", cfg.as_ref().map_or("N/A", |c| c.hinting.label())));
    out.push_str(&format!("default_size_dp: {}\n", cfg.as_ref().map_or(0, |c| c.default_size_dp)));
    out.push_str(&format!("text_scale_percent: {}\n", cfg.as_ref().map_or(100, |c| c.text_scale_percent)));
    out.push_str(&format!("total_changes: {}\n", changes));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_notifbadge() -> Vec<u8> {
    use alloc::format;
    use super::notifbadge;
    let (count, visible, updates, ops) = notifbadge::stats();
    let mut out = String::from("badge_count: ");
    out.push_str(&format!("{}\n", count));
    out.push_str(&format!("visible_count: {}\n", visible));
    out.push_str(&format!("total_updates: {}\n", updates));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_lockwallpaper() -> Vec<u8> {
    use alloc::format;
    use super::lockwallpaper;
    let (rotations, changes, ops) = lockwallpaper::stats();
    let cfg = lockwallpaper::get_config();
    let mut out = String::from("mode: ");
    out.push_str(cfg.as_ref().map_or("N/A", |c| c.mode.label()));
    out.push('\n');
    out.push_str(&format!("current_image: {}\n", cfg.as_ref().map_or_else(String::new, |c| c.current_image.clone())));
    out.push_str(&format!("total_rotations: {}\n", rotations));
    out.push_str(&format!("total_changes: {}\n", changes));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_systemsounds() -> Vec<u8> {
    use alloc::format;
    use super::systemsounds;
    let (schemes, events, plays, ops) = systemsounds::stats();
    let mut out = String::from("active_scheme: ");
    out.push_str(&systemsounds::active_scheme());
    out.push('\n');
    out.push_str(&format!("scheme_count: {}\n", schemes));
    out.push_str(&format!("event_count: {}\n", events));
    out.push_str(&format!("total_plays: {}\n", plays));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_hotcorners() -> Vec<u8> {
    use alloc::format;
    use super::hotcorners;
    let (enabled, triggers, ops) = hotcorners::stats();
    let all = hotcorners::get_all();
    let mut out = String::from("enabled_corners: ");
    out.push_str(&format!("{}\n", enabled));
    for c in &all {
        out.push_str(&format!("{}: {} (delay={}ms, enabled={})\n", c.corner.label(), c.action.label(), c.delay_ms, c.enabled));
    }
    out.push_str(&format!("total_triggers: {}\n", triggers));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_dynlock() -> Vec<u8> {
    use alloc::format;
    use super::dynlock;
    let (devs, locks, unlocks, ops) = dynlock::stats();
    let mut out = String::from("state: ");
    out.push_str(dynlock::lock_state().label());
    out.push('\n');
    out.push_str(&format!("device_count: {}\n", devs));
    out.push_str(&format!("total_locks: {}\n", locks));
    out.push_str(&format!("total_unlocks: {}\n", unlocks));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_snaplayout() -> Vec<u8> {
    use alloc::format;
    use super::snaplayout;
    let (layouts, groups, snaps, ops) = snaplayout::stats();
    let active = snaplayout::get_active();
    let mut out = String::from("active_layout: ");
    out.push_str(&active.map_or_else(|| String::from("none"), |a| a.name));
    out.push('\n');
    out.push_str(&format!("layout_count: {}\n", layouts));
    out.push_str(&format!("group_count: {}\n", groups));
    out.push_str(&format!("total_snaps: {}\n", snaps));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_haptfeedback() -> Vec<u8> {
    use alloc::format;
    use super::haptfeedback;
    let (devs, maps, fires, ops) = haptfeedback::stats();
    let mut out = String::from("device_count: ");
    out.push_str(&format!("{}\n", devs));
    out.push_str(&format!("mapping_count: {}\n", maps));
    out.push_str(&format!("total_fires: {}\n", fires));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_eyeprotect() -> Vec<u8> {
    use alloc::format;
    use super::eyeprotect;
    let (profiles, breaks, snoozes, skips, ops) = eyeprotect::stats();
    let active = eyeprotect::get_active();
    let mut out = String::from("state: ");
    out.push_str(eyeprotect::break_state().label());
    out.push('\n');
    out.push_str(&format!("active_profile: {}\n", active.map_or_else(|| String::from("none"), |a| a.name)));
    out.push_str(&format!("profile_count: {}\n", profiles));
    out.push_str(&format!("total_breaks: {}\n", breaks));
    out.push_str(&format!("total_snoozes: {}\n", snoozes));
    out.push_str(&format!("total_skips: {}\n", skips));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_pinnedapps() -> Vec<u8> {
    use alloc::format;
    use super::pinnedapps;
    let (total, taskbar, start, launches, ops) = pinnedapps::stats();
    let mut out = String::from("total_pinned: ");
    out.push_str(&format!("{}\n", total));
    out.push_str(&format!("taskbar_pins: {}\n", taskbar));
    out.push_str(&format!("startmenu_pins: {}\n", start));
    out.push_str(&format!("total_launches: {}\n", launches));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_inputmethod() -> Vec<u8> {
    use alloc::format;
    use super::inputmethod;
    let (engines, commits, switches, ops) = inputmethod::stats();
    let mut out = String::from("active_engine: ");
    out.push_str(&inputmethod::active_engine_name());
    out.push('\n');
    out.push_str(&format!("engine_count: {}\n", engines));
    out.push_str(&format!("total_commits: {}\n", commits));
    out.push_str(&format!("total_switches: {}\n", switches));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_storagesense() -> Vec<u8> {
    use alloc::format;
    use super::storagesense;
    let (policies, runs, freed, ops) = storagesense::stats();
    let mut out = String::from("schedule: ");
    out.push_str(storagesense::get_schedule().label());
    out.push('\n');
    out.push_str(&format!("policy_count: {}\n", policies));
    out.push_str(&format!("total_runs: {}\n", runs));
    out.push_str(&format!("total_freed: {}\n", storagesense::format_bytes(freed)));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_autofix() -> Vec<u8> {
    use alloc::format;
    use super::autofix;
    let (issues, scans, fixes, ignored, ops) = autofix::stats();
    let mut out = String::from("issue_count: ");
    out.push_str(&format!("{}\n", issues));
    out.push_str(&format!("total_scans: {}\n", scans));
    out.push_str(&format!("total_fixes: {}\n", fixes));
    out.push_str(&format!("total_ignored: {}\n", ignored));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_recentsearch() -> Vec<u8> {
    use alloc::format;
    use super::recentsearch;
    let (entries, pinned, searches, suggestions, ops) = recentsearch::stats();
    let mut out = String::from("entry_count: ");
    out.push_str(&format!("{}\n", entries));
    out.push_str(&format!("pinned_count: {}\n", pinned));
    out.push_str(&format!("total_searches: {}\n", searches));
    out.push_str(&format!("suggestions_used: {}\n", suggestions));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_sysmaint() -> Vec<u8> {
    use alloc::format;
    use super::sysmaint;
    let (tasks, runs, failures, ops) = sysmaint::stats();
    let mut out = String::from("task_count: ");
    out.push_str(&format!("{}\n", tasks));
    out.push_str(&format!("total_runs: {}\n", runs));
    out.push_str(&format!("total_failures: {}\n", failures));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_multiclip() -> Vec<u8> {
    use alloc::format;
    use super::multiclip;
    let (entries, pinned, copies, pastes, ops) = multiclip::stats();
    let mut out = String::from("entry_count: ");
    out.push_str(&format!("{}\n", entries));
    out.push_str(&format!("pinned_count: {}\n", pinned));
    out.push_str(&format!("total_copies: {}\n", copies));
    out.push_str(&format!("total_pastes: {}\n", pastes));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_focussession() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Focus Session ===\n");
    let state = super::focussession::current_state();
    out.push_str(&format!("state: {}\n", state.label()));
    if let Some(cfg) = super::focussession::get_config() {
        out.push_str(&format!("focus_mins: {}\n", cfg.focus_mins));
        out.push_str(&format!("short_break_mins: {}\n", cfg.short_break_mins));
        out.push_str(&format!("long_break_mins: {}\n", cfg.long_break_mins));
        out.push_str(&format!("sessions_before_long: {}\n", cfg.sessions_before_long));
        out.push_str(&format!("block_notifications: {}\n", cfg.block_notifications));
    }
    let (sessions, abandoned, focus_mins, ops) = super::focussession::stats();
    out.push_str(&format!("total_sessions: {}\n", sessions));
    out.push_str(&format!("total_abandoned: {}\n", abandoned));
    out.push_str(&format!("total_focus_mins: {}\n", focus_mins));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_quicknote() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Quick Notes ===\n");
    let (notes, pinned, created, edits, ops) = super::quicknote::stats();
    out.push_str(&format!("notes: {}\n", notes));
    out.push_str(&format!("pinned: {}\n", pinned));
    out.push_str(&format!("total_created: {}\n", created));
    out.push_str(&format!("total_edits: {}\n", edits));
    let recent = super::quicknote::list(5);
    for n in &recent {
        let trunc: String = n.content.chars().take(40).collect();
        out.push_str(&format!("  [{}] {} — {}\n", n.id, n.title, trunc));
    }
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_colorscheme() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Color Scheme ===\n");
    if let Some(active) = super::colorscheme::get_active() {
        out.push_str(&format!("active: {} (id={})\n", active.name, active.id));
        out.push_str(&format!("mode: {:?}\n", active.mode));
        out.push_str(&format!("accent: {}\n", active.accent));
    }
    let schemes = super::colorscheme::list_schemes();
    out.push_str(&format!("scheme_count: {}\n", schemes.len()));
    for s in &schemes {
        out.push_str(&format!("  [{}] {} ({:?})\n", s.id, s.name, s.mode));
    }
    let (_count, changes, ops) = super::colorscheme::stats();
    out.push_str(&format!("total_changes: {}\n", changes));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_appcompat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== App Compatibility ===\n");
    let (profiles, launches, shim_acts, ops) = super::appcompat::stats();
    out.push_str(&format!("profiles: {}\n", profiles));
    out.push_str(&format!("total_launches: {}\n", launches));
    out.push_str(&format!("total_shim_activations: {}\n", shim_acts));
    let list = super::appcompat::list_profiles();
    for p in &list {
        out.push_str(&format!("  {} — {:?} shims={} enabled={}\n",
            p.app_name, p.compat_level, p.shims.len(), p.enabled));
    }
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_windowrules() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Window Rules ===\n");
    let (rules, enabled, matches, applied, ops) = super::windowrules::stats();
    out.push_str(&format!("rules: {}\n", rules));
    out.push_str(&format!("enabled: {}\n", enabled));
    out.push_str(&format!("total_matches: {}\n", matches));
    out.push_str(&format!("total_applied: {}\n", applied));
    let list = super::windowrules::list_rules();
    for r in &list {
        out.push_str(&format!("  [{}] {} — {} '{}' ({} actions, {})\n",
            r.id, r.name, r.match_type.label(), r.match_value,
            r.actions.len(), if r.enabled { "on" } else { "off" }));
    }
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_spatialaudio() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Spatial Audio ===\n");
    if let Some(cfg) = super::spatialaudio::get_config() {
        out.push_str(&format!("enabled: {}\n", cfg.global_enabled));
        out.push_str(&format!("layout: {} ({}ch)\n", cfg.layout.label(), cfg.layout.channel_count()));
        out.push_str(&format!("room: {}\n", cfg.room_size.label()));
        out.push_str(&format!("head_tracking: {}\n", cfg.head_tracking));
        out.push_str(&format!("reverb: {}%\n", cfg.reverb_level));
        out.push_str(&format!("distance_attenuation: {}\n", cfg.distance_attenuation));
        out.push_str(&format!("doppler: {}\n", cfg.doppler_effect));
    }
    let (apps, streams, changes, ops) = super::spatialaudio::stats();
    out.push_str(&format!("app_configs: {}\n", apps));
    out.push_str(&format!("streams_processed: {}\n", streams));
    out.push_str(&format!("config_changes: {}\n", changes));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_filetransfer() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== File Transfer ===\n");
    let vis = super::filetransfer::get_visibility();
    out.push_str(&format!("visibility: {}\n", vis.label()));
    let devices = super::filetransfer::list_devices();
    out.push_str(&format!("nearby_devices: {}\n", devices.len()));
    for d in &devices {
        out.push_str(&format!("  [{}] {} ({}, {})\n",
            d.id, d.name, d.device_type, d.transport.label()));
    }
    let transfers = super::filetransfer::list_transfers(10);
    if !transfers.is_empty() {
        out.push_str("recent_transfers:\n");
        for t in &transfers {
            let dir = if t.outgoing { "→" } else { "←" };
            out.push_str(&format!("  [{}] {} {} {} ({})\n",
                t.id, dir, t.device_name, t.file_name, t.status.label()));
        }
    }
    let (_devs, sent, recv, _bytes_s, _bytes_r, ops) = super::filetransfer::stats();
    out.push_str(&format!("total_sent: {}\n", sent));
    out.push_str(&format!("total_received: {}\n", recv));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_startupopt() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Startup Optimization ===\n");
    let (stages, boots, last_ms, fastest_ms, analyses, ops) = super::startupopt::stats();
    out.push_str(&format!("boot_count: {}\n", boots));
    out.push_str(&format!("last_boot_ms: {}\n", last_ms));
    out.push_str(&format!("fastest_boot_ms: {}\n", fastest_ms));
    out.push_str(&format!("stages: {}\n", stages));
    let sorted = super::startupopt::get_stages_by_duration();
    for s in sorted.iter().take(10) {
        out.push_str(&format!("  {} ({}) — {}ms\n",
            s.name, s.category.label(), s.duration_ms));
    }
    let suggestions = super::startupopt::get_suggestions();
    if !suggestions.is_empty() {
        out.push_str(&format!("suggestions: {}\n", suggestions.len()));
        for s in &suggestions {
            out.push_str(&format!("  [{}] [{}] {}\n", s.id, s.priority.label(), s.description));
        }
    }
    out.push_str(&format!("total_analyses: {}\n", analyses));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_usagetime() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Usage Time ===\n");
    let (apps, sessions, tracked_ms, limited, ops) = super::usagetime::stats();
    out.push_str(&format!("tracked_apps: {}\n", apps));
    out.push_str(&format!("total_sessions: {}\n", sessions));
    out.push_str(&format!("total_tracked_ms: {}\n", tracked_ms));
    out.push_str(&format!("apps_with_limits: {}\n", limited));
    let top = super::usagetime::top_apps(10);
    for a in &top {
        let hrs = a.total_foreground_ms / 3_600_000;
        let mins = (a.total_foreground_ms % 3_600_000) / 60_000;
        out.push_str(&format!("  {} — {}h {}m ({} sessions)\n",
            a.app_name, hrs, mins, a.session_count));
    }
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_voicecontrol() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Voice Control ===\n");
    let listening = super::voicecontrol::is_listening();
    let wake = super::voicecontrol::get_wake_word();
    out.push_str(&format!("listening: {}\n", listening));
    out.push_str(&format!("wake_word: {}\n", wake));
    let (cmds, recognitions, executed, rejected, ops) = super::voicecontrol::stats();
    out.push_str(&format!("commands: {}\n", cmds));
    out.push_str(&format!("total_recognitions: {}\n", recognitions));
    out.push_str(&format!("total_executed: {}\n", executed));
    out.push_str(&format!("total_rejected: {}\n", rejected));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_devpair() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Device Pairing ===\n");
    let scanning = super::devpair::is_scanning();
    out.push_str(&format!("scanning: {}\n", scanning));
    let (devices, paired, trusted, total_paired, total_failed, ops) = super::devpair::stats();
    out.push_str(&format!("devices: {}\n", devices));
    out.push_str(&format!("paired: {}\n", paired));
    out.push_str(&format!("trusted: {}\n", trusted));
    out.push_str(&format!("total_paired: {}\n", total_paired));
    out.push_str(&format!("total_failed: {}\n", total_failed));
    let list = super::devpair::list_devices();
    for d in &list {
        out.push_str(&format!("  [{}] {} ({}) — {} signal={}\n",
            d.id, d.name, d.device_type.label(), d.state.label(), d.signal_strength));
    }
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_notifgroup() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Notification Grouping ===\n");
    let mode = super::notifgroup::get_mode();
    out.push_str(&format!("mode: {}\n", mode.label()));
    let (groups, total, unread, dismissed, ops) = super::notifgroup::stats();
    out.push_str(&format!("groups: {}\n", groups));
    out.push_str(&format!("total_notifications: {}\n", total));
    out.push_str(&format!("unread: {}\n", unread));
    out.push_str(&format!("total_dismissed: {}\n", dismissed));
    let group_list = super::notifgroup::get_groups();
    for g in &group_list {
        out.push_str(&format!("  [{}] {} — {} notifs ({}{})\n",
            g.group_id, g.app_name, g.notifications.len(),
            if g.expanded { "expanded" } else { "collapsed" },
            if g.muted { ", muted" } else { "" }));
    }
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_playmedia() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Play Media ===\n");
    if let Some(np) = super::playmedia::get_now_playing() {
        out.push_str(&format!("now_playing: {} — {} ({})\n", np.artist, np.title, np.album));
        out.push_str(&format!("state: {}\n", np.state.label()));
        out.push_str(&format!("app: {} ({})\n", np.app_name, np.media_type.label()));
        out.push_str(&format!("shuffle: {}, repeat: {}\n", np.shuffle, np.repeat.label()));
    } else {
        out.push_str("now_playing: none\n");
    }
    let (sessions, plays, tracks, ops) = super::playmedia::stats();
    out.push_str(&format!("sessions: {}\n", sessions));
    out.push_str(&format!("play_commands: {}\n", plays));
    out.push_str(&format!("track_changes: {}\n", tracks));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_kbmacro() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Keyboard Macros ===\n");
    let recording = super::kbmacro::is_recording();
    out.push_str(&format!("recording: {}\n", recording));
    let (macros, plays, recorded, ops) = super::kbmacro::stats();
    out.push_str(&format!("macros: {}\n", macros));
    out.push_str(&format!("total_plays: {}\n", plays));
    out.push_str(&format!("total_recorded: {}\n", recorded));
    let list = super::kbmacro::list_macros();
    for m in &list {
        let hk = m.hotkey.as_deref().unwrap_or("none");
        out.push_str(&format!("  [{}] {} — {} events, hotkey={}, plays={}\n",
            m.id, m.name, m.events.len(), hk, m.play_count));
    }
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_sysresource() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== System Resources ===\n");
    if let Some(snap) = super::sysresource::get_current() {
        out.push_str(&format!("cpu: {}%\n", snap.cpu_percent));
        out.push_str(&format!("memory: {}/{} KB\n", snap.memory_used_kb, snap.memory_total_kb));
        out.push_str(&format!("swap: {}/{} KB\n", snap.swap_used_kb, snap.swap_total_kb));
        out.push_str(&format!("disk_io: {}KB/s read, {}KB/s write\n", snap.disk_read_kb_s, snap.disk_write_kb_s));
        out.push_str(&format!("net_io: {}KB/s rx, {}KB/s tx\n", snap.net_rx_kb_s, snap.net_tx_kb_s));
        out.push_str(&format!("gpu: {}%\n", snap.gpu_percent));
        out.push_str(&format!("processes: {}, threads: {}\n", snap.process_count, snap.thread_count));
    }
    let (samples, hist_size, _alerts, total_alerts, ops) = super::sysresource::stats();
    out.push_str(&format!("total_samples: {}\n", samples));
    out.push_str(&format!("history_size: {}\n", hist_size));
    out.push_str(&format!("total_alerts: {}\n", total_alerts));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_faceunlock() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Face Unlock ===\n");
    let enabled = super::faceunlock::is_enabled();
    let security = super::faceunlock::get_security();
    out.push_str(&format!("enabled: {}\n", enabled));
    out.push_str(&format!("security: {}\n", security.label()));
    let enrollments = super::faceunlock::list_enrollments();
    out.push_str(&format!("enrollments: {}\n", enrollments.len()));
    for e in &enrollments {
        out.push_str(&format!("  user {} ({}) — verified={}, failed={}\n",
            e.user_id, e.user_name, e.verify_count, e.fail_count));
    }
    let (_enr, verifications, matches, rejections, ops) = super::faceunlock::stats();
    out.push_str(&format!("total_verifications: {}\n", verifications));
    out.push_str(&format!("total_matches: {}\n", matches));
    out.push_str(&format!("total_rejections: {}\n", rejections));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_usbpolicy() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (rule_count, log_size, total_allowed, total_denied, ops) = super::usbpolicy::stats();
    out.push_str("subsystem: usbpolicy\n");
    out.push_str(&format!("rule_count: {}\n", rule_count));
    out.push_str(&format!("log_size: {}\n", log_size));
    out.push_str(&format!("total_allowed: {}\n", total_allowed));
    out.push_str(&format!("total_denied: {}\n", total_denied));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_applaunch() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (item_count, total_searches, total_launches, ops) = super::applaunch::stats();
    out.push_str("subsystem: applaunch\n");
    out.push_str(&format!("item_count: {}\n", item_count));
    out.push_str(&format!("total_searches: {}\n", total_searches));
    out.push_str(&format!("total_launches: {}\n", total_launches));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_sysprofiler() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (section_count, total_queries, total_refreshes, ops) = super::sysprofiler::stats();
    out.push_str("subsystem: sysprofiler\n");
    out.push_str(&format!("section_count: {}\n", section_count));
    out.push_str(&format!("total_queries: {}\n", total_queries));
    out.push_str(&format!("total_refreshes: {}\n", total_refreshes));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_clipsync() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (device_count, total_sent, total_received, total_bytes, ops) = super::clipsync::stats();
    out.push_str("subsystem: clipsync\n");
    out.push_str(&format!("device_count: {}\n", device_count));
    out.push_str(&format!("total_sent: {}\n", total_sent));
    out.push_str(&format!("total_received: {}\n", total_received));
    out.push_str(&format!("total_bytes_synced: {}\n", total_bytes));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_netusage() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (app_count, iface_count, total_sent, total_received, total_connections, cap_warnings, ops) = super::netusage::stats();
    out.push_str("subsystem: netusage\n");
    out.push_str(&format!("app_count: {}\n", app_count));
    out.push_str(&format!("interface_count: {}\n", iface_count));
    out.push_str(&format!("total_bytes_sent: {}\n", total_sent));
    out.push_str(&format!("total_bytes_received: {}\n", total_received));
    out.push_str(&format!("total_connections: {}\n", total_connections));
    out.push_str(&format!("cap_warnings: {}\n", cap_warnings));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_touchscreen() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (device_count, gesture_count, total_touches, total_gestures, calibrations, ops) = super::touchscreen::stats();
    out.push_str("subsystem: touchscreen\n");
    out.push_str(&format!("device_count: {}\n", device_count));
    out.push_str(&format!("gesture_count: {}\n", gesture_count));
    out.push_str(&format!("total_touches: {}\n", total_touches));
    out.push_str(&format!("total_gestures: {}\n", total_gestures));
    out.push_str(&format!("calibrations: {}\n", calibrations));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_diskquota() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (entry_count, total_checks, total_denials, total_warnings, ops) = super::diskquota::stats();
    out.push_str("subsystem: diskquota\n");
    out.push_str(&format!("entry_count: {}\n", entry_count));
    out.push_str(&format!("total_checks: {}\n", total_checks));
    out.push_str(&format!("total_denials: {}\n", total_denials));
    out.push_str(&format!("total_warnings: {}\n", total_warnings));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_appdefaults() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (app_count, total_reads, total_writes, total_resets, ops) = super::appdefaults::stats();
    out.push_str("subsystem: appdefaults\n");
    out.push_str(&format!("app_count: {}\n", app_count));
    out.push_str(&format!("total_reads: {}\n", total_reads));
    out.push_str(&format!("total_writes: {}\n", total_writes));
    out.push_str(&format!("total_resets: {}\n", total_resets));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_policyengine() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (rule_count, audit_size, total_evals, total_denials, total_audits, ops) = super::policyengine::stats();
    out.push_str("subsystem: policyengine\n");
    out.push_str(&format!("rule_count: {}\n", rule_count));
    out.push_str(&format!("audit_log_size: {}\n", audit_size));
    out.push_str(&format!("total_evaluations: {}\n", total_evals));
    out.push_str(&format!("total_denials: {}\n", total_denials));
    out.push_str(&format!("total_audits: {}\n", total_audits));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_fontpreview() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (font_count, total_previews, total_comparisons, ops) = super::fontpreview::stats();
    out.push_str("subsystem: fontpreview\n");
    out.push_str(&format!("font_count: {}\n", font_count));
    out.push_str(&format!("total_previews: {}\n", total_previews));
    out.push_str(&format!("total_comparisons: {}\n", total_comparisons));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_wifiscan() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (network_count, saved_count, total_scans, total_connections, total_failures, ops) = super::wifiscan::stats();
    out.push_str("subsystem: wifiscan\n");
    out.push_str(&format!("network_count: {}\n", network_count));
    out.push_str(&format!("saved_count: {}\n", saved_count));
    out.push_str(&format!("total_scans: {}\n", total_scans));
    out.push_str(&format!("total_connections: {}\n", total_connections));
    out.push_str(&format!("total_failures: {}\n", total_failures));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_splitview() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (split_count, total_created, total_panes, total_resizes, ops) = super::splitview::stats();
    out.push_str("subsystem: splitview\n");
    out.push_str(&format!("split_count: {}\n", split_count));
    out.push_str(&format!("total_created: {}\n", total_created));
    out.push_str(&format!("total_panes_added: {}\n", total_panes));
    out.push_str(&format!("total_resizes: {}\n", total_resizes));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_iotdevice() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (device_count, group_count, online_count, total_commands, total_discoveries, ops) = super::iotdevice::stats();
    out.push_str("subsystem: iotdevice\n");
    out.push_str(&format!("device_count: {}\n", device_count));
    out.push_str(&format!("group_count: {}\n", group_count));
    out.push_str(&format!("online_count: {}\n", online_count));
    out.push_str(&format!("total_commands: {}\n", total_commands));
    out.push_str(&format!("total_discoveries: {}\n", total_discoveries));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_prochistory() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (history_size, total_started, total_exited, total_crashed, ops) = super::prochistory::stats();
    out.push_str("subsystem: prochistory\n");
    out.push_str(&format!("history_size: {}\n", history_size));
    out.push_str(&format!("total_started: {}\n", total_started));
    out.push_str(&format!("total_exited: {}\n", total_exited));
    out.push_str(&format!("total_crashed: {}\n", total_crashed));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_notiffilter() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (rule_count, total_evaluated, total_allowed, total_blocked, total_silenced, ops) = super::notiffilter::stats();
    out.push_str("subsystem: notiffilter\n");
    out.push_str(&format!("rule_count: {}\n", rule_count));
    out.push_str(&format!("total_evaluated: {}\n", total_evaluated));
    out.push_str(&format!("total_allowed: {}\n", total_allowed));
    out.push_str(&format!("total_blocked: {}\n", total_blocked));
    out.push_str(&format!("total_silenced: {}\n", total_silenced));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_colorblind() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (preset_count, total_activations, total_changes, ops) = super::colorblind::stats();
    let (enabled, cvd_type, intensity, simulate) = super::colorblind::current();
    out.push_str("subsystem: colorblind\n");
    out.push_str(&format!("enabled: {}\n", enabled));
    out.push_str(&format!("active_type: {}\n", cvd_type.short_label()));
    out.push_str(&format!("intensity: {}\n", intensity));
    out.push_str(&format!("simulate_mode: {}\n", simulate));
    out.push_str(&format!("preset_count: {}\n", preset_count));
    out.push_str(&format!("total_activations: {}\n", total_activations));
    out.push_str(&format!("total_changes: {}\n", total_changes));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_clipaction() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Clipboard Actions ===\n");
    let (action_count, total_detections, total_executions, ops) = crate::fs::clipaction::stats();
    out.push_str(&format!("action_count: {}\n", action_count));
    out.push_str(&format!("total_detections: {}\n", total_detections));
    out.push_str(&format!("total_executions: {}\n", total_executions));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_energysaver() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Energy Saver ===\n");
    let (throttled_count, mode_changes, total_throttles, estimated_min, ops) = crate::fs::energysaver::stats();
    out.push_str(&format!("throttled_count: {}\n", throttled_count));
    out.push_str(&format!("mode_changes: {}\n", mode_changes));
    out.push_str(&format!("total_throttles: {}\n", total_throttles));
    out.push_str(&format!("estimated_minutes: {}\n", estimated_min));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_filerules() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== File Rules ===\n");
    let (rule_count, total_evaluations, total_matches, total_applied, ops) = crate::fs::filerules::stats();
    out.push_str(&format!("rule_count: {}\n", rule_count));
    out.push_str(&format!("total_evaluations: {}\n", total_evaluations));
    out.push_str(&format!("total_matches: {}\n", total_matches));
    out.push_str(&format!("total_applied: {}\n", total_applied));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_secureboot() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Secure Boot ===\n");
    let (key_count, record_count, total_verified, total_rejected, ops) = crate::fs::secureboot::stats();
    out.push_str(&format!("key_count: {}\n", key_count));
    out.push_str(&format!("record_count: {}\n", record_count));
    out.push_str(&format!("total_verified: {}\n", total_verified));
    out.push_str(&format!("total_rejected: {}\n", total_rejected));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_eventlog() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Event Log ===\n");
    let (event_count, total_logged, total_cleared, total_queries, ops) = crate::fs::eventlog::stats();
    out.push_str(&format!("event_count: {}\n", event_count));
    out.push_str(&format!("total_logged: {}\n", total_logged));
    out.push_str(&format!("total_cleared: {}\n", total_cleared));
    out.push_str(&format!("total_queries: {}\n", total_queries));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_sysevents() -> Vec<u8> {
    crate::eventlog::procfs_content().into_bytes()
}

fn gen_logpersist() -> Vec<u8> {
    crate::logpersist::procfs_content().into_bytes()
}

fn gen_svcstart() -> Vec<u8> {
    crate::svcstart::procfs_content().into_bytes()
}

fn gen_sockactivation() -> Vec<u8> {
    crate::sockact::procfs_content().into_bytes()
}

fn gen_drvmon() -> Vec<u8> {
    crate::drvmon::procfs_content().into_bytes()
}

fn gen_reslimit() -> Vec<u8> {
    crate::reslimit::procfs_content().into_bytes()
}

fn gen_initproc() -> Vec<u8> {
    crate::initproc::procfs_content().into_bytes()
}

fn gen_syshealth() -> Vec<u8> {
    crate::syshealth::procfs_content().into_bytes()
}

fn gen_udriver() -> Vec<u8> {
    crate::udriver::procfs_content().into_bytes()
}

fn gen_hotplug() -> Vec<u8> {
    crate::devhotplug::procfs_content().into_bytes()
}

fn gen_devpower() -> Vec<u8> {
    crate::devpower::procfs_content().into_bytes()
}

fn gen_vmguest() -> Vec<u8> {
    crate::vmguest::procfs_content().into_bytes()
}

fn gen_pciids() -> Vec<u8> {
    crate::pciids::procfs_content().into_bytes()
}

fn gen_upnp() -> Vec<u8> {
    crate::net::upnp::procfs_content().into_bytes()
}

fn gen_http() -> Vec<u8> {
    crate::net::http::procfs_content().into_bytes()
}

fn gen_ntp() -> Vec<u8> {
    crate::net::ntp::procfs_content().into_bytes()
}

fn gen_mdns() -> Vec<u8> {
    crate::net::mdns::procfs_content().into_bytes()
}

fn gen_telnet() -> Vec<u8> {
    crate::net::telnet::procfs_content().into_bytes()
}

fn gen_tftp() -> Vec<u8> {
    crate::net::tftp::procfs_content().into_bytes()
}

fn gen_netsyslog() -> Vec<u8> {
    crate::net::syslog::procfs_content().into_bytes()
}

fn gen_wol() -> Vec<u8> {
    crate::net::wol::procfs_content().into_bytes()
}

fn gen_pcap() -> Vec<u8> {
    crate::net::pcap::procfs_content().into_bytes()
}

fn gen_traceroute() -> Vec<u8> {
    crate::net::traceroute::procfs_content().into_bytes()
}

fn gen_dhcpv6() -> Vec<u8> {
    crate::net::dhcpv6::procfs_content().into_bytes()
}

fn gen_firewall() -> Vec<u8> {
    crate::net::firewall::procfs_content().into_bytes()
}

fn gen_igmp() -> Vec<u8> {
    crate::net::igmp::procfs_content().into_bytes()
}

fn gen_mld() -> Vec<u8> {
    crate::net::mld::procfs_content().into_bytes()
}

fn gen_lldp() -> Vec<u8> {
    crate::net::lldp::procfs_content().into_bytes()
}

fn gen_netstat() -> Vec<u8> {
    crate::net::netstat::procfs_content().into_bytes()
}

fn gen_ndisc() -> Vec<u8> {
    crate::net::ndisc::procfs_content().into_bytes()
}

fn gen_netcat() -> Vec<u8> {
    crate::net::netcat::procfs_content().into_bytes()
}

fn gen_iperf() -> Vec<u8> {
    crate::net::iperf::procfs_content().into_bytes()
}

fn gen_snmp() -> Vec<u8> {
    crate::net::snmp::procfs_content().into_bytes()
}

fn gen_ftp() -> Vec<u8> {
    crate::net::ftp::procfs_content().into_bytes()
}

fn gen_smtp() -> Vec<u8> {
    crate::net::smtp::procfs_content().into_bytes()
}

fn gen_vlan() -> Vec<u8> {
    crate::net::vlan::procfs_content().into_bytes()
}

fn gen_qos() -> Vec<u8> {
    crate::net::qos::procfs_content().into_bytes()
}

fn gen_socks() -> Vec<u8> {
    crate::net::socks::procfs_content().into_bytes()
}

fn gen_bridge() -> Vec<u8> {
    crate::net::bridge::procfs_content().into_bytes()
}

fn gen_systemimage() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== System Image ===\n");
    let (image_count, total_created, total_restored, total_verified, total_bytes, ops) = crate::fs::systemimage::stats();
    out.push_str(&format!("image_count: {}\n", image_count));
    out.push_str(&format!("total_created: {}\n", total_created));
    out.push_str(&format!("total_restored: {}\n", total_restored));
    out.push_str(&format!("total_verified: {}\n", total_verified));
    out.push_str(&format!("total_bytes: {}\n", total_bytes));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_raidmgr() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== RAID Manager ===\n");
    let (array_count, total_created, total_rebuilds, total_failures, ops) = crate::fs::raidmgr::stats();
    out.push_str(&format!("array_count: {}\n", array_count));
    out.push_str(&format!("total_created: {}\n", total_created));
    out.push_str(&format!("total_rebuilds: {}\n", total_rebuilds));
    out.push_str(&format!("total_failures: {}\n", total_failures));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_networkbridge() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Network Bridge ===\n");
    let (bridge_count, total_created, total_ifaces, total_forwarded, ops) = crate::fs::networkbridge::stats();
    out.push_str(&format!("bridge_count: {}\n", bridge_count));
    out.push_str(&format!("total_created: {}\n", total_created));
    out.push_str(&format!("total_ifaces_added: {}\n", total_ifaces));
    out.push_str(&format!("total_packets_forwarded: {}\n", total_forwarded));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_secureerase() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Secure Erase ===\n");
    let (job_count, total_started, total_completed, total_bytes, ops) = crate::fs::secureerase::stats();
    out.push_str(&format!("job_count: {}\n", job_count));
    out.push_str(&format!("total_started: {}\n", total_started));
    out.push_str(&format!("total_completed: {}\n", total_completed));
    out.push_str(&format!("total_bytes_erased: {}\n", total_bytes));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_dnssettings() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== DNS Settings ===\n");
    let (server_count, cache_size, total_queries, cache_hits, failures, ops) = crate::fs::dnssettings::stats();
    out.push_str(&format!("server_count: {}\n", server_count));
    out.push_str(&format!("cache_size: {}\n", cache_size));
    out.push_str(&format!("total_queries: {}\n", total_queries));
    out.push_str(&format!("cache_hits: {}\n", cache_hits));
    out.push_str(&format!("total_failures: {}\n", failures));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_backupsched() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Backup Scheduler ===\n");
    let (sched_count, hist_size, total_runs, successful, failed, bytes, ops) = crate::fs::backupsched::stats();
    out.push_str(&format!("schedule_count: {}\n", sched_count));
    out.push_str(&format!("history_size: {}\n", hist_size));
    out.push_str(&format!("total_runs: {}\n", total_runs));
    out.push_str(&format!("total_successful: {}\n", successful));
    out.push_str(&format!("total_failed: {}\n", failed));
    out.push_str(&format!("total_bytes_backed: {}\n", bytes));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_displaycal() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Display Calibration ===\n");
    let (monitor_count, calibrations, profile_changes, ops) = crate::fs::displaycal::stats();
    out.push_str(&format!("monitor_count: {}\n", monitor_count));
    out.push_str(&format!("total_calibrations: {}\n", calibrations));
    out.push_str(&format!("total_profile_changes: {}\n", profile_changes));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_vpnprofile() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== VPN Profiles ===\n");
    let (profile_count, total_connects, total_disconnects, total_errors, ops) = crate::fs::vpnprofile::stats();
    out.push_str(&format!("profile_count: {}\n", profile_count));
    out.push_str(&format!("total_connects: {}\n", total_connects));
    out.push_str(&format!("total_disconnects: {}\n", total_disconnects));
    out.push_str(&format!("total_errors: {}\n", total_errors));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_diskhealth() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Disk Health ===\n");
    let (disk_count, total_checks, total_warnings, total_failures, ops) = crate::fs::diskhealth::stats();
    out.push_str(&format!("disk_count: {}\n", disk_count));
    out.push_str(&format!("total_checks: {}\n", total_checks));
    out.push_str(&format!("total_warnings: {}\n", total_warnings));
    out.push_str(&format!("total_failures_predicted: {}\n", total_failures));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_recoverypart() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Recovery Partition ===\n");
    let (tool_count, total_repairs, total_verifications, total_boots, ops) = crate::fs::recoverypart::stats();
    out.push_str(&format!("tool_count: {}\n", tool_count));
    out.push_str(&format!("total_repairs: {}\n", total_repairs));
    out.push_str(&format!("total_verifications: {}\n", total_verifications));
    out.push_str(&format!("total_boots: {}\n", total_boots));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_userprofile() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== User Profiles ===\n");
    let (profile_count, total_logins, total_switches, ops) = crate::fs::userprofile::stats();
    out.push_str(&format!("profile_count: {}\n", profile_count));
    out.push_str(&format!("total_logins: {}\n", total_logins));
    out.push_str(&format!("total_switches: {}\n", total_switches));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_diskclean() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Disk Cleanup ===\n");
    let (item_count, total_scans, cleaned_bytes, cleaned_items, ops) = crate::fs::diskclean::stats();
    out.push_str(&format!("item_count: {}\n", item_count));
    out.push_str(&format!("total_scans: {}\n", total_scans));
    out.push_str(&format!("cleaned_bytes: {}\n", cleaned_bytes));
    out.push_str(&format!("cleaned_items: {}\n", cleaned_items));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_acl() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Access Control Lists ===\n");
    let s = crate::fs::acl::stats();
    out.push_str(&format!("files_with_acls: {}\n", s.files_with_acls));
    out.push_str(&format!("total_entries: {}\n", s.total_entries));
    out.push_str(&format!("checks_performed: {}\n", s.checks_performed));
    out.push_str(&format!("denials: {}\n", s.denials));
    out.into_bytes()
}

fn gen_associations() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== File Associations ===\n");
    let s = crate::fs::associations::stats();
    out.push_str(&format!("mime_types: {}\n", s.mime_types));
    out.push_str(&format!("total_entries: {}\n", s.total_entries));
    out.push_str(&format!("user_entries: {}\n", s.user_entries));
    out.into_bytes()
}

fn gen_logrotate() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Log Rotation ===\n");
    let (rule_count, total_rotations, bytes_rotated, total_cleanups, ops) = crate::fs::logrotate::stats();
    out.push_str(&format!("rule_count: {}\n", rule_count));
    out.push_str(&format!("total_rotations: {}\n", total_rotations));
    out.push_str(&format!("bytes_rotated: {}\n", bytes_rotated));
    out.push_str(&format!("total_cleanups: {}\n", total_cleanups));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_powerwake() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Power Wake ===\n");
    let (timer_count, wol_count, total_wakes, total_wol_sent, ops) = crate::fs::powerwake::stats();
    out.push_str(&format!("timer_count: {}\n", timer_count));
    out.push_str(&format!("wol_target_count: {}\n", wol_count));
    out.push_str(&format!("total_wakes: {}\n", total_wakes));
    out.push_str(&format!("total_wol_sent: {}\n", total_wol_sent));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_diskio() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Disk I/O ===\n");
    let (dev_count, reads, writes, bytes_read, bytes_written, ops) = crate::fs::diskio::stats();
    out.push_str(&format!("device_count: {}\n", dev_count));
    out.push_str(&format!("global_reads: {}\n", reads));
    out.push_str(&format!("global_writes: {}\n", writes));
    out.push_str(&format!("global_bytes_read: {}\n", bytes_read));
    out.push_str(&format!("global_bytes_written: {}\n", bytes_written));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_sysuptime() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== System Uptime ===\n");
    let uptime = crate::fs::sysuptime::current_uptime_ns();
    let formatted = crate::fs::sysuptime::format_duration(uptime);
    let (hist_count, total_sessions, longest, total_uptime, ops) = crate::fs::sysuptime::stats();
    out.push_str(&format!("current_uptime: {}\n", formatted));
    out.push_str(&format!("current_uptime_ns: {}\n", uptime));
    out.push_str(&format!("session_history: {}\n", hist_count));
    out.push_str(&format!("total_sessions: {}\n", total_sessions));
    out.push_str(&format!("longest_uptime_ns: {}\n", longest));
    out.push_str(&format!("total_uptime_ns: {}\n", total_uptime));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_netspeed() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Network Speed ===\n");
    let (results, ifaces, total_tests, ops) = crate::fs::netspeed::stats();
    out.push_str(&format!("test_results: {}\n", results));
    out.push_str(&format!("interfaces: {}\n", ifaces));
    out.push_str(&format!("total_tests: {}\n", total_tests));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_cpufreq() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== CPU Frequency ===\n");
    let (cpu_count, transitions, gov_changes, boost, ops) = crate::fs::cpufreq::stats();
    out.push_str(&format!("cpu_count: {}\n", cpu_count));
    out.push_str(&format!("total_transitions: {}\n", transitions));
    out.push_str(&format!("governor_changes: {}\n", gov_changes));
    out.push_str(&format!("boost_enabled: {}\n", boost));
    out.push_str(&format!("ops: {}\n", ops));
    if let Some(gov) = crate::fs::cpufreq::get_governor() {
        out.push_str(&format!("governor: {}\n", gov.label()));
    }
    out.into_bytes()
}

fn gen_thermal() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Thermal ===\n");
    let (zones, fans, readings, throttles, ops) = crate::fs::thermal::stats();
    out.push_str(&format!("zone_count: {}\n", zones));
    out.push_str(&format!("fan_count: {}\n", fans));
    out.push_str(&format!("total_readings: {}\n", readings));
    out.push_str(&format!("throttle_events: {}\n", throttles));
    out.push_str(&format!("ops: {}\n", ops));
    for zone in crate::fs::thermal::list_zones() {
        out.push_str(&format!("{}: {}\n", zone.name, crate::fs::thermal::format_temp(zone.temp_mc)));
    }
    out.into_bytes()
}

fn gen_swapmon() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Swap Monitor ===\n");
    // swapmon is a read-through over the real swap subsystem (crate::mm::swap)
    // and the swap-in page-fault counter (crate::mm::fault); it no longer keeps
    // a fabricated parallel device table.  processes_swapped / total_swap_out /
    // swap_out_bytes / ops are honest zeros — the real subsystem does not yet
    // track those (see swapmon::stats).
    let (dev_count, proc_count, swap_in, swap_out, in_bytes, out_bytes, ops) = crate::fs::swapmon::stats();
    let (total, used) = crate::fs::swapmon::total_usage();
    out.push_str(&format!("device_count: {}\n", dev_count));
    out.push_str(&format!("total_bytes: {}\n", total));
    out.push_str(&format!("used_bytes: {}\n", used));
    out.push_str(&format!("processes_swapped: {}\n", proc_count));
    out.push_str(&format!("total_swap_in: {}\n", swap_in));
    out.push_str(&format!("total_swap_out: {}\n", swap_out));
    out.push_str(&format!("swap_in_bytes: {}\n", in_bytes));
    out.push_str(&format!("swap_out_bytes: {}\n", out_bytes));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_sysctlfs() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Sysctl Parameters ===\n");
    let (count, reads, writes, modified, ops) = crate::fs::sysctlfs::stats();
    out.push_str(&format!("param_count: {}\n", count));
    out.push_str(&format!("total_reads: {}\n", reads));
    out.push_str(&format!("total_writes: {}\n", writes));
    out.push_str(&format!("modified: {}\n", modified));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_cputopo() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== CPU Topology ===\n");
    let (cpus, pkgs, numa, smt, queries, ops) = crate::fs::cputopo::stats();
    out.push_str(&format!("logical_cpus: {}\n", cpus));
    out.push_str(&format!("packages: {}\n", pkgs));
    out.push_str(&format!("numa_nodes: {}\n", numa));
    out.push_str(&format!("smt_enabled: {}\n", smt));
    out.push_str(&format!("queries: {}\n", queries));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_memlayout() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Memory Layout ===\n");
    let (regions, ram, reserved, kernel, queries, ops) = crate::fs::memlayout::stats();
    out.push_str(&format!("region_count: {}\n", regions));
    out.push_str(&format!("total_ram: {} ({})\n", ram, crate::fs::memlayout::format_size(ram)));
    out.push_str(&format!("total_reserved: {}\n", reserved));
    out.push_str(&format!("total_kernel: {}\n", kernel));
    out.push_str(&format!("queries: {}\n", queries));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_irqbalance() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== IRQ Balance ===\n");
    // Read the REAL interrupt balancer (crate::irqbalance), which tracks live
    // per-IRQ affinity from IRQ_STATES and the actual CPU count from SMP. The
    // former fs::irqbalance backing seeded a fabricated IRQ table (timer/eth0/
    // usb0 with invented CPU assignments and a hardcoded 4-CPU layout) that its
    // init_defaults() was never even wired to call — surfacing it here would
    // have been invented procfs data. This now reflects the running system.
    let st = crate::irqbalance::stats();
    let irqs = crate::irqbalance::irq_info();
    out.push_str(&format!("enabled: {}\n", st.enabled));
    out.push_str(&format!("cpu_count: {}\n", st.cpu_count));
    out.push_str(&format!("irq_count: {}\n", irqs.len()));
    out.push_str(&format!("balance_ops: {}\n", st.balance_ops));
    out.push_str(&format!("migrations: {}\n", st.migrations));
    for info in &irqs {
        out.push_str(&format!(
            "irq {}: cpu={} pinned={} hint={} rate={}\n",
            info.irq, info.cpu, info.pinned, info.hint, info.rate
        ));
    }
    out.into_bytes()
}

fn gen_fs_loadavg() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (a1, a5, a15, running, total) = crate::fs::loadavg::get();
    out.push_str(&format!("{} {} {} {}/{}\n",
        crate::fs::loadavg::format_load(a1),
        crate::fs::loadavg::format_load(a5),
        crate::fs::loadavg::format_load(a15),
        running, total));
    out.into_bytes()
}

fn gen_kernlog() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Kernel Log ===\n");
    let (count, total, dropped, ops) = crate::fs::kernlog::stats();
    out.push_str(&format!("messages: {}\n", count));
    out.push_str(&format!("total_logged: {}\n", total));
    out.push_str(&format!("total_dropped: {}\n", dropped));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_coredump() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Core Dumps ===\n");
    let (count, total, bytes, cleaned, ops) = crate::fs::coredump::stats();
    out.push_str(&format!("stored_dumps: {}\n", count));
    out.push_str(&format!("total_dumps: {}\n", total));
    out.push_str(&format!("total_bytes: {}\n", bytes));
    out.push_str(&format!("total_cleaned: {}\n", cleaned));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_fwupdate() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Firmware Update ===\n");
    let (devs, updates, failures, checks, ops) = crate::fs::fwupdate::stats();
    out.push_str(&format!("device_count: {}\n", devs));
    out.push_str(&format!("total_updates: {}\n", updates));
    out.push_str(&format!("total_failures: {}\n", failures));
    out.push_str(&format!("total_checks: {}\n", checks));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_timesync() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Time Sync ===\n");
    let (servers, syncs, errors, last_sync, ops) = crate::fs::timesync::stats();
    let status = crate::fs::timesync::get_status();
    out.push_str(&format!("status: {}\n", status.label()));
    out.push_str(&format!("servers: {}\n", servers));
    out.push_str(&format!("total_syncs: {}\n", syncs));
    out.push_str(&format!("total_errors: {}\n", errors));
    out.push_str(&format!("last_sync_ns: {}\n", last_sync));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_kmod() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Kernel Modules ===\n");
    let (live, loads, unloads, errors, ops) = crate::fs::kmod::stats();
    out.push_str(&format!("live_modules: {}\n", live));
    out.push_str(&format!("total_loads: {}\n", loads));
    out.push_str(&format!("total_unloads: {}\n", unloads));
    out.push_str(&format!("total_errors: {}\n", errors));
    out.push_str(&format!("ops: {}\n", ops));
    for m in crate::fs::kmod::list_modules() {
        out.push_str(&format!("  {} {} [{}] {} {}B refs={}\n",
            m.name, m.version, m.state.label(), m.mod_type.label(),
            m.size_bytes, m.ref_count));
    }
    out.into_bytes()
}

fn gen_entropy() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Entropy Pool ===\n");
    let (avail, added, drained, events, reseeds, ops) = crate::fs::entropy::stats();
    let quality = crate::fs::entropy::quality();
    out.push_str(&format!("available_bits: {}\n", avail));
    out.push_str(&format!("quality: {}\n", quality.label()));
    out.push_str(&format!("total_added: {}\n", added));
    out.push_str(&format!("total_drained: {}\n", drained));
    out.push_str(&format!("total_events: {}\n", events));
    out.push_str(&format!("reseed_count: {}\n", reseeds));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_iosched() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== I/O Scheduler ===\n");
    let (devs, dispatched, merged, requeued, ops) = crate::fs::iosched::stats();
    let default = crate::fs::iosched::get_default();
    out.push_str(&format!("default_algorithm: {}\n", default.label()));
    out.push_str(&format!("devices: {}\n", devs));
    out.push_str(&format!("total_dispatched: {}\n", dispatched));
    out.push_str(&format!("total_merged: {}\n", merged));
    out.push_str(&format!("total_requeued: {}\n", requeued));
    out.push_str(&format!("ops: {}\n", ops));
    for d in crate::fs::iosched::list_devices() {
        out.push_str(&format!("  {} algo={} depth={} merge={}\n",
            d.device_name, d.algorithm.label(), d.queue_depth, d.merge_enabled));
    }
    out.into_bytes()
}

fn gen_netmon() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Network Monitor ===\n");
    let (active, created, closed, sent, recv, ops) = crate::fs::netmon::stats();
    out.push_str(&format!("active_connections: {}\n", active));
    out.push_str(&format!("total_created: {}\n", created));
    out.push_str(&format!("total_closed: {}\n", closed));
    out.push_str(&format!("total_bytes_sent: {}\n", sent));
    out.push_str(&format!("total_bytes_recv: {}\n", recv));
    out.push_str(&format!("ops: {}\n", ops));
    for c in crate::fs::netmon::list_connections() {
        out.push_str(&format!("  {} {}:{} → {}:{} [{}] pid={} ({})\n",
            c.protocol.label(), c.local_addr, c.local_port,
            c.remote_addr, c.remote_port, c.state.label(),
            c.pid, c.process_name));
    }
    out.into_bytes()
}

fn gen_groupmgr() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Group Manager ===\n");
    let (count, created, deleted, member_ops, ops) = crate::fs::groupmgr::stats();
    out.push_str(&format!("groups: {}\n", count));
    out.push_str(&format!("total_created: {}\n", created));
    out.push_str(&format!("total_deleted: {}\n", deleted));
    out.push_str(&format!("total_member_ops: {}\n", member_ops));
    out.push_str(&format!("ops: {}\n", ops));
    for g in crate::fs::groupmgr::list_groups() {
        let members: Vec<_> = g.members.iter().map(|m| format!("{}", m)).collect();
        out.push_str(&format!("  {}({}) [{}] members=[{}]\n",
            g.name, g.gid, g.group_type.label(), members.join(",")));
    }
    out.into_bytes()
}

fn gen_sysrq() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== SysRq ===\n");
    let (count, triggers, blocked, mask, ops) = crate::fs::sysrq::stats();
    out.push_str(&format!("handlers: {}\n", count));
    out.push_str(&format!("total_triggers: {}\n", triggers));
    out.push_str(&format!("total_blocked: {}\n", blocked));
    out.push_str(&format!("enabled_mask: 0x{:x}\n", mask));
    out.push_str(&format!("ops: {}\n", ops));
    for h in crate::fs::sysrq::list_handlers() {
        let en = if h.enabled { "ON" } else { "OFF" };
        out.push_str(&format!("  '{}' [{}] {} — {} (triggers={})\n",
            h.key, en, h.category.label(), h.description, h.trigger_count));
    }
    out.into_bytes()
}

fn gen_telemetry() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Telemetry ===\n");
    let (count, samples, exports, enabled, ops) = crate::fs::telemetry::stats();
    out.push_str(&format!("metrics: {}\n", count));
    out.push_str(&format!("total_samples: {}\n", samples));
    out.push_str(&format!("total_exports: {}\n", exports));
    out.push_str(&format!("collection_enabled: {}\n", enabled));
    out.push_str(&format!("ops: {}\n", ops));
    for m in crate::fs::telemetry::list_metrics() {
        let avg = if m.sample_count > 0 { m.total_sum / m.sample_count } else { 0 };
        out.push_str(&format!("  {} [{}] = {} {} (avg={}, samples={})\n",
            m.name, m.metric_type.label(), m.value, m.unit, avg, m.sample_count));
    }
    out.into_bytes()
}

fn gen_fscache() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== FS Cache ===\n");
    let (devs, flushes, readaheads, evictions, ops) = crate::fs::fscache::stats();
    out.push_str(&format!("devices: {}\n", devs));
    out.push_str(&format!("total_flushes: {}\n", flushes));
    out.push_str(&format!("total_readaheads: {}\n", readaheads));
    out.push_str(&format!("total_evictions: {}\n", evictions));
    out.push_str(&format!("ops: {}\n", ops));
    for d in crate::fs::fscache::list_devices() {
        out.push_str(&format!("  {} policy={} ra={} dirty={}/{} cached={}\n",
            d.device_name, d.policy.label(), d.readahead_pages,
            d.dirty_pages, d.dirty_ratio_pct, d.cached_pages));
    }
    out.into_bytes()
}

fn gen_nameservice() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Name Service ===\n");
    let hostname = crate::fs::nameservice::get_hostname();
    let domain = crate::fs::nameservice::get_domain();
    out.push_str(&format!("hostname: {}\n", hostname));
    out.push_str(&format!("domain: {}\n", domain));
    let (hosts, lookups, hits, misses, ops) = crate::fs::nameservice::stats();
    out.push_str(&format!("hosts: {}\n", hosts));
    out.push_str(&format!("total_lookups: {}\n", lookups));
    out.push_str(&format!("cache_hits: {}\n", hits));
    out.push_str(&format!("cache_misses: {}\n", misses));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_oomkiller() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== OOM Killer ===\n");
    let policy = crate::fs::oomkiller::get_policy();
    let (count, kills, freed, invocations, ops) = crate::fs::oomkiller::stats();
    out.push_str(&format!("policy: {}\n", policy.label()));
    out.push_str(&format!("processes: {}\n", count));
    out.push_str(&format!("total_kills: {}\n", kills));
    out.push_str(&format!("memory_freed: {}\n", freed));
    out.push_str(&format!("invocations: {}\n", invocations));
    out.push_str(&format!("ops: {}\n", ops));
    for s in crate::fs::oomkiller::list_scores() {
        let eff = (s.score + s.adj).max(0);
        let ex = if s.exempt { " [EXEMPT]" } else { "" };
        out.push_str(&format!("  pid={} {} score={} adj={} eff={}{}\n",
            s.pid, s.process_name, s.score, s.adj, eff, ex));
    }
    out.into_bytes()
}

fn gen_blktrace() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    out.push_str("=== Block Trace ===\n");
    let (devs, events, bytes, active, ops) = crate::fs::blktrace::stats();
    out.push_str(&format!("devices: {}\n", devs));
    out.push_str(&format!("active: {}\n", active));
    out.push_str(&format!("total_events: {}\n", events));
    out.push_str(&format!("total_bytes: {}\n", bytes));
    out.push_str(&format!("ops: {}\n", ops));
    for d in crate::fs::blktrace::list_devices() {
        let st = if d.active { "ACTIVE" } else { "STOPPED" };
        out.push_str(&format!("  {} [{}] events={} bytes={}\n",
            d.device, st, d.total_events, d.total_bytes));
    }
    out.into_bytes()
}

fn gen_cgroupfs() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (count, created, deleted, changes, ops) = crate::fs::cgroupfs::stats();
    out.push_str(&format!("groups: {}\ncreated: {}\ndeleted: {}\nlimit_changes: {}\nops: {}\n",
        count, created, deleted, changes, ops));
    for g in crate::fs::cgroupfs::list_groups() {
        out.push_str(&format!("  {} cpu_w={} mem_max={} pids={}/{} procs={}\n",
            g.path, g.cpu_weight, g.memory_max, g.pids_current, g.pids_max, g.processes.len()));
    }
    out.into_bytes()
}

fn gen_secpolicy() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (rules, checks, allowed, denied, ops) = crate::fs::secpolicy::stats();
    let mode = crate::fs::secpolicy::get_mode();
    out.push_str(&format!("mode: {}\nrules: {}\nchecks: {}\nallowed: {}\ndenied: {}\nops: {}\n",
        mode.label(), rules, checks, allowed, denied, ops));
    for r in crate::fs::secpolicy::list_rules() {
        out.push_str(&format!("  rule#{} {}->{} {} {}\n",
            r.id, r.subject_label, r.object_label, r.action.label(), r.decision.label()));
    }
    out.into_bytes()
}

fn gen_procstat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (count, updates, ops) = crate::fs::procstat::stats();
    out.push_str(&format!("processes: {}\nupdates: {}\nops: {}\n", count, updates, ops));
    for p in crate::fs::procstat::list_processes() {
        out.push_str(&format!("  pid={} {} [{}] cpu={}us mem={} threads={}\n",
            p.pid, p.name, p.state.label(), p.cpu_time_us, p.memory_bytes, p.threads));
    }
    out.into_bytes()
}

fn gen_kernparam() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (count, lookups, sets, unconsumed, ops) = crate::fs::kernparam::stats();
    out.push_str(&format!("params: {}\nlookups: {}\nsets: {}\nunconsumed: {}\nops: {}\n",
        count, lookups, sets, unconsumed, ops));
    out.push_str(&format!("cmdline: {}\n", crate::fs::kernparam::cmdline()));
    for p in crate::fs::kernparam::list_params() {
        let consumer = p.consumed_by.as_deref().unwrap_or("-");
        out.push_str(&format!("  {}={} [{}] consumer={}\n",
            p.key, p.value, p.origin.label(), consumer));
    }
    out.into_bytes()
}

fn gen_tracemon() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (tp_count, ev_count, total, dropped, enabled, ops) = crate::fs::tracemon::stats();
    out.push_str(&format!("tracepoints: {}\nbuffered: {}\ntotal_events: {}\ndropped: {}\nenabled: {}\nops: {}\n",
        tp_count, ev_count, total, dropped, enabled, ops));
    for tp in crate::fs::tracemon::list_tracepoints() {
        let st = if tp.enabled { "ON" } else { "off" };
        out.push_str(&format!("  {} [{}] {} hits={}\n", tp.name, tp.category.label(), st, tp.hit_count));
    }
    out.into_bytes()
}

fn gen_authbroker() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (creds, grants, attempts, granted, denied, revoked, ops) = crate::fs::authbroker::stats();
    out.push_str(&format!("credentials: {}\ngrants: {}\nattempts: {}\ngranted: {}\ndenied: {}\nrevoked: {}\nops: {}\n",
        creds, grants, attempts, granted, denied, revoked, ops));
    for c in crate::fs::authbroker::list_credentials(None) {
        let locked = if c.locked { " LOCKED" } else { "" };
        out.push_str(&format!("  {} [{}] fails={}{}\n", c.principal, c.method.label(), c.failed_attempts, locked));
    }
    out.into_bytes()
}

fn gen_prociso() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (ns_count, cont_count, attaches, detaches, ops) = crate::fs::prociso::stats();
    out.push_str(&format!("namespaces: {}\ncontainers: {}\nattaches: {}\ndetaches: {}\nops: {}\n",
        ns_count, cont_count, attaches, detaches, ops));
    for ns in crate::fs::prociso::list_namespaces() {
        let parent = ns.parent_id.map_or(String::from("-"), |p| format!("{}", p));
        out.push_str(&format!("  ns#{} {} [{}] iso={} parent={} procs={}\n",
            ns.id, ns.name, ns.ns_type.label(), ns.isolation.label(), parent, ns.processes.len()));
    }
    out.into_bytes()
}

fn gen_dmevent() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (devs, evs, rules, total, matched, ops) = crate::fs::dmevent::stats();
    out.push_str(&format!("devices: {}\nbuffered_events: {}\nrules: {}\ntotal_events: {}\nmatched: {}\nops: {}\n",
        devs, evs, rules, total, matched, ops));
    for d in crate::fs::dmevent::list_devices() {
        let st = if d.online { "online" } else { "offline" };
        out.push_str(&format!("  {} [{}] {} {}\n", d.devname, d.subsystem.label(), st, d.devpath));
    }
    out.into_bytes()
}

fn gen_pftrack() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (procs, evs, total, minor, major, ops) = crate::fs::pftrack::stats();
    out.push_str(&format!("processes: {}\nbuffered: {}\ntotal_faults: {}\nminor: {}\nmajor: {}\nops: {}\n",
        procs, evs, total, minor, major, ops));
    for p in crate::fs::pftrack::list_processes() {
        out.push_str(&format!("  pid={} {} total={} minor={} major={} cow={}\n",
            p.pid, p.name, p.total, p.minor, p.major, p.cow));
    }
    out.into_bytes()
}

fn gen_ipclog() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (channels, msgs, total, bytes, errors, enabled, ops) = crate::fs::ipclog::stats();
    out.push_str(&format!("channels: {}\nbuffered: {}\ntotal_msgs: {}\ntotal_bytes: {}\nerrors: {}\nenabled: {}\nops: {}\n",
        channels, msgs, total, bytes, errors, enabled, ops));
    for ch in crate::fs::ipclog::list_channels() {
        out.push_str(&format!("  ch#{} {} sent={} recv={} lat={}us err={}\n",
            ch.channel_id, ch.name, ch.messages_sent, ch.messages_received, ch.avg_latency_us, ch.errors));
    }
    out.into_bytes()
}

fn gen_numastat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (nodes, allocs, remote, migrations, pct, ops) = crate::fs::numastat::stats();
    out.push_str(&format!("nodes: {}\ntotal_allocs: {}\nremote_allocs: {}\nmigrations: {}\nremote_pct: {}%\nops: {}\n",
        nodes, allocs, remote, migrations, pct, ops));
    for n in crate::fs::numastat::list_nodes() {
        out.push_str(&format!("  node{} [{}] mem={}/{} local={} remote={} lat={}ns cpus={}\n",
            n.id, n.state.label(), n.used_memory, n.total_memory,
            n.local_allocs, n.remote_allocs, n.avg_latency_ns, n.cpus.len()));
    }
    out.into_bytes()
}

fn gen_shmem() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (regions, created, deleted, attaches, detaches, bytes, ops) = crate::fs::shmem::stats();
    out.push_str(&format!("regions: {}\ncreated: {}\ndeleted: {}\nattaches: {}\ndetaches: {}\ntotal_bytes: {}\nops: {}\n",
        regions, created, deleted, attaches, detaches, bytes, ops));
    for r in crate::fs::shmem::list_regions() {
        let pers = if r.persistent { " PERSIST" } else { "" };
        out.push_str(&format!("  #{} {} size={} [{}] owner={} attached={}{}\n",
            r.id, r.name, r.size, r.permission.label(), r.owner_pid, r.attached_pids.len(), pers));
    }
    out.into_bytes()
}

fn gen_wqstat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (count, enqueued, completed, cancelled, ops) = crate::fs::wqstat::stats();
    out.push_str(&format!("workqueues: {}\ntotal_enqueued: {}\ntotal_completed: {}\ncancelled: {}\nops: {}\n",
        count, enqueued, completed, cancelled, ops));
    for q in crate::fs::wqstat::list() {
        out.push_str(&format!("  {} [{}] pend={} active={} done={} lat={}us workers={}\n",
            q.name, q.wq_type.label(), q.pending, q.active, q.completed, q.avg_latency_us, q.workers));
    }
    out.into_bytes()
}

fn gen_slabstat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (count, allocs, frees, reclaims, ops) = crate::fs::slabstat::stats();
    out.push_str(&format!("caches: {}\ntotal_allocs: {}\ntotal_frees: {}\nreclaims: {}\nops: {}\n",
        count, allocs, frees, reclaims, ops));
    for c in crate::fs::slabstat::list() {
        out.push_str(&format!("  {} size={} active={}/{} util={}% hwm={}\n",
            c.name, c.object_size, c.active_objects, c.total_objects, c.utilization_pct(), c.high_watermark));
    }
    out.into_bytes()
}

fn gen_timerq() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (total, pending, created, fired, cancelled, overruns, ops) = crate::fs::timerq::stats();
    out.push_str(&format!("timers: {}\npending: {}\ncreated: {}\nfired: {}\ncancelled: {}\noverruns: {}\nops: {}\n",
        total, pending, created, fired, cancelled, overruns, ops));
    for t in crate::fs::timerq::list_pending() {
        out.push_str(&format!("  #{} {} [{}] deadline={} fires={}\n",
            t.id, t.name, t.timer_type.label(), t.deadline_ns, t.fire_count));
    }
    out.into_bytes()
}

fn gen_fdtable() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (tables, opens, closes, dups, ops) = crate::fs::fdtable::stats();
    out.push_str(&format!("tables: {}\ntotal_opens: {}\ntotal_closes: {}\ntotal_dups: {}\nops: {}\n",
        tables, opens, closes, dups, ops));
    for (pid, count, max) in crate::fs::fdtable::list_tables() {
        out.push_str(&format!("  pid={} fds={}/{}\n", pid, count, max));
    }
    out.into_bytes()
}

fn gen_rcustat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (cpus, gp, total_gp, total_cb, stalls, ops) = crate::fs::rcustat::stats();
    out.push_str(&format!("cpus: {}\n", cpus));
    out.push_str(&format!("current_gp: {}\n", gp));
    out.push_str(&format!("total_gp: {}\n", total_gp));
    out.push_str(&format!("total_callbacks: {}\n", total_cb));
    out.push_str(&format!("total_stalls: {}\n", stalls));
    out.push_str(&format!("ops: {}\n", ops));
    for cs in crate::fs::rcustat::cpu_stats() {
        out.push_str(&format!("cpu{}: pending={} invoked={} qs={}\n",
            cs.cpu_id, cs.callbacks_pending, cs.callbacks_invoked, cs.quiescent_states));
    }
    out.into_bytes()
}

fn gen_kconsole() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (count, active_id, switches, writes, ops) = crate::fs::kconsole::stats();
    out.push_str(&format!("consoles: {}\n", count));
    out.push_str(&format!("active_id: {}\n", active_id));
    out.push_str(&format!("total_switches: {}\n", switches));
    out.push_str(&format!("total_writes: {}\n", writes));
    out.push_str(&format!("ops: {}\n", ops));
    for c in crate::fs::kconsole::list() {
        out.push_str(&format!("{}: {} type={} {}x{} active={} written={}\n",
            c.id, c.name, c.console_type.label(), c.cols, c.rows, c.active, c.bytes_written));
    }
    out.into_bytes()
}

fn gen_signalq() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (procs, sent, delivered, dropped, ops) = crate::fs::signalq::stats();
    out.push_str(&format!("processes: {}\n", procs));
    out.push_str(&format!("total_sent: {}\n", sent));
    out.push_str(&format!("total_delivered: {}\n", delivered));
    out.push_str(&format!("total_dropped: {}\n", dropped));
    out.push_str(&format!("ops: {}\n", ops));
    for ps in crate::fs::signalq::list_processes() {
        out.push_str(&format!("pid={}: pending={} delivered={} blocked_mask={:#x}\n",
            ps.pid, ps.pending.len(), ps.total_delivered, ps.blocked_mask));
    }
    out.into_bytes()
}

fn gen_memcg() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (groups, charges, uncharges, failures, oom, ops) = crate::fs::memcg::stats();
    out.push_str(&format!("groups: {}\n", groups));
    out.push_str(&format!("total_charges: {}\n", charges));
    out.push_str(&format!("total_uncharges: {}\n", uncharges));
    out.push_str(&format!("total_failures: {}\n", failures));
    out.push_str(&format!("total_oom: {}\n", oom));
    out.push_str(&format!("ops: {}\n", ops));
    for g in crate::fs::memcg::list() {
        out.push_str(&format!("{}: usage={} limit={} swap={} oom_kills={}\n",
            g.path, g.usage_bytes, g.limit_bytes, g.swap_usage, g.oom_kills));
    }
    out.into_bytes()
}

fn gen_tlbstat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (cpus, hits, misses, shootdowns, flushes, ops) = crate::fs::tlbstat::stats();
    out.push_str(&format!("cpus: {}\n", cpus));
    out.push_str(&format!("total_hits: {}\n", hits));
    out.push_str(&format!("total_misses: {}\n", misses));
    out.push_str(&format!("hit_rate: {}%\n", crate::fs::tlbstat::hit_rate()));
    out.push_str(&format!("total_shootdowns: {}\n", shootdowns));
    out.push_str(&format!("total_flushes: {}\n", flushes));
    out.push_str(&format!("ops: {}\n", ops));
    for cs in crate::fs::tlbstat::cpu_stats() {
        out.push_str(&format!("cpu{}: hits={} misses={} shootdowns_sent={} flushes={}\n",
            cs.cpu_id, cs.hits, cs.misses, cs.shootdowns_sent, cs.flushes));
    }
    out.into_bytes()
}

fn gen_pagestat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (zones, allocs, frees, reclaims, fails, ops) = crate::fs::pagestat::stats();
    out.push_str(&format!("zones: {}\n", zones));
    out.push_str(&format!("total_allocs: {}\n", allocs));
    out.push_str(&format!("total_frees: {}\n", frees));
    out.push_str(&format!("total_reclaims: {}\n", reclaims));
    out.push_str(&format!("total_fails: {}\n", fails));
    out.push_str(&format!("ops: {}\n", ops));
    let (hp_total, hp_free, hp_res) = crate::fs::pagestat::hugepage_info();
    out.push_str(&format!("hugepages: total={} free={} reserved={}\n", hp_total, hp_free, hp_res));
    for zs in crate::fs::pagestat::zone_stats() {
        out.push_str(&format!("{}: total={} free={} alloc={} frag={}%\n",
            zs.zone.label(), zs.total_pages, zs.free_pages, zs.allocated, zs.fragmentation_pct));
    }
    out.into_bytes()
}

fn gen_dmastat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (devs, maps, unmaps, bytes, faults, ops) = crate::fs::dmastat::stats();
    out.push_str(&format!("devices: {}\n", devs));
    out.push_str(&format!("total_maps: {}\n", maps));
    out.push_str(&format!("total_unmaps: {}\n", unmaps));
    out.push_str(&format!("total_bytes: {}\n", bytes));
    out.push_str(&format!("total_faults: {}\n", faults));
    out.push_str(&format!("ops: {}\n", ops));
    for d in crate::fs::dmastat::device_stats() {
        out.push_str(&format!("{}: maps={} active={} xfer={} faults={} iommu={}\n",
            d.name, d.maps, d.active_mappings, d.bytes_transferred, d.faults, d.iommu_enabled));
    }
    out.into_bytes()
}

fn gen_compstat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (zones, attempts, migrations, stalls, stall_ns, ops) = crate::fs::compstat::stats();
    out.push_str(&format!("zones: {}\n", zones));
    out.push_str(&format!("total_attempts: {}\n", attempts));
    out.push_str(&format!("total_migrations: {}\n", migrations));
    out.push_str(&format!("total_stalls: {}\n", stalls));
    out.push_str(&format!("total_stall_ns: {}\n", stall_ns));
    out.push_str(&format!("success_rate: {}%\n", crate::fs::compstat::success_rate()));
    out.push_str(&format!("ops: {}\n", ops));
    for zs in crate::fs::compstat::zone_stats() {
        out.push_str(&format!("{}: attempts={} success={} failed={} migrated={} stalls={}\n",
            zs.zone.label(), zs.attempts, zs.successes, zs.failures, zs.pages_migrated, zs.stalls));
    }
    out.into_bytes()
}

fn gen_irqstat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (irqs, cpus, total, spurious, _samples, ops) = crate::fs::irqstat::stats();
    out.push_str(&format!("irq_lines: {}\n", irqs));
    out.push_str(&format!("cpus: {}\n", cpus));
    out.push_str(&format!("total_irqs: {}\n", total));
    out.push_str(&format!("total_spurious: {}\n", spurious));
    out.push_str(&format!("ops: {}\n", ops));
    for line in crate::fs::irqstat::irq_lines() {
        out.push_str(&format!("IRQ{}: {} ({}) count={} spurious={}\n",
            line.irq_num, line.name, line.irq_type.label(), line.count, line.spurious));
    }
    for cs in crate::fs::irqstat::per_cpu() {
        out.push_str(&format!("cpu{}: total={} ipi={} timer={} avg_lat={}ns max_lat={}ns\n",
            cs.cpu_id, cs.total_irqs, cs.total_ipi, cs.total_timer, cs.avg_latency_ns, cs.max_latency_ns));
    }
    out.into_bytes()
}

fn gen_epollstat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (count, creates, waits, events, timeouts, ops) = crate::fs::epollstat::stats();
    out.push_str(&format!("instances: {}\n", count));
    out.push_str(&format!("total_creates: {}\n", creates));
    out.push_str(&format!("total_waits: {}\n", waits));
    out.push_str(&format!("total_events: {}\n", events));
    out.push_str(&format!("total_timeouts: {}\n", timeouts));
    out.push_str(&format!("ops: {}\n", ops));
    for inst in crate::fs::epollstat::list_instances() {
        out.push_str(&format!("epoll#{}: pid={} fds={} waits={} events={}\n",
            inst.id, inst.owner_pid, inst.registered_fds, inst.wait_calls, inst.events_delivered));
    }
    out.into_bytes()
}

fn gen_vmmap() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (procs, vmas, maps, unmaps, ops) = crate::fs::vmmap::stats();
    out.push_str(&format!("processes: {}\n", procs));
    out.push_str(&format!("total_vmas: {}\n", vmas));
    out.push_str(&format!("total_maps: {}\n", maps));
    out.push_str(&format!("total_unmaps: {}\n", unmaps));
    out.push_str(&format!("ops: {}\n", ops));
    for (pid, vma_count, mapped) in crate::fs::vmmap::list_processes() {
        out.push_str(&format!("pid={}: vmas={} mapped={}\n", pid, vma_count, mapped));
    }
    out.into_bytes()
}

fn gen_softirq() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (cpus, types, raised, executed, tasklets, ops) = crate::fs::softirq::stats();
    out.push_str(&format!("cpus: {}\n", cpus));
    out.push_str(&format!("types: {}\n", types));
    out.push_str(&format!("total_raised: {}\n", raised));
    out.push_str(&format!("total_executed: {}\n", executed));
    out.push_str(&format!("total_tasklets: {}\n", tasklets));
    out.push_str(&format!("ops: {}\n", ops));
    for ts in crate::fs::softirq::type_stats() {
        out.push_str(&format!("{}: raised={} executed={} total_ns={}\n",
            ts.softirq_type.label(), ts.raised, ts.executed, ts.total_ns));
    }
    out.into_bytes()
}

fn gen_netfilter() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (rules, ct, packets, accepted, dropped, ops) = crate::fs::netfilter::stats();
    out.push_str(&format!("rules: {}\n", rules));
    out.push_str(&format!("conntrack: {}\n", ct));
    out.push_str(&format!("total_packets: {}\n", packets));
    out.push_str(&format!("total_accepted: {}\n", accepted));
    out.push_str(&format!("total_dropped: {}\n", dropped));
    out.push_str(&format!("ops: {}\n", ops));
    for r in crate::fs::netfilter::list_rules() {
        out.push_str(&format!("#{}: {} {} {} matches={} enabled={}\n",
            r.id, r.chain.label(), r.action.label(), r.description, r.matches, r.enabled));
    }
    out.into_bytes()
}

fn gen_schedclass() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (tasks, classes, switches, migrations, ops) = crate::fs::schedclass::stats();
    out.push_str(&format!("tasks: {}\n", tasks));
    out.push_str(&format!("classes: {}\n", classes));
    out.push_str(&format!("total_switches: {}\n", switches));
    out.push_str(&format!("total_migrations: {}\n", migrations));
    out.push_str(&format!("ops: {}\n", ops));
    for cs in crate::fs::schedclass::class_stats() {
        out.push_str(&format!("{}: tasks={} switches={} runtime={}ns avg_slice={}ns\n",
            cs.class.label(), cs.task_count, cs.context_switches, cs.total_runtime_ns, cs.avg_slice_ns));
    }
    out.into_bytes()
}

fn gen_cpuidle() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (cpus, transitions, idle_ns, ops) = crate::fs::cpuidle::stats();
    out.push_str(&format!("cpus: {}\n", cpus));
    out.push_str(&format!("total_transitions: {}\n", transitions));
    out.push_str(&format!("total_idle_ns: {}\n", idle_ns));
    out.push_str(&format!("ops: {}\n", ops));
    for cs in crate::fs::cpuidle::per_cpu() {
        out.push_str(&format!("cpu{}: state={} idle={}% idle_ns={}\n",
            cs.cpu_id, cs.current_state.label(), crate::fs::cpuidle::idle_pct(cs.cpu_id), cs.total_idle_ns));
    }
    out.into_bytes()
}

fn gen_futexstat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (addrs, procs, waits, wakes, timeouts, ops) = crate::fs::futexstat::stats();
    out.push_str(&format!("tracked_addrs: {}\n", addrs));
    out.push_str(&format!("tracked_procs: {}\n", procs));
    out.push_str(&format!("total_waits: {}\n", waits));
    out.push_str(&format!("total_wakes: {}\n", wakes));
    out.push_str(&format!("total_timeouts: {}\n", timeouts));
    out.push_str(&format!("ops: {}\n", ops));
    for h in crate::fs::futexstat::hotspots(10) {
        out.push_str(&format!("{:#x}: waits={} wakes={} waiters={} max={}\n",
            h.address, h.waits, h.wakes, h.current_waiters, h.max_waiters));
    }
    out.into_bytes()
}

fn gen_writeback() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (devs, dirty, written, flushes, threshold, ops) = crate::fs::writeback::stats();
    out.push_str(&format!("devices: {}\n", devs));
    out.push_str(&format!("total_dirty_pages: {}\n", dirty));
    out.push_str(&format!("total_written_pages: {}\n", written));
    out.push_str(&format!("total_flushes: {}\n", flushes));
    out.push_str(&format!("dirty_threshold_pct: {}\n", threshold));
    out.push_str(&format!("ops: {}\n", ops));
    for d in crate::fs::writeback::device_stats() {
        out.push_str(&format!("{}: dirty={} wb={} written={} bytes={} flushes={} cong={}\n",
            d.device, d.dirty_pages, d.writeback_pages, d.written_pages,
            d.written_bytes, d.flushes, d.congestion_count));
    }
    out.into_bytes()
}

fn gen_iolatency() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (devs, ios, slow, threshold, ops) = crate::fs::iolatency::stats();
    out.push_str(&format!("devices: {}\n", devs));
    out.push_str(&format!("total_ios: {}\n", ios));
    out.push_str(&format!("total_slow: {}\n", slow));
    out.push_str(&format!("slow_threshold_ns: {}\n", threshold));
    out.push_str(&format!("ops: {}\n", ops));
    for d in crate::fs::iolatency::per_device() {
        out.push_str(&format!("{}: rd={}/{} wr={}/{} max_rd={} max_wr={} slow={}\n",
            d.device, d.read_count, d.read_avg_ns, d.write_count, d.write_avg_ns,
            d.read_max_ns, d.write_max_ns, d.slow_count));
    }
    out.into_bytes()
}

fn gen_taskstats() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (tasks, cpu, io, delays, ops) = crate::fs::taskstats::stats();
    out.push_str(&format!("tasks: {}\n", tasks));
    out.push_str(&format!("total_cpu_ns: {}\n", cpu));
    out.push_str(&format!("total_io_bytes: {}\n", io));
    out.push_str(&format!("total_delays_ns: {}\n", delays));
    out.push_str(&format!("ops: {}\n", ops));
    for t in crate::fs::taskstats::top_cpu(10) {
        out.push_str(&format!("pid={} {}: cpu={}ns usr={}ns sys={}ns rd={} wr={}\n",
            t.pid, t.name, t.cpu_time_ns, t.user_time_ns, t.sys_time_ns,
            t.read_bytes, t.write_bytes));
    }
    out.into_bytes()
}

fn gen_kprobes() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (probes, hits, misses, overhead, ops) = crate::fs::kprobes::stats();
    out.push_str(&format!("probes: {}\n", probes));
    out.push_str(&format!("total_hits: {}\n", hits));
    out.push_str(&format!("total_misses: {}\n", misses));
    out.push_str(&format!("total_overhead_ns: {}\n", overhead));
    out.push_str(&format!("ops: {}\n", ops));
    for p in crate::fs::kprobes::list() {
        out.push_str(&format!("[{}] {} {:#x}: hits={} misses={} overhead={}ns {}\n",
            p.probe_type.label(), p.name, p.address, p.hits, p.misses,
            p.overhead_ns, if p.enabled { "enabled" } else { "disabled" }));
    }
    out.into_bytes()
}

fn gen_netsock() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (socks, opened, closed, rx, tx, retrans, ops) = crate::fs::netsock::stats();
    out.push_str(&format!("sockets: {}\n", socks));
    out.push_str(&format!("total_opened: {}\n", opened));
    out.push_str(&format!("total_closed: {}\n", closed));
    out.push_str(&format!("total_rx_bytes: {}\n", rx));
    out.push_str(&format!("total_tx_bytes: {}\n", tx));
    out.push_str(&format!("total_retransmits: {}\n", retrans));
    out.push_str(&format!("ops: {}\n", ops));
    for s in crate::fs::netsock::list() {
        out.push_str(&format!("{} pid={} {}:{} -> {}:{} {} rx={} tx={}\n",
            s.proto.label(), s.pid, s.local_addr, s.local_port,
            s.remote_addr, s.remote_port, s.state.label(), s.rx_bytes, s.tx_bytes));
    }
    out.into_bytes()
}

fn gen_blkqueue() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (devs, submitted, completed, merged, plugs, ops) = crate::fs::blkqueue::stats();
    out.push_str(&format!("devices: {}\n", devs));
    out.push_str(&format!("total_submitted: {}\n", submitted));
    out.push_str(&format!("total_completed: {}\n", completed));
    out.push_str(&format!("total_merged: {}\n", merged));
    out.push_str(&format!("total_plugs: {}\n", plugs));
    out.push_str(&format!("ops: {}\n", ops));
    for d in crate::fs::blkqueue::device_queues() {
        out.push_str(&format!("{}: depth={}/{} sub={} comp={} merged={} plugs={}/{} {}\n",
            d.device, d.queue_depth, d.max_depth, d.submitted, d.completed,
            d.merged, d.plug_count, d.unplug_count,
            if d.plugged { "PLUGGED" } else { "unplugged" }));
    }
    out.into_bytes()
}

fn gen_powerstat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (domains, energy, transitions, wakes, ops) = crate::fs::powerstat::stats();
    out.push_str(&format!("domains: {}\n", domains));
    out.push_str(&format!("total_energy_uj: {}\n", energy));
    out.push_str(&format!("total_transitions: {}\n", transitions));
    out.push_str(&format!("total_wakes: {}\n", wakes));
    out.push_str(&format!("ops: {}\n", ops));
    for d in crate::fs::powerstat::domain_stats() {
        out.push_str(&format!("{}: {} energy={}uJ trans={} active={}ns idle={}ns\n",
            d.domain.label(), d.current_state.label(), d.energy_uj,
            d.transitions, d.active_time_ns, d.idle_time_ns));
    }
    out.into_bytes()
}

fn gen_inodestat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (fss, allocs, frees, evictions, lookups, ops) = crate::fs::inodestat::stats();
    out.push_str(&format!("filesystems: {}\n", fss));
    out.push_str(&format!("total_allocs: {}\n", allocs));
    out.push_str(&format!("total_frees: {}\n", frees));
    out.push_str(&format!("total_evictions: {}\n", evictions));
    out.push_str(&format!("dcache_lookups: {}\n", lookups));
    out.push_str(&format!("ops: {}\n", ops));
    let d = crate::fs::inodestat::dcache_stats();
    let rate = crate::fs::inodestat::dcache_hit_rate();
    out.push_str(&format!("dcache: entries={} hits={} misses={} rate={}.{}%\n",
        d.entries, d.hits, d.misses, rate / 100, rate % 100));
    for f in crate::fs::inodestat::fs_stats() {
        out.push_str(&format!("{} ({}): active={} alloc={} free={} evict={} dirty={}\n",
            f.mount_point, f.fs_type.label(), f.active, f.allocated, f.freed,
            f.evicted, f.dirty));
    }
    out.into_bytes()
}

fn gen_migstat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (cpus, tasks, migs, numa, ops) = crate::fs::migstat::stats();
    out.push_str(&format!("cpus: {}\n", cpus));
    out.push_str(&format!("tracked_tasks: {}\n", tasks));
    out.push_str(&format!("total_migrations: {}\n", migs));
    out.push_str(&format!("total_numa_crosses: {}\n", numa));
    out.push_str(&format!("ops: {}\n", ops));
    for (reason, count) in crate::fs::migstat::reason_stats() {
        out.push_str(&format!("{}: {}\n", reason.label(), count));
    }
    for c in crate::fs::migstat::per_cpu() {
        out.push_str(&format!("cpu{}: in={} out={} numa={}\n",
            c.cpu_id, c.migrations_in, c.migrations_out, c.numa_crosses));
    }
    out.into_bytes()
}

fn gen_pagecache() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (devs, hits, misses, evictions, readahead, ops) = crate::fs::pagecache::stats();
    out.push_str(&format!("devices: {}\n", devs));
    out.push_str(&format!("total_hits: {}\n", hits));
    out.push_str(&format!("total_misses: {}\n", misses));
    out.push_str(&format!("total_evictions: {}\n", evictions));
    out.push_str(&format!("total_readahead: {}\n", readahead));
    let rate = crate::fs::pagecache::hit_rate();
    out.push_str(&format!("hit_rate: {}.{}%\n", rate / 100, rate % 100));
    out.push_str(&format!("ops: {}\n", ops));
    for d in crate::fs::pagecache::per_device() {
        out.push_str(&format!("{}: cached={} hits={} misses={} evict={} dirty={}\n",
            d.device, d.cached_pages, d.hits, d.misses, d.evictions, d.dirty_pages));
    }
    out.into_bytes()
}

fn gen_netdev() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (ifaces, rx, tx, errors, drops, ops) = crate::fs::netdev::stats();
    out.push_str(&format!("interfaces: {}\n", ifaces));
    out.push_str(&format!("total_rx_bytes: {}\n", rx));
    out.push_str(&format!("total_tx_bytes: {}\n", tx));
    out.push_str(&format!("total_errors: {}\n", errors));
    out.push_str(&format!("total_drops: {}\n", drops));
    out.push_str(&format!("ops: {}\n", ops));
    for i in crate::fs::netdev::list() {
        out.push_str(&format!("{} ({}) {}: rx={}/{} tx={}/{} err={}/{} drop={}/{}\n",
            i.name, i.nic_type.label(), if i.link_up { "UP" } else { "DOWN" },
            i.rx_bytes, i.rx_packets, i.tx_bytes, i.tx_packets,
            i.rx_errors, i.tx_errors, i.rx_drops, i.tx_drops));
    }
    out.into_bytes()
}

fn gen_cpustat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (cpus, ctxsw, irqs, ops) = crate::fs::cpustat::stats();
    out.push_str(&format!("cpus: {}\n", cpus));
    let util = crate::fs::cpustat::utilization();
    out.push_str(&format!("utilization: {}.{}%\n", util / 100, util % 100));
    out.push_str(&format!("context_switches: {}\n", ctxsw));
    out.push_str(&format!("interrupts: {}\n", irqs));
    out.push_str(&format!("ops: {}\n", ops));
    let modes = ["user", "nice", "system", "idle", "iowait", "irq", "softirq", "steal"];
    for c in crate::fs::cpustat::per_cpu() {
        let parts: Vec<_> = modes.iter().zip(c.times_ns.iter())
            .map(|(m, ns)| format!("{}={}ns", m, ns))
            .collect();
        out.push_str(&format!("cpu{}: {} ctxsw={} irq={}\n",
            c.cpu_id, parts.join(" "), c.context_switches, c.interrupts));
    }
    out.into_bytes()
}

fn gen_filelock() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (active, acquired, released, contentions, deadlocks, ops) = crate::fs::filelock::stats();
    out.push_str(&format!("active_locks: {}\n", active));
    out.push_str(&format!("total_acquired: {}\n", acquired));
    out.push_str(&format!("total_released: {}\n", released));
    out.push_str(&format!("total_contentions: {}\n", contentions));
    out.push_str(&format!("total_deadlocks: {}\n", deadlocks));
    out.push_str(&format!("ops: {}\n", ops));
    for l in crate::fs::filelock::active_locks() {
        out.push_str(&format!("[{}] pid={} {} {}-{} {} cont={}\n",
            l.lock_type.label(), l.pid, l.path, l.start,
            if l.end == u64::MAX { String::from("EOF") } else { format!("{}", l.end) },
            if l.blocking { "BLOCK" } else { "NONBLOCK" }, l.contentions));
    }
    out.into_bytes()
}

fn gen_pidstat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (nss, alloc, freed, reuses, ops) = crate::fs::pidstat::stats();
    out.push_str(&format!("namespaces: {}\n", nss));
    out.push_str(&format!("total_allocated: {}\n", alloc));
    out.push_str(&format!("total_freed: {}\n", freed));
    out.push_str(&format!("total_reuses: {}\n", reuses));
    out.push_str(&format!("ops: {}\n", ops));
    for ns in crate::fs::pidstat::ns_list() {
        out.push_str(&format!("ns{}: parent={} active={} max={} alloc={} free={} hwm={}\n",
            ns.ns_id, ns.parent_id.map_or(String::from("none"), |p| format!("{}", p)),
            ns.active_pids, ns.max_pid, ns.allocated, ns.freed, ns.high_watermark));
    }
    out.into_bytes()
}

fn gen_binfmt() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (fmts, loads, errors, ops) = crate::fs::binfmt::stats();
    out.push_str(&format!("formats: {}\n", fmts));
    out.push_str(&format!("total_loads: {}\n", loads));
    out.push_str(&format!("total_errors: {}\n", errors));
    out.push_str(&format!("ops: {}\n", ops));
    for f in crate::fs::binfmt::format_stats() {
        out.push_str(&format!("{}: loads={} errors={} avg={}ns max={}ns\n",
            f.format.label(), f.loads, f.errors, f.avg_load_ns, f.max_load_ns));
    }
    for (err, count) in crate::fs::binfmt::error_breakdown() {
        if count > 0 {
            out.push_str(&format!("err_{}: {}\n", err.label(), count));
        }
    }
    out.into_bytes()
}

fn gen_pipestat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (active, created, destroyed, bytes, blocks, ops) = crate::fs::pipestat::stats();
    out.push_str(&format!("active_pipes: {}\n", active));
    out.push_str(&format!("total_created: {}\n", created));
    out.push_str(&format!("total_destroyed: {}\n", destroyed));
    out.push_str(&format!("total_bytes: {}\n", bytes));
    out.push_str(&format!("total_blocks: {}\n", blocks));
    out.push_str(&format!("ops: {}\n", ops));
    for p in crate::fs::pipestat::list() {
        out.push_str(&format!("[{}] {} rd={} wr={} buf={}/{} written={} read={}\n",
            p.pipe_type.label(), p.id, p.reader_pid, p.writer_pid,
            p.buffered_bytes, p.buffer_size, p.bytes_written, p.bytes_read));
    }
    out.into_bytes()
}

fn gen_sockbuf() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (pools, allocs, frees, drops, bytes, ops) = crate::fs::sockbuf::stats();
    out.push_str(&format!("pools: {}\n", pools));
    out.push_str(&format!("total_allocs: {}\n", allocs));
    out.push_str(&format!("total_frees: {}\n", frees));
    out.push_str(&format!("total_drops: {}\n", drops));
    out.push_str(&format!("total_bytes: {}\n", bytes));
    out.push_str(&format!("ops: {}\n", ops));
    for p in crate::fs::sockbuf::pool_stats() {
        out.push_str(&format!("{}: active={} bytes={} allocs={} frees={} drops={} peak={}\n",
            p.pool.label(), p.active_buffers, p.total_bytes, p.allocs, p.frees, p.drops, p.peak_buffers));
    }
    out.into_bytes()
}

fn gen_schedlat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (cpus, wakes, runqs, preempts, max_ns, ops) = crate::fs::schedlat::stats();
    out.push_str(&format!("cpus: {}\n", cpus));
    out.push_str(&format!("total_wakeups: {}\n", wakes));
    out.push_str(&format!("total_runq_waits: {}\n", runqs));
    out.push_str(&format!("total_preempts: {}\n", preempts));
    out.push_str(&format!("global_max_ns: {}\n", max_ns));
    out.push_str(&format!("ops: {}\n", ops));
    let labels = ["<1us", "<10us", "<100us", "<1ms", "<10ms", "<100ms", "<1s", ">=1s"];
    let hist = crate::fs::schedlat::global_histogram();
    for (i, count) in hist.iter().enumerate() {
        out.push_str(&format!("{}: {}\n", labels[i], count));
    }
    out.into_bytes()
}

fn gen_mempress() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let c = crate::fs::mempress::current();
    out.push_str(&format!("level: {}\n", c.level.label()));
    out.push_str(&format!("oom_proximity: {}%\n", c.oom_proximity));
    out.push_str(&format!("total_stall_ns: {}\n", c.total_stall_ns));
    out.push_str(&format!("total_reclaim_pages: {}\n", c.total_reclaim_pages));
    let (stalls, reclaims, _stall_ns, _pages, changes, _oom, ops) = crate::fs::mempress::stats();
    out.push_str(&format!("stall_events: {}\n", stalls));
    out.push_str(&format!("reclaim_events: {}\n", reclaims));
    out.push_str(&format!("level_changes: {}\n", changes));
    out.push_str(&format!("ops: {}\n", ops));
    out.into_bytes()
}

fn gen_cpucache() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (levels, hits, misses, ops) = crate::fs::cpucache::stats();
    out.push_str(&format!("levels: {}\n", levels));
    out.push_str(&format!("total_hits: {}\n", hits));
    out.push_str(&format!("total_misses: {}\n", misses));
    let rate = crate::fs::cpucache::overall_hit_rate();
    out.push_str(&format!("overall_hit_rate: {}.{}%\n", rate / 100, rate % 100));
    out.push_str(&format!("ops: {}\n", ops));
    for c in crate::fs::cpucache::topology() {
        let r = crate::fs::cpucache::hit_rate(c.level);
        out.push_str(&format!("{}: {}KB {}B/line {}way {}set shared={} hits={} miss={} rate={}.{}%\n",
            c.level.label(), c.size_kb, c.line_size, c.ways, c.sets, c.shared_cpus,
            c.hits, c.misses, r / 100, r % 100));
    }
    out.into_bytes()
}

fn gen_aiostat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (rings, submitted, completed, overflows, ops) = super::aiostat::stats();
    out.push_str("=== Async I/O Stats ===\n");
    out.push_str(&format!("Rings: {}  Submitted: {}  Completed: {}  Overflows: {}  Ops: {}\n\n", rings, submitted, completed, overflows, ops));
    for r in super::aiostat::ring_stats() {
        out.push_str(&format!("Ring {} (pid {})  SQ {}/{}  CQ pending {}  submitted {}  completed {}  overflows {}  sq_full {}\n",
            r.ring_id, r.pid, r.sq_pending, r.sq_size, r.cq_pending,
            r.submitted, r.completed, r.overflows, r.sq_full_count));
    }
    out.into_bytes()
}

fn gen_kthread() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (threads, created, exited, ops) = super::kthread::stats();
    out.push_str("=== Kernel Thread Stats ===\n");
    out.push_str(&format!("Active: {}  Created: {}  Exited: {}  Ops: {}\n\n", threads, created, exited, ops));
    for t in super::kthread::list() {
        out.push_str(&format!("  [{}] {} cpu={} state={} cpu_time={}ns wakeups={}\n",
            t.id, t.name, t.cpu, t.state.label(), t.cpu_time_ns, t.wakeups));
    }
    out.into_bytes()
}

fn gen_mmapstat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (procs, maps, unmaps, protects, bytes, ops) = super::mmapstat::stats();
    out.push_str("=== Mmap Stats ===\n");
    out.push_str(&format!("Processes: {}  Maps: {}  Unmaps: {}  Protects: {}  Bytes: {}  Ops: {}\n\n", procs, maps, unmaps, protects, bytes, ops));
    out.push_str("Type breakdown:\n");
    for (mt, count) in &super::mmapstat::type_breakdown() {
        out.push_str(&format!("  {:<12} {}\n", mt.label(), count));
    }
    out.push_str("\nPer-process:\n");
    for p in super::mmapstat::per_process() {
        out.push_str(&format!("  pid={:<6} {:<12} regions={} bytes={} maps={} unmaps={} protects={}\n",
            p.pid, p.name, p.regions, p.total_bytes, p.maps, p.unmaps, p.protects));
    }
    out.into_bytes()
}

fn gen_rqstat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (cpus, enqueues, dequeues, balances, ops) = super::rqstat::stats();
    out.push_str("=== Runqueue Stats ===\n");
    out.push_str(&format!("CPUs: {}  Enqueues: {}  Dequeues: {}  Balances: {}  Ops: {}\n\n", cpus, enqueues, dequeues, balances, ops));
    for c in super::rqstat::per_cpu() {
        let avg_wait = if c.dequeues > 0 { c.total_wait_ns / c.dequeues } else { 0 };
        out.push_str(&format!("  CPU {} depth={}/{} enq={} deq={} pull={} push={} avg_wait={}ns\n",
            c.cpu_id, c.current_depth, c.max_depth, c.enqueues, c.dequeues,
            c.balance_pulls, c.balance_pushes, avg_wait));
    }
    out.into_bytes()
}

fn gen_thpstat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (promos, demos, scans, collapses, ops) = super::thpstat::stats();
    out.push_str("=== Transparent Huge Pages Stats ===\n");
    out.push_str(&format!("Promotions: {}  Demotions: {}  Khugepaged scans: {}  Collapses: {}  Ops: {}\n\n", promos, demos, scans, collapses, ops));
    out.push_str("Per-size:\n");
    for s in super::thpstat::per_size() {
        let success_rate = if s.alloc_attempts > 0 { s.promotions * 10000 / s.alloc_attempts } else { 0 };
        out.push_str(&format!("  {:<6} promo={} demo={} split={} attempts={} failures={} success={}.{}%\n",
            s.size.label(), s.promotions, s.demotions, s.splits,
            s.alloc_attempts, s.alloc_failures, success_rate / 100, success_rate % 100));
    }
    let (cs, cf, cd, ck) = super::thpstat::compaction_stats();
    out.push_str(&format!("\nCompaction: success={} failed={} deferred={} skipped={}\n", cs, cf, cd, ck));
    out.into_bytes()
}

fn gen_cgiostat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (cgs, rbytes, wbytes, throttles, ops) = super::cgiostat::stats();
    out.push_str("=== Cgroup I/O Stats ===\n");
    out.push_str(&format!("Cgroups: {}  Read: {} bytes  Write: {} bytes  Throttles: {}  Ops: {}\n\n", cgs, rbytes, wbytes, throttles, ops));
    for cg in super::cgiostat::per_cgroup() {
        let bw = if cg.bw_limit_bps > 0 { alloc::format!("{}B/s", cg.bw_limit_bps) } else { String::from("unlimited") };
        out.push_str(&format!("  [{}] {:<16} R: {} bytes ({} ios)  W: {} bytes ({} ios)  throttle={} bw={}\n",
            cg.cg_id, cg.name, cg.read_bytes, cg.read_ios, cg.write_bytes, cg.write_ios, cg.throttle_count, bw));
    }
    out.into_bytes()
}

fn gen_bpfstat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (progs, maps, loaded, runs, verr, ops) = super::bpfstat::stats();
    out.push_str("=== BPF Stats ===\n");
    out.push_str(&format!("Programs: {}  Maps: {}  Total loaded: {}  Runs: {}  Verifier errors: {}  Ops: {}\n\n", progs, maps, loaded, runs, verr, ops));
    out.push_str("Programs:\n");
    for p in super::bpfstat::list_programs() {
        let avg_ns = if p.run_count > 0 { p.run_time_ns / p.run_count } else { 0 };
        out.push_str(&format!("  [{}] {:<20} type={:<14} insns={} runs={} avg={}ns maps={}\n",
            p.id, p.name, p.prog_type.label(), p.insn_count, p.run_count, avg_ns, p.map_count));
    }
    out.push_str("\nMaps:\n");
    for m in super::bpfstat::list_maps() {
        out.push_str(&format!("  [{}] {:<24} entries={}/{} key={}B val={}B\n",
            m.id, m.name, m.used_entries, m.max_entries, m.key_size, m.value_size));
    }
    out.into_bytes()
}

fn gen_pgtable() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (pages, walks, flushes, avg, ops) = super::pgtable::stats();
    out.push_str("=== Page Table Stats ===\n");
    out.push_str(&format!("Pages used: {}  Walks: {}  TLB flushes: {}  Avg depth: {}.{}  Ops: {}\n\n", pages, walks, flushes, avg / 100, avg % 100, ops));
    out.push_str("Per-level:\n");
    for l in super::pgtable::per_level() {
        out.push_str(&format!("  {:<6} alloc={} freed={} active={}\n",
            l.level.label(), l.allocated, l.freed, l.active));
    }
    let (fs, fr, ff, fg) = super::pgtable::flush_stats();
    out.push_str(&format!("\nTLB flushes: single={} range={} full={} global={}\n", fs, fr, ff, fg));
    out.into_bytes()
}

fn gen_zramstat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (devs, orig, compr, reads, writes, ops) = super::zramstat::stats();
    let ratio = if compr > 0 { orig * 100 / compr } else { 0 };
    out.push_str("=== ZRAM Stats ===\n");
    out.push_str(&format!("Devices: {}  Orig: {}  Compr: {}  Ratio: {}.{}x  Reads: {}  Writes: {}  Ops: {}\n\n",
        devs, orig, compr, ratio / 100, ratio % 100, reads, writes, ops));
    for d in super::zramstat::per_device() {
        let r = if d.compr_data_size > 0 { d.orig_data_size * 100 / d.compr_data_size } else { 0 };
        out.push_str(&format!("  {} disk={}  orig={}  compr={}  ratio={}.{}x  mem={}  R={} W={} D={}\n",
            d.name, d.disk_size, d.orig_data_size, d.compr_data_size,
            r / 100, r % 100, d.mem_used, d.reads, d.writes, d.discards));
    }
    out.into_bytes()
}

fn gen_ksmstat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (shared, sharing, merges, unmerges, saved, ops) = super::ksmstat::stats();
    let (scans, pages_scanned) = super::ksmstat::scan_stats();
    out.push_str("=== KSM Stats ===\n");
    out.push_str(&format!("Shared: {}  Sharing: {}  Merges: {}  Unmerges: {}  Saved: {} bytes  Ops: {}\n", shared, sharing, merges, unmerges, saved, ops));
    out.push_str(&format!("Scans: {}  Pages scanned: {}\n\n", scans, pages_scanned));
    out.push_str("Per-process:\n");
    for p in super::ksmstat::per_process() {
        out.push_str(&format!("  pid={:<6} {:<12} shared={} unshared={} volatile={}\n",
            p.pid, p.name, p.shared_pages, p.unshared_pages, p.volatile_pages));
    }
    out.into_bytes()
}

fn gen_clocksrc() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (sources, reads, skews, ops) = super::clocksrc::stats();
    out.push_str("=== Clock Source Stats ===\n");
    out.push_str(&format!("Sources: {}  Reads: {}  Skew corrections: {}  Ops: {}\n\n", sources, reads, skews, ops));
    for s in super::clocksrc::list() {
        let cur = if s.is_current { " [CURRENT]" } else { "" };
        let _avg_skew = if s.skew_corrections > 0 { s.total_skew_ns / s.skew_corrections } else { 0 };
        out.push_str(&format!("  [{}] {:<10} {}Hz rating={} reads={} skew_corr={} max_skew={}ns latency={}ns{}\n",
            s.id, s.name, s.freq_hz, s.rating.label(), s.reads, s.skew_corrections, s.max_skew_ns, s.read_latency_ns, cur));
    }
    out.into_bytes()
}

fn gen_pmcstat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (cpus, samples, mx, ipc, ops) = super::pmcstat::stats();
    let cmr = super::pmcstat::cache_miss_rate_x10000();
    out.push_str("=== PMC Stats ===\n");
    out.push_str(&format!("CPUs: {}  Samples: {}  Multiplex: {}  IPC: {}.{}  Cache miss: {}.{}%  Ops: {}\n\n",
        cpus, samples, mx, ipc / 100, ipc % 100, cmr / 100, cmr % 100, ops));
    let events = ["cycles", "insns", "cache-miss", "cache-ref", "br-miss", "br-insn", "bus-cyc", "stall-fe"];
    for c in super::pmcstat::per_cpu() {
        out.push_str(&format!("  CPU {}:", c.cpu_id));
        for (i, label) in events.iter().enumerate() {
            out.push_str(&format!(" {}={}", label, c.counters[i]));
        }
        out.push_str(&format!(" samples={}\n", c.samples));
    }
    out.into_bytes()
}

fn gen_cputhr() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (cpus, events, ms, caps, ops) = super::cputhr::stats();
    out.push_str("=== CPU Thermal Throttle Stats ===\n");
    out.push_str(&format!("CPUs: {}  Throttle events: {}  Total: {}ms  Freq caps: {}  Ops: {}\n\n", cpus, events, ms, caps, ops));
    for c in super::cputhr::per_cpu() {
        let temp = c.temp_mc / 1000;
        let temp_frac = (c.temp_mc % 1000) / 100;
        let cap = if c.freq_cap_mhz > 0 { alloc::format!("{}MHz", c.freq_cap_mhz) } else { String::from("none") };
        let throttled = if c.is_throttled { " [THROTTLED]" } else { "" };
        out.push_str(&format!("  CPU {} pkg={} temp={}.{}°C throttle={} total={}ms max={}ms cap={}{}\n",
            c.cpu_id, c.package_id, temp, temp_frac, c.throttle_count, c.total_throttle_ms, c.max_throttle_ms, cap, throttled));
    }
    out.into_bytes()
}

fn gen_ipcns() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (nss, shm, sem, msg, ops) = super::ipcns::stats();
    out.push_str("=== IPC Namespace Stats ===\n");
    out.push_str(&format!("Namespaces: {}  SHM: {}  SEM: {}  MSG: {}  Ops: {}\n\n", nss, shm, sem, msg, ops));
    for ns in super::ipcns::ns_list() {
        out.push_str(&format!("  [{}] {:<16} shm={}({} bytes) sem={}({}) msg={}({} bytes)\n",
            ns.ns_id, ns.name, ns.shm_segments, ns.shm_bytes, ns.sem_sets, ns.sem_total, ns.msg_queues, ns.msg_bytes));
    }
    out.into_bytes()
}

fn gen_netqueue() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (queues, rx, tx, napi, drops, ops) = super::netqueue::stats();
    out.push_str("=== Network Queue Stats ===\n");
    out.push_str(&format!("Queues: {}  RX: {} pkts  TX: {} pkts  NAPI: {}  Drops: {}  Ops: {}\n\n", queues, rx, tx, napi, drops, ops));
    for q in super::netqueue::per_queue() {
        out.push_str(&format!("  {} q{} {:<3} pkts={} bytes={} drops={} napi={} budget_ex={} backlog={}\n",
            q.iface, q.queue_id, q.direction.label(), q.packets, q.bytes, q.drops, q.napi_polls, q.napi_budget_exhausted, q.backlog_len));
    }
    out.into_bytes()
}

fn gen_secmod() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (mods, checks, denials, audits, ops) = super::secmod::stats();
    let deny_rate = if checks > 0 { denials * 10000 / checks } else { 0 };
    out.push_str("=== Security Module Stats ===\n");
    out.push_str(&format!("Modules: {}  Checks: {}  Denials: {} ({}.{}%)  Audits: {}  Ops: {}\n\n",
        mods, checks, denials, deny_rate / 100, deny_rate % 100, audits, ops));
    let hooks = ["file_open", "file_perm", "inode_cr", "inode_rm", "task_al", "task_kl", "sock_cr", "sock_cn"];
    for m in super::secmod::per_module() {
        let status = if m.enabled { "on" } else { "off" };
        out.push_str(&format!("  {} [{}] checks={} denials={} audits={}\n", m.name, status, m.total_checks, m.total_denials, m.audit_events));
        for (i, h) in hooks.iter().enumerate() {
            if m.checks[i] > 0 {
                out.push_str(&format!("    {:<10} checks={} denials={}\n", h, m.checks[i], m.denials[i]));
            }
        }
    }
    out.into_bytes()
}

fn gen_vmballoon() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (cur, target, inf, def, oom, ops) = super::vmballoon::stats();
    out.push_str("=== VM Balloon Stats ===\n");
    out.push_str(&format!("Current: {} pages  Target: {} pages  Inflates: {}  Deflates: {}  OOM: {}  Ops: {}\n",
        cur, target, inf, def, oom, ops));
    if let Some(s) = super::vmballoon::status() {
        let cur_mb = s.current_pages * 16 / 1024;
        let max_mb = s.max_pages * 16 / 1024;
        out.push_str(&format!("Size: {} MiB / {} MiB  Hints: {}\n", cur_mb, max_mb, s.free_page_hints));
    }
    out.into_bytes()
}

fn gen_devfreq() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (devs, trans, ops) = super::devfreq::stats();
    out.push_str("=== Device Frequency Stats ===\n");
    out.push_str(&format!("Devices: {}  Transitions: {}  Ops: {}\n\n", devs, trans, ops));
    for d in super::devfreq::list() {
        out.push_str(&format!("  [{}] {:<12} {}-{} kHz  cur={} kHz  gov={}  trans={}\n",
            d.id, d.name, d.min_freq_khz, d.max_freq_khz, d.cur_freq_khz, d.governor.label(), d.transitions));
    }
    out.into_bytes()
}

fn gen_hwrng() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (generated, requested, reseeds, bits, ops) = super::hwrng::stats();
    let ps = super::hwrng::pool_status();
    let ready = if ps.ready { "YES" } else { "NO" };
    out.push_str("=== Hardware RNG Stats ===\n");
    out.push_str(&format!("Generated: {} B  Requested: {} B  Reseeds: {}  Pool: {}/{} bits  Ready: {}  Ops: {}\n\n",
        generated, requested, reseeds, bits, ps.pool_size_bits, ready, ops));
    out.push_str("Sources:\n");
    for (src, bytes, failures) in super::hwrng::source_breakdown() {
        out.push_str(&format!("  {:<12} {} bytes  {} failures\n", src.label(), bytes, failures));
    }
    out.into_bytes()
}

fn gen_acpistat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (events, gpes, suspends, resumes, ops) = super::acpistat::stats();
    out.push_str("=== ACPI Stats ===\n");
    out.push_str(&format!("Events: {}  GPEs: {}  Suspends: {}  Resumes: {}  Ops: {}\n\n", events, gpes, suspends, resumes, ops));
    out.push_str("Event types:\n");
    for (ev, count) in &super::acpistat::event_counts() {
        out.push_str(&format!("  {:<14} {}\n", ev.label(), count));
    }
    out.push_str("\nGPEs:\n");
    for g in super::acpistat::gpe_list() {
        let en = if g.enabled { "on" } else { "off" };
        out.push_str(&format!("  GPE 0x{:02x}  count={}  [{}]\n", g.gpe_num, g.count, en));
    }
    out.into_bytes()
}

fn gen_userfault() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (handlers, faults, resolves, copies, zeros, ops) = super::userfault::stats();
    out.push_str("=== Userfaultfd Stats ===\n");
    out.push_str(&format!("Handlers: {}  Faults: {}  Resolves: {}  Copies: {}  Zeros: {}  Ops: {}\n\n",
        handlers, faults, resolves, copies, zeros, ops));
    out.push_str("Per-process:\n");
    for h in super::userfault::per_process() {
        let avg_ns = if h.resolves > 0 { h.total_resolve_ns / h.resolves } else { 0 };
        out.push_str(&format!("  PID {:>5}  ranges={}  miss={}  wp={}  minor={}  resolves={}  avg_ns={}  max_ns={}  copy={}  zero={}\n",
            h.pid, h.registered_ranges, h.faults_missing, h.faults_wp, h.faults_minor,
            h.resolves, avg_ns, h.max_resolve_ns, h.copy_pages, h.zero_pages));
    }
    out.into_bytes()
}

fn gen_ioport() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (regions, reads, writes, ur, uw, ops) = super::ioport::stats();
    out.push_str("=== I/O Port Stats ===\n");
    out.push_str(&format!("Regions: {}  Reads: {}  Writes: {}  Untracked R: {}  Untracked W: {}  Ops: {}\n\n",
        regions, reads, writes, ur, uw, ops));
    out.push_str("Per-region:\n");
    for r in super::ioport::per_region() {
        out.push_str(&format!("  {:<6} 0x{:04x}-0x{:04x}  reads={}  writes={}  rbytes={}  wbytes={}\n",
            r.name, r.base, r.base + r.length - 1, r.reads, r.writes, r.read_bytes, r.write_bytes));
    }
    out.into_bytes()
}

fn gen_msivec() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (devs, vecs, ints, allocs, frees, ops) = super::msivec::stats();
    out.push_str("=== MSI Vector Stats ===\n");
    out.push_str(&format!("Devices: {}  Vectors: {}  Interrupts: {}  Allocs: {}  Frees: {}  Ops: {}\n\n",
        devs, vecs, ints, allocs, frees, ops));
    out.push_str("Per-device:\n");
    for d in super::msivec::per_device() {
        out.push_str(&format!("  {:<10} {:<5}  alloc={}  active={}  ints={}  cpu={}\n",
            d.device, d.msi_type.label(), d.vectors_allocated, d.vectors_active, d.interrupts, d.target_cpu));
    }
    out.into_bytes()
}

fn gen_cpuset() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (sets, assignments, affinity, ops) = super::cpuset::stats();
    out.push_str("=== CPU Set Stats ===\n");
    out.push_str(&format!("Sets: {}  Assignments: {}  Affinity changes: {}  Ops: {}\n\n",
        sets, assignments, affinity, ops));
    out.push_str("CPU sets:\n");
    for s in super::cpuset::list() {
        let excl = if s.exclusive { "excl" } else { "shared" };
        out.push_str(&format!("  [{}] {:<10} cpus=0x{:x}  mem=0x{:x}  procs={}  affinity={}  [{}]\n",
            s.id, s.name, s.cpu_mask, s.mem_mask, s.processes, s.affinity_changes, excl));
    }
    out.into_bytes()
}

fn gen_ftrace() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (probes, hits, misses, overhead, ops) = super::ftrace::stats();
    let enabled = if super::ftrace::is_enabled() { "ON" } else { "OFF" };
    out.push_str("=== Function Trace Stats ===\n");
    out.push_str(&format!("Tracing: {}  Probes: {}  Hits: {}  Misses: {}  Overhead: {} ns  Ops: {}\n\n",
        enabled, probes, hits, misses, overhead, ops));
    out.push_str("Probes:\n");
    for p in super::ftrace::per_probe() {
        let avg = if p.hits > 0 { p.total_ns / p.hits } else { 0 };
        let en = if p.enabled { "on" } else { "off" };
        out.push_str(&format!("  {:<20} {:<4}  hits={}  miss={}  avg={}ns  max={}ns  [{}]\n",
            p.func_name, p.kind.label(), p.hits, p.misses, avg, p.max_ns, en));
    }
    out.into_bytes()
}

fn gen_kstack() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (cpus, overflows, guards, samples, ops) = super::kstack::stats();
    out.push_str("=== Kernel Stack Stats ===\n");
    out.push_str(&format!("CPUs: {}  Overflows: {}  Guard hits: {}  Samples: {}  Ops: {}\n\n",
        cpus, overflows, guards, samples, ops));
    out.push_str("Per-CPU:\n");
    for c in super::kstack::per_cpu() {
        let avg = if c.samples > 0 { c.total_used_samples / c.samples } else { 0 };
        let pct = if c.stack_size > 0 { (c.high_water as u64) * 100 / (c.stack_size as u64) } else { 0 };
        out.push_str(&format!("  CPU {:>2}  size={}  cur={}  hwm={}  ({}%)  avg={}  overflows={}  guards={}\n",
            c.cpu_id, c.stack_size, c.current_used, c.high_water, pct, avg, c.overflows, c.guard_hits));
    }
    out.into_bytes()
}

fn gen_fnotify() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (watches, events, overflows, ops) = super::fnotify::stats();
    out.push_str("=== File Notification Stats ===\n");
    out.push_str(&format!("Watches: {}  Events: {}  Overflows: {}  Ops: {}\n\n", watches, events, overflows, ops));
    for t in &super::fnotify::per_type() {
        out.push_str(&format!("{}: watches={}/{}  events={}  overflows={}  queue={}/{}\n",
            t.notify_type.label(), t.watches, t.max_watches, t.events, t.overflows,
            t.queue_depth, t.max_queue_depth));
    }
    out.into_bytes()
}

fn gen_netlat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (ifaces, rtt, proc_s, ops) = super::netlat::stats();
    out.push_str("=== Network Latency Stats ===\n");
    out.push_str(&format!("Interfaces: {}  RTT samples: {}  Processing samples: {}  Ops: {}\n\n",
        ifaces, rtt, proc_s, ops));
    let labels = super::netlat::bucket_labels();
    for i in super::netlat::per_interface() {
        let avg_rtt = if i.rtt_samples > 0 { i.rtt_total_ns / i.rtt_samples } else { 0 };
        let avg_proc = if i.proc_samples > 0 { i.proc_total_ns / i.proc_samples } else { 0 };
        out.push_str(&format!("{}:  rtt: avg={}ns  min={}ns  max={}ns  ({} samples)\n",
            i.name, avg_rtt, i.rtt_min_ns, i.rtt_max_ns, i.rtt_samples));
        out.push_str(&format!("      proc: avg={}ns  max={}ns  ({} samples)\n",
            avg_proc, i.proc_max_ns, i.proc_samples));
        out.push_str("      histogram:");
        for (j, &count) in i.rtt_histogram.iter().enumerate() {
            out.push_str(&format!(" {}={}", labels[j], count));
        }
        out.push('\n');
    }
    out.into_bytes()
}

fn gen_diskstat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (devs, reads, writes, rb, wb, ops) = super::diskstat::stats();
    out.push_str("=== Disk Stats ===\n");
    out.push_str(&format!("Devices: {}  Reads: {}  Writes: {}  ReadBytes: {}  WriteBytes: {}  Ops: {}\n\n",
        devs, reads, writes, rb, wb, ops));
    out.push_str("Per-device:\n");
    for d in super::diskstat::per_device() {
        let avg_r = if d.reads > 0 { d.read_ns / d.reads } else { 0 };
        let avg_w = if d.writes > 0 { d.write_ns / d.writes } else { 0 };
        out.push_str(&format!("  {:<10} R: {}({} B, avg {}ns)  W: {}({} B, avg {}ns)  disc={}  flush={}  merges: r={} w={}\n",
            d.name, d.reads, d.read_bytes, avg_r, d.writes, d.write_bytes, avg_w,
            d.discards, d.flushes, d.merges_read, d.merges_write));
    }
    out.into_bytes()
}

fn gen_taskio() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (tasks, rb, wb, cancelled, io_wait, ops) = super::taskio::stats();
    out.push_str("=== Per-Task I/O Stats ===\n");
    out.push_str(&format!("Tasks: {}  ReadBytes: {}  WriteBytes: {}  Cancelled: {}  IOWait: {} ns  Ops: {}\n\n",
        tasks, rb, wb, cancelled, io_wait, ops));
    out.push_str("Per-task:\n");
    for t in super::taskio::per_task() {
        out.push_str(&format!("  PID {:>5}  read={}({} calls)  write={}({} calls)  cancel={}  iowait={}ns  majflt={}\n",
            t.pid, t.read_bytes, t.read_syscalls, t.write_bytes, t.write_syscalls,
            t.cancelled_write_bytes, t.io_wait_ns, t.page_faults_io));
    }
    out.into_bytes()
}

fn gen_ttystat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (ttys, rb, wb, sigs, overruns, ops) = super::ttystat::stats();
    out.push_str("=== TTY Stats ===\n");
    out.push_str(&format!("TTYs: {}  ReadBytes: {}  WriteBytes: {}  Signals: {}  Overruns: {}  Ops: {}\n\n",
        ttys, rb, wb, sigs, overruns, ops));
    out.push_str("Per-TTY:\n");
    for t in super::ttystat::per_tty() {
        let pct = if t.buf_size > 0 { (t.buf_used as u64) * 100 / (t.buf_size as u64) } else { 0 };
        out.push_str(&format!("  {:<10} [{}]  read={}({} ops)  write={}({} ops)  sigs={}  overruns={}  buf={}/{}({}%)\n",
            t.name, t.tty_type.label(), t.read_bytes, t.read_ops, t.write_bytes, t.write_ops,
            t.signals_sent, t.overruns, t.buf_used, t.buf_size, pct));
    }
    out.into_bytes()
}

fn gen_swapact() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (areas, si, so, sip, sop, ops) = super::swapact::stats();
    out.push_str("=== Swap Activity Stats ===\n");
    out.push_str(&format!("Areas: {}  SwapIn: {}  SwapOut: {}  InPages: {}  OutPages: {}  Ops: {}\n\n",
        areas, si, so, sip, sop, ops));
    out.push_str("Per-area:\n");
    for a in super::swapact::per_area() {
        let pct = if a.total_pages > 0 { a.used_pages * 100 / a.total_pages } else { 0 };
        let avg_in = if a.swap_in_count > 0 { a.swap_in_ns / a.swap_in_count } else { 0 };
        let avg_out = if a.swap_out_count > 0 { a.swap_out_ns / a.swap_out_count } else { 0 };
        out.push_str(&format!("  {:<15} [{}] prio={}  used={}/{}({}%)  in: {}({} pg, avg {}ns)  out: {}({} pg, avg {}ns)\n",
            a.name, a.swap_type.label(), a.priority, a.used_pages, a.total_pages, pct,
            a.swap_in_count, a.swap_in_pages, avg_in,
            a.swap_out_count, a.swap_out_pages, avg_out));
    }
    out.into_bytes()
}

fn gen_schedwait() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (waits, ns, ops) = super::schedwait::stats();
    let avg = if waits > 0 { ns / waits } else { 0 };
    out.push_str("=== Scheduler Wait Stats ===\n");
    out.push_str(&format!("Total waits: {}  Total ns: {}  Avg ns: {}  Ops: {}\n\n", waits, ns, avg, ops));
    out.push_str("Per-reason:\n");
    for (reason, count, total, max) in super::schedwait::per_reason() {
        let avg_r = if count > 0 { total / count } else { 0 };
        out.push_str(&format!("  {:<10} count={}  total={}ns  avg={}ns  max={}ns\n",
            reason.label(), count, total, avg_r, max));
    }
    let (labels, counts) = super::schedwait::histogram();
    out.push_str("\nHistogram:");
    for (i, &count) in counts.iter().enumerate() {
        out.push_str(&format!(" {}={}", labels[i], count));
    }
    out.push('\n');
    out.into_bytes()
}

fn gen_ratestat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (limiters, allows, denies, bursts, ops) = super::ratestat::stats();
    out.push_str("=== Rate Limiter Stats ===\n");
    out.push_str(&format!("Limiters: {}  Allows: {}  Denies: {}  Bursts: {}  Ops: {}\n\n",
        limiters, allows, denies, bursts, ops));
    out.push_str("Per-limiter:\n");
    for l in super::ratestat::per_limiter() {
        let deny_pct = if l.allows + l.denies > 0 { l.denies * 10000 / (l.allows + l.denies) } else { 0 };
        out.push_str(&format!("  {:<15} rate={}/s  burst={}  tokens={}  allow={}  deny={}  ({}.{}%)  bursts={}\n",
            l.name, l.rate_per_sec, l.burst_size, l.current_tokens,
            l.allows, l.denies, deny_pct / 100, deny_pct % 100, l.burst_events));
    }
    out.into_bytes()
}

fn gen_iomem() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (regs, reads, writes, ops) = super::iomem::stats();
    out.push_str("=== I/O Memory Stats ===\n");
    out.push_str(&format!("Regions: {}  Reads: {}  Writes: {}  Ops: {}\n\n", regs, reads, writes, ops));
    out.push_str("Regions:\n");
    for r in super::iomem::regions() {
        let cache = if r.cacheable { "C" } else { "UC" };
        let pf = if r.prefetchable { "+PF" } else { "" };
        out.push_str(&format!("  {:<12} 0x{:016x}-0x{:016x} ({} B)  reads={}  writes={}  [{}{}]\n",
            r.name, r.base, r.base + r.size - 1, r.size, r.reads, r.writes, cache, pf));
    }
    out.into_bytes()
}

fn gen_vmzone() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (zones, allocs, frees, reclaims, ops) = super::vmzone::stats();
    out.push_str("=== VM Zone Stats ===\n");
    out.push_str(&format!("Zones: {}  Allocs: {}  Frees: {}  Reclaims: {}  Ops: {}\n\n",
        zones, allocs, frees, reclaims, ops));
    out.push_str("Per-zone:\n");
    for z in super::vmzone::per_zone() {
        let pct = if z.total_pages > 0 { z.free_pages * 100 / z.total_pages } else { 0 };
        out.push_str(&format!("  {:<10} [{}]  total={}  free={}({}%)  active={}  inactive={}  wmark: {}/{}/{}  alloc={}  free={}  reclaim={}\n",
            z.name, z.zone_type.label(), z.total_pages, z.free_pages, pct,
            z.active_pages, z.inactive_pages, z.wmark_min, z.wmark_low, z.wmark_high,
            z.allocs, z.frees, z.reclaim_count));
    }
    out.into_bytes()
}

fn gen_budstat() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (zones, splits, coalesces, ops) = super::budstat::stats();
    out.push_str("=== Buddy Allocator Stats ===\n");
    out.push_str(&format!("Zones: {}  Splits: {}  Coalesces: {}  Ops: {}\n\n", zones, splits, coalesces, ops));
    for z in super::budstat::per_zone() {
        out.push_str(&format!("{}:\n  Free:  ", z.zone_name));
        for (i, &c) in z.free_counts.iter().enumerate() { out.push_str(&format!(" O{}={}", i, c)); }
        out.push_str("\n  Split: ");
        for (i, &s) in z.splits.iter().enumerate() { out.push_str(&format!(" O{}={}", i, s)); }
        out.push_str("\n  Coal:  ");
        for (i, &c) in z.coalesces.iter().enumerate() { out.push_str(&format!(" O{}={}", i, c)); }
        out.push('\n');
    }
    out.into_bytes()
}

fn gen_cgmem() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (cgroups, charges, uncharges, ooms, ops) = super::cgmem::stats();
    out.push_str("=== Cgroup Memory Stats ===\n");
    out.push_str(&format!("Cgroups: {}  Charges: {}  Uncharges: {}  OOM kills: {}  Ops: {}\n\n",
        cgroups, charges, uncharges, ooms, ops));
    for c in super::cgmem::per_cgroup() {
        let pct = if c.limit_pages < u64::MAX && c.limit_pages > 0 { c.usage_pages * 100 / c.limit_pages } else { 0 };
        let limit = if c.limit_pages == u64::MAX { "unlimited".into() } else { format!("{}", c.limit_pages) };
        out.push_str(&format!("  [{}] {:<10} usage={}/{}({}%)  rss={}  cache={}  swap={}  charges={}  oom={}\n",
            c.cg_id, c.name, c.usage_pages, limit, pct, c.rss_pages, c.cache_pages,
            c.swap_pages, c.charges, c.oom_kills));
    }
    out.into_bytes()
}

fn gen_vmfrag() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (zones, compactions, success, fail, ops) = super::vmfrag::stats();
    let rate = if compactions > 0 { success * 100 / compactions } else { 0 };
    out.push_str("=== VM Fragmentation Index ===\n");
    out.push_str(&format!("Zones: {}  Compactions: {}  Success: {}({}%)  Fail: {}  Ops: {}\n\n",
        zones, compactions, success, rate, fail, ops));
    for z in super::vmfrag::per_zone() {
        let rate_z = if z.compactions > 0 { z.compact_success * 100 / z.compactions } else { 0 };
        out.push_str(&format!("{}:  compact: {}/{}({}% ok)\n  Index:", z.zone_name, z.compact_success, z.compactions, rate_z));
        for (i, &idx) in z.frag_index.iter().enumerate() {
            out.push_str(&format!(" O{}={}.{}", i, idx / 10, idx % 10));
        }
        out.push('\n');
    }
    out.into_bytes()
}

fn gen_pidfd() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();
    let (pids, creates, polls, signals, waits, closes, ops) = super::pidfd::stats();
    out.push_str("=== Pidfd Stats ===\n");
    out.push_str(&format!("Tracked PIDs: {}  Creates: {}  Polls: {}  Signals: {}  Waits: {}  Closes: {}  Ops: {}\n\n",
        pids, creates, polls, signals, waits, closes, ops));
    out.push_str("Per-PID:\n");
    for p in super::pidfd::per_pid() {
        out.push_str(&format!("  PID {:>5}  creates={}  polls={}  signals={}  waits={}  closes={}\n",
            p.pid, p.creates, p.polls, p.signals, p.waits, p.close_count));
    }
    out.into_bytes()
}

fn gen_columnview() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (col_count, pref_count, compute_count) = super::columnview::stats();

    out.push_str("Column View\n");
    out.push_str("===========\n\n");
    out.push_str(&format!("Columns:    {}/{}\n", col_count, 512));
    out.push_str(&format!("User prefs: {}/{}\n", pref_count, 256));
    out.push_str(&format!("Computes:   {}\n\n", compute_count));

    let cols = super::columnview::list_columns();
    if !cols.is_empty() {
        out.push_str(&format!("{:24} {:16} {:8} {:6} {}\n",
            "ID", "HEADER", "TYPE", "WIDTH", "APPLIES TO"));
        for c in cols.iter().take(30) {
            let type_str = match c.col_type {
                super::columnview::ColumnType::Text => "text",
                super::columnview::ColumnType::Integer => "int",
                super::columnview::ColumnType::Size => "size",
                super::columnview::ColumnType::DateTime => "date",
                super::columnview::ColumnType::Duration => "dur",
                super::columnview::ColumnType::Boolean => "bool",
                super::columnview::ColumnType::Dimensions => "dim",
            };
            let applies = if c.applies_to.is_empty() {
                String::from("*")
            } else {
                format!("{}", c.applies_to.len())
            };
            out.push_str(&format!("{:24} {:16} {:8} {:6} {}\n",
                c.id, c.header, type_str, c.default_width, applies));
        }
    }

    out.into_bytes()
}

fn gen_pathbar() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (nav_count, complete_count, hist_len, recent_len) = super::pathbar::stats();

    out.push_str("Path Bar / Navigation\n");
    out.push_str("=====================\n\n");
    out.push_str(&format!("Navigations:   {}\n", nav_count));
    out.push_str(&format!("Completions:   {}\n", complete_count));
    out.push_str(&format!("History:       {}/{}\n", hist_len, 256));
    out.push_str(&format!("Recent dirs:   {}/{}\n", recent_len, 32));
    out.push_str(&format!("Current:       {}\n", super::pathbar::current()));
    out.push_str(&format!("Can go back:   {}\n", super::pathbar::can_go_back()));
    out.push_str(&format!("Can go forward:{}\n", super::pathbar::can_go_forward()));

    out.into_bytes()
}

fn gen_viewstate() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (saved, templates, gets, sets) = super::viewstate::stats();

    out.push_str("View State\n");
    out.push_str("==========\n\n");
    out.push_str(&format!("Saved states: {}/{}\n", saved, 4096));
    out.push_str(&format!("Templates:    {}/{}\n", templates, 64));
    out.push_str(&format!("Lookups:      {}\n", gets));
    out.push_str(&format!("Saves:        {}\n", sets));

    out.into_bytes()
}

fn gen_contextmenu() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (builds, executes, ext_count) = super::contextmenu::stats();

    out.push_str("Context Menu\n");
    out.push_str("============\n\n");
    out.push_str(&format!("Builds:      {}\n", builds));
    out.push_str(&format!("Executions:  {}\n", executes));
    out.push_str(&format!("Extensions:  {}\n", ext_count));

    let exts = super::contextmenu::list_extensions();
    if !exts.is_empty() {
        out.push_str("\nRegistered Extensions:\n");
        for (id, name, enabled, items) in &exts {
            out.push_str(&format!("  #{}: {} ({} items) {}\n", id, name, items,
                                  if *enabled { "enabled" } else { "disabled" }));
        }
    }

    out.into_bytes()
}

fn gen_deskicons() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (loads, arranges, count) = super::deskicons::stats();

    out.push_str("Desktop Icons\n");
    out.push_str("=============\n\n");
    out.push_str(&format!("Icons:    {}\n", count));
    out.push_str(&format!("Loads:    {}\n", loads));
    out.push_str(&format!("Arranges: {}\n", arranges));

    if let Some(layout) = super::deskicons::get_layout() {
        out.push_str(&format!("Mode:     {:?}\n", layout.mode));
        out.push_str(&format!("Sort:     {:?}\n", layout.sort_by));
        out.push_str(&format!("Screen:   {}x{}\n", layout.screen_w, layout.screen_h));
    }

    out.into_bytes()
}

fn gen_fileselect() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (selects, deselects, active) = super::fileselect::stats();

    out.push_str("File Selection\n");
    out.push_str("==============\n\n");
    out.push_str(&format!("Active sets:     {}\n", active));
    out.push_str(&format!("Select ops:      {}\n", selects));
    out.push_str(&format!("Deselect ops:    {}\n", deselects));

    let sets = super::fileselect::list_sets();
    if !sets.is_empty() {
        out.push_str("\nSets:\n");
        for (id, dir, count) in &sets {
            out.push_str(&format!("  #{}: {} ({} items)\n", id, dir, count));
        }
    }

    out.into_bytes()
}

fn gen_filetype() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (lookups, registers, type_count, app_icons) = super::filetype::stats();

    out.push_str("File Types\n");
    out.push_str("==========\n\n");
    out.push_str(&format!("Types:       {}\n", type_count));
    out.push_str(&format!("App icons:   {}\n", app_icons));
    out.push_str(&format!("Lookups:     {}\n", lookups));
    out.push_str(&format!("Registers:   {}\n", registers));

    out.into_bytes()
}

fn gen_sidebar() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (builds, sections, hidden) = super::sidebar::stats();

    out.push_str("Sidebar\n");
    out.push_str("=======\n\n");
    out.push_str(&format!("Builds:   {}\n", builds));
    out.push_str(&format!("Sections: {}\n", sections));
    out.push_str(&format!("Hidden:   {}\n", hidden));

    let sidebar = super::sidebar::build();
    for section in &sidebar.sections {
        out.push_str(&format!("\n[{}] {} ({} items)\n",
                              if section.expanded { "v" } else { ">" },
                              section.label,
                              section.items.len()));
    }

    out.into_bytes()
}

fn gen_statusbar() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let gen_count = super::statusbar::stats();

    out.push_str("Status Bar\n");
    out.push_str("==========\n\n");
    out.push_str(&format!("Generates: {}\n", gen_count));

    out.into_bytes()
}

fn gen_openwith() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (opens, defaults, recent, apps) = super::openwith::stats();

    out.push_str("Open With\n");
    out.push_str("=========\n\n");
    out.push_str(&format!("Opens:           {}\n", opens));
    out.push_str(&format!("Default changes: {}\n", defaults));
    out.push_str(&format!("Recent entries:  {}\n", recent));
    out.push_str(&format!("Known apps:      {}\n", apps));

    out.into_bytes()
}

fn gen_properties() -> Vec<u8> {
    use alloc::format;
    let mut out = String::new();

    let (gathers, checksums) = super::properties::stats();

    out.push_str("File Properties\n");
    out.push_str("===============\n\n");
    out.push_str(&format!("Gather calls: {}\n", gathers));
    out.push_str(&format!("Checksums:    {}\n", checksums));

    out.into_bytes()
}

/// Check if a task ID currently exists in the scheduler.
///
/// Uses the scheduler's cheap map lookup rather than building the full task
/// list (which would allocate and scan every task's stack) just to test for
/// membership.
fn task_exists(task_id: u64) -> bool {
    crate::sched::task_exists(task_id)
}

// ---------------------------------------------------------------------------
// Path resolution helpers
// ---------------------------------------------------------------------------

/// Strip leading "/" to get the relative path within procfs.
fn strip_root(path: &str) -> &str {
    path.strip_prefix('/').unwrap_or(path)
}

/// Generate content for a root-level virtual file by name.
fn generate(name: &str) -> KernelResult<Vec<u8>> {
    match name {
        "version" => Ok(gen_version()),
        "uptime" => Ok(gen_uptime()),
        "meminfo" => Ok(gen_meminfo()),
        "cpuinfo" => Ok(gen_cpuinfo()),
        "config" => Ok(gen_config()),
        "mounts" => Ok(gen_mounts()),
        "stat" => Ok(gen_stat()),
        "filesystems" => Ok(gen_filesystems()),
        "cmdline" => Ok(gen_cmdline()),
        "loadavg" => Ok(gen_loadavg()),
        "cacheinfo" => Ok(gen_cacheinfo()),
        "locks" => Ok(gen_locks()),
        "fdinfo" => Ok(gen_fdinfo()),
        "diskstats" => Ok(gen_diskstats()),
        "partitions" => Ok(gen_partitions()),
        "interrupts" => Ok(gen_interrupts()),
        "devices" => Ok(gen_devices()),
        "net" => Ok(gen_net()),
        "vmstat" => Ok(gen_vmstat()),
        "buddyinfo" => Ok(gen_buddyinfo()),
        "swaps" => Ok(gen_swaps()),
        "fsstats" => Ok(gen_fsstats()),
        "heapinfo" => Ok(gen_heapinfo()),
        "bcache" => Ok(gen_bcache()),
        "cas" => Ok(gen_cas()),
        "integrity" => Ok(gen_integrity()),
        "fhistory" => Ok(gen_fhistory()),
        "quotas" => Ok(gen_quotas()),
        "security" => Ok(gen_security()),
        "pipes" => Ok(gen_pipes()),
        "overlays" => Ok(gen_overlays()),
        "namespaces" => Ok(gen_namespaces()),
        "rlimits" => Ok(gen_rlimits()),
        "audit" => Ok(gen_audit()),
        "snapshots" => Ok(gen_snapshots()),
        "reclaim" => Ok(gen_reclaim()),
        "transactions" => Ok(gen_transactions()),
        "certmgr" => Ok(gen_certmgr()),
        "installer" => Ok(gen_installer()),
        "changetrack" => Ok(gen_changetrack()),
        "fcompress" => Ok(gen_fcompress()),
        "encryption" => Ok(gen_encryption()),
        "dedup" => Ok(gen_dedup()),
        "search" => Ok(gen_search()),
        "tags" => Ok(gen_tags()),
        "usage" => Ok(gen_usage()),
        "health" => Ok(gen_health()),
        "dirsync" => Ok(gen_dirsync()),
        "backup" => Ok(gen_backup()),
        "undelete" => Ok(gen_undelete()),
        "archives" => Ok(gen_archives()),
        "batch" => Ok(gen_batch()),
        "linkcheck" => Ok(gen_linkcheck()),
        "profile" => Ok(gen_profile()),
        "fspolicy" => Ok(gen_fspolicy()),
        "fsbench" => Ok(gen_fsbench()),
        "ioprio" => Ok(gen_ioprio()),
        "atime" => Ok(gen_atime()),
        "prefetch" => Ok(gen_prefetch()),
        "splice" => Ok(gen_splice()),
        "directio" => Ok(gen_directio()),
        "fstrim" => Ok(gen_fstrim()),
        "fstune" => Ok(gen_fstune()),
        "fontmgr" => Ok(gen_fontmgr()),
        "sparse" => Ok(gen_sparse()),
        "readdir_plus" => Ok(gen_readdir_plus()),
        "freeze" => Ok(gen_freeze()),
        "sealing" => Ok(gen_sealing()),
        "recent" => Ok(gen_recent()),
        "fileinfo" => Ok(gen_fileinfo()),
        "fswalk" => Ok(gen_fswalk()),
        "findex" => Ok(gen_findex()),
        "thumbcache" => Ok(gen_thumbcache()),
        "bookmarks" => Ok(gen_bookmarks()),
        "clipboard" => Ok(gen_clipboard()),
        "dragdrop" => Ok(gen_dragdrop()),
        "contextmenu" => Ok(gen_contextmenu()),
        "deskicons" => Ok(gen_deskicons()),
        "fileops" => Ok(gen_fileops()),
        "fileselect" => Ok(gen_fileselect()),
        "filetype" => Ok(gen_filetype()),
        "openwith" => Ok(gen_openwith()),
        "preview" => Ok(gen_preview()),
        "sidebar" => Ok(gen_sidebar()),
        "statusbar" => Ok(gen_statusbar()),
        "templates" => Ok(gen_templates()),
        "toolbar" => Ok(gen_toolbar()),
        "queryable" => Ok(gen_queryable()),
        "immutable" => Ok(gen_immutable()),
        "fcomment" => Ok(gen_fcomment()),
        "rundialog" => Ok(gen_rundialog()),
        "notifcenter" => Ok(gen_notifcenter()),
        "appregistry" => Ok(gen_appregistry()),
        "systray" => Ok(gen_systray()),
        "taskbar" => Ok(gen_taskbar()),
        "startmenu" => Ok(gen_startmenu()),
        "filepicker" => Ok(gen_filepicker()),
        "theme" => Ok(gen_theme()),
        "hotkeys" => Ok(gen_hotkeys()),
        "widgets" => Ok(gen_widgets()),
        "soundmixer" => Ok(gen_soundmixer()),
        "wallpaper" => Ok(gen_wallpaper()),
        "credentials" => Ok(gen_credentials()),
        "power" => Ok(gen_power()),
        "display" => Ok(gen_display()),
        "vdesktop" => Ok(gen_vdesktop()),
        "keylayout" => Ok(gen_keylayout()),
        "screenshot" => Ok(gen_screenshot()),
        "a11y" => Ok(gen_a11y()),
        "ime" => Ok(gen_ime()),
        "netindicator" => Ok(gen_netindicator()),
        "winsnap" => Ok(gen_winsnap()),
        "colorpicker" => Ok(gen_colorpicker()),
        "cursorsettings" => Ok(gen_cursorsettings()),
        "kbsettings" => Ok(gen_kbsettings()),
        "detailcols" => Ok(gen_detailcols()),
        "partmgr" => Ok(gen_partmgr()),
        "locale" => Ok(gen_locale()),
        "useracct" => Ok(gen_useracct()),
        "progmgr" => Ok(gen_progmgr()),
        "scriptlang" => Ok(gen_scriptlang()),
        "osreset" => Ok(gen_osreset()),
        "bootcfg" => Ok(gen_bootcfg()),
        "swapcfg" => Ok(gen_swapcfg()),
        "timezone" => Ok(gen_timezone()),
        "autostart" => Ok(gen_autostart()),
        "schedtune" => Ok(gen_schedtune()),
        "mmtune" => Ok(gen_mmtune()),
        "capsettings" => Ok(gen_capsettings()),
        "vpn" => Ok(gen_vpn()),
        "dyndns" => Ok(gen_dyndns()),
        "loginscreen" => Ok(gen_loginscreen()),
        "appnotify" => Ok(gen_appnotify()),
        "kernelbuild" => Ok(gen_kernelbuild()),
        "wakesensor" => Ok(gen_wakesensor()),
        "netsettings" => Ok(gen_netsettings()),
        "sysinfo" => Ok(gen_sysinfo()),
        "perfmon" => Ok(gen_perfmon()),
        "focusassist" => Ok(gen_focusassist()),
        "storageclean" => Ok(gen_storageclean()),
        "sysdiag" => Ok(gen_sysdiag()),
        "nightlight" => Ok(gen_nightlight()),
        "tasksched" => Ok(gen_tasksched()),
        "envvars" => Ok(gen_envvars()),
        "bluetooth" => Ok(gen_bluetooth()),
        "printmgr" => Ok(gen_printmgr()),
        "screenrec" => Ok(gen_screenrec()),
        "datausage" => Ok(gen_datausage()),
        "mousesettings" => Ok(gen_mousesettings()),
        "touchpad" => Ok(gen_touchpad()),
        "powerprofile" => Ok(gen_powerprofile()),
        "defaultapps" => Ok(gen_defaultapps()),
        "monitors" => Ok(gen_monitors()),
        "fwsettings" => Ok(gen_fwsettings()),
        "updatemgr" => Ok(gen_updatemgr()),
        "notifprefs" => Ok(gen_notifprefs()),
        "fileshare" => Ok(gen_fileshare()),
        "parental" => Ok(gen_parental()),
        "audiodevice" => Ok(gen_audiodevice()),
        "sessionmgr" => Ok(gen_sessionmgr()),
        "crashreport" => Ok(gen_crashreport()),
        "netproxy" => Ok(gen_netproxy()),
        "fileversion" => Ok(gen_fileversion()),
        "devicemgr" => Ok(gen_devicemgr()),
        "location" => Ok(gen_location()),
        "diskencrypt" => Ok(gen_diskencrypt()),
        "pkgmgr" => Ok(gen_pkgmgr()),
        "remotedesktop" => Ok(gen_remotedesktop()),
        "restorepoint" => Ok(gen_restorepoint()),
        "battery" => Ok(gen_battery()),
        "dictation" => Ok(gen_dictation()),
        "screenreader" => Ok(gen_screenreader()),
        "langpack" => Ok(gen_langpack()),
        "spellcheck" => Ok(gen_spellcheck()),
        "screentime" => Ok(gen_screentime()),
        "disksmart" => Ok(gen_disksmart()),
        "magnifier" => Ok(gen_magnifier()),
        "cloudsync" => Ok(gen_cloudsync()),
        "gestures" => Ok(gen_gestures()),
        "soundevents" => Ok(gen_soundevents()),
        "usbmgr" => Ok(gen_usbmgr()),
        "cliphistory" => Ok(gen_cliphistory()),
        "displaycolor" => Ok(gen_displaycolor()),
        "syslog" => Ok(gen_syslog()),
        "inputa11y" => Ok(gen_inputa11y()),
        "driverupdate" => Ok(gen_driverupdate()),
        "netshare" => Ok(gen_netshare()),
        "startuprepair" => Ok(gen_startuprepair()),
        "remoteassist" => Ok(gen_remoteassist()),
        "taskmon" => Ok(gen_taskmon()),
        "printqueue" => Ok(gen_printqueue()),
        "servicemgr" => Ok(gen_servicemgr()),
        "hwmonitor" => Ok(gen_hwmonitor()),
        "appsandbox" => Ok(gen_appsandbox()),
        "gamepadinput" => Ok(gen_gamepadinput()),
        "sysrestore" => Ok(gen_sysrestore()),
        "audiomux" => Ok(gen_audiomux()),
        "netthrottle" => Ok(gen_netthrottle()),
        "dumpanalyzer" => Ok(gen_dumpanalyzer()),
        "memdiag" => Ok(gen_memdiag()),
        "parentaltime" => Ok(gen_parentaltime()),
        "mediakeys" => Ok(gen_mediakeys()),
        "webcam" => Ok(gen_webcam()),
        "speechio" => Ok(gen_speechio()),
        "mobilelink" => Ok(gen_mobilelink()),
        "screenlock" => Ok(gen_screenlock()),
        "appstore" => Ok(gen_appstore()),
        "wintiling" => Ok(gen_wintiling()),
        "peninput" => Ok(gen_peninput()),
        "brightness" => Ok(gen_brightness()),
        "quicksettings" => Ok(gen_quicksettings()),
        "volumeosd" => Ok(gen_volumeosd()),
        "netdiag" => Ok(gen_netdiag()),
        "sharesheet" => Ok(gen_sharesheet()),
        "oobe" => Ok(gen_oobe()),
        "hdrdisplay" => Ok(gen_hdrdisplay()),
        "surroundsound" => Ok(gen_surroundsound()),
        "audioeq" => Ok(gen_audioeq()),
        "screensaver" => Ok(gen_screensaver()),
        "colortemp" => Ok(gen_colortemp()),
        "gamemode" => Ok(gen_gamemode()),
        "dpiscaling" => Ok(gen_dpiscaling()),
        "netprofile" => Ok(gen_netprofile()),
        "apppermissions" => Ok(gen_apppermissions()),
        "kbshortcuts" => Ok(gen_kbshortcuts()),
        "displayarrange" => Ok(gen_displayarrange()),
        "sysanimations" => Ok(gen_sysanimations()),
        "filevault" => Ok(gen_filevault()),
        "mousegestures" => Ok(gen_mousegestures()),
        "fontsettings" => Ok(gen_fontsettings()),
        "notifbadge" => Ok(gen_notifbadge()),
        "lockwallpaper" => Ok(gen_lockwallpaper()),
        "systemsounds" => Ok(gen_systemsounds()),
        "hotcorners" => Ok(gen_hotcorners()),
        "dynlock" => Ok(gen_dynlock()),
        "snaplayout" => Ok(gen_snaplayout()),
        "haptfeedback" => Ok(gen_haptfeedback()),
        "eyeprotect" => Ok(gen_eyeprotect()),
        "pinnedapps" => Ok(gen_pinnedapps()),
        "inputmethod" => Ok(gen_inputmethod()),
        "storagesense" => Ok(gen_storagesense()),
        "autofix" => Ok(gen_autofix()),
        "recentsearch" => Ok(gen_recentsearch()),
        "sysmaint" => Ok(gen_sysmaint()),
        "multiclip" => Ok(gen_multiclip()),
        "focussession" => Ok(gen_focussession()),
        "quicknote" => Ok(gen_quicknote()),
        "colorscheme" => Ok(gen_colorscheme()),
        "appcompat" => Ok(gen_appcompat()),
        "windowrules" => Ok(gen_windowrules()),
        "spatialaudio" => Ok(gen_spatialaudio()),
        "filetransfer" => Ok(gen_filetransfer()),
        "startupopt" => Ok(gen_startupopt()),
        "usagetime" => Ok(gen_usagetime()),
        "voicecontrol" => Ok(gen_voicecontrol()),
        "devpair" => Ok(gen_devpair()),
        "notifgroup" => Ok(gen_notifgroup()),
        "playmedia" => Ok(gen_playmedia()),
        "kbmacro" => Ok(gen_kbmacro()),
        "sysresource" => Ok(gen_sysresource()),
        "faceunlock" => Ok(gen_faceunlock()),
        "usbpolicy" => Ok(gen_usbpolicy()),
        "applaunch" => Ok(gen_applaunch()),
        "sysprofiler" => Ok(gen_sysprofiler()),
        "clipsync" => Ok(gen_clipsync()),
        "netusage" => Ok(gen_netusage()),
        "touchscreen" => Ok(gen_touchscreen()),
        "diskquota" => Ok(gen_diskquota()),
        "appdefaults" => Ok(gen_appdefaults()),
        "policyengine" => Ok(gen_policyengine()),
        "fontpreview" => Ok(gen_fontpreview()),
        "wifiscan" => Ok(gen_wifiscan()),
        "splitview" => Ok(gen_splitview()),
        "iotdevice" => Ok(gen_iotdevice()),
        "prochistory" => Ok(gen_prochistory()),
        "notiffilter" => Ok(gen_notiffilter()),
        "colorblind" => Ok(gen_colorblind()),
        "clipaction" => Ok(gen_clipaction()),
        "energysaver" => Ok(gen_energysaver()),
        "filerules" => Ok(gen_filerules()),
        "secureboot" => Ok(gen_secureboot()),
        "eventlog" => Ok(gen_eventlog()),
        "sysevents" => Ok(gen_sysevents()),
        "logpersist" => Ok(gen_logpersist()),
        "svcstart" => Ok(gen_svcstart()),
        "sockactivation" => Ok(gen_sockactivation()),
        "drvmon" => Ok(gen_drvmon()),
        "reslimit" => Ok(gen_reslimit()),
        "initproc" => Ok(gen_initproc()),
        "syshealth" => Ok(gen_syshealth()),
        "udriver" => Ok(gen_udriver()),
        "hotplug" => Ok(gen_hotplug()),
        "devpower" => Ok(gen_devpower()),
        "vmguest" => Ok(gen_vmguest()),
        "pciids" => Ok(gen_pciids()),
        "upnp" => Ok(gen_upnp()),
        "http" => Ok(gen_http()),
        "ntp" => Ok(gen_ntp()),
        "mdns" => Ok(gen_mdns()),
        "telnet" => Ok(gen_telnet()),
        "tftp" => Ok(gen_tftp()),
        "netsyslog" => Ok(gen_netsyslog()),
        "wol" => Ok(gen_wol()),
        "pcap" => Ok(gen_pcap()),
        "traceroute" => Ok(gen_traceroute()),
        "dhcpv6" => Ok(gen_dhcpv6()),
        "firewall" => Ok(gen_firewall()),
        "igmp" => Ok(gen_igmp()),
        "mld" => Ok(gen_mld()),
        "lldp" => Ok(gen_lldp()),
        "netstat" => Ok(gen_netstat()),
        "ndisc" => Ok(gen_ndisc()),
        "netcat" => Ok(gen_netcat()),
        "iperf" => Ok(gen_iperf()),
        "snmp" => Ok(gen_snmp()),
        "ftp" => Ok(gen_ftp()),
        "smtp" => Ok(gen_smtp()),
        "vlan" => Ok(gen_vlan()),
        "qos" => Ok(gen_qos()),
        "socks" => Ok(gen_socks()),
        "bridge" => Ok(gen_bridge()),
        "systemimage" => Ok(gen_systemimage()),
        "raidmgr" => Ok(gen_raidmgr()),
        "networkbridge" => Ok(gen_networkbridge()),
        "secureerase" => Ok(gen_secureerase()),
        "dnssettings" => Ok(gen_dnssettings()),
        "backupsched" => Ok(gen_backupsched()),
        "displaycal" => Ok(gen_displaycal()),
        "vpnprofile" => Ok(gen_vpnprofile()),
        "diskhealth" => Ok(gen_diskhealth()),
        "recoverypart" => Ok(gen_recoverypart()),
        "userprofile" => Ok(gen_userprofile()),
        "diskclean" => Ok(gen_diskclean()),
        "acl" => Ok(gen_acl()),
        "associations" => Ok(gen_associations()),
        "logrotate" => Ok(gen_logrotate()),
        "powerwake" => Ok(gen_powerwake()),
        "diskio" => Ok(gen_diskio()),
        "sysuptime" => Ok(gen_sysuptime()),
        "netspeed" => Ok(gen_netspeed()),
        "cpufreq" => Ok(gen_cpufreq()),
        "thermal" => Ok(gen_thermal()),
        "swapmon" => Ok(gen_swapmon()),
        "sysctlfs" => Ok(gen_sysctlfs()),
        "cputopo" => Ok(gen_cputopo()),
        "memlayout" => Ok(gen_memlayout()),
        "irqbalance" => Ok(gen_irqbalance()),
        "fs_loadavg" => Ok(gen_fs_loadavg()),
        "kernlog" => Ok(gen_kernlog()),
        "coredump" => Ok(gen_coredump()),
        "fwupdate" => Ok(gen_fwupdate()),
        "timesync" => Ok(gen_timesync()),
        "kmod" => Ok(gen_kmod()),
        "entropy" => Ok(gen_entropy()),
        "iosched" => Ok(gen_iosched()),
        "netmon" => Ok(gen_netmon()),
        "groupmgr" => Ok(gen_groupmgr()),
        "sysrq" => Ok(gen_sysrq()),
        "telemetry" => Ok(gen_telemetry()),
        "fscache" => Ok(gen_fscache()),
        "nameservice" => Ok(gen_nameservice()),
        "oomkiller" => Ok(gen_oomkiller()),
        "blktrace" => Ok(gen_blktrace()),
        "cgroupfs" => Ok(gen_cgroupfs()),
        "secpolicy" => Ok(gen_secpolicy()),
        "procstat" => Ok(gen_procstat()),
        "kernparam" => Ok(gen_kernparam()),
        "tracemon" => Ok(gen_tracemon()),
        "authbroker" => Ok(gen_authbroker()),
        "prociso" => Ok(gen_prociso()),
        "dmevent" => Ok(gen_dmevent()),
        "pftrack" => Ok(gen_pftrack()),
        "ipclog" => Ok(gen_ipclog()),
        "numastat" => Ok(gen_numastat()),
        "shmem" => Ok(gen_shmem()),
        "wqstat" => Ok(gen_wqstat()),
        "slabstat" => Ok(gen_slabstat()),
        "timerq" => Ok(gen_timerq()),
        "fdtable" => Ok(gen_fdtable()),
        "rcustat" => Ok(gen_rcustat()),
        "kconsole" => Ok(gen_kconsole()),
        "signalq" => Ok(gen_signalq()),
        "memcg" => Ok(gen_memcg()),
        "tlbstat" => Ok(gen_tlbstat()),
        "pagestat" => Ok(gen_pagestat()),
        "dmastat" => Ok(gen_dmastat()),
        "compstat" => Ok(gen_compstat()),
        "irqstat" => Ok(gen_irqstat()),
        "epollstat" => Ok(gen_epollstat()),
        "vmmap" => Ok(gen_vmmap()),
        "softirq" => Ok(gen_softirq()),
        "netfilter" => Ok(gen_netfilter()),
        "schedclass" => Ok(gen_schedclass()),
        "cpuidle" => Ok(gen_cpuidle()),
        "futexstat" => Ok(gen_futexstat()),
        "writeback" => Ok(gen_writeback()),
        "iolatency" => Ok(gen_iolatency()),
        "taskstats" => Ok(gen_taskstats()),
        "kprobes" => Ok(gen_kprobes()),
        "netsock" => Ok(gen_netsock()),
        "blkqueue" => Ok(gen_blkqueue()),
        "powerstat" => Ok(gen_powerstat()),
        "inodestat" => Ok(gen_inodestat()),
        "migstat" => Ok(gen_migstat()),
        "pagecache" => Ok(gen_pagecache()),
        "netdev" => Ok(gen_netdev()),
        "cpustat" => Ok(gen_cpustat()),
        "filelock" => Ok(gen_filelock()),
        "pidstat" => Ok(gen_pidstat()),
        "binfmt" => Ok(gen_binfmt()),
        "pipestat" => Ok(gen_pipestat()),
        "sockbuf" => Ok(gen_sockbuf()),
        "schedlat" => Ok(gen_schedlat()),
        "mempress" => Ok(gen_mempress()),
        "cpucache" => Ok(gen_cpucache()),
        "aiostat" => Ok(gen_aiostat()),
        "kthread" => Ok(gen_kthread()),
        "mmapstat" => Ok(gen_mmapstat()),
        "rqstat" => Ok(gen_rqstat()),
        "thpstat" => Ok(gen_thpstat()),
        "cgiostat" => Ok(gen_cgiostat()),
        "bpfstat" => Ok(gen_bpfstat()),
        "pgtable" => Ok(gen_pgtable()),
        "zramstat" => Ok(gen_zramstat()),
        "ksmstat" => Ok(gen_ksmstat()),
        "clocksrc" => Ok(gen_clocksrc()),
        "pmcstat" => Ok(gen_pmcstat()),
        "cputhr" => Ok(gen_cputhr()),
        "ipcns" => Ok(gen_ipcns()),
        "netqueue" => Ok(gen_netqueue()),
        "secmod" => Ok(gen_secmod()),
        "vmballoon" => Ok(gen_vmballoon()),
        "devfreq" => Ok(gen_devfreq()),
        "hwrng" => Ok(gen_hwrng()),
        "acpistat" => Ok(gen_acpistat()),
        "userfault" => Ok(gen_userfault()),
        "ioport" => Ok(gen_ioport()),
        "msivec" => Ok(gen_msivec()),
        "cpuset" => Ok(gen_cpuset()),
        "ftrace" => Ok(gen_ftrace()),
        "kstack" => Ok(gen_kstack()),
        "fnotify" => Ok(gen_fnotify()),
        "netlat" => Ok(gen_netlat()),
        "diskstat" => Ok(gen_diskstat()),
        "taskio" => Ok(gen_taskio()),
        "ttystat" => Ok(gen_ttystat()),
        "swapact" => Ok(gen_swapact()),
        "schedwait" => Ok(gen_schedwait()),
        "ratestat" => Ok(gen_ratestat()),
        "iomem" => Ok(gen_iomem()),
        "vmzone" => Ok(gen_vmzone()),
        "budstat" => Ok(gen_budstat()),
        "cgmem" => Ok(gen_cgmem()),
        "vmfrag" => Ok(gen_vmfrag()),
        "pidfd" => Ok(gen_pidfd()),
        "columnview" => Ok(gen_columnview()),
        "pathbar" => Ok(gen_pathbar()),
        "viewstate" => Ok(gen_viewstate()),
        "properties" => Ok(gen_properties()),
        _ => Err(KernelError::NotFound),
    }
}

/// Classify a relative procfs path into a typed request.
///
/// Returns:
/// - `ProcPath::Root` — the root directory itself
/// - `ProcPath::RootFile(name)` — a file in the root (e.g., "meminfo")
/// - `ProcPath::PidDir(id)` — a per-PID directory (e.g., "1")
/// - `ProcPath::PidFile(id, name)` — a file inside a PID dir (e.g., "1/status")
/// - `ProcPath::NotFound` — unrecognized path
enum ProcPath<'a> {
    Root,
    RootFile(&'a str),
    PidDir(u64),
    PidFile(u64, &'a str),
    /// A per-PID symbolic link (e.g. "1/cwd"). Resolved via `readlink`.
    PidLink(u64, &'a str),
    /// The magic `/proc/self` symlink itself (the bare path, no trailing
    /// component).  Linux makes this a symlink whose target is the caller's
    /// pid; `readlink` resolves it.  A path *under* self (e.g. `self/status`)
    /// is resolved to the current pid's file/link instead, not this variant.
    SelfLink,
    /// The `/proc/<pid>/task` directory listing the process's thread tids.
    PidTaskDir(u64),
    /// A `/proc/<pid>/task/<tid>` thread directory.
    PidTaskTidDir(u64, u64),
    /// A file inside a thread directory, e.g. `/proc/<pid>/task/<tid>/comm`.
    PidTaskFile(u64, u64, &'a str),
    /// The `/proc/<pid>/fd` directory listing the process's open fds.
    PidFdDir(u64),
    /// A `/proc/<pid>/fd/<n>` magic symlink to the backing object of fd `n`.
    PidFdLink(u64, i32),
    /// The `/proc/<pid>/fdinfo` directory listing the process's open fds.
    PidFdInfoDir(u64),
    /// A `/proc/<pid>/fdinfo/<n>` regular file describing fd `n` (pos/flags).
    PidFdInfoFile(u64, i32),
    /// A directory in the `/proc/sys` sysctl tree.  The `&str` is the path
    /// *under* `/proc/sys` (`""` is `/proc/sys` itself, `"kernel"` is
    /// `/proc/sys/kernel`, etc.).
    SysDir(&'a str),
    /// A file in the `/proc/sys` sysctl tree.  The `&str` is the path under
    /// `/proc/sys` (e.g. `"kernel/osrelease"`).
    SysFile(&'a str),
    NotFound,
}

/// True if `candidate` is a *direct* child (one path component below) of the
/// sysctl directory `parent` (both expressed as paths under `/proc/sys`,
/// `""` being the root).  Returns the child's basename when so.
///
/// Used to enumerate a `/proc/sys` directory's immediate entries from the
/// flat [`SYS_DIRS`] / [`SYS_FILES`] tables without a tree structure.
fn sys_immediate_child<'a>(parent: &str, candidate: &'a str) -> Option<&'a str> {
    if candidate.is_empty() {
        // The root pseudo-entry ("") is never a child of anything.
        return None;
    }
    let rest = if parent.is_empty() {
        candidate
    } else {
        // Must sit strictly below `parent/`, not merely share a prefix
        // (so "kernelfoo" is not treated as a child of "kernel").
        candidate.strip_prefix(parent)?.strip_prefix('/')?
    };
    // A direct child has exactly one remaining component.
    if rest.is_empty() || rest.contains('/') {
        None
    } else {
        Some(rest)
    }
}

/// The basename (last path component) of a `/proc/sys` relative path; `"sys"`
/// for the empty root path.  Used to fill the `name` field of a `DirEntry`.
fn sys_basename(rel: &str) -> &str {
    if rel.is_empty() {
        "sys"
    } else {
        rel.rsplit('/').next().unwrap_or(rel)
    }
}

/// List the immediate children of the `/proc/sys` directory `dir` (a path
/// under `/proc/sys`, `""` for the root) — its subdirectories followed by its
/// files, derived from the flat [`SYS_DIRS`] / [`SYS_FILES`] tables.
fn sys_children(dir: &str) -> Vec<DirEntry> {
    let mut entries: Vec<DirEntry> = Vec::new();
    for &d in SYS_DIRS {
        if let Some(name) = sys_immediate_child(dir, d) {
            entries.push(DirEntry {
                name: String::from(name),
                entry_type: EntryType::Directory,
                size: 0,
            });
        }
    }
    for &f in SYS_FILES {
        if let Some(name) = sys_immediate_child(dir, f) {
            let size = gen_sys(f).map_or(0, |data| data.len() as u64);
            entries.push(DirEntry {
                name: String::from(name),
                entry_type: EntryType::File,
                size,
            });
        }
    }
    entries
}

/// Format 16 bytes as an RFC 4122 version-4 UUID string (lowercase,
/// hyphenated `8-4-4-4-12`).  The version nibble and variant bits are forced
/// per RFC 4122 §4.4 so the result is a well-formed v4 UUID regardless of the
/// input bytes.  No indexing/`unwrap` — `char::from_digit` of a value < 16 is
/// always `Some`, with `'0'` as an unreachable fallback.
fn format_uuid_v4(bytes: [u8; 16]) -> String {
    let mut s = String::with_capacity(36);
    for (i, &raw) in bytes.iter().enumerate() {
        let byte = match i {
            6 => (raw & 0x0f) | 0x40, // version 4
            8 => (raw & 0x3f) | 0x80, // RFC 4122 variant (10xx)
            _ => raw,
        };
        if matches!(i, 4 | 6 | 8 | 10) {
            s.push('-');
        }
        s.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        s.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or('0'));
    }
    s
}

/// The per-boot machine identifier reported by `/proc/sys/kernel/random/boot_id`.
///
/// Linux generates a random UUID once per boot and reports the same value for
/// the lifetime of the boot (systemd, D-Bus and friends key session state off
/// it).  We mirror that: generate from the kernel CSPRNG on first read and
/// cache it for the rest of the boot.
fn boot_id() -> &'static str {
    static BOOT_ID: spin::Once<String> = spin::Once::new();
    BOOT_ID.call_once(|| {
        let mut bytes = [0u8; 16];
        crate::rng::fill(&mut bytes);
        format_uuid_v4(bytes)
    })
}

/// Generate the contents of a `/proc/sys` file.
///
/// Values are real kernel state or real ABI ceilings (see [`SYS_FILES`]);
/// `random/uuid` returns a fresh CSPRNG-derived UUID on every read (matching
/// Linux, where each read of that file yields a new UUID), while `random/boot_id`
/// is stable for the boot.  An unknown path is `NotFound`.
fn gen_sys(rel: &str) -> KernelResult<Vec<u8>> {
    let text: String = match rel {
        // The uname(2) surface — must stay byte-consistent with sys_uname.
        "kernel/ostype" => String::from("Linux\n"),
        "kernel/osrelease" => String::from("6.6.0-slateos\n"),
        "kernel/version" => String::from("#1 SMP\n"),
        "kernel/hostname" => {
            crate::fs::nameservice::init_defaults();
            format!("{}\n", crate::fs::nameservice::get_hostname())
        }
        "kernel/domainname" => {
            crate::fs::nameservice::init_defaults();
            format!("{}\n", crate::fs::nameservice::get_domain())
        }
        // Real PID ceiling (the per-namespace process cap).
        "kernel/pid_max" => format!("{}\n", crate::pidns::MAX_PIDS_PER_NS),
        // Real per-process fd-table size.
        "fs/nr_open" => format!("{}\n", crate::proc::linux_fd::MAX_FDS),
        "kernel/random/uuid" => {
            let mut bytes = [0u8; 16];
            crate::rng::fill(&mut bytes);
            format!("{}\n", format_uuid_v4(bytes))
        }
        "kernel/random/boot_id" => format!("{}\n", boot_id()),
        // CSPRNG entropy surface.  Our RNG is a ChaCha20 CSPRNG keyed with 256
        // bits of seed material; modern Linux likewise tops its entropy pool at
        // POOL_BITS = 256 (random.c since ~5.18), so 256 is the honest ceiling.
        "kernel/random/poolsize" => String::from("256\n"),
        // Reports the full 256 bits once the CSPRNG has been seeded, 0 before
        // init.  Mirrors Linux semantics where entropy_avail rises to poolsize
        // once the pool is fully credited; userspace reads this to decide
        // whether the random source is ready.
        "kernel/random/entropy_avail" => {
            let avail = if crate::rng::is_initialized() { 256 } else { 0 };
            format!("{avail}\n")
        }
        // Memory-commit policy as seen through the Linux ABI.  `/proc/sys/vm/`
        // is a Linux-ABI-only surface — native Slate OS programs use native APIs,
        // so the only readers of this file are Linux binaries.  Per the
        // memory-commit policy decision (design-decisions.md §11), this file is
        // the canonical userspace name for the Linux-ABI system-wide commit
        // default (`mm.linux_lazy_default`, the Linux counterpart of the native
        // `mm.lazy_default`).  We mirror the live sysctl honestly:
        //   linux_lazy_default == 1 (lazy/overcommit) → "0" (heuristic overcommit)
        //   linux_lazy_default == 0 (committed/strict) → "2" (never overcommit)
        // The default is `1` → "0", matching Linux's typical heuristic-overcommit
        // behavior.  This is real, not fabricated: when set to lazy a Linux
        // program's `mmap` genuinely is backed on touch, not up front.
        //
        // We deliberately do NOT expose `overcommit_ratio` / `overcommit_kbytes`:
        // those only parameterize Linux's *strict commit accounting*
        // (overcommit_memory = 2), which we do not perform, so advertising them
        // would imply a knob with no backing (violates the "never advertise an
        // unhonored feature" rule, design-decisions.md §1).
        "vm/overcommit_memory" => {
            let lazy = crate::sysctl::get(crate::sysctl::PARAM_MM_LINUX_LAZY_DEFAULT)
                != Some(0);
            if lazy {
                String::from("0\n")
            } else {
                String::from("2\n")
            }
        }
        _ => return Err(KernelError::NotFound),
    };
    Ok(text.into_bytes())
}

/// True if `tid` is a live thread of process `pid`.
///
/// Threads are tracked in the process record's thread list (`get_threads`);
/// a tid that has exited has already been removed, so membership here also
/// implies the thread is live.  Used to gate every `task/<tid>/` path so a
/// bogus or stale tid returns `NotFound`.
fn thread_belongs(pid: u64, tid: u64) -> bool {
    crate::proc::pcb::get_threads(pid).is_some_and(|ts| ts.contains(&tid))
}

fn classify_path(rel: &str) -> ProcPath<'_> {
    if rel.is_empty() {
        return ProcPath::Root;
    }

    // Split into first component and optional remainder.
    let (first, rest) = match rel.find('/') {
        Some(pos) => {
            let (a, b) = rel.split_at(pos);
            // b starts with '/'; strip it.
            (a, b.get(1..).unwrap_or(""))
        }
        None => (rel, ""),
    };

    // Try root-level file first.
    if rest.is_empty() && ROOT_FILES.contains(&first) {
        return ProcPath::RootFile(first);
    }

    // The bare `/proc/self` path is a symlink, not a directory: Linux's
    // readlink("/proc/self") yields the caller's pid.  Classify it as
    // SelfLink so stat reports a symlink and readlink resolves the pid.
    // A path *under* self (self/<file>) still resolves to the current
    // pid's file via the alias below.
    if first == "self" && rest.is_empty() {
        return ProcPath::SelfLink;
    }

    // The `/proc/sys` sysctl tree.  Like per-PID `task/`, it has interior
    // directories, so it is routed here (before the numeric-PID parse, since
    // "sys" is not a number).  `rest` is the path under `/proc/sys`.
    if first == "sys" {
        if SYS_FILES.contains(&rest) {
            return ProcPath::SysFile(rest);
        }
        if SYS_DIRS.contains(&rest) {
            return ProcPath::SysDir(rest);
        }
        return ProcPath::NotFound;
    }

    // "self" is a magic alias for the current task's PID.
    // Linux provides /proc/self as a symlink → /proc/<current_pid>.
    // We resolve it inline since procfs is a virtual filesystem.
    let pid = if first == "self" {
        crate::sched::current_task_id()
    } else if let Ok(p) = first.parse::<u64>() {
        p
    } else {
        return ProcPath::NotFound;
    };

    if rest.is_empty() {
        return ProcPath::PidDir(pid);
    }
    // File inside PID directory (no nested subdirs).
    if !rest.contains('/') && PID_FILES.contains(&rest) {
        return ProcPath::PidFile(pid, rest);
    }
    // Symlink inside PID directory (cwd, root).
    if !rest.contains('/') && PID_LINKS.contains(&rest) {
        return ProcPath::PidLink(pid, rest);
    }

    // The `fd/` subtree: `<pid>/fd` (directory) and `<pid>/fd/<n>` (a
    // magic symlink to fd n's backing object).  Only Linux-ABI processes
    // have a kernel-visible fd table; for others the directory lists no
    // entries and each `<n>` resolves to NotFound.
    if rest == "fd" {
        return ProcPath::PidFdDir(pid);
    }
    if let Some(sub) = rest.strip_prefix("fd/") {
        if !sub.contains('/') {
            if let Ok(n) = sub.parse::<i32>() {
                return ProcPath::PidFdLink(pid, n);
            }
        }
        return ProcPath::NotFound;
    }

    // The `fdinfo/` subtree: `<pid>/fdinfo` (directory) and
    // `<pid>/fdinfo/<n>` (a regular file with the fd's pos/flags).  Like
    // `fd/`, only Linux-ABI processes have a kernel-visible fd table, so
    // the directory is empty and each `<n>` is NotFound otherwise.
    if rest == "fdinfo" {
        return ProcPath::PidFdInfoDir(pid);
    }
    if let Some(sub) = rest.strip_prefix("fdinfo/") {
        if !sub.contains('/') {
            if let Ok(n) = sub.parse::<i32>() {
                return ProcPath::PidFdInfoFile(pid, n);
            }
        }
        return ProcPath::NotFound;
    }

    // The `task/` subtree: `<pid>/task`, `<pid>/task/<tid>`, and
    // `<pid>/task/<tid>/<file>` (the only nested directories in procfs).
    if rest == "task" {
        return ProcPath::PidTaskDir(pid);
    }
    if let Some(sub) = rest.strip_prefix("task/") {
        // `sub` is `<tid>` or `<tid>/<file>`.
        let (tid_str, file) = match sub.find('/') {
            Some(pos) => {
                let (a, b) = sub.split_at(pos);
                (a, b.get(1..).unwrap_or(""))
            }
            None => (sub, ""),
        };
        if let Ok(tid) = tid_str.parse::<u64>() {
            if file.is_empty() {
                return ProcPath::PidTaskTidDir(pid, tid);
            }
            if !file.contains('/') && TASK_FILES.contains(&file) {
                return ProcPath::PidTaskFile(pid, tid, file);
            }
        }
        return ProcPath::NotFound;
    }

    ProcPath::NotFound
}

// ---------------------------------------------------------------------------
// FileSystem trait implementation
// ---------------------------------------------------------------------------

impl FileSystem for ProcFs {
    fn fs_type(&self) -> &'static str {
        "procfs"
    }

    fn readdir(&mut self, path: &str) -> KernelResult<Vec<DirEntry>> {
        let rel = strip_root(path);

        match classify_path(rel) {
            ProcPath::Root => {
                // Root directory: list virtual files + per-PID directories.
                let mut entries: Vec<DirEntry> = ROOT_FILES
                    .iter()
                    .map(|name| {
                        let size = generate(name).map_or(0, |d| d.len() as u64);
                        DirEntry {
                            name: String::from(*name),
                            entry_type: EntryType::File,
                            size,
                        }
                    })
                    .collect();

                // "self" — magic symlink to the current task's PID directory.
                entries.push(DirEntry {
                    name: String::from("self"),
                    entry_type: EntryType::Symlink,
                    size: 0,
                });

                // "sys" — the sysctl tree (a directory, not in ROOT_FILES).
                entries.push(DirEntry {
                    name: String::from("sys"),
                    entry_type: EntryType::Directory,
                    size: 0,
                });

                // Add per-PID directories for all live tasks.
                for task in &crate::sched::task_list() {
                    entries.push(DirEntry {
                        name: format!("{}", task.id),
                        entry_type: EntryType::Directory,
                        size: 0,
                    });
                }

                Ok(entries)
            }
            ProcPath::PidDir(pid) => {
                // Per-PID directory: list virtual files inside it.
                if !task_exists(pid) {
                    return Err(KernelError::NotFound);
                }
                let mut entries: Vec<DirEntry> = PID_FILES
                    .iter()
                    .map(|name| {
                        let size = generate_pid(pid, name).map_or(0, |d| d.len() as u64);
                        DirEntry {
                            name: String::from(*name),
                            entry_type: EntryType::File,
                            size,
                        }
                    })
                    .collect();
                // Per-PID symbolic links (cwd, root).
                for name in PID_LINKS {
                    entries.push(DirEntry {
                        name: String::from(*name),
                        entry_type: EntryType::Symlink,
                        size: 0,
                    });
                }
                // The `task/` subdirectory (per-thread view).
                entries.push(DirEntry {
                    name: String::from("task"),
                    entry_type: EntryType::Directory,
                    size: 0,
                });
                // The `fd/` subdirectory (open file descriptors).
                entries.push(DirEntry {
                    name: String::from("fd"),
                    entry_type: EntryType::Directory,
                    size: 0,
                });
                // The `fdinfo/` subdirectory (per-fd pos/flags).
                entries.push(DirEntry {
                    name: String::from("fdinfo"),
                    entry_type: EntryType::Directory,
                    size: 0,
                });
                Ok(entries)
            }
            ProcPath::PidFdInfoDir(pid) => {
                // `/proc/<pid>/fdinfo` — one regular file per open fd, each
                // holding that fd's pos/flags.  Same fd source and
                // honestly-empty-for-native behaviour as `fd/`.
                if !task_exists(pid) {
                    return Err(KernelError::NotFound);
                }
                let entries = crate::proc::pcb::linux_fd_list(pid)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(fd, entry)| {
                        // Render from the entry we already hold — no need to
                        // re-lock PROCESS_TABLE per fd via gen_pid_fdinfo.
                        let size = fdinfo_from_entry(&entry).len() as u64;
                        DirEntry {
                            name: format!("{fd}"),
                            entry_type: EntryType::File,
                            size,
                        }
                    })
                    .collect();
                Ok(entries)
            }
            ProcPath::PidFdDir(pid) => {
                // `/proc/<pid>/fd` — one symlink per open fd.  Only
                // Linux-ABI processes have a kernel-visible fd table; for a
                // native process (fds live in userspace) the list is
                // legitimately empty rather than fabricated.
                if !task_exists(pid) {
                    return Err(KernelError::NotFound);
                }
                let entries = crate::proc::pcb::linux_fd_list(pid)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(fd, _entry)| DirEntry {
                        name: format!("{fd}"),
                        entry_type: EntryType::Symlink,
                        size: 0,
                    })
                    .collect();
                Ok(entries)
            }
            ProcPath::PidTaskDir(pid) => {
                // `/proc/<pid>/task` — one subdirectory per live thread tid.
                let threads = crate::proc::pcb::get_threads(pid)
                    .ok_or(KernelError::NotFound)?;
                let entries = threads
                    .iter()
                    .map(|tid| DirEntry {
                        name: format!("{tid}"),
                        entry_type: EntryType::Directory,
                        size: 0,
                    })
                    .collect();
                Ok(entries)
            }
            ProcPath::PidTaskTidDir(pid, tid) => {
                // `/proc/<pid>/task/<tid>` — the per-thread file set.
                if !thread_belongs(pid, tid) {
                    return Err(KernelError::NotFound);
                }
                let entries = TASK_FILES
                    .iter()
                    .map(|name| {
                        let size = generate_task(pid, tid, name)
                            .map_or(0, |d| d.len() as u64);
                        DirEntry {
                            name: String::from(*name),
                            entry_type: EntryType::File,
                            size,
                        }
                    })
                    .collect();
                Ok(entries)
            }
            ProcPath::SysDir(rel) => Ok(sys_children(rel)),
            ProcPath::RootFile(_) | ProcPath::PidFile(_, _)
            | ProcPath::PidLink(_, _) | ProcPath::SelfLink
            | ProcPath::PidTaskFile(_, _, _) | ProcPath::PidFdLink(_, _)
            | ProcPath::PidFdInfoFile(_, _) | ProcPath::SysFile(_) => {
                Err(KernelError::NotADirectory)
            }
            ProcPath::NotFound => Err(KernelError::NotFound),
        }
    }

    fn read_file(&mut self, path: &str) -> KernelResult<Vec<u8>> {
        let rel = strip_root(path);

        match classify_path(rel) {
            ProcPath::Root | ProcPath::PidDir(_)
            | ProcPath::PidTaskDir(_) | ProcPath::PidTaskTidDir(_, _)
            | ProcPath::PidFdDir(_) | ProcPath::PidFdInfoDir(_)
            | ProcPath::SysDir(_) => {
                Err(KernelError::IsADirectory)
            }
            ProcPath::SysFile(rel) => gen_sys(rel),
            ProcPath::RootFile(name) => generate(name),
            ProcPath::PidFile(pid, file_name) => {
                if !task_exists(pid) {
                    return Err(KernelError::NotFound);
                }
                generate_pid(pid, file_name)
            }
            ProcPath::PidTaskFile(pid, tid, file_name) => {
                if !thread_belongs(pid, tid) {
                    return Err(KernelError::NotFound);
                }
                generate_task(pid, tid, file_name)
            }
            // Reading a symlink's bytes directly is invalid; the VFS follows
            // it via readlink instead.  Mirrors Linux read() → EINVAL on a
            // symlink opened without O_PATH.
            ProcPath::PidLink(_, _) | ProcPath::SelfLink
            | ProcPath::PidFdLink(_, _) => {
                Err(KernelError::InvalidArgument)
            }
            ProcPath::PidFdInfoFile(pid, fd) => {
                if !task_exists(pid) {
                    return Err(KernelError::NotFound);
                }
                gen_pid_fdinfo(pid, fd)
            }
            ProcPath::NotFound => Err(KernelError::NotFound),
        }
    }

    /// Procfs is read-only except for the handful of tunable per-PID
    /// control files.  Currently only `/proc/<pid>/oom_score_adj` accepts
    /// writes (mirroring Linux's `echo N > .../oom_score_adj`); every other
    /// path stays `NotSupported`, preserving the read-only contract the
    /// rest of procfs relies on.
    fn write_file(&mut self, path: &str, data: &[u8]) -> KernelResult<()> {
        let rel = strip_root(path);

        match classify_path(rel) {
            ProcPath::PidFile(pid, "oom_score_adj") => {
                if !task_exists(pid) {
                    return Err(KernelError::NotFound);
                }
                set_pid_oom_score_adj(pid, data)
            }
            ProcPath::NotFound => Err(KernelError::NotFound),
            _ => Err(KernelError::NotSupported),
        }
    }

    fn stat(&mut self, path: &str) -> KernelResult<DirEntry> {
        let rel = strip_root(path);

        match classify_path(rel) {
            ProcPath::Root => Ok(DirEntry {
                name: String::from("/"),
                entry_type: EntryType::Directory,
                size: 0,
            }),
            ProcPath::RootFile(name) => {
                let size = generate(name).map_or(0, |d| d.len() as u64);
                Ok(DirEntry {
                    name: String::from(name),
                    entry_type: EntryType::File,
                    size,
                })
            }
            ProcPath::PidDir(pid) => {
                if !task_exists(pid) {
                    return Err(KernelError::NotFound);
                }
                Ok(DirEntry {
                    name: format!("{pid}"),
                    entry_type: EntryType::Directory,
                    size: 0,
                })
            }
            ProcPath::PidFile(pid, file_name) => {
                if !task_exists(pid) {
                    return Err(KernelError::NotFound);
                }
                let size = generate_pid(pid, file_name).map_or(0, |d| d.len() as u64);
                Ok(DirEntry {
                    name: String::from(file_name),
                    entry_type: EntryType::File,
                    size,
                })
            }
            ProcPath::PidLink(pid, link_name) => {
                if !task_exists(pid) {
                    return Err(KernelError::NotFound);
                }
                Ok(DirEntry {
                    name: String::from(link_name),
                    entry_type: EntryType::Symlink,
                    size: 0,
                })
            }
            ProcPath::SelfLink => Ok(DirEntry {
                name: String::from("self"),
                entry_type: EntryType::Symlink,
                size: 0,
            }),
            ProcPath::PidTaskDir(pid) => {
                if !task_exists(pid) {
                    return Err(KernelError::NotFound);
                }
                Ok(DirEntry {
                    name: String::from("task"),
                    entry_type: EntryType::Directory,
                    size: 0,
                })
            }
            ProcPath::PidTaskTidDir(pid, tid) => {
                if !thread_belongs(pid, tid) {
                    return Err(KernelError::NotFound);
                }
                Ok(DirEntry {
                    name: format!("{tid}"),
                    entry_type: EntryType::Directory,
                    size: 0,
                })
            }
            ProcPath::PidTaskFile(pid, tid, file_name) => {
                if !thread_belongs(pid, tid) {
                    return Err(KernelError::NotFound);
                }
                let size = generate_task(pid, tid, file_name).map_or(0, |d| d.len() as u64);
                Ok(DirEntry {
                    name: String::from(file_name),
                    entry_type: EntryType::File,
                    size,
                })
            }
            ProcPath::PidFdDir(pid) => {
                if !task_exists(pid) {
                    return Err(KernelError::NotFound);
                }
                Ok(DirEntry {
                    name: String::from("fd"),
                    entry_type: EntryType::Directory,
                    size: 0,
                })
            }
            ProcPath::PidFdLink(pid, fd) => {
                // Only a currently-open fd is a valid symlink; an absent
                // fd (or a process with no kernel fd table) is NotFound.
                crate::proc::pcb::linux_fd_lookup(pid, fd)
                    .ok_or(KernelError::NotFound)?;
                Ok(DirEntry {
                    name: format!("{fd}"),
                    entry_type: EntryType::Symlink,
                    size: 0,
                })
            }
            ProcPath::PidFdInfoDir(pid) => {
                if !task_exists(pid) {
                    return Err(KernelError::NotFound);
                }
                Ok(DirEntry {
                    name: String::from("fdinfo"),
                    entry_type: EntryType::Directory,
                    size: 0,
                })
            }
            ProcPath::PidFdInfoFile(pid, fd) => {
                // A regular file, present only for a currently-open fd.
                let size = gen_pid_fdinfo(pid, fd)?.len() as u64;
                Ok(DirEntry {
                    name: format!("{fd}"),
                    entry_type: EntryType::File,
                    size,
                })
            }
            ProcPath::SysDir(rel) => Ok(DirEntry {
                name: String::from(sys_basename(rel)),
                entry_type: EntryType::Directory,
                size: 0,
            }),
            ProcPath::SysFile(rel) => {
                let size = gen_sys(rel).map_or(0, |d| d.len() as u64);
                Ok(DirEntry {
                    name: String::from(sys_basename(rel)),
                    entry_type: EntryType::File,
                    size,
                })
            }
            ProcPath::NotFound => Err(KernelError::NotFound),
        }
    }

    /// Resolve a per-PID symbolic link (`cwd`, `root`).
    ///
    /// `cwd` reflects the process's stored current working directory;
    /// `root` is always `/` (no per-process mount namespaces / chroot
    /// yet).  Returns `NotFound` for a task id with no live process
    /// (a bare scheduler task carries no cwd), and `InvalidArgument`
    /// for any non-link path.
    ///
    /// NOTE: the VFS `readlink` API returns `String`, but a cwd is stored
    /// as raw bytes (paths may contain any byte except `/` and NUL).  We
    /// surface a non-UTF-8 cwd as an error rather than lossily mangling
    /// it — silent corruption of a path is never acceptable.  In practice
    /// canonical cwds are ASCII/UTF-8, so this is a theoretical edge.
    fn readlink(&mut self, path: &str) -> KernelResult<String> {
        let rel = strip_root(path);
        match classify_path(rel) {
            ProcPath::PidLink(pid, "root") => {
                if !task_exists(pid) {
                    return Err(KernelError::NotFound);
                }
                Ok(String::from("/"))
            }
            ProcPath::PidLink(pid, "cwd") => {
                let cwd = crate::proc::pcb::get_cwd(pid)
                    .ok_or(KernelError::NotFound)?;
                String::from_utf8(cwd).map_err(|_| KernelError::InvalidArgument)
            }
            ProcPath::PidLink(pid, "exe") => {
                // Empty path means the process has not exec'd a binary
                // (e.g. a bare scheduler task or a not-yet-exec'd child):
                // Linux reports no /proc/<pid>/exe target in that case.
                let exe = crate::proc::pcb::get_exe_path(pid)
                    .ok_or(KernelError::NotFound)?;
                if exe.is_empty() {
                    return Err(KernelError::NotFound);
                }
                String::from_utf8(exe).map_err(|_| KernelError::InvalidArgument)
            }
            ProcPath::PidLink(_, _) => Err(KernelError::NotFound),
            // `/proc/<pid>/fd/<n>` → the backing object of fd n.
            ProcPath::PidFdLink(pid, fd) => {
                let entry = crate::proc::pcb::linux_fd_lookup(pid, fd)
                    .ok_or(KernelError::NotFound)?;
                Ok(fd_link_target(&entry))
            }
            // `/proc/self` → the caller's pid, as a relative target (Linux
            // returns the bare pid number, e.g. "7", resolved against /proc).
            ProcPath::SelfLink => Ok(format!("{}", crate::sched::current_task_id())),
            _ => Err(KernelError::InvalidArgument),
        }
    }

    fn metadata(&mut self, path: &str) -> KernelResult<FileMeta> {
        // Verify the path exists by calling stat.
        let entry = self.stat(path)?;

        let perms = if entry.entry_type == EntryType::Directory {
            0o555
        } else {
            0o444
        };

        Ok(FileMeta {
            size: entry.size,
            entry_type: entry.entry_type,
            permissions: perms,
            nlinks: 1,
            blocks: 0,
            ..FileMeta::minimal(entry.entry_type, entry.size)
        })
    }

    fn statvfs(&mut self) -> KernelResult<FsInfo> {
        let task_count = crate::sched::task_list().len();
        Ok(FsInfo {
            fs_type: String::from("procfs"),
            volume_label: String::new(),
            block_size: 0,
            total_blocks: 0,
            free_blocks: 0,
            total_inodes: (ROOT_FILES.len() + task_count) as u64,
            free_inodes: 0,
            max_name_len: 255,
            read_only: true,
        })
    }

    fn debug_stats(&self) -> String {
        let task_count = crate::sched::task_list().len();
        format!(
            "procfs: {} root files, {} task dirs",
            ROOT_FILES.len(),
            task_count
        )
    }
}

// ---------------------------------------------------------------------------
// Mount helper
// ---------------------------------------------------------------------------

/// Mount procfs at the given path (typically `/proc`).
pub fn mount(mount_path: &str) -> KernelResult<()> {
    let fs = ProcFs::new();
    crate::fs::Vfs::mount(mount_path, alloc::boxed::Box::new(fs))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Test the procfs implementation.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    serial_println!("[procfs] Running self-test...");

    let mut fs = ProcFs::new();

    // Test root readdir — should have root files + at least 1 PID directory.
    let entries = fs.readdir("/")?;
    let min_expected = ROOT_FILES.len();
    if entries.len() < min_expected {
        serial_println!(
            "[procfs]   FAIL: readdir returned {} entries, expected >= {}",
            entries.len(),
            min_expected
        );
        return Err(KernelError::InternalError);
    }
    // Count PID directories.
    let pid_dirs = entries.iter()
        .filter(|e| e.entry_type == EntryType::Directory)
        .count();
    serial_println!(
        "[procfs]   readdir /: {} entries ({} files, {} pid dirs) OK",
        entries.len(),
        entries.len().saturating_sub(pid_dirs),
        pid_dirs
    );

    // Test stat on root.
    let root_stat = fs.stat("/")?;
    if root_stat.entry_type != EntryType::Directory {
        serial_println!("[procfs]   FAIL: stat / not a directory");
        return Err(KernelError::InternalError);
    }
    serial_println!("[procfs]   stat /: directory OK");

    // Test each virtual file.
    for name in ROOT_FILES {
        let path = format!("/{name}");

        // stat should succeed.
        let entry = fs.stat(&path)?;
        if entry.entry_type != EntryType::File {
            serial_println!("[procfs]   FAIL: stat {path} not a file");
            return Err(KernelError::InternalError);
        }

        // read_file should return non-empty data.
        let data = fs.read_file(&path)?;
        if data.is_empty() {
            serial_println!("[procfs]   FAIL: read_file {path} returned empty");
            return Err(KernelError::InternalError);
        }

        // Verify it's valid UTF-8 (all our files are text).
        if core::str::from_utf8(&data).is_err() {
            serial_println!("[procfs]   FAIL: {path} is not valid UTF-8");
            return Err(KernelError::InternalError);
        }

        serial_println!("[procfs]   {name}: {} bytes OK", data.len());
    }

    // --- /proc/cpuinfo Linux per-processor format ---
    // Tools (`grep -c ^processor`), glibc, and lscpu/hwinfo expect one block
    // per online CPU, each beginning with a `processor\t: N` line and carrying
    // the well-known keys.  The previous custom format (a header block plus
    // `acpi_id`/`apic_id` keys) was miscounted as an extra CPU by
    // block-counting parsers; pin the new shape so it can't regress.
    {
        let cpu_data = fs.read_file("/cpuinfo")?;
        let cpu_text = core::str::from_utf8(&cpu_data)
            .map_err(|_| KernelError::InternalError)?;
        // `grep -c ^processor` — count lines that start a processor block.
        let block_count = cpu_text
            .lines()
            .filter(|l| l.starts_with("processor\t:"))
            .count();
        let want = crate::acpi::processor_count();
        if block_count != want {
            serial_println!(
                "[procfs]   FAIL: cpuinfo has {} processor blocks, want {} (online CPUs)",
                block_count, want
            );
            return Err(KernelError::InternalError);
        }
        if block_count == 0 {
            serial_println!("[procfs]   FAIL: cpuinfo has no processor blocks");
            return Err(KernelError::InternalError);
        }
        // First block must be index 0 (Linux numbers from 0).
        if !cpu_text.starts_with("processor\t: 0\n") {
            serial_println!("[procfs]   FAIL: cpuinfo first line not 'processor\\t: 0'");
            return Err(KernelError::InternalError);
        }
        // Per-block keys consumers scrape must each appear once per CPU.
        for key in ["vendor_id\t:", "cpu family\t:", "model name\t:",
                    "flags\t\t:", "cpu cores\t:"] {
            let n = cpu_text.lines().filter(|l| l.starts_with(key)).count();
            if n != want {
                serial_println!(
                    "[procfs]   FAIL: cpuinfo key {:?} appears {} times, want {}",
                    key, n, want
                );
                return Err(KernelError::InternalError);
            }
        }
        // The legacy header line must be gone (it broke block-counting).
        if cpu_text.contains("processors:") || cpu_text.contains("acpi_id") {
            serial_println!("[procfs]   FAIL: cpuinfo still contains legacy header keys");
            return Err(KernelError::InternalError);
        }
        serial_println!(
            "[procfs]   cpuinfo: {} Linux-format processor block(s) OK", block_count
        );
    }

    // Test stat on nonexistent file.
    if fs.stat("/nonexistent").is_ok() {
        serial_println!("[procfs]   FAIL: stat /nonexistent should fail");
        return Err(KernelError::InternalError);
    }
    serial_println!("[procfs]   stat /nonexistent: NotFound OK");

    // Test read on directory.
    if fs.read_file("/").is_ok() {
        serial_println!("[procfs]   FAIL: read_file / should fail (IsADirectory)");
        return Err(KernelError::InternalError);
    }
    serial_println!("[procfs]   read_file /: IsADirectory OK");

    // Test write to a read-only root file (should fail — NotSupported).
    if fs.write_file("/version", b"hacked").is_ok() {
        serial_println!("[procfs]   FAIL: write_file should fail (NotSupported)");
        return Err(KernelError::InternalError);
    }
    serial_println!("[procfs]   write_file: NotSupported OK");

    // The one writable per-PID file: a malformed oom_score_adj write must
    // be rejected before it can reach the OOM subsystem.  Use a live PID
    // path so it gets past classify_path; any existing task id works since
    // the value is invalid regardless.
    {
        let probe_tid = crate::sched::current_task_id();
        let bad_path = format!("/{probe_tid}/oom_score_adj");
        if fs.write_file(&bad_path, b"not-a-number").is_ok() {
            serial_println!(
                "[procfs]   FAIL: oom_score_adj accepted malformed write"
            );
            return Err(KernelError::InternalError);
        }
        serial_println!("[procfs]   oom_score_adj write: rejects malformed OK");
    }

    // End-to-end write path: prove a write reaches ProcFs::write_file
    // through the full VFS stack (check_writable + mount routing), not just
    // the direct fs call above.  procfs is mounted rw, so the VFS must not
    // short-circuit with ReadOnlyFilesystem; the fs itself decides.
    // - A root file rejects the write at the fs layer (NotSupported).
    // - A well-formed oom_score_adj write to a non-existent PID reaches the
    //   fs and returns NotFound (task_exists() false) — *not*
    //   ReadOnlyFilesystem, which would mean the write never routed here.
    {
        match crate::fs::Vfs::write_file("/proc/version", b"x") {
            Err(KernelError::NotSupported) => {}
            other => {
                serial_println!(
                    "[procfs]   FAIL: VFS write /proc/version = {:?}, want NotSupported",
                    other
                );
                return Err(KernelError::InternalError);
            }
        }
        match crate::fs::Vfs::write_file("/proc/999999/oom_score_adj", b"100") {
            Err(KernelError::NotFound) => {}
            other => {
                serial_println!(
                    "[procfs]   FAIL: VFS write /proc/999999/oom_score_adj = {:?}, want NotFound",
                    other
                );
                return Err(KernelError::InternalError);
            }
        }
        serial_println!("[procfs]   oom_score_adj write: end-to-end VFS routing OK");
    }

    // --- task/<tid>/ path classification ---
    // classify_path is the single router for every procfs path; verify the
    // new nested `task/` subtree resolves to the right typed variants and
    // that malformed / unsupported thread paths fall through to NotFound.
    {
        let cases: &[(&str, &str)] = &[
            ("5/task", "taskdir"),
            ("5/task/7", "tiddir"),
            ("5/task/7/comm", "file"),
            ("5/task/7/schedstat", "file"),
            ("5/task/7/stat", "file"),       // stat is a thread file too
            ("5/task/7/status", "file"),     // status is a thread file too
            ("5/task/7/maps", "notfound"),   // maps not in TASK_FILES
            ("5/task/abc", "notfound"),      // non-numeric tid
            ("5/task/7/comm/x", "notfound"), // nested beyond a thread file
        ];
        for (path, want) in cases {
            let got = match classify_path(path) {
                ProcPath::PidTaskDir(5) => "taskdir",
                ProcPath::PidTaskTidDir(5, 7) => "tiddir",
                ProcPath::PidTaskFile(5, 7, _) => "file",
                ProcPath::NotFound => "notfound",
                _ => "other",
            };
            if got != *want {
                serial_println!(
                    "[procfs]   FAIL: classify_path({:?}) = {}, want {}",
                    path, got, want
                );
                return Err(KernelError::InternalError);
            }
        }
        serial_println!("[procfs]   task/ classify: {} cases OK", cases.len());
    }

    // --- task/ directory, live ---
    // Every PID directory now lists a `task` subdirectory.  For the bare
    // scheduler task running the self-test (no PCB, no thread list),
    // reading `task/` itself returns NotFound, and any thread file under a
    // bogus tid is NotFound — exercising the gating without depending on a
    // live multi-threaded process existing at self-test time.
    {
        let probe = crate::sched::current_task_id();
        let dir_entries = fs.readdir(&format!("/{probe}"))?;
        if !dir_entries.iter().any(|e| {
            e.name == "task" && e.entry_type == EntryType::Directory
        }) {
            serial_println!(
                "[procfs]   FAIL: /{}/ missing `task` subdirectory", probe
            );
            return Err(KernelError::InternalError);
        }
        // A non-existent thread under any pid must be NotFound.
        match fs.read_file(&format!("/{probe}/task/999999/comm")) {
            Err(KernelError::NotFound) => {}
            other => {
                serial_println!(
                    "[procfs]   FAIL: bogus thread file = {:?}, want NotFound", other
                );
                return Err(KernelError::InternalError);
            }
        }
        // Listing task/ for the bare task (no thread list) is NotFound;
        // for a real process it would enumerate tids.  Tolerate both.
        match fs.readdir(&format!("/{probe}/task")) {
            Ok(tids) => serial_println!(
                "[procfs]   /{}/task: {} thread(s) OK", probe, tids.len()
            ),
            Err(KernelError::NotFound) => serial_println!(
                "[procfs]   /{}/task: NotFound (bare task, no thread list) OK", probe
            ),
            Err(e) => {
                serial_println!("[procfs]   FAIL: /task readdir error {:?}", e);
                return Err(KernelError::InternalError);
            }
        }
        serial_println!("[procfs]   task/ directory: gating OK");
    }

    // --- fd/ path classification + link rendering ---
    // The `fd/` subtree mirrors the `task/` router: `<pid>/fd` is a
    // directory and `<pid>/fd/<n>` a magic symlink; malformed/nested fd
    // paths fall through to NotFound.  fd_link_target is a pure renderer
    // over the fd-table entry, so its output is deterministic per kind.
    {
        let cases: &[(&str, &str)] = &[
            ("5/fd", "fddir"),
            ("5/fd/0", "fdlink"),
            ("5/fd/255", "fdlink"),
            ("5/fd/abc", "notfound"),  // non-numeric fd
            ("5/fd/0/x", "notfound"),  // nested beyond an fd
        ];
        for (path, want) in cases {
            let got = match classify_path(path) {
                ProcPath::PidFdDir(5) => "fddir",
                ProcPath::PidFdLink(5, _) => "fdlink",
                ProcPath::NotFound => "notfound",
                _ => "other",
            };
            if got != *want {
                serial_println!(
                    "[procfs]   FAIL: classify_path({:?}) = {}, want {}",
                    path, got, want
                );
                return Err(KernelError::InternalError);
            }
        }

        // Pure link-target rendering, one per fd kind.
        use crate::proc::linux_fd::FdEntry;
        let renders: &[(FdEntry, &str)] = &[
            (FdEntry::console(0), "/dev/console"),
            (FdEntry::pipe(42, 0), "pipe:[42]"),
            (FdEntry::eventfd(0, 0), "anon_inode:[eventfd]"),
            (FdEntry::pidfd(7, 0), "anon_inode:[pidfd]"),
            (FdEntry::memfd(0, 0), "anon_inode:[memfd]"),
        ];
        for (entry, want) in renders {
            let got = fd_link_target(entry);
            if got != *want {
                serial_println!(
                    "[procfs]   FAIL: fd_link_target = {:?}, want {:?}", got, want
                );
                return Err(KernelError::InternalError);
            }
        }
        // A File entry whose handle cannot be resolved must fall back to the
        // anon label, never an invented path and never empty/panic.
        let file_target = fd_link_target(&FdEntry::file(u64::MAX, 0));
        if file_target.is_empty() {
            serial_println!("[procfs]   FAIL: File fd target empty");
            return Err(KernelError::InternalError);
        }
        serial_println!(
            "[procfs]   fd/ classify: {} cases OK; link render OK", cases.len()
        );
    }

    // --- fd/ directory, live ---
    // Every PID directory now lists an `fd` subdirectory.  The bare
    // scheduler task running the self-test has no kernel fd table, so its
    // fd/ listing is legitimately empty and any `fd/<n>` link is NotFound —
    // exercising the native-process path without a live Linux-ABI process.
    {
        let probe = crate::sched::current_task_id();
        let dir_entries = fs.readdir(&format!("/{probe}"))?;
        if !dir_entries.iter().any(|e| {
            e.name == "fd" && e.entry_type == EntryType::Directory
        }) {
            serial_println!("[procfs]   FAIL: /{}/ missing `fd` subdirectory", probe);
            return Err(KernelError::InternalError);
        }
        // fd/ readdir must succeed (empty for a process with no kernel fd
        // table) rather than erroring.
        let fds = fs.readdir(&format!("/{probe}/fd"))?;
        serial_println!("[procfs]   /{}/fd: {} open fd(s) OK", probe, fds.len());
        // A link for an fd that isn't open must be NotFound.
        match fs.readlink(&format!("/{probe}/fd/0")) {
            Err(KernelError::NotFound) => {}
            other => {
                serial_println!(
                    "[procfs]   FAIL: /{}/fd/0 readlink = {:?}, want NotFound",
                    probe, other
                );
                return Err(KernelError::InternalError);
            }
        }
        serial_println!("[procfs]   fd/ directory: gating OK");
    }

    // --- fdinfo/ path classification + renderer ---
    // The `fdinfo/` subtree parallels `fd/` but yields regular files
    // (`<pid>/fdinfo` is a directory, `<pid>/fdinfo/<n>` a file).  The
    // body renderer and the cloexec-folding flags helper are pure, so we
    // pin their output deterministically without needing a live fd.
    {
        let cases: &[(&str, &str)] = &[
            ("5/fdinfo", "fdinfodir"),
            ("5/fdinfo/0", "fdinfofile"),
            ("5/fdinfo/255", "fdinfofile"),
            ("5/fdinfo/abc", "notfound"),  // non-numeric fd
            ("5/fdinfo/0/x", "notfound"),  // nested beyond an fd
        ];
        for (path, want) in cases {
            let got = match classify_path(path) {
                ProcPath::PidFdInfoDir(5) => "fdinfodir",
                ProcPath::PidFdInfoFile(5, _) => "fdinfofile",
                ProcPath::NotFound => "notfound",
                _ => "other",
            };
            if got != *want {
                serial_println!(
                    "[procfs]   FAIL: classify_path({:?}) = {}, want {}",
                    path, got, want
                );
                return Err(KernelError::InternalError);
            }
        }

        // Pure body render: Linux's `pos:\t..\nflags:\t0<octal>\n` shape.
        let body = render_pid_fdinfo(4096, 0o2);
        let btext = core::str::from_utf8(&body).unwrap_or("");
        if btext != "pos:\t4096\nflags:\t02\n" {
            serial_println!("[procfs]   FAIL: render_pid_fdinfo body = {:?}", btext);
            return Err(KernelError::InternalError);
        }

        // fdinfo_flags folds the descriptor's FD_CLOEXEC into O_CLOEXEC and
        // never double-counts a stale O_CLOEXEC already in status_flags.
        use crate::proc::linux_fd::{FdEntry, FD_CLOEXEC, O_CLOEXEC};
        // file(handle, status_flags): O_RDWR(2), no cloexec on the fd.
        let no_cloexec = FdEntry::file(1, 0o2);
        if fdinfo_flags(&no_cloexec) != 0o2 {
            serial_println!("[procfs]   FAIL: fdinfo_flags(no cloexec) wrong");
            return Err(KernelError::InternalError);
        }
        // Stale O_CLOEXEC in status_flags but FD_CLOEXEC unset -> cleared.
        let stale = FdEntry::file(1, 0o2 | O_CLOEXEC);
        if fdinfo_flags(&stale) != 0o2 {
            serial_println!("[procfs]   FAIL: fdinfo_flags(stale cloexec) wrong");
            return Err(KernelError::InternalError);
        }
        // FD_CLOEXEC set -> O_CLOEXEC appears regardless of status_flags.
        let mut cloexec = FdEntry::file(1, 0o2);
        cloexec.fd_flags = FD_CLOEXEC;
        if fdinfo_flags(&cloexec) != (0o2 | O_CLOEXEC) {
            serial_println!("[procfs]   FAIL: fdinfo_flags(cloexec) wrong");
            return Err(KernelError::InternalError);
        }
        serial_println!(
            "[procfs]   fdinfo/ classify: {} cases OK; render+flags OK", cases.len()
        );
    }

    // --- fdinfo/ directory, live ---
    // Mirrors the fd/ live test: every PID dir lists an `fdinfo`
    // subdirectory; the bare self-test task has no kernel fd table so the
    // listing is empty and any `fdinfo/<n>` file is NotFound.
    {
        let probe = crate::sched::current_task_id();
        let dir_entries = fs.readdir(&format!("/{probe}"))?;
        if !dir_entries.iter().any(|e| {
            e.name == "fdinfo" && e.entry_type == EntryType::Directory
        }) {
            serial_println!("[procfs]   FAIL: /{}/ missing `fdinfo` subdirectory", probe);
            return Err(KernelError::InternalError);
        }
        let fds = fs.readdir(&format!("/{probe}/fdinfo"))?;
        serial_println!("[procfs]   /{}/fdinfo: {} fd(s) OK", probe, fds.len());
        // Reading fdinfo for an fd that isn't open must be NotFound.
        match fs.read_file(&format!("/{probe}/fdinfo/0")) {
            Err(KernelError::NotFound) => {}
            other => {
                serial_println!(
                    "[procfs]   FAIL: /{}/fdinfo/0 read = {:?}, want NotFound",
                    probe, other.map(|d| d.len())
                );
                return Err(KernelError::InternalError);
            }
        }
        serial_println!("[procfs]   fdinfo/ directory: gating OK");
    }

    // --- thread stat, live ---
    // gen_thread_stat keys its thread-specific fields off the tid and its
    // process-wide fields off the owning pid.  A bogus tid must be NotFound;
    // for the probe task (which is a live scheduler task), calling it with
    // proc_id == tid == probe must succeed and report the probe as field 1.
    {
        let probe = crate::sched::current_task_id();
        match gen_thread_stat(probe, 999999) {
            Err(KernelError::NotFound) => {}
            other => {
                serial_println!(
                    "[procfs]   FAIL: gen_thread_stat bogus tid = {:?}, want NotFound",
                    other.map(|d| d.len())
                );
                return Err(KernelError::InternalError);
            }
        }
        let stat = gen_thread_stat(probe, probe)?;
        let text = core::str::from_utf8(&stat).unwrap_or("");
        let field1 = text.split(' ').next().unwrap_or("");
        if field1 != format!("{probe}") {
            serial_println!(
                "[procfs]   FAIL: thread stat field1 = {:?}, want {}", field1, probe
            );
            return Err(KernelError::InternalError);
        }
        serial_println!("[procfs]   thread stat: field1=={} OK", probe);

        // gen_thread_status: same id-space rules.  Bogus tid -> NotFound; for
        // the probe (proc_id == tid) the Pid: line must report the probe id,
        // and Tgid: must equal proc_id (here also the probe).
        match gen_thread_status(probe, 999999) {
            Err(KernelError::NotFound) => {}
            other => {
                serial_println!(
                    "[procfs]   FAIL: gen_thread_status bogus tid = {:?}, want NotFound",
                    other.map(|d| d.len())
                );
                return Err(KernelError::InternalError);
            }
        }
        let status = gen_thread_status(probe, probe)?;
        let stext = core::str::from_utf8(&status).unwrap_or("");
        let want_pid = format!("Pid:\t{probe}");
        let want_tgid = format!("Tgid:\t{probe}");
        if !stext.lines().any(|l| l == want_pid) {
            serial_println!(
                "[procfs]   FAIL: thread status missing {:?}", want_pid
            );
            return Err(KernelError::InternalError);
        }
        if !stext.lines().any(|l| l == want_tgid) {
            serial_println!(
                "[procfs]   FAIL: thread status missing {:?}", want_tgid
            );
            return Err(KernelError::InternalError);
        }
        serial_println!("[procfs]   thread status: Pid/Tgid=={} OK", probe);
    }

    // --- build_pid_stat starttime wiring (deterministic) ---
    // The live stat test above runs in the boot task (start_tick 0), so it
    // would still pass if field 22 were hardcoded 0.  Drive build_pid_stat
    // directly with a synthetic task carrying a distinctive start_tick and a
    // proc_id that maps to no process (process-wide lookups fall back to
    // their defaults); field 22 must echo the synthetic start_tick, proving
    // the field is really wired through and lands in the right column.
    {
        let mut name = [0u8; 32];
        name[..4].copy_from_slice(b"synt");
        let synth = crate::sched::TaskInfo {
            id: 4242,
            name,
            name_len: 4,
            state: crate::sched::task::TaskState::Ready,
            priority: 20,
            total_ticks: 7,
            user_ticks: 5,
            sys_ticks: 2,
            min_flt: 0,
            maj_flt: 0,

            nvcsw: 0,

            nivcsw: 0,
            total_cycles: 0,
            schedule_count: 0,
            start_tick: 99_999,
            last_cpu: 3,
            cpu_quota_pct: 0,
            throttled: false,
            total_wait_ticks: 0,
            max_wait_ticks: 0,
            stack_used: None,
            stack_pct: None,
        };
        let data = build_pid_stat(&synth, 999_999);
        let text = core::str::from_utf8(&data).unwrap_or("");
        let line = text.strip_suffix('\n').unwrap_or(text);
        let close = line.rfind(')').unwrap_or(0);
        let tail = line.get(close..).unwrap_or("");
        let rest: Vec<&str> = tail.strip_prefix(')').unwrap_or(tail)
            .trim_start().split(' ').filter(|s| !s.is_empty()).collect();
        // field 22 sits at index 22 - 3 == 19 of the post-comm fields.
        if rest.get(19).and_then(|f| f.parse::<u64>().ok()) != Some(99_999) {
            serial_println!(
                "[procfs]   FAIL: synthetic stat starttime = {:?}, want 99999",
                rest.get(19)
            );
            return Err(KernelError::InternalError);
        }
        // field 39 (processor) sits at index 39 - 3 == 36; must echo last_cpu.
        if rest.get(36).and_then(|f| f.parse::<usize>().ok()) != Some(3) {
            serial_println!(
                "[procfs]   FAIL: synthetic stat processor (field 39) = {:?}, want 3",
                rest.get(36)
            );
            return Err(KernelError::InternalError);
        }
        // field 14 (utime) sits at index 11; field 15 (stime) at index 12.
        // The synthetic task has user_ticks=5, sys_ticks=2 (sum == total 7).
        if rest.get(11).and_then(|f| f.parse::<u64>().ok()) != Some(5) {
            serial_println!(
                "[procfs]   FAIL: synthetic stat utime (field 14) = {:?}, want 5",
                rest.get(11)
            );
            return Err(KernelError::InternalError);
        }
        if rest.get(12).and_then(|f| f.parse::<u64>().ok()) != Some(2) {
            serial_println!(
                "[procfs]   FAIL: synthetic stat stime (field 15) = {:?}, want 2",
                rest.get(12)
            );
            return Err(KernelError::InternalError);
        }
        // field 1 must be the synthetic task id (sanity on the whole line).
        if line.split(' ').next() != Some("4242") {
            serial_println!("[procfs]   FAIL: synthetic stat field1 != 4242");
            return Err(KernelError::InternalError);
        }
        serial_println!("[procfs]   build_pid_stat: synthetic starttime+processor+utime/stime OK");
    }

    // --- stat <-> status State-char consistency (deterministic) ---
    // build_pid_stat (stat field 3) and build_pid_status (the `State:` line)
    // map TaskState -> single char via two INDEPENDENT match blocks.  Their
    // comments claim they never disagree; if a future edit changes one and not
    // the other, ps (reads stat) and htop (reads status) would report
    // different states for the same task.  Drive both with the same synthetic
    // task across every TaskState and assert the chars match.
    {
        use crate::sched::task::TaskState;
        let states = [
            TaskState::Running,
            TaskState::Ready,
            TaskState::Blocked,
            TaskState::Suspended,
            TaskState::Dead,
        ];
        for st in states {
            let mut name = [0u8; 32];
            name[..4].copy_from_slice(b"stcc");
            let synth = crate::sched::TaskInfo {
                id: 5151,
                name,
                name_len: 4,
                state: st,
                priority: 20,
                total_ticks: 0,
                user_ticks: 0,
                sys_ticks: 0,
                min_flt: 0,
                maj_flt: 0,

                nvcsw: 0,

                nivcsw: 0,
                total_cycles: 0,
                schedule_count: 0,
                start_tick: 0,
                last_cpu: 0,
                cpu_quota_pct: 0,
                throttled: false,
                total_wait_ticks: 0,
                max_wait_ticks: 0,
                stack_used: None,
                stack_pct: None,
            };
            // stat field 3 is the first token after the `(comm) ` prefix.  The
            // synthetic comm has no parens, so `") "` locates the boundary.
            let stat = build_pid_stat(&synth, 999_999);
            let stat_text = core::str::from_utf8(&stat).unwrap_or("");
            let stat_char = stat_text
                .rfind(") ")
                .and_then(|p| stat_text.get(p.saturating_add(2)..))
                .and_then(|s| s.chars().next());
            // status: the char immediately after `State:\t` (7 bytes).
            let status = build_pid_status(&synth, 999_999);
            let status_text = core::str::from_utf8(&status).unwrap_or("");
            let status_char = status_text
                .find("State:\t")
                .and_then(|p| status_text.get(p.saturating_add(7)..))
                .and_then(|s| s.chars().next());
            if stat_char.is_none() || stat_char != status_char {
                serial_println!(
                    "[procfs]   FAIL: stat/status State disagree: stat={:?} status={:?}",
                    stat_char,
                    status_char
                );
                return Err(KernelError::InternalError);
            }
        }
        serial_println!("[procfs]   stat/status State char: consistent across all states OK");
    }

    // --- comm truncation consistency (stat field 2 vs comm) ---
    // A name longer than TASK_COMM_LEN-1 (15) must be truncated identically in
    // build_pid_stat's `(comm)` token and in gen_pid_comm.  Drive both with a
    // 20-byte name and confirm they agree, and that the result is exactly 15
    // bytes, so strict parsers sizing buffers to TASK_COMM_LEN never overflow.
    {
        // Helper assertion first: comm_truncate caps at 15 on a boundary.
        let long = "0123456789abcdefghij"; // 20 ASCII bytes
        let cut = comm_truncate(long);
        if cut.len() != 15 || cut != "0123456789abcde" {
            serial_println!(
                "[procfs]   FAIL: comm_truncate(\"{long}\") = {:?} (len {}), want \"0123456789abcde\"",
                cut, cut.len()
            );
            return Err(KernelError::InternalError);
        }

        let mut name = [0u8; 32];
        name[..20].copy_from_slice(long.as_bytes());
        let synth = crate::sched::TaskInfo {
            id: 4243,
            name,
            name_len: 20,
            state: crate::sched::task::TaskState::Ready,
            priority: 20,
            total_ticks: 0,
            user_ticks: 0,
            sys_ticks: 0,
            min_flt: 0,
            maj_flt: 0,

            nvcsw: 0,

            nivcsw: 0,
            total_cycles: 0,
            schedule_count: 0,
            start_tick: 0,
            last_cpu: 0,
            cpu_quota_pct: 0,
            throttled: false,
            total_wait_ticks: 0,
            max_wait_ticks: 0,
            stack_used: None,
            stack_pct: None,
        };
        let data = build_pid_stat(&synth, 999_999);
        let text = core::str::from_utf8(&data).unwrap_or("");
        // comm is the token between the first '(' and the last ')'.
        let open = text.find('(').map_or(0, |i| i.saturating_add(1));
        let close = text.rfind(')').unwrap_or(open);
        let comm = text.get(open..close).unwrap_or("");
        if comm != cut {
            serial_println!(
                "[procfs]   FAIL: stat comm field = {:?}, want truncated {:?}",
                comm, cut
            );
            return Err(KernelError::InternalError);
        }
        serial_println!("[procfs]   build_pid_stat: comm truncated to 15 bytes OK");

        // /proc/<pid>/status `Name:` is sourced from comm too, so it must
        // carry the SAME 15-byte truncation.  Parse the first line.
        let sdata = build_pid_status(&synth, 999_999);
        let stext = core::str::from_utf8(&sdata).unwrap_or("");
        let name_line = stext.lines().next().unwrap_or("");
        let status_name = name_line.strip_prefix("Name:\t").unwrap_or("");
        if status_name != cut {
            serial_println!(
                "[procfs]   FAIL: status Name: field = {:?}, want truncated {:?}",
                status_name, cut
            );
            return Err(KernelError::InternalError);
        }
        serial_println!("[procfs]   build_pid_status: Name truncated to 15 bytes OK");

        // NoNewPrivs and Seccomp must be present (Linux always prints them) and,
        // for this synthetic proc_id with no PCB / no installed filter, both
        // default to 0.  Key-based parsers (systemd, WINE) grep these labels.
        if !stext.contains("\nNoNewPrivs:\t0\n") {
            serial_println!("[procfs]   FAIL: status missing 'NoNewPrivs:\\t0' line");
            return Err(KernelError::InternalError);
        }
        if !stext.contains("\nSeccomp:\t0\n") {
            serial_println!("[procfs]   FAIL: status missing 'Seccomp:\\t0' line");
            return Err(KernelError::InternalError);
        }
        // Linux orders NoNewPrivs/Seccomp after Threads and before the
        // ctxt-switch lines; verify that relative ordering holds.
        let p_threads = stext.find("\nThreads:\t");
        let p_nnp = stext.find("\nNoNewPrivs:\t");
        let p_vctx = stext.find("\nvoluntary_ctxt_switches:\t");
        if !(p_threads < p_nnp && p_nnp < p_vctx) {
            serial_println!(
                "[procfs]   FAIL: status field order Threads<NoNewPrivs<voluntary_ctxt_switches violated"
            );
            return Err(KernelError::InternalError);
        }
        serial_println!("[procfs]   build_pid_status: NoNewPrivs/Seccomp present and ordered OK");

        // Cpus_allowed/Cpus_allowed_list lines must be present and ordered
        // after Seccomp, before the ctxt-switch lines.
        if !stext.contains("\nCpus_allowed:\t") || !stext.contains("\nCpus_allowed_list:\t") {
            serial_println!("[procfs]   FAIL: status missing Cpus_allowed lines");
            return Err(KernelError::InternalError);
        }
        let p_seccomp = stext.find("\nSeccomp:\t");
        let p_cpus = stext.find("\nCpus_allowed:\t");
        let p_vctx2 = stext.find("\nvoluntary_ctxt_switches:\t");
        if !(p_seccomp < p_cpus && p_cpus < p_vctx2) {
            serial_println!(
                "[procfs]   FAIL: status order Seccomp<Cpus_allowed<voluntary_ctxt_switches violated"
            );
            return Err(KernelError::InternalError);
        }
        serial_println!("[procfs]   build_pid_status: Cpus_allowed present and ordered OK");

        // Mems_allowed/Mems_allowed_list lines must be present and ordered
        // after Cpus_allowed_list, before the ctxt-switch lines.
        if !stext.contains("\nMems_allowed:\t") || !stext.contains("\nMems_allowed_list:\t") {
            serial_println!("[procfs]   FAIL: status missing Mems_allowed lines");
            return Err(KernelError::InternalError);
        }
        let p_cpus_list = stext.find("\nCpus_allowed_list:\t");
        let p_mems = stext.find("\nMems_allowed:\t");
        if !(p_cpus_list < p_mems && p_mems < p_vctx2) {
            serial_println!(
                "[procfs]   FAIL: status order Cpus_allowed_list<Mems_allowed<voluntary_ctxt_switches violated"
            );
            return Err(KernelError::InternalError);
        }
        serial_println!("[procfs]   build_pid_status: Mems_allowed present and ordered OK");
    }

    // --- Bitmap hex/list formatter unit tests (deterministic) ---
    // Exact Linux %*pb (bitmap_string) expectations across chunk boundaries.
    // These formatters back both Cpus_allowed and Mems_allowed, so the cases
    // cover small (NUMA-node-style) widths as well as full CPU-mask widths.
    {
        let hex_cases: [(u64, usize, &str); 12] = [
            (0xff, 8, "ff"),
            (1, 8, "01"),
            (0xfff, 12, "fff"),
            (1, 12, "001"),
            (0xffff_ffff, 32, "ffffffff"),
            (1, 32, "00000001"),
            (u64::MAX, 64, "ffffffff,ffffffff"),
            (1, 64, "00000000,00000001"),
            (0xff_ffff_ffff, 40, "ff,ffffffff"),
            // Small NUMA-node-style widths (Mems_allowed).
            (1, 1, "1"),
            (3, 2, "3"),
            (0xff, 8, "ff"),
        ];
        for (mask, nbits, want) in hex_cases {
            let got = format_bitmap_hex(mask, nbits);
            if got != want {
                serial_println!(
                    "[procfs]   FAIL: format_bitmap_hex({mask:#x}, {nbits}) = {:?}, want {:?}",
                    got, want
                );
                return Err(KernelError::InternalError);
            }
        }
        let list_cases: [(u64, usize, &str); 7] = [
            (0xff, 8, "0-7"),
            (0b1101, 8, "0,2-3"),
            (1, 8, "0"),
            (0, 8, ""),
            (0b1010_0001, 8, "0,5,7"),
            // Small NUMA-node-style widths (Mems_allowed_list).
            (1, 1, "0"),
            (3, 2, "0-1"),
        ];
        for (mask, nbits, want) in list_cases {
            let got = format_bitmap_list(mask, nbits);
            if got != want {
                serial_println!(
                    "[procfs]   FAIL: format_bitmap_list({mask:#x}, {nbits}) = {:?}, want {:?}",
                    got, want
                );
                return Err(KernelError::InternalError);
            }
        }
        serial_println!("[procfs]   format_bitmap_hex/list: all bitmap cases OK");
    }

    // --- Per-PID directory tests ---

    // Get the current task ID to test against a known-live PID.
    let current_tid = crate::sched::current_task_id();
    let pid_path = format!("/{current_tid}");
    let status_path = format!("/{current_tid}/status");

    // stat on PID directory.
    let pid_stat = fs.stat(&pid_path)?;
    if pid_stat.entry_type != EntryType::Directory {
        serial_println!("[procfs]   FAIL: stat {pid_path} not a directory");
        return Err(KernelError::InternalError);
    }
    serial_println!("[procfs]   stat {}: directory OK", pid_path);

    // readdir on PID directory — PID_FILES + PID_LINKS, plus the three
    // subdirectories every PID directory exposes: `task` (per-thread tree),
    // `fd` (open file descriptors) and `fdinfo` (per-fd pos/flags).
    let pid_entries = fs.readdir(&pid_path)?;
    let expected_pid_entries = PID_FILES.len() + PID_LINKS.len() + 3;
    if pid_entries.len() != expected_pid_entries {
        serial_println!(
            "[procfs]   FAIL: readdir {} returned {} entries, expected {}",
            pid_path, pid_entries.len(), expected_pid_entries
        );
        return Err(KernelError::InternalError);
    }
    // The extra entries must be the `task`, `fd` and `fdinfo` directories.
    for subdir in ["task", "fd", "fdinfo"] {
        if !pid_entries.iter().any(|e| {
            e.name == subdir && e.entry_type == EntryType::Directory
        }) {
            serial_println!(
                "[procfs]   FAIL: readdir {} missing `{}` subdirectory",
                pid_path, subdir
            );
            return Err(KernelError::InternalError);
        }
    }
    serial_println!("[procfs]   readdir {}: {} entries OK", pid_path, pid_entries.len());

    // read_file on status.
    let status_data = fs.read_file(&status_path)?;
    if status_data.is_empty() {
        serial_println!("[procfs]   FAIL: read_file {} returned empty", status_path);
        return Err(KernelError::InternalError);
    }
    let status_text = core::str::from_utf8(&status_data)
        .map_err(|_| KernelError::InternalError)?;
    // Verify the Linux /proc/<pid>/status shape: the well-known tab-separated
    // keys that ps/htop/glibc/WINE scrape must be present.
    for key in ["Name:\t", "State:\t", "Tgid:\t", "Pid:\t", "PPid:\t",
                "Uid:\t", "Gid:\t", "Threads:\t"] {
        if !status_text.contains(key) {
            serial_println!("[procfs]   FAIL: status missing key {:?}", key);
            return Err(KernelError::InternalError);
        }
    }
    // Pid: line must carry this task's id, and Tgid must equal Pid (no
    // separate thread-group object) — consistent with getpid()/gettid().
    let pid_line = format!("Pid:\t{current_tid}\n");
    let tgid_line = format!("Tgid:\t{current_tid}\n");
    if !status_text.contains(&pid_line) || !status_text.contains(&tgid_line) {
        serial_println!(
            "[procfs]   FAIL: status Pid/Tgid not {} (getpid-consistent)",
            current_tid
        );
        return Err(KernelError::InternalError);
    }
    // State: must be one of the Linux "<char> (<word>)" strings, and its
    // leading char must match the single-char state in /proc/<pid>/stat.
    if !["R (running)", "S (sleeping)", "T (stopped)", "Z (zombie)"]
        .iter()
        .any(|st| status_text.contains(&format!("State:\t{st}\n")))
    {
        serial_println!("[procfs]   FAIL: status State: not a Linux state string");
        return Err(KernelError::InternalError);
    }
    serial_println!("[procfs]   {}/status: Linux format, {} bytes OK",
        current_tid, status_data.len());

    // read_file on PID directory should fail (IsADirectory).
    if fs.read_file(&pid_path).is_ok() {
        serial_println!("[procfs]   FAIL: read_file on PID dir should fail");
        return Err(KernelError::InternalError);
    }
    serial_println!("[procfs]   read_file on PID dir: IsADirectory OK");

    // stat on nonexistent PID should fail.
    if fs.stat("/999999").is_ok() {
        serial_println!("[procfs]   FAIL: stat on bogus PID should fail");
        return Err(KernelError::InternalError);
    }
    serial_println!("[procfs]   stat /999999: NotFound OK");

    // --- /proc/self magic symlink ---
    // The bare /proc/self must be a SYMLINK (not a directory), readlink must
    // resolve to the caller's pid, reading its bytes directly must be
    // rejected, and a path *under* it (self/status) must resolve to the
    // current task's file — matching Linux's /proc/self semantics.
    let self_stat = fs.stat("/self")?;
    if self_stat.entry_type != EntryType::Symlink {
        serial_println!(
            "[procfs]   FAIL: /self is {:?}, expected Symlink", self_stat.entry_type
        );
        return Err(KernelError::InternalError);
    }
    let self_target = fs.readlink("/self")?;
    if self_target.parse::<u64>() != Ok(current_tid) {
        serial_println!(
            "[procfs]   FAIL: /self -> {:?}, expected pid {}", self_target, current_tid
        );
        return Err(KernelError::InternalError);
    }
    if fs.read_file("/self") != Err(KernelError::InvalidArgument) {
        serial_println!("[procfs]   FAIL: read_file(/self) should be InvalidArgument");
        return Err(KernelError::InternalError);
    }
    // self/status must resolve to the live current task's status file.
    let self_status = fs.read_file("/self/status")?;
    if self_status.is_empty() {
        serial_println!("[procfs]   FAIL: /self/status returned empty");
        return Err(KernelError::InternalError);
    }
    serial_println!("[procfs]   /self -> {:?} (symlink) OK", self_target);

    // --- /proc/<pid>/maps rendering ---
    // render_maps is the pure formatter behind the maps file; exercise it
    // with synthetic VMAs (no PCB needed) to lock down the Linux line
    // format and the perms/pathname mapping for each VMA kind.
    {
        use crate::mm::page_table::PageFlags;
        use crate::mm::vma::{Vma, VmaKind};
        let vmas = [
            // Anonymous data: present, user, writable, no-exec → "rw-p", no path.
            Vma {
                start: 0x1_0000,
                end: 0x2_0000,
                kind: VmaKind::Anonymous,
                flags: PageFlags::PRESENT
                    | PageFlags::USER_ACCESSIBLE
                    | PageFlags::WRITABLE
                    | PageFlags::NO_EXECUTE,
            },
            // Stack: present, user, writable, no-exec → "rw-p [stack]".
            Vma {
                start: 0x10_0000,
                end: 0x14_0000,
                kind: VmaKind::Stack,
                flags: PageFlags::PRESENT
                    | PageFlags::USER_ACCESSIBLE
                    | PageFlags::WRITABLE
                    | PageFlags::NO_EXECUTE,
            },
            // Guard: never mapped → "---p [guard]".
            Vma {
                start: 0x0c_0000,
                end: 0x10_0000,
                kind: VmaKind::Guard,
                flags: PageFlags::empty(),
            },
            // Large (>32-bit) address: must print natural width, NOT padded
            // beyond its own digits — verifies the {:08x} minimum-width does
            // not truncate or over-pad wide addresses (user executable text).
            Vma {
                start: 0x5555_5555_0000,
                end: 0x5555_5556_0000,
                kind: VmaKind::Anonymous,
                flags: PageFlags::PRESENT | PageFlags::USER_ACCESSIBLE,
            },
            // PROT_NONE anonymous: PRESENT but NOT user-accessible → "---p"
            // (real guard/trap region; the 'r' bit must drop, design-decisions
            // §32).  NO_EXECUTE is set (no PROT_EXEC) but renders moot since the
            // region is wholly inaccessible.
            Vma {
                start: 0x20_0000,
                end: 0x21_0000,
                kind: VmaKind::Anonymous,
                flags: PageFlags::PRESENT | PageFlags::NO_EXECUTE,
            },
        ];
        let rendered = render_maps(&vmas);
        let maps_text = core::str::from_utf8(&rendered)
            .map_err(|_| KernelError::InternalError)?;
        let lines: Vec<&str> = maps_text.lines().collect();
        // Start/end zero-padded to a minimum of 8 hex digits, matching Linux's
        // %08lx; the wide address keeps its natural 12-digit width.
        let expected = [
            "00010000-00020000 rw-p 00000000 00:00 0 ",
            "00100000-00140000 rw-p 00000000 00:00 0 [stack]",
            "000c0000-00100000 ---p 00000000 00:00 0 [guard]",
            "555555550000-555555560000 r-xp 00000000 00:00 0 ",
            "00200000-00210000 ---p 00000000 00:00 0 ",
        ];
        if lines.len() != expected.len() {
            serial_println!(
                "[procfs]   FAIL: maps rendered {} lines, expected {}",
                lines.len(), expected.len()
            );
            return Err(KernelError::InternalError);
        }
        for (got, want) in lines.iter().zip(expected.iter()) {
            if got != want {
                serial_println!(
                    "[procfs]   FAIL: maps line {:?} != expected {:?}", got, want
                );
                return Err(KernelError::InternalError);
            }
        }
        serial_println!("[procfs]   maps render: {} VMA lines OK", lines.len());
    }

    // --- /proc/<pid>/mountinfo rendering ---
    // render_mountinfo is the pure formatter behind the mountinfo file;
    // exercise it with a synthetic mount list (no VFS lock needed) to lock
    // down the 11-field Linux layout, the id/minor numbering, and that the
    // options string appears in both the per-mount and super-options slots.
    {
        use crate::fs::vfs::MountOptions;
        let mounts = [
            (String::from("/"), String::from("ext4"), MountOptions::defaults()),
            (
                String::from("/tmp"),
                String::from("tmpfs"),
                MountOptions::parse("ro,noatime"),
            ),
            // A mount point containing a space exercises the Linux
            // `mangle()`-equivalent escaping: the space must become `\040`
            // so the space-separated layout stays parseable.
            (
                String::from("/mnt/my disk"),
                String::from("ext4"),
                MountOptions::defaults(),
            ),
        ];
        let rendered = render_mountinfo(&mounts);
        let mi_text = core::str::from_utf8(&rendered)
            .map_err(|_| KernelError::InternalError)?;
        let lines: Vec<&str> = mi_text.lines().collect();
        let expected = [
            "20 20 0:1 / / rw - ext4 none rw",
            "21 20 0:2 / /tmp ro,noatime - tmpfs none ro,noatime",
            "22 20 0:3 / /mnt/my\\040disk rw - ext4 none rw",
        ];
        if lines.len() != expected.len() {
            serial_println!(
                "[procfs]   FAIL: mountinfo rendered {} lines, expected {}",
                lines.len(), expected.len()
            );
            return Err(KernelError::InternalError);
        }
        for (got, want) in lines.iter().zip(expected.iter()) {
            if got != want {
                serial_println!(
                    "[procfs]   FAIL: mountinfo line {:?} != expected {:?}", got, want
                );
                return Err(KernelError::InternalError);
            }
        }
        serial_println!("[procfs]   mountinfo render: {} mount lines OK", lines.len());
    }

    // --- /proc/<pid>/mountinfo for a container (jailed) process ---
    // render_container_mountinfo maps a container's own mount view onto the
    // mountinfo layout: the rootfs at guest `/`, then each volume/tmpfs at its
    // guest path.  Each entry's fstype is resolved from the host mount backing
    // it, the source is hidden (`none`), and the rw/ro flag comes from the
    // container's own view.  This pins that a container sees ITS mounts, not
    // the host's, and never leaks host backing paths.
    {
        use crate::fs::vfs::MountOptions;
        use crate::ipc::namespace::MountViewEntry;
        // Host mount table the container's targets resolve their fstype against:
        // the rootfs overlay, the ext4 host root, and a tmpfs-backing memfs.
        let global = [
            (String::from("/"), String::from("ext4"), MountOptions::defaults()),
            (
                String::from("/containers/c1/rootfs"),
                String::from("overlay"),
                MountOptions::defaults(),
            ),
            (
                String::from("/var/lib/slate/tmpfs/1-0"),
                String::from("tmpfs"),
                MountOptions::defaults(),
            ),
        ];
        // Container view: read-only rootfs, a read-only bind volume served by
        // the ext4 host root, and a writable tmpfs.
        let view = [
            MountViewEntry {
                guest_path: String::from("/"),
                host_target: String::from("/containers/c1/rootfs"),
                read_only: true,
            },
            MountViewEntry {
                guest_path: String::from("/logs"),
                host_target: String::from("/var/log/app"),
                read_only: true,
            },
            MountViewEntry {
                guest_path: String::from("/tmp"),
                host_target: String::from("/var/lib/slate/tmpfs/1-0"),
                read_only: false,
            },
        ];
        let rendered = render_container_mountinfo(&view, &global);
        let text = core::str::from_utf8(&rendered)
            .map_err(|_| KernelError::InternalError)?;
        let lines: Vec<&str> = text.lines().collect();
        let expected = [
            // rootfs `/` → overlay, read-only, source hidden.
            "20 20 0:1 / / ro - overlay none ro",
            // /logs bind → served by the ext4 host root, read-only.
            "21 20 0:2 / /logs ro - ext4 none ro",
            // /tmp tmpfs → served by the memfs mount, writable.
            "22 20 0:3 / /tmp rw - tmpfs none rw",
        ];
        if lines.len() != expected.len() {
            serial_println!(
                "[procfs]   FAIL: container mountinfo {} lines, expected {}",
                lines.len(), expected.len()
            );
            return Err(KernelError::InternalError);
        }
        for (got, want) in lines.iter().zip(expected.iter()) {
            if got != want {
                serial_println!(
                    "[procfs]   FAIL: container mountinfo {:?} != {:?}", got, want
                );
                return Err(KernelError::InternalError);
            }
        }
        // Boundary safety: a `/data` mount must not be reported as covering
        // `/database` (prefix without a path separator).
        if mount_path_covers("/data", "/database") {
            serial_println!("[procfs]   FAIL: /data must not cover /database");
            return Err(KernelError::InternalError);
        }
        if !mount_path_covers("/data", "/data/x") || !mount_path_covers("/", "/anything") {
            serial_println!("[procfs]   FAIL: mount_path_covers parent/root check");
            return Err(KernelError::InternalError);
        }
        serial_println!("[procfs]   container mountinfo render: {} lines OK", lines.len());

        // The `/proc/mounts` line format for the same container view:
        // `none <mount_point> <fstype> <opts> 0 0`, source hidden.
        let mounts_rendered = render_container_mounts(&view, &global);
        let mounts_text = core::str::from_utf8(&mounts_rendered)
            .map_err(|_| KernelError::InternalError)?;
        let mounts_lines: Vec<&str> = mounts_text.lines().collect();
        let mounts_expected = [
            "none / overlay ro 0 0",
            "none /logs ext4 ro 0 0",
            "none /tmp tmpfs rw 0 0",
        ];
        if mounts_lines.len() != mounts_expected.len() {
            serial_println!(
                "[procfs]   FAIL: container mounts {} lines, expected {}",
                mounts_lines.len(), mounts_expected.len()
            );
            return Err(KernelError::InternalError);
        }
        for (got, want) in mounts_lines.iter().zip(mounts_expected.iter()) {
            if got != want {
                serial_println!(
                    "[procfs]   FAIL: container mounts {:?} != {:?}", got, want
                );
                return Err(KernelError::InternalError);
            }
        }
        serial_println!("[procfs]   container mounts render: {} lines OK", mounts_lines.len());
    }

    // --- /proc/<pid>/cgroup rendering ---
    // render_cgroup is the pure formatter behind the cgroup file; exercise
    // it with a synthetic group list (no cgroupfs lock needed) to confirm
    // the v2 "0::<path>" line, that an assigned PID resolves to its group
    // path, and that an unassigned PID falls back to the root cgroup "/".
    {
        use crate::fs::cgroupfs::Cgroup;
        let groups = [
            Cgroup {
                path: String::from("/"),
                cpu_weight: 100, cpu_max_us: 0, memory_max: 0, memory_current: 0,
                io_weight: 100, pids_max: 0, pids_current: 0,
                processes: Vec::new(), created_ns: 0,
                kernel_id: crate::cgroup::ROOT_CGROUP,
            },
            Cgroup {
                path: String::from("/app.slice"),
                cpu_weight: 100, cpu_max_us: 0, memory_max: 0, memory_current: 0,
                io_weight: 100, pids_max: 0, pids_current: 1,
                processes: alloc::vec![42u32], created_ns: 0,
                kernel_id: crate::cgroup::ROOT_CGROUP,
            },
        ];
        let assigned = render_cgroup(42, &groups);
        let unassigned = render_cgroup(99, &groups);
        let assigned_text = core::str::from_utf8(&assigned)
            .map_err(|_| KernelError::InternalError)?;
        let unassigned_text = core::str::from_utf8(&unassigned)
            .map_err(|_| KernelError::InternalError)?;
        if assigned_text != "0::/app.slice\n" {
            serial_println!(
                "[procfs]   FAIL: cgroup assigned {:?} != \"0::/app.slice\\n\"",
                assigned_text
            );
            return Err(KernelError::InternalError);
        }
        if unassigned_text != "0::/\n" {
            serial_println!(
                "[procfs]   FAIL: cgroup unassigned {:?} != \"0::/\\n\"",
                unassigned_text
            );
            return Err(KernelError::InternalError);
        }
        serial_println!("[procfs]   cgroup render: assigned + root fallback OK");

        // /proc/<pid>/cpuset shares the unified hierarchy with cgroup, so it
        // reports the same membership path but as a bare newline-terminated
        // line (no "0::" prefix).  Reuse the same synthetic group list.
        let cs_assigned = render_cpuset(42, &groups);
        let cs_unassigned = render_cpuset(99, &groups);
        let cs_assigned_text = core::str::from_utf8(&cs_assigned)
            .map_err(|_| KernelError::InternalError)?;
        let cs_unassigned_text = core::str::from_utf8(&cs_unassigned)
            .map_err(|_| KernelError::InternalError)?;
        if cs_assigned_text != "/app.slice\n" {
            serial_println!(
                "[procfs]   FAIL: cpuset assigned {:?} != \"/app.slice\\n\"",
                cs_assigned_text
            );
            return Err(KernelError::InternalError);
        }
        if cs_unassigned_text != "/\n" {
            serial_println!(
                "[procfs]   FAIL: cpuset unassigned {:?} != \"/\\n\"",
                cs_unassigned_text
            );
            return Err(KernelError::InternalError);
        }
        serial_println!("[procfs]   cpuset render: assigned + root fallback OK");
    }

    // --- /proc/<pid>/oom_score{,_adj} rendering ---
    // Pure formatters: oom_score folds adj into the base badness and
    // clamps to 0..=1000; oom_score_adj echoes the adjustment verbatim.
    {
        let cases = [
            (render_oom_score(500, 0), "500\n"),
            (render_oom_score(800, 200), "1000\n"),   // capped at 1000
            (render_oom_score(100, -500), "0\n"),     // floored at 0
            (render_oom_score_adj(-1000), "-1000\n"),
            (render_oom_score_adj(200), "200\n"),
        ];
        for (got, want) in &cases {
            let got_text = core::str::from_utf8(got)
                .map_err(|_| KernelError::InternalError)?;
            if got_text != *want {
                serial_println!(
                    "[procfs]   FAIL: oom render {:?} != {:?}", got_text, want
                );
                return Err(KernelError::InternalError);
            }
        }
        serial_println!("[procfs]   oom_score/oom_score_adj render: {} cases OK", cases.len());
    }

    // --- /proc/<pid>/oom_score_adj write parsing ---
    // parse_oom_score_adj is the pure parser behind the writable file: it
    // trims whitespace/newlines, parses a signed integer, and rejects the
    // out-of-range and malformed cases with InvalidArgument.
    {
        // Accepted forms, including the shell's trailing newline and spaces.
        let ok_cases: [(&[u8], i32); 6] = [
            (b"0", 0),
            (b"500\n", 500),
            (b"-1000\n", -1000),
            (b"1000", 1000),
            (b"  42  \n", 42),
            (b"-7", -7),
        ];
        for (input, want) in ok_cases {
            match parse_oom_score_adj(input) {
                Ok(v) if v == want => {}
                other => {
                    serial_println!(
                        "[procfs]   FAIL: parse_oom_score_adj({:?}) = {:?}, want {}",
                        input, other, want
                    );
                    return Err(KernelError::InternalError);
                }
            }
        }
        // Rejected forms: out of range, empty, non-numeric.
        let bad_cases: [&[u8]; 5] = [b"1001", b"-1001", b"", b"abc", b"12x"];
        for input in bad_cases {
            if parse_oom_score_adj(input).is_ok() {
                serial_println!(
                    "[procfs]   FAIL: parse_oom_score_adj({:?}) accepted, want InvalidArgument",
                    input
                );
                return Err(KernelError::InternalError);
            }
        }
        serial_println!(
            "[procfs]   oom_score_adj parse: {} accept + {} reject OK",
            ok_cases.len(), bad_cases.len()
        );
    }

    // --- /proc/<pid>/schedstat rendering ---
    // Pure formatter: three space-separated integers (cpu_ns run_delay_ns
    // timeslices) on one line, newline-terminated.
    {
        let rendered = render_schedstat(12_500_000, 3_000_000, 7);
        let sched_text = core::str::from_utf8(&rendered)
            .map_err(|_| KernelError::InternalError)?;
        if sched_text != "12500000 3000000 7\n" {
            serial_println!(
                "[procfs]   FAIL: schedstat render {:?} != \"12500000 3000000 7\\n\"",
                sched_text
            );
            return Err(KernelError::InternalError);
        }
        serial_println!("[procfs]   schedstat render: OK");
    }

    // --- /proc/<pid>/io rendering ---
    // Pure formatter: seven `key: value\n` lines.  rchar/wchar/syscr/syscw
    // carry the supplied honestly-tracked counters; the three storage-layer
    // counters are always 0 (untracked — we never fabricate them).
    {
        let rendered = render_pid_io(4096, 1024, 12, 3);
        let io_text = core::str::from_utf8(&rendered)
            .map_err(|_| KernelError::InternalError)?;
        let expected = "rchar: 4096\n\
                        wchar: 1024\n\
                        syscr: 12\n\
                        syscw: 3\n\
                        read_bytes: 0\n\
                        write_bytes: 0\n\
                        cancelled_write_bytes: 0\n";
        if io_text != expected {
            serial_println!("[procfs]   FAIL: io render {:?} != {:?}", io_text, expected);
            return Err(KernelError::InternalError);
        }
        // The live file is process-only (gated on PCB existence, like
        // oom_score): when it resolves it must expose all seven Linux keys;
        // a bare scheduler task with no PCB legitimately returns NotFound.
        match fs.read_file(&format!("/{current_tid}/io")) {
            Ok(io_data) => {
                let live = core::str::from_utf8(&io_data)
                    .map_err(|_| KernelError::InternalError)?;
                for key in [
                    "rchar:", "wchar:", "syscr:", "syscw:",
                    "read_bytes:", "write_bytes:", "cancelled_write_bytes:",
                ] {
                    if !live.contains(key) {
                        serial_println!(
                            "[procfs]   FAIL: /{}/io missing key {:?}", current_tid, key
                        );
                        return Err(KernelError::InternalError);
                    }
                }
                serial_println!("[procfs]   io render + live read: OK");
            }
            Err(KernelError::NotFound) => {
                serial_println!(
                    "[procfs]   io render OK; /{}/io NotFound (bare task, no PCB) OK",
                    current_tid
                );
            }
            Err(e) => {
                serial_println!("[procfs]   FAIL: /{}/io unexpected error {:?}", current_tid, e);
                return Err(KernelError::InternalError);
            }
        }
    }

    // --- /proc/<pid>/loginuid,sessionid rendering ---
    // Pure formatter: bare decimal, NO trailing newline (Linux audit
    // files); unset audit id is u32::MAX.
    {
        let cases = [
            (render_audit_id(AUDIT_UNSET), "4294967295"),
            (render_audit_id(0), "0"),
            (render_audit_id(1000), "1000"),
        ];
        for (got, want) in &cases {
            let got_text = core::str::from_utf8(got)
                .map_err(|_| KernelError::InternalError)?;
            if got_text != *want {
                serial_println!(
                    "[procfs]   FAIL: audit_id render {:?} != {:?}", got_text, want
                );
                return Err(KernelError::InternalError);
            }
        }
        serial_println!("[procfs]   loginuid/sessionid render: {} cases OK", cases.len());
    }

    // --- New per-PID files: comm, statm, limits ---

    // /proc/<pid>/comm — non-empty, newline-terminated, <= 16 bytes
    // (TASK_COMM_LEN), matching Linux's `comm` shape.
    let comm_data = fs.read_file(&format!("/{current_tid}/comm"))?;
    if comm_data.is_empty()
        || comm_data.last() != Some(&b'\n')
        || comm_data.len() > 16
    {
        serial_println!(
            "[procfs]   FAIL: comm malformed (len={}, last={:?})",
            comm_data.len(), comm_data.last()
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[procfs]   {}/comm: {} bytes OK", current_tid, comm_data.len());

    // /proc/<pid>/limits — works for any live task (falls back to
    // DEFAULT_RLIMITS without a PCB).  Must carry the Linux header and
    // the well-known rows tools scrape.
    let limits_data = fs.read_file(&format!("/{current_tid}/limits"))?;
    let limits_text = core::str::from_utf8(&limits_data)
        .map_err(|_| KernelError::InternalError)?;
    if !limits_text.contains("Soft Limit")
        || !limits_text.contains("Max open files")
        || !limits_text.contains("Max stack size")
    {
        serial_println!("[procfs]   FAIL: limits missing expected rows");
        return Err(KernelError::InternalError);
    }
    serial_println!("[procfs]   {}/limits: {} bytes OK", current_tid, limits_data.len());

    // /proc/<pid>/statm — only processes carry the address-space charge,
    // so a bare scheduler task legitimately returns NotFound.  When it
    // does succeed, it must be seven space-separated integers + newline.
    match fs.read_file(&format!("/{current_tid}/statm")) {
        Ok(statm_data) => {
            let statm_text = core::str::from_utf8(&statm_data)
                .map_err(|_| KernelError::InternalError)?;
            let trimmed = statm_text.strip_suffix('\n').unwrap_or(statm_text);
            let fields: Vec<&str> = trimmed.split(' ').collect();
            if fields.len() != 7
                || !fields.iter().all(|f| f.parse::<u64>().is_ok())
            {
                serial_println!(
                    "[procfs]   FAIL: statm not 7 integers ({:?})", trimmed
                );
                return Err(KernelError::InternalError);
            }
            serial_println!("[procfs]   {}/statm: 7 fields OK", current_tid);
        }
        Err(KernelError::NotFound) => {
            serial_println!(
                "[procfs]   {}/statm: NotFound (bare task, no AS charge) OK",
                current_tid
            );
        }
        Err(e) => {
            serial_println!("[procfs]   FAIL: statm unexpected error {:?}", e);
            return Err(KernelError::InternalError);
        }
    }

    // /proc/<pid>/mountinfo — process-only (like statm), so a bare
    // scheduler task legitimately returns NotFound.  When it succeeds,
    // every line must carry the `-` separator field and at least the 10
    // mandatory positional fields that precede the super-options.
    match fs.read_file(&format!("/{current_tid}/mountinfo")) {
        Ok(mi_data) => {
            let mi_text = core::str::from_utf8(&mi_data)
                .map_err(|_| KernelError::InternalError)?;
            for line in mi_text.lines() {
                let fields: Vec<&str> = line.split(' ').collect();
                // mount_id parent maj:min root mp opts - fstype src superopts
                if fields.len() < 10 || !fields.contains(&"-") {
                    serial_println!(
                        "[procfs]   FAIL: mountinfo line malformed ({:?})", line
                    );
                    return Err(KernelError::InternalError);
                }
            }
            serial_println!(
                "[procfs]   {}/mountinfo: {} bytes OK", current_tid, mi_data.len()
            );
        }
        Err(KernelError::NotFound) => {
            serial_println!(
                "[procfs]   {}/mountinfo: NotFound (bare task, no PCB) OK",
                current_tid
            );
        }
        Err(e) => {
            serial_println!("[procfs]   FAIL: mountinfo unexpected error {:?}", e);
            return Err(KernelError::InternalError);
        }
    }

    // /proc/<pid>/cgroup — process-only (like statm/mountinfo), so a bare
    // scheduler task returns NotFound.  When it succeeds, it must be the
    // single cgroup v2 line "0::<path>\n".
    match fs.read_file(&format!("/{current_tid}/cgroup")) {
        Ok(cg_data) => {
            let cg_text = core::str::from_utf8(&cg_data)
                .map_err(|_| KernelError::InternalError)?;
            if !cg_text.starts_with("0::/") || cg_text.lines().count() != 1 {
                serial_println!(
                    "[procfs]   FAIL: cgroup malformed ({:?})", cg_text
                );
                return Err(KernelError::InternalError);
            }
            serial_println!(
                "[procfs]   {}/cgroup: {} bytes OK", current_tid, cg_data.len()
            );
        }
        Err(KernelError::NotFound) => {
            serial_println!(
                "[procfs]   {}/cgroup: NotFound (bare task, no PCB) OK",
                current_tid
            );
        }
        Err(e) => {
            serial_println!("[procfs]   FAIL: cgroup unexpected error {:?}", e);
            return Err(KernelError::InternalError);
        }
    }

    // /proc/<pid>/cpuset — process-only; when it succeeds it must be a
    // single absolute-path line (starts with "/", no "0::" prefix), the
    // bare cgroup path shared with the cgroup file.
    match fs.read_file(&format!("/{current_tid}/cpuset")) {
        Ok(cs_data) => {
            let cs_text = core::str::from_utf8(&cs_data)
                .map_err(|_| KernelError::InternalError)?;
            if !cs_text.starts_with('/') || cs_text.lines().count() != 1 {
                serial_println!(
                    "[procfs]   FAIL: cpuset malformed ({:?})", cs_text
                );
                return Err(KernelError::InternalError);
            }
            serial_println!(
                "[procfs]   {}/cpuset: {} bytes OK", current_tid, cs_data.len()
            );
        }
        Err(KernelError::NotFound) => {
            serial_println!(
                "[procfs]   {}/cpuset: NotFound (bare task, no PCB) OK",
                current_tid
            );
        }
        Err(e) => {
            serial_println!("[procfs]   FAIL: cpuset unexpected error {:?}", e);
            return Err(KernelError::InternalError);
        }
    }

    // /proc/<pid>/oom_score and oom_score_adj — process-only.  When they
    // succeed, each must be a single integer line within Linux's ranges
    // (oom_score 0..=1000, oom_score_adj -1000..=1000).
    for (name, lo, hi) in [("oom_score", 0i32, 1000i32), ("oom_score_adj", -1000, 1000)] {
        match fs.read_file(&format!("/{current_tid}/{name}")) {
            Ok(data) => {
                let text = core::str::from_utf8(&data)
                    .map_err(|_| KernelError::InternalError)?;
                let trimmed = text.strip_suffix('\n').unwrap_or(text);
                match trimmed.parse::<i32>() {
                    Ok(v) if (lo..=hi).contains(&v) => {
                        serial_println!("[procfs]   {}/{}: {} OK", current_tid, name, v);
                    }
                    _ => {
                        serial_println!(
                            "[procfs]   FAIL: {} out of range/parse ({:?})", name, trimmed
                        );
                        return Err(KernelError::InternalError);
                    }
                }
            }
            Err(KernelError::NotFound) => {
                serial_println!(
                    "[procfs]   {}/{}: NotFound (bare task, no PCB) OK", current_tid, name
                );
            }
            Err(e) => {
                serial_println!("[procfs]   FAIL: {} unexpected error {:?}", name, e);
                return Err(KernelError::InternalError);
            }
        }
    }

    // /proc/<pid>/schedstat — served for any live scheduler task.  Must be
    // exactly three space-separated integers, newline-terminated.
    match fs.read_file(&format!("/{current_tid}/schedstat")) {
        Ok(sched_data) => {
            let sched_text = core::str::from_utf8(&sched_data)
                .map_err(|_| KernelError::InternalError)?;
            let trimmed = sched_text.strip_suffix('\n').unwrap_or(sched_text);
            let fields: Vec<&str> = trimmed.split(' ').collect();
            if fields.len() != 3 || !fields.iter().all(|f| f.parse::<u64>().is_ok()) {
                serial_println!(
                    "[procfs]   FAIL: schedstat not 3 integers ({:?})", trimmed
                );
                return Err(KernelError::InternalError);
            }
            serial_println!("[procfs]   {}/schedstat: 3 fields OK", current_tid);
        }
        Err(KernelError::NotFound) => {
            serial_println!(
                "[procfs]   {}/schedstat: NotFound (no scheduler task) OK", current_tid
            );
        }
        Err(e) => {
            serial_println!("[procfs]   FAIL: schedstat unexpected error {:?}", e);
            return Err(KernelError::InternalError);
        }
    }

    // /proc/<pid>/loginuid and sessionid — process-only audit files.  When
    // they succeed, each must be a bare decimal integer with no trailing
    // newline (Linux audit-file convention).
    for name in ["loginuid", "sessionid"] {
        match fs.read_file(&format!("/{current_tid}/{name}")) {
            Ok(data) => {
                let text = core::str::from_utf8(&data)
                    .map_err(|_| KernelError::InternalError)?;
                if text.ends_with('\n') || text.parse::<u32>().is_err() {
                    serial_println!(
                        "[procfs]   FAIL: {} not a bare decimal ({:?})", name, text
                    );
                    return Err(KernelError::InternalError);
                }
                serial_println!("[procfs]   {}/{}: {} OK", current_tid, name, text);
            }
            Err(KernelError::NotFound) => {
                serial_println!(
                    "[procfs]   {}/{}: NotFound (bare task, no PCB) OK", current_tid, name
                );
            }
            Err(e) => {
                serial_println!("[procfs]   FAIL: {} unexpected error {:?}", name, e);
                return Err(KernelError::InternalError);
            }
        }
    }

    // /proc/<pid>/cmdline — always succeeds for a live task: full argv from
    // the persistent snapshot, or the process/task name as a single
    // NUL-terminated argument.  Must be non-empty and NUL-terminated.
    let cmdline_data = fs.read_file(&format!("/{current_tid}/cmdline"))?;
    if cmdline_data.is_empty() || cmdline_data.last() != Some(&0) {
        serial_println!(
            "[procfs]   FAIL: cmdline malformed (len={}, last={:?})",
            cmdline_data.len(), cmdline_data.last()
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[procfs]   {}/cmdline: {} bytes OK", current_tid, cmdline_data.len());

    // /proc/<pid>/environ — served from the persistent envp snapshot.  Like
    // statm, only real processes carry an environment, so a bare scheduler
    // task legitimately returns NotFound.  When it succeeds it is either
    // empty (spawned without env) or a run of NUL-terminated entries.
    match fs.read_file(&format!("/{current_tid}/environ")) {
        Ok(environ_data) => {
            if !environ_data.is_empty() && environ_data.last() != Some(&0) {
                serial_println!(
                    "[procfs]   FAIL: environ not NUL-terminated (len={})",
                    environ_data.len()
                );
                return Err(KernelError::InternalError);
            }
            serial_println!(
                "[procfs]   {}/environ: {} bytes OK", current_tid, environ_data.len()
            );
        }
        Err(KernelError::NotFound) => {
            serial_println!(
                "[procfs]   {}/environ: NotFound (bare task, no env) OK",
                current_tid
            );
        }
        Err(e) => {
            serial_println!("[procfs]   FAIL: environ unexpected error {:?}", e);
            return Err(KernelError::InternalError);
        }
    }

    // /proc/<pid>/root — always resolves to "/" for a live task.
    let root_link = fs.readlink(&format!("/{current_tid}/root"))?;
    if root_link != "/" {
        serial_println!("[procfs]   FAIL: root link = {:?}, expected \"/\"", root_link);
        return Err(KernelError::InternalError);
    }
    serial_println!("[procfs]   {}/root -> {:?} OK", current_tid, root_link);

    // /proc/<pid>/cwd — symlink whose target is the process cwd.  A bare
    // scheduler task has no PCB and thus no cwd (NotFound); a real process
    // resolves to an absolute path.  Also confirm reading the link's bytes
    // directly is rejected (EINVAL-style) and that lstat reports a symlink.
    let cwd_lstat = fs.stat(&format!("/{current_tid}/cwd"))?;
    if cwd_lstat.entry_type != EntryType::Symlink {
        serial_println!("[procfs]   FAIL: cwd not a symlink ({:?})", cwd_lstat.entry_type);
        return Err(KernelError::InternalError);
    }
    if fs.read_file(&format!("/{current_tid}/cwd")) != Err(KernelError::InvalidArgument) {
        serial_println!("[procfs]   FAIL: read_file on cwd symlink should be InvalidArgument");
        return Err(KernelError::InternalError);
    }
    match fs.readlink(&format!("/{current_tid}/cwd")) {
        Ok(target) => {
            if !target.starts_with('/') {
                serial_println!("[procfs]   FAIL: cwd target {:?} not absolute", target);
                return Err(KernelError::InternalError);
            }
            serial_println!("[procfs]   {}/cwd -> {:?} OK", current_tid, target);
        }
        Err(KernelError::NotFound) => {
            serial_println!(
                "[procfs]   {}/cwd: NotFound (bare task, no cwd) OK", current_tid
            );
        }
        Err(e) => {
            serial_println!("[procfs]   FAIL: cwd readlink unexpected error {:?}", e);
            return Err(KernelError::InternalError);
        }
    }

    // /proc/<pid>/exe — symlink to the executable image.  The self-test
    // task is a bare kernel task that never exec'd a binary, so NotFound
    // is the expected, correct outcome; when it does resolve (a real
    // exec'd process), the target must be an absolute path.  Reading the
    // link's bytes directly must be rejected, and lstat must report a
    // symlink regardless.
    let exe_lstat = fs.stat(&format!("/{current_tid}/exe"))?;
    if exe_lstat.entry_type != EntryType::Symlink {
        serial_println!("[procfs]   FAIL: exe not a symlink ({:?})", exe_lstat.entry_type);
        return Err(KernelError::InternalError);
    }
    if fs.read_file(&format!("/{current_tid}/exe")) != Err(KernelError::InvalidArgument) {
        serial_println!("[procfs]   FAIL: read_file on exe symlink should be InvalidArgument");
        return Err(KernelError::InternalError);
    }
    match fs.readlink(&format!("/{current_tid}/exe")) {
        Ok(target) => {
            if !target.starts_with('/') {
                serial_println!("[procfs]   FAIL: exe target {:?} not absolute", target);
                return Err(KernelError::InternalError);
            }
            serial_println!("[procfs]   {}/exe -> {:?} OK", current_tid, target);
        }
        Err(KernelError::NotFound) => {
            serial_println!(
                "[procfs]   {}/exe: NotFound (task never exec'd a binary) OK", current_tid
            );
        }
        Err(e) => {
            serial_println!("[procfs]   FAIL: exe readlink unexpected error {:?}", e);
            return Err(KernelError::InternalError);
        }
    }

    // /proc/<pid>/stat — the full Linux 52-field single line.  Positional
    // parsers (ps/top/htop/glibc/WINE) depend on the exact field count and
    // order, so verify: exactly 52 whitespace-separated fields, the comm in
    // parentheses at field 2, field 1 == pid, a valid state char at field 3,
    // and that every non-comm field is a parseable integer (comm may hold
    // arbitrary bytes so it is exempt).  Note: the comm itself can contain
    // spaces, so split on the *last* ')' to isolate the post-comm fields the
    // way real /proc parsers do.
    let stat_data = fs.read_file(&format!("/{current_tid}/stat"))?;
    let stat_text = core::str::from_utf8(&stat_data)
        .map_err(|_| KernelError::InternalError)?;
    let stat_line = stat_text.strip_suffix('\n').unwrap_or(stat_text);
    // Field 1 (pid) and field 2 (comm) come before the last ')'.
    let close = stat_line.rfind(')').ok_or_else(|| {
        serial_println!("[procfs]   FAIL: stat has no comm ')' delimiter");
        KernelError::InternalError
    })?;
    let (head, tail) = stat_line.split_at(close);
    // head == "<pid> (<comm>", tail == ") <field3> <field4> ...".
    let open = head.find('(').ok_or_else(|| {
        serial_println!("[procfs]   FAIL: stat has no comm '(' delimiter");
        KernelError::InternalError
    })?;
    let pid_field = head.get(..open).map(str::trim).unwrap_or("");
    if pid_field.parse::<u64>() != Ok(current_tid) {
        serial_println!(
            "[procfs]   FAIL: stat field 1 = {:?}, expected pid {}",
            pid_field, current_tid
        );
        return Err(KernelError::InternalError);
    }
    // Remaining fields are everything after ") ": fields 3..=52 (50 fields).
    let rest = tail.strip_prefix(')').unwrap_or(tail).trim_start();
    let rest_fields: Vec<&str> = rest.split(' ').filter(|s| !s.is_empty()).collect();
    // 1 (pid) + 1 (comm) + 50 (rest) == 52.
    if rest_fields.len() != 50 {
        serial_println!(
            "[procfs]   FAIL: stat has {} post-comm fields, expected 50 (total != 52)",
            rest_fields.len()
        );
        return Err(KernelError::InternalError);
    }
    // Field 3 (first of rest) must be a single state char R/S/D/T/Z.
    if !matches!(rest_fields.first(), Some(&("R" | "S" | "D" | "T" | "Z"))) {
        serial_println!(
            "[procfs]   FAIL: stat state field = {:?}, expected R/S/D/T/Z",
            rest_fields.first()
        );
        return Err(KernelError::InternalError);
    }
    // All fields after the state char (4..=52) are integers.  Most are
    // signed (tpgid is -1), but rsslim can be RLIM_INFINITY == u64::MAX,
    // which overflows i64 — accept either signed or unsigned.
    if !rest_fields.iter().skip(1)
        .all(|f| f.parse::<i64>().is_ok() || f.parse::<u64>().is_ok())
    {
        serial_println!("[procfs]   FAIL: stat has a non-integer numeric field");
        return Err(KernelError::InternalError);
    }
    // Fields 5 (pgrp) and 6 (session) must agree with the getpgid/getsid
    // model: pgrp == session, and both equal the pid for a real process or
    // 0 for a bare kernel task (no PCB).  rest_fields is field3-based, so
    // index 2 == field 5 (pgrp) and index 3 == field 6 (session).
    let pgrp_field = rest_fields.get(2).and_then(|f| f.parse::<u64>().ok());
    let session_field = rest_fields.get(3).and_then(|f| f.parse::<u64>().ok());
    let expected_pgrp = if crate::proc::pcb::state(current_tid).is_some() {
        current_tid
    } else {
        0
    };
    if pgrp_field != Some(expected_pgrp) || session_field != Some(expected_pgrp) {
        serial_println!(
            "[procfs]   FAIL: stat pgrp/session = {:?}/{:?}, expected {}/{}",
            pgrp_field, session_field, expected_pgrp, expected_pgrp
        );
        return Err(KernelError::InternalError);
    }
    // Field 22 (starttime) must equal the live task's captured start_tick.
    // rest_fields is field3-based, so field 22 sits at index 22 - 3 == 19.
    // This guards the field-position wiring (a regression would shift every
    // field after it) and confirms we serve the real value, not a 0 stub.
    let live_start_tick = crate::sched::task_list()
        .iter()
        .find(|t| t.id == current_tid)
        .map(|t| t.start_tick);
    let starttime_field = rest_fields.get(19).and_then(|f| f.parse::<u64>().ok());
    if let Some(expected) = live_start_tick {
        if starttime_field != Some(expected) {
            serial_println!(
                "[procfs]   FAIL: stat starttime (field 22) = {:?}, expected {}",
                starttime_field, expected
            );
            return Err(KernelError::InternalError);
        }
        serial_println!(
            "[procfs]   {}/stat: starttime field 22 == {} OK", current_tid, expected
        );
    }
    serial_println!("[procfs]   {}/stat: 52 fields OK", current_tid);

    // --- system-wide /proc/stat Linux layout ---
    // gen_stat now emits the Linux fs/proc/stat.c show_stat() layout instead of
    // the old custom "tasks:/running:" format.  Lock down the line structure so
    // a future edit can't silently regress what top/htop/vmstat/glibc parse.
    {
        let data = fs.read_file("/stat")?;
        let text = core::str::from_utf8(&data).map_err(|_| KernelError::InternalError)?;

        // The aggregate CPU line is first and uses the "cpu" label followed by
        // TWO spaces (Linux quirk), then exactly 10 jiffy columns.
        let first = text.lines().next().unwrap_or("");
        if !first.starts_with("cpu  ") {
            serial_println!("[procfs]   FAIL: /stat first line {:?}, want 'cpu  ...'", first);
            return Err(KernelError::InternalError);
        }
        let cols: Vec<&str> = first.split(' ').filter(|s| !s.is_empty()).collect();
        // "cpu" + 10 numeric columns = 11 tokens.
        if cols.len() != 11 {
            serial_println!(
                "[procfs]   FAIL: /stat cpu line has {} tokens, want 11 (cpu + 10 cols)",
                cols.len()
            );
            return Err(KernelError::InternalError);
        }
        if !cols.iter().skip(1).all(|c| c.parse::<u64>().is_ok()) {
            serial_println!("[procfs]   FAIL: /stat cpu line has a non-numeric column");
            return Err(KernelError::InternalError);
        }
        // user (col 1) and nice (col 2) are honestly 0 — we do not yet split
        // user-vs-kernel CPU time, so these must never be fabricated non-zero.
        if cols.get(1) != Some(&"0") || cols.get(2) != Some(&"0") {
            serial_println!(
                "[procfs]   FAIL: /stat user/nice cols = {:?}/{:?}, want 0/0 (untracked)",
                cols.get(1), cols.get(2)
            );
            return Err(KernelError::InternalError);
        }
        // The mandatory summary keys every parser expects, each on its own line.
        for key in ["\nintr ", "\nctxt ", "\nbtime ", "\nprocesses ",
                    "\nprocs_running ", "\nprocs_blocked ", "\nsoftirq "] {
            if !text.contains(key) {
                serial_println!("[procfs]   FAIL: /stat missing line {:?}", key);
                return Err(KernelError::InternalError);
            }
        }
        serial_println!("[procfs]   /stat: Linux show_stat layout OK ({} bytes)", data.len());
    }

    // --- system-wide /proc/uptime Linux layout ---
    // gen_uptime now emits the Linux fs/proc/uptime.c two-field
    // "<uptime> <idle>" centisecond format.  A strict two-field parser
    // (sscanf "%lf %lf") must find exactly two decimal fields.
    {
        let data = fs.read_file("/uptime")?;
        let text = core::str::from_utf8(&data).map_err(|_| KernelError::InternalError)?;
        let line = text.strip_suffix('\n').unwrap_or(text);
        let fields: Vec<&str> = line.split(' ').filter(|s| !s.is_empty()).collect();
        if fields.len() != 2 {
            serial_println!(
                "[procfs]   FAIL: /uptime has {} fields, want 2 (<uptime> <idle>)",
                fields.len()
            );
            return Err(KernelError::InternalError);
        }
        // Each field is "<secs>.<centis>" — must split into two integer parts,
        // and the centisecond part must be exactly 2 digits.
        for f in &fields {
            let mut parts = f.split('.');
            let secs = parts.next().and_then(|s| s.parse::<u64>().ok());
            let centis = parts.next();
            let centis_ok = centis.is_some_and(|c| c.len() == 2 && c.parse::<u64>().is_ok());
            if secs.is_none() || parts.next().is_some() || !centis_ok {
                serial_println!(
                    "[procfs]   FAIL: /uptime field {:?} not '<secs>.<2-digit-centis>'", f
                );
                return Err(KernelError::InternalError);
            }
        }
        serial_println!("[procfs]   /uptime: two-field Linux layout OK ({:?})", line);
    }

    // --- system-wide /proc/loadavg Linux layout ---
    // gen_loadavg now emits the Linux fs/proc/loadavg.c five-field
    // "<load1> <load5> <load15> <runnable>/<total> <last_pid>" format,
    // with the three loads as <int>.<2-digit-frac> fixed-point figures.
    {
        let data = fs.read_file("/loadavg")?;
        let text = core::str::from_utf8(&data).map_err(|_| KernelError::InternalError)?;
        let line = text.strip_suffix('\n').unwrap_or(text);
        let fields: Vec<&str> = line.split(' ').filter(|s| !s.is_empty()).collect();
        if fields.len() != 5 {
            serial_println!(
                "[procfs]   FAIL: /loadavg has {} fields, want 5", fields.len()
            );
            return Err(KernelError::InternalError);
        }
        // Fields 0..3 are the three load averages: "<int>.<2-digit-frac>".
        for f in fields.iter().take(3) {
            let mut parts = f.split('.');
            let int_part = parts.next().and_then(|s| s.parse::<u64>().ok());
            let frac = parts.next();
            let frac_ok = frac.is_some_and(|c| c.len() == 2 && c.parse::<u64>().is_ok());
            if int_part.is_none() || parts.next().is_some() || !frac_ok {
                serial_println!(
                    "[procfs]   FAIL: /loadavg load field {:?} not '<int>.<2-digit-frac>'", f
                );
                return Err(KernelError::InternalError);
            }
        }
        // Field 3 is "<runnable>/<total>" — two integers separated by '/'.
        let rt_field = fields.get(3).copied().unwrap_or("");
        {
            let mut rt = rt_field.split('/');
            let runnable = rt.next().and_then(|s| s.parse::<u64>().ok());
            let total = rt.next().and_then(|s| s.parse::<u64>().ok());
            if runnable.is_none() || total.is_none() || rt.next().is_some() {
                serial_println!(
                    "[procfs]   FAIL: /loadavg field 4 {:?} not '<runnable>/<total>'", rt_field
                );
                return Err(KernelError::InternalError);
            }
        }
        // Field 4 is the last PID — a plain integer.
        let pid_field = fields.get(4).copied().unwrap_or("");
        if pid_field.parse::<u64>().is_err() {
            serial_println!(
                "[procfs]   FAIL: /loadavg last_pid {:?} not an integer", pid_field
            );
            return Err(KernelError::InternalError);
        }
        serial_println!("[procfs]   /loadavg: five-field Linux layout OK ({:?})", line);
    }

    // --- system-wide /proc/buddyinfo Linux layout ---
    // gen_buddyinfo now emits only the Linux mm/vmstat.c frag_show line(s):
    // "Node 0, zone Normal <c0> <c1> ... <c10>" with no trailing comment block.
    // Verify the prefix and that every per-order column parses as an integer.
    {
        let data = fs.read_file("/buddyinfo")?;
        let text = core::str::from_utf8(&data).map_err(|_| KernelError::InternalError)?;
        let line = text.lines().next().unwrap_or("");
        if !line.starts_with("Node 0, zone") {
            serial_println!("[procfs]   FAIL: /buddyinfo missing 'Node 0, zone' prefix: {:?}", line);
            return Err(KernelError::InternalError);
        }
        // Tokenise; the zone-name token ("Normal") is followed by the per-order
        // free-block counts, all of which must be integers.  Skip the fixed
        // "Node" "0," "zone" "Normal" header tokens (4 of them).
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let counts = tokens.get(4..).unwrap_or(&[]);
        if counts.is_empty() {
            serial_println!("[procfs]   FAIL: /buddyinfo has no per-order columns: {:?}", line);
            return Err(KernelError::InternalError);
        }
        for c in counts {
            if c.parse::<u64>().is_err() {
                serial_println!("[procfs]   FAIL: /buddyinfo column {:?} not an integer", c);
                return Err(KernelError::InternalError);
            }
        }
        // And no stray non-zone lines (the old comment block) should remain.
        for extra in text.lines().skip(1) {
            if !extra.trim().is_empty() && !extra.starts_with("Node ") {
                serial_println!("[procfs]   FAIL: /buddyinfo has non-Linux trailing line: {:?}", extra);
                return Err(KernelError::InternalError);
            }
        }
        serial_println!("[procfs]   /buddyinfo: Linux frag_show layout OK ({} columns)", counts.len());
    }

    // --- /proc/sys sysctl tree ---
    // The sysctl tree is the only nested-directory subtree besides per-PID
    // `task/`/`fd/`.  Verify the router classifies its dirs/files/bogus paths,
    // that directory listings reflect the flat tables, that the values are
    // byte-consistent with the uname(2) surface and real ABI ceilings, and
    // that the UUID formatter forces the v4/variant bits deterministically.
    {
        // 1. Path classification.
        let cases: &[(&str, &str)] = &[
            ("sys", "sysdir"),
            ("sys/kernel", "sysdir"),
            ("sys/kernel/random", "sysdir"),
            ("sys/vm", "sysdir"),
            ("sys/fs", "sysdir"),
            ("sys/kernel/osrelease", "sysfile"),
            ("sys/kernel/random/boot_id", "sysfile"),
            ("sys/vm/overcommit_memory", "sysfile"),
            ("sys/fs/nr_open", "sysfile"),
            ("sys/bogus", "notfound"),             // unknown subdir
            ("sys/kernel/bogus", "notfound"),      // unknown file
            ("sys/kernel/osrelease/x", "notfound"),// nested beyond a file
        ];
        for (path, want) in cases {
            let got = match classify_path(path) {
                ProcPath::SysDir(_) => "sysdir",
                ProcPath::SysFile(_) => "sysfile",
                ProcPath::NotFound => "notfound",
                _ => "other",
            };
            if got != *want {
                serial_println!(
                    "[procfs]   FAIL: classify_path({:?}) = {}, want {}",
                    path, got, want
                );
                return Err(KernelError::InternalError);
            }
        }

        // 2. stat: /proc/sys and an interior dir are directories.
        for d in ["/sys", "/sys/kernel", "/sys/kernel/random", "/sys/vm", "/sys/fs"] {
            if fs.stat(d)?.entry_type != EntryType::Directory {
                serial_println!("[procfs]   FAIL: stat {} not a directory", d);
                return Err(KernelError::InternalError);
            }
        }

        // 3. readdir /proc/sys lists exactly the two interior dirs (no files
        //    sit directly at the root) — order: dirs before files.
        let root = fs.readdir("/sys")?;
        for d in ["kernel", "vm", "fs"] {
            if !root.iter().any(|e| e.name == d && e.entry_type == EntryType::Directory) {
                serial_println!("[procfs]   FAIL: /sys missing dir {}", d);
                return Err(KernelError::InternalError);
            }
        }
        if root.iter().any(|e| e.entry_type == EntryType::File) {
            serial_println!("[procfs]   FAIL: /sys has unexpected file entries");
            return Err(KernelError::InternalError);
        }

        // 4. readdir /proc/sys/kernel: the `random` subdir + the six files.
        let kern = fs.readdir("/sys/kernel")?;
        if !kern.iter().any(|e| e.name == "random" && e.entry_type == EntryType::Directory) {
            serial_println!("[procfs]   FAIL: /sys/kernel missing `random` subdir");
            return Err(KernelError::InternalError);
        }
        for f in ["ostype", "osrelease", "version", "hostname", "domainname", "pid_max"] {
            if !kern.iter().any(|e| e.name == f && e.entry_type == EntryType::File) {
                serial_println!("[procfs]   FAIL: /sys/kernel missing file {}", f);
                return Err(KernelError::InternalError);
            }
        }

        // 5. Values: uname-surface consistency + parseable ceilings.
        let osrelease = fs.read_file("/sys/kernel/osrelease")?;
        if core::str::from_utf8(&osrelease).ok() != Some("6.6.0-slateos\n") {
            serial_println!("[procfs]   FAIL: osrelease = {:?}", osrelease);
            return Err(KernelError::InternalError);
        }
        let ostype = fs.read_file("/sys/kernel/ostype")?;
        if core::str::from_utf8(&ostype).ok() != Some("Linux\n") {
            serial_println!("[procfs]   FAIL: ostype != \"Linux\"");
            return Err(KernelError::InternalError);
        }
        let nr_open = core::str::from_utf8(&fs.read_file("/sys/fs/nr_open")?)
            .unwrap_or("").trim().parse::<usize>().ok();
        if nr_open != Some(crate::proc::linux_fd::MAX_FDS) {
            serial_println!("[procfs]   FAIL: fs/nr_open = {:?}, want {}",
                nr_open, crate::proc::linux_fd::MAX_FDS);
            return Err(KernelError::InternalError);
        }
        let pid_max = core::str::from_utf8(&fs.read_file("/sys/kernel/pid_max")?)
            .unwrap_or("").trim().parse::<usize>().ok();
        if pid_max != Some(crate::pidns::MAX_PIDS_PER_NS) {
            serial_println!("[procfs]   FAIL: kernel/pid_max = {:?}, want {}",
                pid_max, crate::pidns::MAX_PIDS_PER_NS);
            return Err(KernelError::InternalError);
        }

        // 5b. /sys/vm lists overcommit_memory, which reads as the Linux-ABI
        //     default policy `0` (heuristic). This surface is Linux-compat only:
        //     Slate OS's Linux mmap path always passes MAP_LAZY (demand-paged), so
        //     reporting overcommit_memory=0 is honest — Linux programs see the
        //     lazy/overcommit allocation idiom they expect. overcommit_ratio /
        //     overcommit_kbytes are deliberately absent (no commit accounting).
        let vm = fs.readdir("/sys/vm")?;
        if !vm.iter().any(|e| e.name == "overcommit_memory"
            && e.entry_type == EntryType::File)
        {
            serial_println!("[procfs]   FAIL: /sys/vm missing overcommit_memory");
            return Err(KernelError::InternalError);
        }
        let overcommit = core::str::from_utf8(
            &fs.read_file("/sys/vm/overcommit_memory")?)
            .unwrap_or("").trim().parse::<u32>().ok();
        if overcommit != Some(0) {
            serial_println!("[procfs]   FAIL: vm/overcommit_memory = {:?}, want 0",
                overcommit);
            return Err(KernelError::InternalError);
        }

        // 6. boot_id is stable across reads; uuid is well-formed v4.
        let b1 = fs.read_file("/sys/kernel/random/boot_id")?;
        let b2 = fs.read_file("/sys/kernel/random/boot_id")?;
        if b1 != b2 || b1.len() != 37 {
            serial_println!("[procfs]   FAIL: boot_id unstable or wrong length");
            return Err(KernelError::InternalError);
        }

        // 6b. CSPRNG entropy surface: poolsize is the fixed 256-bit ceiling;
        //     entropy_avail is 0 or 256 and never exceeds poolsize.  Also check
        //     /sys/kernel/random lists all four files.
        let rnd = fs.readdir("/sys/kernel/random")?;
        for f in ["uuid", "boot_id", "poolsize", "entropy_avail"] {
            if !rnd.iter().any(|e| e.name == f && e.entry_type == EntryType::File) {
                serial_println!("[procfs]   FAIL: /sys/kernel/random missing file {}", f);
                return Err(KernelError::InternalError);
            }
        }
        let poolsize = core::str::from_utf8(&fs.read_file("/sys/kernel/random/poolsize")?)
            .unwrap_or("").trim().parse::<u32>().ok();
        if poolsize != Some(256) {
            serial_println!("[procfs]   FAIL: random/poolsize = {:?}, want 256", poolsize);
            return Err(KernelError::InternalError);
        }
        let entropy = core::str::from_utf8(&fs.read_file("/sys/kernel/random/entropy_avail")?)
            .unwrap_or("").trim().parse::<u32>().ok();
        match entropy {
            Some(e) if e == 0 || e == 256 => {}
            _ => {
                serial_println!("[procfs]   FAIL: random/entropy_avail = {:?}, want 0 or 256", entropy);
                return Err(KernelError::InternalError);
            }
        }

        // 7. Directory/file kind enforcement.
        if fs.read_file("/sys/kernel") != Err(KernelError::IsADirectory) {
            serial_println!("[procfs]   FAIL: read_file(/sys/kernel) not IsADirectory");
            return Err(KernelError::InternalError);
        }
        if !matches!(fs.readdir("/sys/kernel/osrelease"), Err(KernelError::NotADirectory)) {
            serial_println!("[procfs]   FAIL: readdir(/sys/kernel/osrelease) not NotADirectory");
            return Err(KernelError::InternalError);
        }

        // 8. Deterministic UUID formatter: version nibble forced to 4, variant
        //    bits to 10xx, regardless of input bytes.
        let uuid = format_uuid_v4([
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef,
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef,
        ]);
        if uuid != "01234567-89ab-4def-8123-456789abcdef" {
            serial_println!("[procfs]   FAIL: format_uuid_v4 = {:?}", uuid);
            return Err(KernelError::InternalError);
        }

        serial_println!("[procfs]   /proc/sys: {} classify cases, listings, values, UUID OK",
            cases.len());
    }

    serial_println!("[procfs] Self-test PASSED");
    Ok(())
}
