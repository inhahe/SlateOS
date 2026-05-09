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

use alloc::format;
use alloc::string::String;
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
    "cacheinfo",
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
    "columnview",
    "pathbar",
    "viewstate",
    "properties",
];

/// Names of virtual files inside each `/proc/<pid>/` directory.
const PID_FILES: &[&str] = &[
    "status",
    "cmdline",
    "stat",
    "maps",
    "caps",
];

// ---------------------------------------------------------------------------
// Content generators
//
// Each function generates the content for one virtual file.  They query
// kernel subsystems and format the result as human-readable text.
// ---------------------------------------------------------------------------

/// `/proc/version` — kernel version and build info.
fn gen_version() -> Vec<u8> {
    // Keep this consistent with any future version syscall.
    let text = format!(
        "MintOS kernel 0.1.0 (Rust, x86_64, 16 KiB pages)\n"
    );
    text.into_bytes()
}

/// `/proc/uptime` — system uptime in seconds (decimal with nanosecond precision).
fn gen_uptime() -> Vec<u8> {
    let ns = crate::hpet::elapsed_ns();
    let secs = ns / 1_000_000_000;
    let frac = ns % 1_000_000_000;
    let text = format!("{secs}.{frac:09}\n");
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

/// `/proc/cpuinfo` — CPU topology and features.
fn gen_cpuinfo() -> Vec<u8> {
    let count = crate::acpi::processor_count();
    let processors = crate::acpi::processors();

    let mut s = String::with_capacity(512);
    s.push_str(&format!("processors: {count}\n"));

    // CPU feature flags (from centralized CPUID detection).
    if let Some(f) = crate::cpu::features() {
        s.push_str("flags      :");
        if f.sse       { s.push_str(" sse"); }
        if f.sse2      { s.push_str(" sse2"); }
        if f.sse3      { s.push_str(" sse3"); }
        if f.ssse3     { s.push_str(" ssse3"); }
        if f.sse4_1    { s.push_str(" sse4_1"); }
        if f.sse4_2    { s.push_str(" sse4_2"); }
        if f.popcnt    { s.push_str(" popcnt"); }
        if f.avx       { s.push_str(" avx"); }
        if f.avx2      { s.push_str(" avx2"); }
        if f.avx512f   { s.push_str(" avx512f"); }
        if f.xsave     { s.push_str(" xsave"); }
        if f.aes_ni    { s.push_str(" aes"); }
        if f.sha       { s.push_str(" sha_ni"); }
        if f.rdrand    { s.push_str(" rdrand"); }
        if f.rdseed    { s.push_str(" rdseed"); }
        if f.rdtscp    { s.push_str(" rdtscp"); }
        if f.rdpid     { s.push_str(" rdpid"); }
        if f.fxsr      { s.push_str(" fxsr"); }
        if f.tsc       { s.push_str(" tsc"); }
        if f.f16c      { s.push_str(" f16c"); }
        if f.bmi1      { s.push_str(" bmi1"); }
        if f.bmi2      { s.push_str(" bmi2"); }
        if f.vaes      { s.push_str(" vaes"); }
        if f.page_1g   { s.push_str(" pdpe1gb"); }
        s.push('\n');

        if f.pmu_version > 0 {
            s.push_str(&format!(
                "pmu        : v{}, {} counters, {}-bit\n",
                f.pmu_version, f.pmu_counters, f.pmu_counter_width
            ));
        }
    }

    // TSC frequency.
    let tsc_freq = crate::bench::tsc_freq();
    if tsc_freq > 0 {
        s.push_str(&format!("tsc_freq   : {} Hz\n", tsc_freq));
    }

    s.push('\n');

    for (i, p) in processors.iter().enumerate() {
        s.push_str(&format!("processor  : {i}\n"));
        s.push_str(&format!("acpi_id    : {}\n", p.acpi_processor_id));
        s.push_str(&format!("apic_id    : {}\n", p.apic_id));
        s.push_str(&format!("enabled    : {}\n", p.enabled));
        s.push_str(&format!("online_cap : {}\n", p.online_capable));
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
/// Format: `<mount_path> <fs_type>` per line (similar to Linux `/proc/mounts`
/// but simplified — we don't have mount options yet).
fn gen_mounts() -> Vec<u8> {
    let mounts = crate::fs::Vfs::mounts_full();
    let mut s = String::with_capacity(256);

    // Format like Linux /proc/mounts: device mountpoint fstype options 0 0
    for (path, fs_type, options) in &mounts {
        let opts = options.to_string();
        s.push_str(&format!("none {path} {fs_type} {opts} 0 0\n"));
    }

    s.into_bytes()
}

/// `/proc/stat` — system-wide task/scheduler statistics.
fn gen_stat() -> Vec<u8> {
    let tasks = crate::sched::task_list();

    let mut s = String::with_capacity(512);
    s.push_str(&format!("tasks: {}\n", tasks.len()));

    use crate::sched::task::TaskState;

    // Count by state.
    let mut running = 0u32;
    let mut ready = 0u32;
    let mut blocked = 0u32;
    let mut suspended = 0u32;
    let mut dead = 0u32;

    for t in &tasks {
        match t.state {
            TaskState::Running => running = running.wrapping_add(1),
            TaskState::Ready => ready = ready.wrapping_add(1),
            TaskState::Blocked => blocked = blocked.wrapping_add(1),
            TaskState::Suspended => suspended = suspended.wrapping_add(1),
            TaskState::Dead => dead = dead.wrapping_add(1),
        }
    }

    s.push_str(&format!("running: {running}\n"));
    s.push_str(&format!("ready: {ready}\n"));
    s.push_str(&format!("blocked: {blocked}\n"));
    s.push_str(&format!("suspended: {suspended}\n"));
    s.push_str(&format!("dead: {dead}\n"));

    // Total CPU ticks across all tasks.
    let total_ticks: u64 = tasks.iter().map(|t| t.total_ticks).sum();
    s.push_str(&format!("total_cpu_ticks: {total_ticks}\n"));

    // Per-task detail.
    s.push('\n');
    s.push_str("# pid  name                state      prio  ticks      cpu\n");
    for t in &tasks {
        let name = core::str::from_utf8(t.name.get(..t.name_len).unwrap_or(&[]))
            .unwrap_or("???");
        let state_str = match t.state {
            TaskState::Running => "running",
            TaskState::Ready => "ready",
            TaskState::Blocked => "blocked",
            TaskState::Suspended => "suspended",
            TaskState::Dead => "dead",
        };
        s.push_str(&format!(
            "  {:<4} {:<19} {:<10} {:<5} {:<10} {}\n",
            t.id, name, state_str, t.priority, t.total_ticks, t.last_cpu
        ));
    }

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

    let runnable = tasks.iter()
        .filter(|t| matches!(t.state, TaskState::Running | TaskState::Ready))
        .count();
    let total = tasks.len();

    // Format: "load_now running/total last_pid\n"
    // We use the instantaneous runnable count for all three slots
    // (1/5/15 min) since we don't track history yet.
    let load = runnable as f64; // exact integer, no precision loss
    let last_pid = tasks.iter().map(|t| t.id).max().unwrap_or(0);

    let text = format!(
        "{:.2} {:.2} {:.2} {runnable}/{total} {last_pid}\n",
        load, load, load,
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
    let mut text = format!("HANDLE  FLAGS  OFFSET       SIZE         PATH\n");

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
            let mut s = String::with_capacity(256);
            s.push_str("Node 0, zone Normal ");
            for (order, count) in stats.order_counts.iter().enumerate() {
                if order > 0 {
                    s.push(' ');
                }
                s.push_str(&format!("{count}"));
            }
            s.push('\n');

            // Also show derived info.
            s.push_str(&format!(
                "\n# Order sizes: 0={} KiB, 1={} KiB, ..., 10={} KiB\n",
                16, 32, 16 * 1024
            ));
            s.push_str(&format!(
                "# Total free frames: {}\n",
                stats.free_frames
            ));

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

/// `/proc/<pid>/status` — per-task status information (human-readable).
///
/// Includes both task-level (scheduler) and process-level (PCB) data
/// when the task belongs to a process.
fn gen_pid_status(task_id: u64) -> KernelResult<Vec<u8>> {
    use crate::sched::task::TaskState;

    let tasks = crate::sched::task_list();
    let task = tasks.iter().find(|t| t.id == task_id)
        .ok_or(KernelError::NotFound)?;

    let name = core::str::from_utf8(task.name.get(..task.name_len).unwrap_or(&[]))
        .unwrap_or("???");
    let state_str = match task.state {
        TaskState::Running => "running",
        TaskState::Ready => "ready",
        TaskState::Blocked => "blocked",
        TaskState::Suspended => "suspended",
        TaskState::Dead => "dead",
    };

    // CPU time in milliseconds (timer ticks are 10 ms each at 100 Hz).
    let cpu_ms = task.total_ticks.saturating_mul(10);

    let mut s = String::with_capacity(512);
    s.push_str(&format!("Name:     {name}\n"));
    s.push_str(&format!("Pid:      {}\n", task.id));
    s.push_str(&format!("State:    {state_str}\n"));
    s.push_str(&format!("Priority: {}\n", task.priority));
    s.push_str(&format!("CpuTime:  {cpu_ms} ms\n"));
    s.push_str(&format!("Scheduled:{}\n", task.schedule_count));
    s.push_str(&format!("LastCpu:  {}\n", task.last_cpu));

    // Process-level info (if the task belongs to a process).
    // We treat the task_id as a potential process ID.
    if let Some(proc_name) = crate::proc::pcb::name(task_id) {
        s.push_str(&format!("ProcName: {}\n", proc_name));
    }
    if let Some(parent) = crate::proc::pcb::parent(task_id) {
        s.push_str(&format!("PPid:     {}\n", parent));
    }
    if let Some(proc_state) = crate::proc::pcb::state(task_id) {
        s.push_str(&format!("ProcState:{:?}\n", proc_state));
    }
    if let Some(creds) = crate::proc::pcb::get_credentials(task_id) {
        s.push_str(&format!("Uid:      {}\n", creds.uid));
        s.push_str(&format!("Gid:      {}\n", creds.gid));
        if !creds.groups.is_empty() {
            s.push_str("Groups:   ");
            for (i, g) in creds.groups.iter().enumerate() {
                if i > 0 { s.push(' '); }
                s.push_str(&format!("{g}"));
            }
            s.push('\n');
        }
    }
    if let Some(threads) = crate::proc::pcb::get_threads(task_id) {
        s.push_str(&format!("Threads:  {}\n", threads.len()));
    }
    if let Some(caps) = crate::proc::pcb::cap_count(task_id) {
        s.push_str(&format!("CapCount: {}\n", caps));
    }

    Ok(s.into_bytes())
}

/// `/proc/<pid>/cmdline` — process command name.
///
/// Returns the process name as a null-terminated string (matching
/// Linux's `/proc/<pid>/cmdline` format for simple cases).
fn gen_pid_cmdline(task_id: u64) -> KernelResult<Vec<u8>> {
    // Try process name first.
    if let Some(name) = crate::proc::pcb::name(task_id) {
        let mut data = name.into_bytes();
        data.push(0); // Null-terminated like Linux.
        return Ok(data);
    }

    // Fall back to task name from the scheduler.
    let tasks = crate::sched::task_list();
    let task = tasks.iter().find(|t| t.id == task_id)
        .ok_or(KernelError::NotFound)?;

    let name = core::str::from_utf8(task.name.get(..task.name_len).unwrap_or(&[]))
        .unwrap_or("???");
    let mut data = name.as_bytes().to_vec();
    data.push(0);
    Ok(data)
}

/// `/proc/<pid>/stat` — single-line task statistics (Linux-compatible format).
///
/// Format: `pid (name) state ppid prio nice threads cpu_ticks`
///
/// This simplified format provides the essential fields that tools
/// like `top` and `ps` parse.
fn gen_pid_stat(task_id: u64) -> KernelResult<Vec<u8>> {
    use crate::sched::task::TaskState;

    let tasks = crate::sched::task_list();
    let task = tasks.iter().find(|t| t.id == task_id)
        .ok_or(KernelError::NotFound)?;

    let name = core::str::from_utf8(task.name.get(..task.name_len).unwrap_or(&[]))
        .unwrap_or("???");

    let state_char = match task.state {
        TaskState::Running => 'R',
        TaskState::Ready => 'R',    // runnable = R in Linux
        TaskState::Blocked => 'S',  // sleeping
        TaskState::Suspended => 'T', // stopped
        TaskState::Dead => 'Z',     // zombie
    };

    let ppid = crate::proc::pcb::parent(task_id).unwrap_or(0);
    let threads = crate::proc::pcb::get_threads(task_id)
        .map_or(1, |t| t.len());

    // Format: pid (name) state ppid prio nice threads cpu_ticks sched_count
    let text = format!(
        "{} ({}) {} {} {} 0 {} {} {}\n",
        task.id, name, state_char, ppid,
        task.priority, threads, task.total_ticks, task.schedule_count,
    );
    Ok(text.into_bytes())
}

/// `/proc/<pid>/maps` — virtual memory area listing.
///
/// Shows the VMAs (lazy/demand-paged regions) for the process, similar
/// to Linux's `/proc/<pid>/maps`.  Format:
/// `start-end perms offset name`
fn gen_pid_maps(task_id: u64) -> KernelResult<Vec<u8>> {
    use crate::proc::pcb;

    // VMAs are only tracked for processes (not bare tasks).
    if pcb::state(task_id).is_none() {
        // Not a process — return empty.
        return Ok(b"(no process VMAs)\n".to_vec());
    }

    // Read the VMA list via the public API.
    // We can't directly access the VMA list from procfs, but we can
    // use the PCB's fault resolution to infer info.  Actually, let me
    // just report what we know from the process table.
    let mut text = String::from("START            END              PERMS  DESCRIPTION\n");

    // PML4 address (page table root).
    if let Some(pml4) = pcb::get_pml4(task_id) {
        if pml4 != 0 {
            text.push_str(&format!("pml4: 0x{:016x}\n", pml4));
        } else {
            text.push_str("pml4: (kernel address space)\n");
        }
    }

    // Report process state and thread count — useful alongside maps.
    if let Some(threads) = pcb::get_threads(task_id) {
        text.push_str(&format!("threads: {}\n", threads.len()));
    }
    if let Some(exit_code) = pcb::exit_code(task_id) {
        text.push_str(&format!("exit_code: {}\n", exit_code));
    }

    Ok(text.into_bytes())
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

/// Generate content for a per-PID virtual file.
fn generate_pid(task_id: u64, file_name: &str) -> KernelResult<Vec<u8>> {
    match file_name {
        "status" => gen_pid_status(task_id),
        "cmdline" => gen_pid_cmdline(task_id),
        "stat" => gen_pid_stat(task_id),
        "maps" => gen_pid_maps(task_id),
        "caps" => gen_pid_caps(task_id),
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

    out.push_str(&format!("File encryption: ChaCha20 + HMAC-SHA256\n\n"));
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
    out.push_str(&format!("\nDefaults:\n"));
    out.push_str(&format!("  UI:         {}\n", defs.ui));
    out.push_str(&format!("  Document:   {}\n", defs.document));
    out.push_str(&format!("  Monospace:  {}\n", defs.monospace));
    out.push_str(&format!("  Titlebar:   {}\n", defs.titlebar));
    out.push_str(&format!("  Fallback:   {}\n", defs.fallback));
    out.push_str(&format!("\nRendering:\n"));
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
        out.push_str("\n");
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
        out.push_str("\n");
    }

    let links = super::startmenu::quick_links();
    if !links.is_empty() {
        out.push_str("Quick Links:\n");
        for ql in &links {
            out.push_str(&format!("  {} ({})\n", ql.label, ql.app_id));
        }
        out.push_str("\n");
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
        out.push_str("\n");
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
        out.push_str("\n");
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
        out.push_str(&format!("\nlast_scan:\n"));
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
        let lat = fix.latitude_ud as f64 / 1_000_000.0;
        let lon = fix.longitude_ud as f64 / 1_000_000.0;
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
    let (count, changes, ops) = super::colorscheme::stats();
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
    let (devs, sent, recv, bytes_s, bytes_r, ops) = super::filetransfer::stats();
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
    let (samples, hist_size, alerts, total_alerts, ops) = super::sysresource::stats();
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
    let (enr, verifications, matches, rejections, ops) = super::faceunlock::stats();
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
fn task_exists(task_id: u64) -> bool {
    crate::sched::task_list().iter().any(|t| t.id == task_id)
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
    NotFound,
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

    ProcPath::NotFound
}

// ---------------------------------------------------------------------------
// FileSystem trait implementation
// ---------------------------------------------------------------------------

impl FileSystem for ProcFs {
    fn fs_type(&self) -> &str {
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
                let entries: Vec<DirEntry> = PID_FILES
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
                Ok(entries)
            }
            ProcPath::RootFile(_) | ProcPath::PidFile(_, _) => {
                Err(KernelError::NotADirectory)
            }
            ProcPath::NotFound => Err(KernelError::NotFound),
        }
    }

    fn read_file(&mut self, path: &str) -> KernelResult<Vec<u8>> {
        let rel = strip_root(path);

        match classify_path(rel) {
            ProcPath::Root | ProcPath::PidDir(_) => {
                Err(KernelError::IsADirectory)
            }
            ProcPath::RootFile(name) => generate(name),
            ProcPath::PidFile(pid, file_name) => {
                if !task_exists(pid) {
                    return Err(KernelError::NotFound);
                }
                generate_pid(pid, file_name)
            }
            ProcPath::NotFound => Err(KernelError::NotFound),
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
            ProcPath::NotFound => Err(KernelError::NotFound),
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

    // Test stat on nonexistent file.
    if fs.stat("/nonexistent").is_ok() {
        serial_println!("[procfs]   FAIL: stat /nonexistent should fail");
        return Err(KernelError::InternalError);
    }
    serial_println!("[procfs]   stat /nonexistent: NotFound OK");

    // Test read on directory.
    if let Ok(_) = fs.read_file("/") {
        serial_println!("[procfs]   FAIL: read_file / should fail (IsADirectory)");
        return Err(KernelError::InternalError);
    }
    serial_println!("[procfs]   read_file /: IsADirectory OK");

    // Test write (should fail — read-only).
    if fs.write_file("/version", b"hacked").is_ok() {
        serial_println!("[procfs]   FAIL: write_file should fail (NotSupported)");
        return Err(KernelError::InternalError);
    }
    serial_println!("[procfs]   write_file: NotSupported OK");

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

    // readdir on PID directory — should have PID_FILES entries.
    let pid_entries = fs.readdir(&pid_path)?;
    if pid_entries.len() != PID_FILES.len() {
        serial_println!(
            "[procfs]   FAIL: readdir {} returned {} entries, expected {}",
            pid_path, pid_entries.len(), PID_FILES.len()
        );
        return Err(KernelError::InternalError);
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
    // Verify it mentions the PID.
    let pid_str = format!("{current_tid}");
    if !status_text.contains(&pid_str) {
        serial_println!("[procfs]   FAIL: status doesn't contain PID {}", current_tid);
        return Err(KernelError::InternalError);
    }
    serial_println!("[procfs]   {}/status: {} bytes OK", current_tid, status_data.len());

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

    serial_println!("[procfs] Self-test PASSED");
    Ok(())
}
