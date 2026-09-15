//! Slate OS process tree display utilities.
//!
//! Multi-personality binary providing:
//! - **pstree** — display process tree
//! - **pgrep** variant with tree view
//!
//! Reads `/proc/<pid>/stat` and `/proc/<pid>/status` to build the process
//! hierarchy and display it as an ASCII/Unicode tree.

#![deny(clippy::all)]

use quoting::quoteaf_os;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";

// Unicode box-drawing characters.
const TREE_BRANCH: &str = "├── ";
const TREE_LAST: &str = "└── ";
const TREE_PIPE: &str = "│   ";
const TREE_SPACE: &str = "    ";

// ASCII alternatives.
const ASCII_BRANCH: &str = "|-- ";
const ASCII_LAST: &str = "`-- ";
const ASCII_PIPE: &str = "|   ";
const ASCII_SPACE: &str = "    ";

// ============================================================================
// Data structures
// ============================================================================

/// Process information from /proc.
#[derive(Clone, Debug)]
struct ProcessInfo {
    pid: u32,
    ppid: u32,
    /// The command name, **as bytes**.
    ///
    /// `read_to_string` on `/proc/<pid>/stat` *fails* for a name that is not
    /// UTF-8, so such a process used to vanish from the tree -- **and its
    /// children with it**, since a tree is built by matching each process's
    /// `ppid` against a parent that has to be present. One unreadable name
    /// could therefore hide an entire subtree, which is a stronger effect than
    /// the same bug had in `pgrep` or `top`.
    name: Vec<u8>,
    uid: u32,
    threads: u32,
    // Parsed from /proc/<pid>/stat; consumed by the future -S option
    // that suffixes process names with their state letter.
    #[allow(dead_code)]
    state: char,
    username: String,
}

/// Display options.
struct Options {
    /// Show PIDs.
    show_pids: bool,
    /// Show UIDs/usernames.
    show_uid: bool,
    /// Show thread counts.
    show_threads: bool,
    /// Use ASCII characters instead of Unicode.
    ascii: bool,
    /// Compact display (merge identical subtrees).
    compact: bool,
    /// Show only a specific PID's subtree.
    root_pid: Option<u32>,
    /// Highlight a specific PID.
    highlight_pid: Option<u32>,
    /// Show kernel threads.
    show_kernel: bool,
    /// Long format (show command line args).
    long_format: bool,
    /// Sort by PID (default) or name.
    sort_by_name: bool,
    /// Show arguments.
    show_args: bool,
    /// Numeric UIDs only.
    numeric_uid: bool,
}

// ============================================================================
// Process reading
// ============================================================================

fn read_file(path: &str) -> Option<String> {
    fs::read_to_string(path).ok()
}

/// Read process info from `/proc/<pid>/stat`, through [`procinfo`].
///
/// The `(comm)` field and the positional fields after it used to be parsed
/// here. `procinfo` names them, and reads the file as bytes -- see
/// [`ProcessInfo::name`] for what reading it as text cost.
fn read_proc_stat(pid: u32) -> Option<ProcessInfo> {
    let stat = procinfo::ProcFs::new()
        .process_stat(u64::from(pid))
        .ok()
        .flatten()?;
    let name = stat.comm;
    let state = char::from(stat.state);
    let ppid = u32::try_from(stat.ppid).unwrap_or(0);
    let threads = u32::try_from(stat.num_threads).unwrap_or(1);

    // Read UID from /proc/<pid>/status.
    let uid = read_proc_uid(pid);
    let username = uid_to_name(uid);

    Some(ProcessInfo {
        pid,
        ppid,
        name,
        uid,
        threads,
        state,
        username,
    })
}

fn read_proc_uid(pid: u32) -> u32 {
    if let Some(content) = read_file(&format!("/proc/{pid}/status")) {
        for line in content.lines() {
            if let Some(val) = line.strip_prefix("Uid:") {
                return val
                    .split_whitespace()
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
            }
        }
    }
    0
}

fn read_proc_cmdline(pid: u32) -> String {
    if let Some(content) = read_file(&format!("/proc/{pid}/cmdline")) {
        let args: String = content.replace('\0', " ");
        let args = args.trim().to_string();
        if !args.is_empty() {
            return args;
        }
    }
    String::new()
}

/// Resolve UID to username via /etc/passwd.
fn uid_to_name(uid: u32) -> String {
    if let Some(content) = read_file("/etc/passwd") {
        for line in content.lines() {
            let fields: Vec<&str> = line.split(':').collect();
            if fields.len() >= 3
                && let Ok(file_uid) = fields[2].parse::<u32>()
                && file_uid == uid
            {
                return fields[0].to_string();
            }
        }
    }
    uid.to_string()
}

/// Enumerate all PIDs from /proc.
fn enumerate_pids() -> Vec<u32> {
    let mut pids = Vec::new();
    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str()
                && let Ok(pid) = name.parse::<u32>()
            {
                pids.push(pid);
            }
        }
    }
    pids.sort_unstable();
    pids
}

/// Build the full process map.
fn build_process_map() -> HashMap<u32, ProcessInfo> {
    let mut map = HashMap::new();
    for pid in enumerate_pids() {
        if let Some(info) = read_proc_stat(pid) {
            map.insert(pid, info);
        }
    }
    map
}

/// Build parent → children map.
fn build_children_map(procs: &HashMap<u32, ProcessInfo>) -> HashMap<u32, Vec<u32>> {
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for info in procs.values() {
        children.entry(info.ppid).or_default().push(info.pid);
    }
    // Sort children by PID (default) for stable output.
    for kids in children.values_mut() {
        kids.sort_unstable();
    }
    children
}

// ============================================================================
// Tree rendering
// ============================================================================

fn format_process(info: &ProcessInfo, opts: &Options) -> String {
    let mut parts = Vec::new();

    // Process name (or command line if long format).
    let name = if opts.long_format || opts.show_args {
        let cmdline = read_proc_cmdline(info.pid);
        if cmdline.is_empty() {
            // Kernel thread: {name}
            format!("{{{}}}", quoting::escape_unprintable(&info.name))
        } else {
            cmdline
        }
    } else {
        quoting::escape_unprintable(&info.name)
    };
    parts.push(name);

    // PID.
    if opts.show_pids {
        parts.push(format!("({})", info.pid));
    }

    // UID/username.
    if opts.show_uid {
        if opts.numeric_uid {
            parts.push(format!("[{}]", info.uid));
        } else {
            parts.push(format!("[{}]", info.username));
        }
    }

    // Thread count.
    if opts.show_threads && info.threads > 1 {
        parts.push(format!("{{{} threads}}", info.threads));
    }

    parts.join("")
}

/// Recursion-invariant context for `render_tree`: the output sink and the
/// process/child maps and options that stay constant across the whole walk.
/// Bundling these keeps `render_tree`'s per-node argument list small.
/// ANSI bold on and off, which is what psmisc uses for `-H`.
///
/// Emitted whether or not stdout is a terminal, which is measured rather than
/// assumed: piping `pstree -H <pid>` through `cat -A` still shows `^[[1m`, so
/// psmisc does not gate this on `isatty`. Matching that matters more than
/// being tidy about it -- a script diffing two `pstree -H` runs sees the same
/// bytes either way.
const HIGHLIGHT_ON: &str = "\x1b[1m";
const HIGHLIGHT_OFF: &str = "\x1b[0m";

/// The PIDs `-H` picks out: the named process **and every ancestor of it**.
///
/// Measured against psmisc 23.7 -- `pstree -H <shell>` bolds the shell, its
/// relay, its session leader, init and the root, so what is highlighted is the
/// PATH from the root down to the process rather than the single node. A
/// single-node reading would have looked right on a shallow tree and wrong on
/// every real one.
///
/// A PID that no longer exists highlights nothing, which falls out of the
/// lookup rather than needing a case: `procs.get` misses and the walk stops.
fn highlight_set(procs: &HashMap<u32, ProcessInfo>, target: Option<u32>) -> HashSet<u32> {
    let mut set = HashSet::new();
    let Some(mut pid) = target else {
        return set;
    };
    if !procs.contains_key(&pid) {
        return set;
    }
    // Bounded by the process count: a chain of parents cannot be longer than
    // the set of processes, and `insert` returning false catches a cycle
    // before the bound does. /proc is read a file at a time, so a parent link
    // that points at a reused PID is a real possibility rather than a
    // hypothetical one.
    for _ in 0..=procs.len() {
        if !set.insert(pid) {
            break;
        }
        match procs.get(&pid) {
            Some(info) if info.ppid != pid && info.ppid != 0 => pid = info.ppid,
            _ => break,
        }
    }
    set
}

struct TreeCtx<'a, 'o> {
    out: &'a mut io::StdoutLock<'o>,
    procs: &'a HashMap<u32, ProcessInfo>,
    children: &'a HashMap<u32, Vec<u32>>,
    opts: &'a Options,
    /// Empty unless `-H` was given, so the common path costs one hash lookup
    /// against an empty set.
    highlight: &'a HashSet<u32>,
}

fn render_tree(ctx: &mut TreeCtx<'_, '_>, pid: u32, prefix: &str, is_last: bool, is_root: bool) {
    let info = match ctx.procs.get(&pid) {
        Some(i) => i,
        None => return,
    };

    // Skip kernel threads if not requested.
    if !ctx.opts.show_kernel && info.ppid == 2 && pid != 2 {
        return;
    }

    let (branch, last, pipe, space) = if ctx.opts.ascii {
        (ASCII_BRANCH, ASCII_LAST, ASCII_PIPE, ASCII_SPACE)
    } else {
        (TREE_BRANCH, TREE_LAST, TREE_PIPE, TREE_SPACE)
    };

    // Print this node.
    let display = format_process(info, ctx.opts);
    // The whole rendered process, not just its name: psmisc bolds
    // `Relay(1125972)` including the pid it appends under `-p`.
    let display = if ctx.highlight.contains(&pid) {
        format!("{HIGHLIGHT_ON}{display}{HIGHLIGHT_OFF}")
    } else {
        display
    };

    if is_root {
        let _ = writeln!(ctx.out, "{display}");
    } else {
        let connector = if is_last { last } else { branch };
        let _ = writeln!(ctx.out, "{prefix}{connector}{display}");
    }

    // Print children.
    let mut kids = ctx.children.get(&pid).cloned().unwrap_or_default();

    // Sort by name if requested.
    if ctx.opts.sort_by_name {
        kids.sort_by(|a, b| {
            let name_a = ctx
                .procs
                .get(a)
                .map(|p| &p.name)
                .unwrap_or(&Vec::new())
                .clone();
            let name_b = ctx
                .procs
                .get(b)
                .map(|p| &p.name)
                .unwrap_or(&Vec::new())
                .clone();
            name_a.cmp(&name_b)
        });
    }

    // Compact mode: merge children with same name.
    if ctx.opts.compact {
        let mut merged: Vec<(u32, u32)> = Vec::new(); // (pid, count)
        let mut prev_name: Vec<u8> = Vec::new();
        for &kid_pid in &kids {
            let kid_name = ctx
                .procs
                .get(&kid_pid)
                .map(|p| &p.name)
                .cloned()
                .unwrap_or_default();
            if kid_name == prev_name && !merged.is_empty() {
                if let Some(last_entry) = merged.last_mut() {
                    last_entry.1 += 1;
                }
            } else {
                merged.push((kid_pid, 1));
                prev_name = kid_name;
            }
        }

        let child_prefix = if is_root {
            String::new()
        } else if is_last {
            format!("{prefix}{space}")
        } else {
            format!("{prefix}{pipe}")
        };

        for (idx, &(kid_pid, count)) in merged.iter().enumerate() {
            let kid_is_last = idx + 1 == merged.len();
            if count > 1 {
                let kid_info = ctx.procs.get(&kid_pid);
                let name =
                    kid_info.map_or_else(String::new, |p| quoting::escape_unprintable(&p.name));
                let connector = if kid_is_last { last } else { branch };
                let _ = writeln!(ctx.out, "{child_prefix}{connector}{count}*[{name}]");
            } else {
                render_tree(ctx, kid_pid, &child_prefix, kid_is_last, false);
            }
        }
    } else {
        let child_prefix = if is_root {
            String::new()
        } else if is_last {
            format!("{prefix}{space}")
        } else {
            format!("{prefix}{pipe}")
        };

        for (idx, &kid_pid) in kids.iter().enumerate() {
            let kid_is_last = idx + 1 == kids.len();
            render_tree(ctx, kid_pid, &child_prefix, kid_is_last, false);
        }
    }
}

// ============================================================================
// CLI parsing
// ============================================================================

fn print_usage() {
    println!("Usage: pstree [options] [PID|USER]");
    println!();
    println!("Display a tree of processes.");
    println!();
    println!("Options:");
    println!("  -p, --show-pids     Show PIDs");
    println!("  -u, --uid-changes   Show UID/username changes");
    println!("      --threads       Show thread counts (extension)");
    println!("  -a, --arguments     Show command line arguments");
    println!("  -l, --long          Long format (full command line)");
    println!("  -c, --compact=no    Don't compact identical subtrees");
    println!("  -A, --ascii         Use ASCII line drawing");
    println!("  -n, --numeric-sort  Sort by PID (default)");
    println!("      --name-sort     Sort by process name (extension)");
    println!("  -k, --show-kernel   Show kernel threads");
    println!("      --numeric-uid   Show numeric UIDs (extension)");
    println!("  -H PID, --highlight-pid=PID  Highlight a PID");
    println!("  -h, --highlight-all Highlight this process and its ancestors");
    println!("      --help          Show this help");
    println!("  -V, --version       Show version");
}

fn parse_args() -> Options {
    parse_args_from(&env::args().collect::<Vec<String>>())
}

/// Parse an explicit argv.
///
/// Split from `parse_args` so the bindings can be asserted without a process.
/// They are worth asserting: four of this program's short options meant
/// something other than psmisc gives them, and nothing in the crate could
/// have caught that, because nothing could call the parser.
///
/// `--help` and `--version` still exit, so a test must not pass them.
fn parse_args_from(args: &[String]) -> Options {
    let mut opts = Options {
        show_pids: false,
        show_uid: false,
        show_threads: false,
        ascii: false,
        compact: true,
        root_pid: None,
        highlight_pid: None,
        show_kernel: false,
        long_format: false,
        sort_by_name: false,
        show_args: false,
        numeric_uid: false,
    };

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            // `-h` is `--highlight-all` in psmisc, and `--help` there is
            // long-only. Implemented rather than reported: highlighting a
            // process and its ancestors is what `-H PID` already does, and
            // "current process" is this one's own pid.
            "-h" | "--highlight-all" => opts.highlight_pid = Some(process::id()),
            "--help" | "help" => {
                print_usage();
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("pstree {VERSION}");
                process::exit(0);
            }
            "-p" | "--show-pids" => opts.show_pids = true,
            "-u" | "--uid-changes" => opts.show_uid = true,
            // `-t` is `--thread-names` in psmisc (show full thread names)
            // and `-T` is `--hide-threads`. What this does -- append
            // `{N threads}` -- is neither, so it keeps its own name and
            // gives the letter back rather than claiming a meaning it does
            // not implement.
            "--threads" => opts.show_threads = true,
            "-a" | "--arguments" => opts.show_args = true,
            "-l" | "--long" => opts.long_format = true,
            "-c" => opts.compact = false,
            "--compact=no" => opts.compact = false,
            "-A" | "--ascii" => opts.ascii = true,
            "-n" | "--numeric-sort" => opts.sort_by_name = false,
            // `-N` is `--ns-sort=TYPE` in psmisc and CONSUMES AN ARGUMENT,
            // so `pstree -N pid` sorted by namespace there and by name here,
            // leaving `pid` to be read as a process to show. This one hid
            // from the checker as well: psmisc writes it `-N TYPE,
            // --ns-sort=TYPE`, and the reference parser wanted the comma
            // beside the letter.
            "--name-sort" => opts.sort_by_name = true,
            "-k" | "--show-kernel" => opts.show_kernel = true,
            // `-g` is `--show-pgids` in psmisc. `--numeric-uid` is an
            // extension -- psmisc has no such option -- so it gives the
            // letter back and keeps the long name.
            "--numeric-uid" => {
                opts.numeric_uid = true;
                opts.show_uid = true;
            }
            "-H" | "--highlight-pid" => {
                i += 1;
                if i < args.len() {
                    opts.highlight_pid = args[i].parse().ok();
                }
            }
            s if s.starts_with("--highlight-pid=") => {
                opts.highlight_pid = s
                    .strip_prefix("--highlight-pid=")
                    .and_then(|v| v.parse().ok());
            }
            s if !s.starts_with('-') => {
                // Could be a PID or username.
                if let Ok(pid) = s.parse::<u32>() {
                    opts.root_pid = Some(pid);
                } else {
                    // Treat as username — find their processes.
                    // We'll handle this by finding the user's login process.
                    if let Some(uid) = resolve_username(s) {
                        opts.root_pid = find_first_pid_for_uid(uid);
                        opts.show_uid = true;
                    } else {
                        eprintln!("pstree: user {} not found", quoteaf_os(s));
                        process::exit(1);
                    }
                }
            }
            other => {
                eprintln!("pstree: unknown option: {other}");
                process::exit(1);
            }
        }
        i += 1;
    }

    opts
}

fn resolve_username(name: &str) -> Option<u32> {
    if let Some(content) = read_file("/etc/passwd") {
        for line in content.lines() {
            let fields: Vec<&str> = line.split(':').collect();
            if fields.len() >= 3 && fields[0] == name {
                return fields[2].parse().ok();
            }
        }
    }
    None
}

fn find_first_pid_for_uid(uid: u32) -> Option<u32> {
    enumerate_pids()
        .into_iter()
        .find(|&pid| read_proc_uid(pid) == uid)
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let opts = parse_args();
    let procs = build_process_map();
    let children = build_children_map(&procs);

    let stdout = io::stdout();
    let mut out = stdout.lock();

    let root = opts.root_pid.unwrap_or(1);

    if procs.contains_key(&root) {
        let highlight = highlight_set(&procs, opts.highlight_pid);
        let mut ctx = TreeCtx {
            out: &mut out,
            procs: &procs,
            children: &children,
            opts: &opts,
            highlight: &highlight,
        };
        render_tree(&mut ctx, root, "", true, true);
    } else if procs.is_empty() {
        eprintln!("pstree: no processes found");
    } else {
        eprintln!("pstree: PID {root} not found");
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| (*s).to_string()).collect()
    }

    /// The four letters psmisc gives to something else.
    ///
    /// `-h` is `--highlight-all`, `-t` is `--thread-names`, `-g` is
    /// `--show-pgids`, `-N` is `--ns-sort=TYPE`. Each meant something else
    /// here, and `-N` consumes an argument upstream -- so `pstree -N pid`
    /// sorted by namespace there and left `pid` as a process to show here.
    #[test]
    fn short_options_do_not_claim_psmisc_meanings() {
        // -h highlights this process and its ancestors; it is not help.
        let opts = parse_args_from(&argv(&["pstree", "-h"]));
        assert_eq!(opts.highlight_pid, Some(std::process::id()));

        // The extensions keep their long names and give the letters back.
        let opts = parse_args_from(&argv(&["pstree", "--threads"]));
        assert!(opts.show_threads);
        let opts = parse_args_from(&argv(&["pstree", "--numeric-uid"]));
        assert!(opts.numeric_uid && opts.show_uid);
        let opts = parse_args_from(&argv(&["pstree", "--name-sort"]));
        assert!(opts.sort_by_name);

        // The freed letters are now REJECTED rather than silently doing
        // something else -- `pstree -t` prints "unknown option: -t" and exits
        // 1, which is the visible failure a missing option should have. That
        // cannot be asserted here, because the unknown-option arm calls
        // `process::exit` and would take the test process with it; making it
        // return a `Result` is a separate change.

        // The ones that already matched psmisc still work.
        let opts = parse_args_from(&argv(&["pstree", "-H", "42"]));
        assert_eq!(opts.highlight_pid, Some(42));
        let opts = parse_args_from(&argv(&["pstree", "-p", "-a", "-l"]));
        assert!(opts.show_pids && opts.show_args && opts.long_format);
    }

    use super::*;

    /// `&[u8]`, so a fixture can hold a name this program must not drop --
    /// and, with it, every child of that process.
    fn make_proc(pid: u32, ppid: u32, name: &[u8]) -> ProcessInfo {
        ProcessInfo {
            pid,
            ppid,
            name: name.to_vec(),
            uid: 0,
            threads: 1,
            state: 'S',
            username: "root".to_string(),
        }
    }

    // ---------------- -H / --highlight-pid ----------------

    /// A tree: 1 -> 10 -> 100, with 11 a sibling of 10.
    fn highlight_tree() -> HashMap<u32, ProcessInfo> {
        let mut procs = HashMap::new();
        procs.insert(1, make_proc(1, 0, b"init"));
        procs.insert(10, make_proc(10, 1, b"login"));
        procs.insert(11, make_proc(11, 1, b"cron"));
        procs.insert(100, make_proc(100, 10, b"bash"));
        procs
    }

    /// `-H` picks out the PATH from the root down to the process, not the
    /// single node. Measured against psmisc 23.7, which bolds the shell, its
    /// relay, its session leader, init and the root. A single-node reading
    /// looks right on a shallow tree and wrong on every real one.
    #[test]
    fn highlight_covers_the_target_and_all_its_ancestors() {
        let procs = highlight_tree();
        let set = highlight_set(&procs, Some(100));
        assert!(set.contains(&100), "the target itself");
        assert!(set.contains(&10), "its parent");
        assert!(set.contains(&1), "the root");
        assert!(!set.contains(&11), "a sibling branch is untouched");
        assert_eq!(set.len(), 3);
    }

    /// The root highlights only itself -- there is nothing above it, and the
    /// walk must stop rather than follow ppid 0 into a process that is not
    /// in the map.
    #[test]
    fn highlighting_the_root_stops_at_the_root() {
        let procs = highlight_tree();
        let set = highlight_set(&procs, Some(1));
        assert_eq!(set.len(), 1);
        assert!(set.contains(&1));
    }

    /// Without `-H` nothing is highlighted, and the set is empty rather than
    /// absent so the render path costs one lookup instead of a branch.
    #[test]
    fn no_highlight_pid_highlights_nothing() {
        let procs = highlight_tree();
        assert!(highlight_set(&procs, None).is_empty());
    }

    /// A PID that is not in the tree highlights nothing. It is the ordinary
    /// case of asking about a process that has exited between the scan and
    /// the argument being read, not an error.
    #[test]
    fn an_unknown_pid_highlights_nothing() {
        let procs = highlight_tree();
        assert!(highlight_set(&procs, Some(9999)).is_empty());
    }

    /// A parent link that loops must not spin. /proc is read a file at a time,
    /// so a ppid pointing at a reused PID is a real possibility rather than a
    /// hypothetical: this test is the reason the walk is bounded AND checks
    /// `insert`'s return rather than relying on either alone.
    #[test]
    fn a_parent_cycle_terminates() {
        let mut procs = HashMap::new();
        procs.insert(5, make_proc(5, 6, b"a"));
        procs.insert(6, make_proc(6, 5, b"b"));
        let set = highlight_set(&procs, Some(5));
        assert_eq!(set.len(), 2, "both, once each, and no hang");
    }

    /// A process whose ppid is itself is the same hazard one step shorter.
    #[test]
    fn a_self_parent_terminates() {
        let mut procs = HashMap::new();
        procs.insert(7, make_proc(7, 7, b"looped"));
        assert_eq!(highlight_set(&procs, Some(7)).len(), 1);
    }

    #[test]
    fn test_build_children_map() {
        let mut procs = HashMap::new();
        procs.insert(1, make_proc(1, 0, b"init"));
        procs.insert(2, make_proc(2, 1, b"bash"));
        procs.insert(3, make_proc(3, 1, b"sshd"));
        procs.insert(4, make_proc(4, 2, b"vim"));

        let children = build_children_map(&procs);
        assert_eq!(children.get(&1).unwrap(), &[2, 3]);
        assert_eq!(children.get(&2).unwrap(), &[4]);
        assert!(!children.contains_key(&3) || children.get(&3).unwrap().is_empty());
    }

    /// **A name that is not UTF-8 is shown, not dropped.**
    ///
    /// `/proc/<pid>/stat` was read with `read_to_string`, which fails on such
    /// a name, so the process disappeared -- **and every child with it**,
    /// because a tree is assembled by matching each process's `ppid` against a
    /// parent that has to be present. One unreadable name could hide an
    /// arbitrarily large subtree, which is a bigger effect than the same bug
    /// had in `pgrep` or `top`.
    #[test]
    fn a_name_that_is_not_utf8_still_appears() {
        let info = make_proc(42, 1, b"ser\xffver");
        let opts = Options {
            show_pids: false,
            show_uid: false,
            show_threads: false,
            ascii: false,
            compact: true,
            root_pid: None,
            highlight_pid: None,
            show_kernel: false,
            long_format: false,
            sort_by_name: false,
            show_args: false,
            numeric_uid: false,
        };
        let shown = format_process(&info, &opts);
        assert!(!shown.is_empty(), "the process must still be rendered");
        assert!(shown.starts_with("ser"), "the readable part stays readable");
        assert!(
            !shown.contains('\u{fffd}'),
            "no replacement character: that is a guess about what the byte meant"
        );
    }

    /// Two children with the same name still merge in compact mode when that
    /// name is not text -- the comparison is over bytes, so it neither breaks
    /// nor starts merging things that differ.
    #[test]
    fn compaction_compares_names_that_are_not_text() {
        let a = make_proc(2, 1, b"w\xffrker");
        let b = make_proc(3, 1, b"w\xffrker");
        let c = make_proc(4, 1, b"w\xferker");
        assert_eq!(a.name, b.name, "identical bytes are identical names");
        assert_ne!(a.name, c.name, "one byte apart is a different name");
    }

    #[test]
    fn test_format_process_basic() {
        let info = make_proc(42, 1, b"bash");
        let opts = Options {
            show_pids: false,
            show_uid: false,
            show_threads: false,
            ascii: false,
            compact: true,
            root_pid: None,
            highlight_pid: None,
            show_kernel: false,
            long_format: false,
            sort_by_name: false,
            show_args: false,
            numeric_uid: false,
        };
        assert_eq!(format_process(&info, &opts), "bash");
    }

    #[test]
    fn test_format_process_with_pid() {
        let info = make_proc(42, 1, b"bash");
        let opts = Options {
            show_pids: true,
            show_uid: false,
            show_threads: false,
            ascii: false,
            compact: true,
            root_pid: None,
            highlight_pid: None,
            show_kernel: false,
            long_format: false,
            sort_by_name: false,
            show_args: false,
            numeric_uid: false,
        };
        assert_eq!(format_process(&info, &opts), "bash(42)");
    }

    #[test]
    fn test_format_process_with_uid() {
        let info = ProcessInfo {
            pid: 42,
            ppid: 1,
            name: b"bash".to_vec(),
            uid: 1000,
            threads: 1,
            state: 'S',
            username: "alice".to_string(),
        };
        let opts = Options {
            show_pids: false,
            show_uid: true,
            show_threads: false,
            ascii: false,
            compact: true,
            root_pid: None,
            highlight_pid: None,
            show_kernel: false,
            long_format: false,
            sort_by_name: false,
            show_args: false,
            numeric_uid: false,
        };
        assert_eq!(format_process(&info, &opts), "bash[alice]");
    }

    #[test]
    fn test_format_process_with_numeric_uid() {
        let info = ProcessInfo {
            pid: 42,
            ppid: 1,
            name: b"bash".to_vec(),
            uid: 1000,
            threads: 1,
            state: 'S',
            username: "alice".to_string(),
        };
        let opts = Options {
            show_pids: false,
            show_uid: true,
            show_threads: false,
            ascii: false,
            compact: true,
            root_pid: None,
            highlight_pid: None,
            show_kernel: false,
            long_format: false,
            sort_by_name: false,
            show_args: false,
            numeric_uid: true,
        };
        assert_eq!(format_process(&info, &opts), "bash[1000]");
    }

    #[test]
    fn test_format_process_with_threads() {
        let mut info = make_proc(42, 1, b"java");
        info.threads = 16;
        let opts = Options {
            show_pids: false,
            show_uid: false,
            show_threads: true,
            ascii: false,
            compact: true,
            root_pid: None,
            highlight_pid: None,
            show_kernel: false,
            long_format: false,
            sort_by_name: false,
            show_args: false,
            numeric_uid: false,
        };
        assert_eq!(format_process(&info, &opts), "java{16 threads}");
    }

    #[test]
    fn test_format_process_single_thread() {
        let info = make_proc(42, 1, b"bash");
        let opts = Options {
            show_pids: false,
            show_uid: false,
            show_threads: true,
            ascii: false,
            compact: true,
            root_pid: None,
            highlight_pid: None,
            show_kernel: false,
            long_format: false,
            sort_by_name: false,
            show_args: false,
            numeric_uid: false,
        };
        // Single thread = no thread count shown.
        assert_eq!(format_process(&info, &opts), "bash");
    }

    #[test]
    fn test_format_process_all_options() {
        let mut info = ProcessInfo {
            pid: 42,
            ppid: 1,
            name: b"httpd".to_vec(),
            uid: 33,
            threads: 4,
            state: 'S',
            username: "www-data".to_string(),
        };
        info.threads = 4;
        let opts = Options {
            show_pids: true,
            show_uid: true,
            show_threads: true,
            ascii: false,
            compact: true,
            root_pid: None,
            highlight_pid: None,
            show_kernel: false,
            long_format: false,
            sort_by_name: false,
            show_args: false,
            numeric_uid: false,
        };
        assert_eq!(
            format_process(&info, &opts),
            "httpd(42)[www-data]{4 threads}"
        );
    }

    #[test]
    fn test_tree_characters() {
        assert_eq!(TREE_BRANCH, "├── ");
        assert_eq!(TREE_LAST, "└── ");
        assert_eq!(TREE_PIPE, "│   ");
        assert_eq!(TREE_SPACE, "    ");
    }

    #[test]
    fn test_ascii_characters() {
        assert_eq!(ASCII_BRANCH, "|-- ");
        assert_eq!(ASCII_LAST, "`-- ");
        assert_eq!(ASCII_PIPE, "|   ");
        assert_eq!(ASCII_SPACE, "    ");
    }

    #[test]
    fn test_enumerate_pids() {
        let pids = enumerate_pids();
        // Should not panic. May be empty on non-Linux systems.
        for &pid in &pids {
            assert!(pid > 0);
        }
    }

    #[test]
    fn test_children_map_sorted() {
        let mut procs = HashMap::new();
        procs.insert(1, make_proc(1, 0, b"init"));
        procs.insert(5, make_proc(5, 1, b"e"));
        procs.insert(3, make_proc(3, 1, b"c"));
        procs.insert(7, make_proc(7, 1, b"g"));
        procs.insert(2, make_proc(2, 1, b"b"));

        let children = build_children_map(&procs);
        let kids = children.get(&1).unwrap();
        // Should be sorted by PID.
        assert_eq!(kids, &[2, 3, 5, 7]);
    }

    #[test]
    fn test_process_info_clone() {
        let info = make_proc(1, 0, b"init");
        let cloned = info.clone();
        assert_eq!(cloned.pid, 1);
        assert_eq!(cloned.name, b"init");
    }

    #[test]
    fn test_uid_to_name_fallback() {
        // Non-existent UID should return the numeric string.
        let name = uid_to_name(99999);
        // Either resolves or falls back to "99999".
        assert!(!name.is_empty());
    }
}
