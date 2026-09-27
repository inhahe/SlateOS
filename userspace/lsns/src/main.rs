//! lsns -- list namespaces.
//!
//! A port of util-linux 2.39.3's `sys-utils/lsns.c`, function by function
//! and with upstream's names, printing through `smartcols` (the
//! libsmartcols port) and reading the mount table through `ulmount` (the
//! libmount port) as upstream prints through libsmartcols and reads through
//! libmount; measured against `lsns from util-linux 2.39.3` by
//! `scripts/lsns-diff.sh`.
//!
//! This replaces a hand-written program that laid out its own table, read
//! `/proc` its own way, and had no `--tree`, NETNSID, NSFS or persistent
//! namespaces.
//!
//! Upstream's behaviour is kept where it shows:
//!
//! * A namespace is found through the processes in it, which `/proc` lists
//!   only as far as the caller may look: another user's `ns/` links answer
//!   `EACCES`, and that process's namespaces are not listed.
//! * The default listing is a tree of processes -- a namespace's line under
//!   the line its lowest process's parent was shown on -- but `--output-all`
//!   alone gives a list (the default tree is set only with the default
//!   columns).
//! * An `ioctl` the kernel does not have (`ENOTTY`) is warned about as
//!   `Unsupported ioctl NS_GET_...` and ends the listing with status 2.
//! * Persistent namespaces are those bind-mounted on nsfs (`ip netns add`);
//!   with `-P` only those with no process are listed.
//! * Only the first operand is read.
//! * `-l` and `-T` are meant to exclude each other, but upstream's table
//!   of exclusive options lists them out of the order its check needs, so
//!   the pair is accepted and the tree wins.
//! * A process whose `/proc/PID/stat` line cannot be read -- a command
//!   name holding a newline cuts the line before its `)` -- ends the whole
//!   listing with status 1 and no table.
//!
//! # What is not upstream's
//!
//! * **A name in a diagnostic** has its unprintable bytes escaped
//!   (design-decisions §370).
//! * **An orphan namespace with no process** -- a persistent one whose
//!   parent or owner is not listed -- is left without that relation, where
//!   upstream dereferences its missing process.

mod sys;

use getoptlong::{Opt, Program, Takes};
use quoting::{escape_unprintable, os_bytes};
use smartcols::{ColumnId, FL_RIGHT, FL_TREE, FL_TRUNC, FL_WRAP, JsonType, LineId, Table};
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::process::ExitCode;
use ulclosestream::{Stdout, stderr_write, warn, warnx};
use ulmount::tab::Table as MntTable;
use ulstrutils::{IdListError, num_error_message, string_add_to_idarray};

/// Getopt's errors are only sentences here; the referral follows them.
const LSNS: Program = Program::new("lsns", 1);

/// Upstream's option string.
const SHORTS: &str = "JlPp:o:nruhVt:T::W";

/// `OPT_OUTPUT_ALL`: `CHAR_MAX + 1`.
const OPT_OUTPUT_ALL: i32 = 128;

/// Upstream's `long_opts[]`, in its order, and each one's `val`.
const LONGS: &[(&str, Takes)] = &[
    ("json", Takes::Nothing),
    ("task", Takes::Required),
    ("help", Takes::Nothing),
    ("output", Takes::Required),
    ("output-all", Takes::Nothing),
    ("persistent", Takes::Nothing),
    ("notruncate", Takes::Nothing),
    ("version", Takes::Nothing),
    ("noheadings", Takes::Nothing),
    ("nowrap", Takes::Nothing),
    ("list", Takes::Nothing),
    ("raw", Takes::Nothing),
    ("type", Takes::Required),
    ("tree", Takes::Optional),
];
const LONG_VALS: [i32; 14] = [
    b'J' as i32,
    b'p' as i32,
    b'h' as i32,
    b'o' as i32,
    OPT_OUTPUT_ALL,
    b'P' as i32,
    b'u' as i32,
    b'V' as i32,
    b'n' as i32,
    b'W' as i32,
    b'l' as i32,
    b'r' as i32,
    b't' as i32,
    b'T' as i32,
];

/// `excl[]`, as upstream has it -- including its last row, which is out of
/// the ASCII order `err_exclusive_options` needs: the scan stops at the
/// first row starting past the option, so `-T` never reaches it and `-l`
/// with `-T` is accepted (the tree wins).
const EXCL: [&[i32]; 3] = [
    &[b'J' as i32, b'r' as i32],
    &[b'P' as i32, b'p' as i32],
    &[b'l' as i32, b'T' as i32],
];

/// `EXIT_UNSUPPORTED_IOCTL`.
const EXIT_UNSUPPORTED_IOCTL: u8 = 2;
/// `MNT_EX_FAIL`: `failed to parse /proc/self/mountinfo`.
const MNT_EX_FAIL: u8 = 32;
/// `LSNS_NETNS_UNUSABLE`.
const NETNS_UNUSABLE: i32 = -2;
/// `NETNSA_NSID_NOT_ASSIGNED`.
const NSID_NOT_ASSIGNED: i32 = -1;
/// `EACCES`, `ENOENT`, `EPERM`.
const EACCES: i32 = 13;
const ENOENT: i32 = 2;
const EPERM: i32 = 1;

/// `ns_names[]`, in `LSNS_ID_*` order.
const NS_NAMES: [&str; 8] = ["mnt", "net", "pid", "uts", "ipc", "user", "cgroup", "time"];
const ID_NET: usize = 1;
const ID_PID: usize = 2;
const ID_USER: usize = 5;

/// `RELA_PARENT`, `RELA_OWNER`: how one namespace is related to another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Relation {
    /// The parent of a PID or user namespace.
    Parent,
    /// The user namespace that owns a namespace.
    Owner,
}

/// `[RELA_PARENT]` and `[RELA_OWNER]` of one of upstream's `MAX_RELA`
/// arrays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Rela<T> {
    parent: T,
    owner: T,
}

impl<T: Copy> Rela<T> {
    /// The value for `rela`.
    fn get(&self, rela: Relation) -> T {
        match rela {
            Relation::Parent => self.parent,
            Relation::Owner => self.owner,
        }
    }

    /// Set the value for `rela`.
    fn set(&mut self, rela: Relation, value: T) {
        match rela {
            Relation::Parent => self.parent = value,
            Relation::Owner => self.owner = value,
        }
    }
}

/// The file system a persistent namespace is bind-mounted from.
const NSFS: &[u8] = b"nsfs";

/// `COL_*`, in upstream's enum order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Col {
    Ns,
    Type,
    Path,
    Nprocs,
    Pid,
    Ppid,
    Command,
    Uid,
    User,
    Netnsid,
    Nsfs,
    Pns,
    Ons,
}

/// `struct colinfo`.
struct ColInfo {
    id: Col,
    name: &'static str,
    whint: f64,
    flags: u32,
    help: &'static str,
    json: JsonType,
}

/// `infos[]`.
const INFOS: [ColInfo; 13] = [
    ColInfo {
        id: Col::Ns,
        name: "NS",
        whint: 10.0,
        flags: FL_RIGHT,
        help: "namespace identifier (inode number)",
        json: JsonType::Number,
    },
    ColInfo {
        id: Col::Type,
        name: "TYPE",
        whint: 5.0,
        flags: 0,
        help: "kind of namespace",
        json: JsonType::String,
    },
    ColInfo {
        id: Col::Path,
        name: "PATH",
        whint: 0.0,
        flags: 0,
        help: "path to the namespace",
        json: JsonType::String,
    },
    ColInfo {
        id: Col::Nprocs,
        name: "NPROCS",
        whint: 5.0,
        flags: FL_RIGHT,
        help: "number of processes in the namespace",
        json: JsonType::Number,
    },
    ColInfo {
        id: Col::Pid,
        name: "PID",
        whint: 5.0,
        flags: FL_RIGHT,
        help: "lowest PID in the namespace",
        json: JsonType::Number,
    },
    ColInfo {
        id: Col::Ppid,
        name: "PPID",
        whint: 5.0,
        flags: FL_RIGHT,
        help: "PPID of the PID",
        json: JsonType::Number,
    },
    ColInfo {
        id: Col::Command,
        name: "COMMAND",
        whint: 0.0,
        flags: FL_TRUNC,
        help: "command line of the PID",
        json: JsonType::String,
    },
    ColInfo {
        id: Col::Uid,
        name: "UID",
        whint: 0.0,
        flags: FL_RIGHT,
        help: "UID of the PID",
        json: JsonType::Number,
    },
    ColInfo {
        id: Col::User,
        name: "USER",
        whint: 0.0,
        flags: 0,
        help: "username of the PID",
        json: JsonType::String,
    },
    ColInfo {
        id: Col::Netnsid,
        name: "NETNSID",
        whint: 0.0,
        flags: FL_RIGHT,
        help: "namespace ID as used by network subsystem",
        json: JsonType::String,
    },
    ColInfo {
        id: Col::Nsfs,
        name: "NSFS",
        whint: 0.0,
        flags: FL_WRAP,
        help: "nsfs mountpoint (usually used network subsystem)",
        json: JsonType::String,
    },
    ColInfo {
        id: Col::Pns,
        name: "PNS",
        whint: 10.0,
        flags: FL_RIGHT,
        help: "parent namespace identifier (inode number)",
        json: JsonType::Number,
    },
    ColInfo {
        id: Col::Ons,
        name: "ONS",
        whint: 10.0,
        flags: FL_RIGHT,
        help: "owner namespace identifier (inode number)",
        json: JsonType::Number,
    },
];

/// `columns[ARRAY_SIZE(infos) * 2]`.
const MAX_COLUMNS: usize = 26;

/// `LSNS_TREE_*`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Tree {
    #[default]
    None,
    Process,
    Owner,
    Parent,
}

/// `struct lsns_process`.
#[derive(Clone, Debug, Default)]
struct Process {
    pid: i32,
    ppid: i32,
    uid: u32,
    ns_ids: [u64; 8],
    ns_pids: [u64; 8],
    ns_oids: [u64; 8],
    /// Index of the parent process, if it is listed.
    parent: Option<usize>,
    /// The line this process was last shown on.
    outline: Option<LineId>,
    netnsid: i32,
}

/// `struct lsns_namespace`.
#[derive(Clone, Debug, Default)]
struct Namespace {
    id: u64,
    ty: usize,
    nprocs: i32,
    related_id: Rela<u64>,
    /// The process with the lowest PID in it.
    proc_: Option<usize>,
    related_ns: Rela<Option<usize>>,
    ns_outline: Option<LineId>,
    uid_fallback: u32,
    /// Its processes, in the order they were added.
    processes: Vec<usize>,
}

/// `struct lsns`, and the globals: the UID cache and the netlink socket.
#[derive(Default)]
struct Lsns {
    processes: Vec<Process>,
    /// Every namespace, by a stable index...
    namespaces: Vec<Namespace>,
    /// ...and in list order (sorted by ID once read).
    order: Vec<usize>,
    fltr_pid: i32,
    fltr_ns: u64,
    fltr_types: [bool; 8],
    fltr_ntypes: usize,
    raw: bool,
    json: bool,
    tree: Tree,
    persist: bool,
    no_trunc: bool,
    no_headings: bool,
    no_wrap: bool,
    tab: MntTable,
    columns: Vec<Col>,
    /// `uid_cache`: each UID's name, in the order they were added.
    uid_cache: Vec<(u32, Vec<u8>)>,
    netlink: Option<sys::Netlink>,
    /// `netnsids_cache`: newest first, as `list_add` puts them.
    netnsids: Vec<(u64, i32)>,
    short: Vec<u8>,
    /// The locale's codeset is UTF-8: how a user's name is measured.
    utf8: bool,
}

/// `program_invocation_short_name`: argv[0] past its last `/`.
fn short_name(arg0: &OsStr) -> Vec<u8> {
    let bytes = os_bytes(arg0);
    let start = bytes
        .iter()
        .rposition(|&b| b == b'/')
        .map_or(0, |i| i.saturating_add(1));
    bytes.get(start..).unwrap_or_default().to_vec()
}

/// Bytes shown in a diagnostic: upstream's text, unprintable bytes escaped.
fn shown(text: &[u8]) -> String {
    escape_unprintable(text)
}

/// `errtryhelp(EXIT_FAILURE)`.
fn errtryhelp(short: &[u8]) -> u8 {
    stderr_write(format!("Try '{} --help' for more information.\n", shown(short)).as_bytes());
    1
}

/// The column's `infos[]` entry: `INFOS` is in `Col`'s order.
fn info(col: Col) -> &'static ColInfo {
    INFOS.get(col as usize).unwrap_or(&INFOS[0])
}

/// `usage()`.
fn usage(short: &[u8]) -> Vec<u8> {
    let mut t = String::from("\nUsage:\n");
    t.push_str(&format!(" {} [options] [<namespace>]\n", shown(short)));
    t.push_str("\nList system namespaces.\n");
    t.push_str("\nOptions:\n");
    t.push_str(" -J, --json             use JSON output format\n");
    t.push_str(" -l, --list             use list format output\n");
    t.push_str(" -n, --noheadings       don't print headings\n");
    t.push_str(" -o, --output <list>    define which output columns to use\n");
    t.push_str("     --output-all       output all columns\n");
    t.push_str(" -P, --persistent       namespaces without processes\n");
    t.push_str(" -p, --task <pid>       print process namespaces\n");
    t.push_str(" -r, --raw              use the raw output format\n");
    t.push_str(" -u, --notruncate       don't truncate text in columns\n");
    t.push_str(" -W, --nowrap           don't use multi-line representation\n");
    t.push_str(
        " -t, --type <name>      namespace type (mnt, net, ipc, user, pid, uts, cgroup, time)\n",
    );
    t.push_str(" -T, --tree <rel>       use tree format (parent, owner, or process)\n");
    t.push('\n');
    t.push_str(&format!(
        "{:<24}{}\n{:<24}{}\n",
        " -h, --help", "display this help", " -V, --version", "display version"
    ));
    t.push_str("\nAvailable output columns:\n");
    for i in &INFOS {
        t.push_str(&format!(" {:>11}  {}\n", i.name, i.help));
    }
    t.push_str("\nFor more details see lsns(8).\n");
    t.into_bytes()
}

/// `column_name_to_id(name, namesz)`.
fn column_name_to_id(name: &[u8], rest: &[u8], short: &[u8]) -> Option<Col> {
    let found = INFOS
        .iter()
        .find(|i| i.name.as_bytes().eq_ignore_ascii_case(name))
        .map(|i| i.id);
    if found.is_none() {
        warnx(short, &format!("unknown column: {}", shown(rest)));
    }
    found
}

/// The `errno` of a failed call. Every failure here is a system call's, so
/// each carries one; `EINVAL`, which nothing tolerates, stands in otherwise.
fn errno_of(e: &std::io::Error) -> i32 {
    e.raw_os_error().unwrap_or(EINVAL)
}

/// The `stat` of a path relative to a process's directory, following links
/// (`fstatat(dir, path, &st, 0)`): its inode.
fn stat_ino(path: &str) -> Result<u64, i32> {
    std::fs::metadata(path)
        .map(|m| ino_of(&m))
        .map_err(|e| errno_of(&e))
}

/// `procfs_dirent_get_pid(d, &pid)`: a `/proc` entry that is a directory
/// (or of a type the file system does not say) named by a number, which
/// `ul_strtou64` reads and a cast makes a `pid_t`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    reason = "upstream's `(pid_t) num`: /proc names no process by a number past a pid_t"
)]
fn dirent_pid(entry: &std::fs::DirEntry) -> Option<i32> {
    if !entry.file_type().is_ok_and(|t| t.is_dir()) {
        return None;
    }
    let name = os_bytes(&entry.file_name()).into_owned();
    if !name.first().is_some_and(u8::is_ascii_digit) {
        return None;
    }
    ulstrutils::ul_strtou64(&name, 10).ok().map(|n| n as i32)
}

/// `st_ino`.
fn ino_of(m: &std::fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        m.ino()
    }
    #[cfg(not(unix))]
    {
        let _ = m;
        0
    }
}

/// `st_uid`.
fn uid_of(m: &std::fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        m.uid()
    }
    #[cfg(not(unix))]
    {
        let _ = m;
        0
    }
}

/// `EINVAL`: what `parse_proc_stat` answers for a line it cannot read.
const EINVAL: i32 = 22;

/// `%d` in `sscanf`: white space skipped, then a number read as glibc reads
/// it -- as a `long`, clamped when it overflows, then cut to an `int` --
/// with where the digits ended.
#[allow(
    clippy::cast_possible_truncation,
    reason = "glibc stores `%d` by casting its `long` to `int`, which is the truncation"
)]
fn scan_int(s: &[u8]) -> Option<(i32, usize)> {
    let sc = ulstrutils::scan_integer(s, 10)?;
    // `LONG_MIN` is both -2^63 read exactly and anything below it clamped.
    let v: i64 = match i64::try_from(sc.magnitude) {
        Ok(m) if sc.negative => m.saturating_neg(),
        Ok(m) => m,
        Err(_) if sc.negative => i64::MIN,
        Err(_) => i64::MAX,
    };
    Some((v as i32, sc.end))
}

/// `parse_proc_stat(fp, &pid, &state, &ppid)`: the first line of
/// `/proc/PID/stat` read by `sscanf(line, "%d (", pid)` and, from its last
/// `)`, by `sscanf(p, ") %c %d*[^\n]", state, ppid)`.
///
/// `sscanf` counts conversions, not literals, so the first scan asks only
/// for a number: what follows it is not looked at. The line ends at its
/// first newline, as `getline` ends it, so a command name holding one leaves
/// no `)` on the line -- `EINVAL`, which ends the whole listing, as it ends
/// upstream's.
fn parse_proc_stat(text: &[u8]) -> Result<(i32, i32), i32> {
    let line = text.split(|&b| b == b'\n').next().unwrap_or_default();
    // `strrchr` stops at a NUL, which a C string ends with.
    let line = line.split(|&b| b == 0).next().unwrap_or_default();
    let close = line.iter().rposition(|&b| b == b')').ok_or(EINVAL)?;
    let (pid, _) = scan_int(line).ok_or(EINVAL)?;
    // `) %c %d`: the `)`, any white space, one byte, and a number (which
    // skips white space itself).
    let after = line.get(close.saturating_add(1)..).unwrap_or_default();
    let ws = after
        .iter()
        .take_while(|&&b| ulstrutils::c_isspace(b))
        .count();
    let after = after.get(ws..).unwrap_or_default();
    if after.is_empty() {
        return Err(EINVAL);
    }
    let (ppid, _) = scan_int(after.get(1..).unwrap_or_default()).ok_or(EINVAL)?;
    Ok((pid, ppid))
}

/// `strdup_procfs_file(pid, name)`: `/proc/PID/NAME` read by `read_all`
/// into a `BUFSIZ` buffer, each NUL made a blank and the last byte cut;
/// `None` when nothing was read.
///
/// `read_all` keeps what it read before a failure, retries `EINTR` and
/// `EAGAIN` five times a quarter of a second apart, and stops at the end of
/// the buffer -- so a command line longer than 8191 bytes is cut there.
fn strdup_procfs_file(pid: i32, name: &str) -> Option<Vec<u8>> {
    use std::io::{ErrorKind, Read};
    let mut f = File::open(format!("/proc/{pid}/{name}")).ok()?;
    let mut buf = vec![0u8; 8192];
    let mut n = 0usize;
    let mut tries = 0u32;
    while n < buf.len() {
        match f.read(buf.get_mut(n..).unwrap_or_default()) {
            Ok(0) => break,
            Ok(k) => {
                tries = 0;
                n = n.saturating_add(k);
            }
            Err(e)
                if matches!(e.kind(), ErrorKind::Interrupted | ErrorKind::WouldBlock)
                    && tries < 5 =>
            {
                tries = tries.saturating_add(1);
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            Err(_) => break,
        }
    }
    if n == 0 {
        return None;
    }
    buf.truncate(n);
    for b in &mut buf {
        if *b == 0 {
            *b = b' ';
        }
    }
    buf.pop();
    Some(buf)
}

/// `LOGIN_NAME_MAX`: how many characters of a name `add_id` measures.
const LOGIN_NAME_MAX: usize = 256;

/// Whether util-linux's `add_id` keeps a user's name rather than its number:
/// when `mbstowcs` converts it, its `wcswidth` must be above 0 -- so a name
/// holding a character that does not print, or only characters of no width,
/// is shown as the number -- and when it does not convert (a byte the
/// locale has no character for), any name that is not empty is kept.
fn name_is_kept(name: &[u8], utf8: bool) -> bool {
    let converted = if utf8 || name.is_ascii() {
        std::str::from_utf8(name).ok()
    } else {
        None
    };
    match converted {
        Some(s) if !s.is_empty() => {
            let mut width = 0usize;
            for c in s.chars().take(LOGIN_NAME_MAX) {
                if !quoting::printable_char(c) {
                    return false;
                }
                width = width.saturating_add(charwidth::char_width(c).unwrap_or(0));
            }
            width > 0
        }
        _ => !name.is_empty(),
    }
}

impl Lsns {
    /// `add_uid(uid_cache, uid)`: the user's name, or the number when the
    /// password database has none (or only one that prints as nothing).
    fn add_uid(&mut self, uid: u32) {
        if self.uid_cache.iter().any(|(u, _)| *u == uid) {
            return;
        }
        let utf8 = self.utf8;
        let name = sys::getpwuid_name(uid).filter(|n| name_is_kept(n, utf8));
        let name = name.unwrap_or_else(|| uid.to_string().into_bytes());
        self.uid_cache.push((uid, name));
    }

    /// `get_id(uid_cache, uid)->name`.
    fn user_name(&self, uid: u32) -> Vec<u8> {
        self.uid_cache
            .iter()
            .find(|(u, _)| *u == uid)
            .map(|(_, n)| n.clone())
            .unwrap_or_else(|| uid.to_string().into_bytes())
    }

    /// An nsfs `ioctl`, warned about as upstream's `lsns_ioctl` warns when
    /// the kernel does not have it.
    fn ns_ioctl_fd(&self, file: &File, request: u64, name: &str) -> Result<File, i32> {
        let r = sys::ioctl_fd(file, request);
        if r.as_ref().err() == Some(&sys::ENOTTY) {
            warnx(&self.short, &format!("Unsupported ioctl {name}"));
        }
        r
    }

    /// `get_ns_ino(dir, nsname, &ino, &pino, &oino)`: the namespace's inode,
    /// and those of its parent (a PID or user namespace's) and owner.
    ///
    /// As upstream's, it writes as it goes: a failure after the `stat` has
    /// already set `ino` (and cleared the other two), and the caller keeps
    /// what was written when the failure is one it tolerates.
    fn get_ns_ino(
        &self,
        pid: i32,
        nsname: &str,
        ino: &mut u64,
        pino: &mut u64,
        oino: &mut u64,
    ) -> Result<(), i32> {
        let path = format!("/proc/{pid}/ns/{nsname}");
        *ino = stat_ino(&path)?;
        *pino = 0;
        *oino = 0;
        let fd = File::open(&path).map_err(|e| errno_of(&e))?;
        if nsname == "pid" || nsname == "user" {
            match self.ns_ioctl_fd(&fd, sys::NS_GET_PARENT, "NS_GET_PARENT") {
                Ok(pfd) => {
                    *pino = pfd
                        .metadata()
                        .map(|m| ino_of(&m))
                        .map_err(|e| errno_of(&e))?
                }
                // The root of the hierarchy, or one outside ours.
                Err(EPERM) => {}
                Err(e) => return Err(e),
            }
        }
        match self.ns_ioctl_fd(&fd, sys::NS_GET_USERNS, "NS_GET_USERNS") {
            Ok(ofd) => {
                *oino = ofd
                    .metadata()
                    .map(|m| ino_of(&m))
                    .map_err(|e| errno_of(&e))?
            }
            Err(EPERM) => {}
            Err(e) => return Err(e),
        }
        Ok(())
    }

    /// `get_netnsid(dir, netino)`: from the cache, else by netlink.
    fn get_netnsid(&mut self, pid: i32, netino: u64) -> i32 {
        if let Some(&(_, id)) = self.netnsids.iter().find(|(i, _)| *i == netino) {
            return id;
        }
        let id = match &self.netlink {
            None => NETNS_UNUSABLE,
            Some(nl) => match File::open(format!("/proc/{pid}/ns/net")) {
                Ok(target) => nl.get_nsid(&target).unwrap_or(NETNS_UNUSABLE),
                Err(_) => NETNS_UNUSABLE,
            },
        };
        self.netnsids.insert(0, (netino, id));
        id
    }

    /// `read_process(ls, pid)`: `Err(errno)` for a process that could not be
    /// read.
    fn read_process(&mut self, pid: i32) -> Result<(), i32> {
        let dir = format!("/proc/{pid}");
        // `opendir`: a process that has gone is `ENOENT`.
        std::fs::read_dir(&dir).map_err(|e| errno_of(&e))?;
        let mut p = Process {
            netnsid: NETNS_UNUSABLE,
            ..Process::default()
        };
        // `fstat` of the directory: the process's owner. Upstream carries on
        // without one when it fails.
        if let Ok(meta) = std::fs::metadata(&dir) {
            p.uid = uid_of(&meta);
            self.add_uid(p.uid);
        }
        let stat = std::fs::read(format!("{dir}/stat")).map_err(|e| errno_of(&e))?;
        let (spid, ppid) = parse_proc_stat(&stat)?;
        p.pid = spid;
        p.ppid = ppid;
        for (i, name) in NS_NAMES.iter().enumerate() {
            if !self.fltr_types.get(i).copied().unwrap_or(false) {
                continue;
            }
            let (mut ino, mut pino, mut oino) = (0u64, 0u64, 0u64);
            let rc = self.get_ns_ino(pid, name, &mut ino, &mut pino, &mut oino);
            for (slot, value) in [
                (&mut p.ns_ids, ino),
                (&mut p.ns_pids, pino),
                (&mut p.ns_oids, oino),
            ] {
                if let Some(x) = slot.get_mut(i) {
                    *x = value;
                }
            }
            match rc {
                // Another user's process, or one that has gone: its
                // namespaces are not listed.
                Ok(()) | Err(EACCES | ENOENT) => {}
                Err(e) => return Err(e),
            }
            if i == ID_NET {
                p.netnsid = self.get_netnsid(pid, ino);
            }
        }
        self.processes.push(p);
        Ok(())
    }

    /// `read_processes(ls)`: every process `/proc` lists, in its order.
    fn read_processes(&mut self) -> Result<(), i32> {
        let dir = std::fs::read_dir("/proc").map_err(|e| errno_of(&e))?;
        // `xreaddir`: an entry that will not read ends the directory.
        for e in dir.map_while(Result::ok) {
            let Some(pid) = dirent_pid(&e) else {
                continue;
            };
            match self.read_process(pid) {
                Ok(()) | Err(EACCES | ENOENT) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// `get_namespace(ls, ino)`.
    fn get_namespace(&self, ino: u64) -> Option<usize> {
        self.order
            .iter()
            .copied()
            .find(|&i| self.namespaces.get(i).is_some_and(|n| n.id == ino))
    }

    /// `add_namespace(ls, type, ino, parent_ino, owner_ino)`.
    fn add_namespace(&mut self, ty: usize, ino: u64, parent: u64, owner: u64) -> usize {
        let i = self.namespaces.len();
        self.namespaces.push(Namespace {
            id: ino,
            ty,
            related_id: Rela { parent, owner },
            ..Namespace::default()
        });
        self.order.push(i);
        i
    }

    /// `add_process_to_namespace(ls, ns, proc)`.
    fn add_process_to_namespace(&mut self, ns: usize, proc_: usize) {
        let (pid, ppid) = match self.processes.get(proc_) {
            Some(p) => (p.pid, p.ppid),
            None => return,
        };
        for x in 0..self.processes.len() {
            let Some(xp) = self.processes.get(x) else {
                continue;
            };
            if xp.pid == ppid {
                if let Some(p) = self.processes.get_mut(proc_) {
                    p.parent = Some(x);
                }
            } else if xp.ppid == pid
                && let Some(xp) = self.processes.get_mut(x)
            {
                xp.parent = Some(proc_);
            }
        }
        let lowest = self
            .namespaces
            .get(ns)
            .and_then(|n| n.proc_)
            .and_then(|i| self.processes.get(i))
            .map(|p| p.pid);
        if let Some(n) = self.namespaces.get_mut(ns) {
            n.processes.push(proc_);
            n.nprocs = n.nprocs.saturating_add(1);
            if lowest.is_none_or(|l| l > pid) {
                n.proc_ = Some(proc_);
            }
        }
    }

    /// `add_namespace_for_nsfd(ls, fd, ino)`: a namespace known only by a
    /// descriptor, with its owner and parent, each found or added in turn.
    fn add_namespace_for_nsfd(&mut self, fd: &File, ino: u64) -> Option<usize> {
        let clone_type = match sys::ioctl_value(fd, sys::NS_GET_NSTYPE) {
            Ok(t) => t,
            Err(e) => {
                if e == sys::ENOTTY {
                    warnx(&self.short, "Unsupported ioctl NS_GET_NSTYPE");
                }
                return None;
            }
        };
        let ty = clone_type_to_lsns_type(clone_type)?;
        if !self.fltr_types.get(ty).copied().unwrap_or(false) {
            return None;
        }
        let fd_owner = self
            .ns_ioctl_fd(fd, sys::NS_GET_USERNS, "NS_GET_USERNS")
            .ok();
        let ino_owner = fd_owner
            .as_ref()
            .and_then(|f| f.metadata().ok())
            .map_or(0, |m| ino_of(&m));
        let fd_parent = self
            .ns_ioctl_fd(fd, sys::NS_GET_PARENT, "NS_GET_PARENT")
            .ok();
        let ino_parent = fd_parent
            .as_ref()
            .and_then(|f| f.metadata().ok())
            .map_or(0, |m| ino_of(&m));
        let ns = self.add_namespace(ty, ino, ino_parent, ino_owner);
        // Upstream ignores whether the owner's UID could be read.
        let uid = match sys::ioctl_owner_uid(fd) {
            Ok(u) => u,
            Err(e) => {
                if e == sys::ENOTTY {
                    warnx(&self.short, "Unsupported ioctl NS_GET_OWNER_UID");
                }
                0
            }
        };
        if let Some(n) = self.namespaces.get_mut(ns) {
            n.uid_fallback = uid;
        }
        self.add_uid(uid);
        if (ty == ID_USER || ty == ID_PID) && ino_parent != ino && ino_parent != 0 {
            let parent = match self.get_namespace(ino_parent) {
                Some(p) => Some(p),
                None => {
                    let added = fd_parent
                        .as_ref()
                        .and_then(|f| self.add_namespace_for_nsfd(f, ino_parent));
                    if let Some(n) = self.namespaces.get_mut(ns) {
                        n.related_ns.parent = added;
                        if ino_parent == ino_owner {
                            n.related_ns.owner = added;
                        }
                    }
                    added
                }
            };
            if let Some(n) = self.namespaces.get_mut(ns) {
                n.related_ns.parent = parent;
            }
        }
        let owner_known = self
            .namespaces
            .get(ns)
            .is_some_and(|n| n.related_ns.owner.is_some());
        if !owner_known && ino_owner != 0 {
            let owner = match self.get_namespace(ino_owner) {
                Some(o) => Some(o),
                None => fd_owner
                    .as_ref()
                    .and_then(|f| self.add_namespace_for_nsfd(f, ino_owner)),
            };
            if let Some(n) = self.namespaces.get_mut(ns) {
                n.related_ns.owner = owner;
            }
        }
        Some(ns)
    }

    /// `interpolate_missing_namespaces(ls, orphan, rela)`: the relation's
    /// namespace, found through the orphan's own process.
    fn interpolate_missing_namespaces(&mut self, orphan: usize, rela: Relation) {
        let Some(o) = self.namespaces.get(orphan) else {
            return;
        };
        let (related_id, ty, proc_) = (o.related_id.get(rela), o.ty, o.proc_);
        if let Some(found) = self.get_namespace(related_id) {
            if let Some(n) = self.namespaces.get_mut(orphan) {
                n.related_ns.set(rela, Some(found));
            }
            return;
        }
        // A namespace with no process has nowhere to be reached from.
        let Some(pid) = proc_.and_then(|p| self.processes.get(p)).map(|p| p.pid) else {
            return;
        };
        let Ok(fd_orphan) = File::open(format!(
            "/proc/{pid}/ns/{}",
            NS_NAMES.get(ty).copied().unwrap_or("")
        )) else {
            return;
        };
        let (request, name) = match rela {
            Relation::Parent => (sys::NS_GET_PARENT, "NS_GET_PARENT"),
            Relation::Owner => (sys::NS_GET_USERNS, "NS_GET_USERNS"),
        };
        let missing = self.ns_ioctl_fd(&fd_orphan, request, name);
        drop(fd_orphan);
        let Ok(fd_missing) = missing else {
            return;
        };
        if fd_missing.metadata().map(|m| ino_of(&m)).ok() != Some(related_id) {
            return;
        }
        let added = self.add_namespace_for_nsfd(&fd_missing, related_id);
        if let Some(n) = self.namespaces.get_mut(orphan) {
            n.related_ns.set(rela, added);
        }
    }

    /// `read_related_namespaces(ls)`: each namespace's parent and owner
    /// among those listed, and the missing ones interpolated.
    fn read_related_namespaces(&mut self) {
        // The orphans of each relation, as upstream chains them: a stack
        // threaded through the relation itself.
        let mut orphan: Rela<Option<usize>> = Rela::default();
        for &ns in &self.order.clone() {
            let Some(n) = self.namespaces.get(ns) else {
                continue;
            };
            let (ty, rel) = (n.ty, n.related_id);
            let mut found = n.related_ns;
            for &pns in &self.order {
                let Some(p) = self.namespaces.get(pns) else {
                    continue;
                };
                if ty == ID_USER || ty == ID_PID {
                    if rel.parent == p.id {
                        found.parent = Some(pns);
                    }
                    if rel.owner == p.id {
                        found.owner = Some(pns);
                    }
                    if found.parent.is_some() && found.owner.is_some() {
                        break;
                    }
                } else if rel.owner == p.id {
                    found.owner = Some(pns);
                    break;
                }
            }
            // lsns finds a namespace only through a process in it, so a
            // related one that is known by ID but not listed is missing,
            // and this one is its orphan.
            for rela in [Relation::Parent, Relation::Owner] {
                if rel.get(rela) != 0 && found.get(rela).is_none() {
                    found.set(rela, orphan.get(rela));
                    orphan.set(rela, Some(ns));
                }
            }
            if let Some(n) = self.namespaces.get_mut(ns) {
                n.related_ns = found;
            }
        }
        for rela in [Relation::Parent, Relation::Owner] {
            while let Some(current) = orphan.get(rela) {
                orphan.set(
                    rela,
                    self.namespaces
                        .get(current)
                        .and_then(|n| n.related_ns.get(rela)),
                );
                if let Some(n) = self.namespaces.get_mut(current) {
                    n.related_ns.set(rela, None);
                }
                self.interpolate_missing_namespaces(current, rela);
            }
        }
    }

    /// `read_persistent_namespaces(ls)`: namespaces bind-mounted on nsfs
    /// that no process is in.
    fn read_persistent_namespaces(&mut self) {
        let mounts: Vec<(Vec<u8>, Vec<u8>)> = self
            .tab
            .ents
            .iter()
            .filter(|fs| fs.match_fstype(Some(NSFS)))
            .filter_map(|fs| Some((fs.root.clone()?, fs.target.clone()?)))
            .collect();
        for (root, target) in mounts {
            let Some(ino) = nsfs_root_ino(&root) else {
                continue;
            };
            if self.get_namespace(ino).is_some() {
                continue;
            }
            let Ok(fd) = File::open(quoting::os_from_bytes(&target)) else {
                continue;
            };
            let _ = self.add_namespace_for_nsfd(&fd, ino);
        }
    }

    /// `read_namespaces(ls)`.
    fn read_namespaces(&mut self) {
        for proc_ in 0..self.processes.len() {
            for i in 0..NS_NAMES.len() {
                let Some(p) = self.processes.get(proc_) else {
                    continue;
                };
                let (id, pid_, oid) = (
                    p.ns_ids.get(i).copied().unwrap_or(0),
                    p.ns_pids.get(i).copied().unwrap_or(0),
                    p.ns_oids.get(i).copied().unwrap_or(0),
                );
                if id == 0 {
                    continue;
                }
                let ns = match self.get_namespace(id) {
                    Some(n) => n,
                    None => self.add_namespace(i, id, pid_, oid),
                };
                self.add_process_to_namespace(ns, proc_);
            }
        }
        self.read_persistent_namespaces();
        if matches!(self.tree, Tree::Owner | Tree::Parent) {
            self.read_related_namespaces();
        }
        // `list_sort(cmp_namespaces)`: by ID, stably.
        let ns = &self.namespaces;
        self.order.sort_by_key(|&i| ns.get(i).map_or(0, |n| n.id));
    }

    /// `nsfs_xasputs(&str, ns, tab, sep)`: the nsfs mount points of the
    /// namespace, each once.
    fn nsfs_text(&self, ns: &Namespace, sep: u8) -> Option<Vec<u8>> {
        let expected = format!("{}:[{}]", NS_NAMES.get(ns.ty).copied().unwrap_or(""), ns.id);
        let mut out: Option<Vec<u8>> = None;
        for fs in &self.tab.ents {
            if !fs.match_fstype(Some(NSFS)) || fs.root.as_deref() != Some(expected.as_bytes()) {
                continue;
            }
            let tgt = fs.target.clone().unwrap_or_default();
            match out.as_mut() {
                None => out = Some(tgt),
                Some(s) => {
                    if !is_path_included(s, &tgt, sep) {
                        s.push(sep);
                        s.extend_from_slice(&tgt);
                    }
                }
            }
        }
        out
    }

    /// `add_scols_line(ls, table, ns, proc)`.
    fn add_scols_line(
        &mut self,
        table: &mut Table,
        ids: &[ColumnId],
        ns: usize,
        proc_: Option<usize>,
    ) {
        let Some(n) = self.namespaces.get(ns).cloned() else {
            return;
        };
        let p = proc_.and_then(|i| self.processes.get(i)).cloned();
        let parent = match self.tree {
            Tree::Process => p
                .as_ref()
                .and_then(|p| p.parent)
                .and_then(|i| self.processes.get(i))
                .and_then(|pp| pp.outline),
            Tree::Parent => n
                .related_ns
                .parent
                .and_then(|i| self.namespaces.get(i))
                .and_then(|r| r.ns_outline),
            Tree::Owner => n
                .related_ns
                .owner
                .and_then(|i| self.namespaces.get(i))
                .and_then(|r| r.ns_outline),
            Tree::None => None,
        };
        let Ok(line) = table.new_line(parent) else {
            warn(
                &self.short,
                "failed to add line to output",
                &std::io::Error::from_raw_os_error(12),
            );
            return;
        };
        for (i, &col) in self.columns.clone().iter().enumerate() {
            let text: Option<Vec<u8>> = match col {
                Col::Ns => Some(n.id.to_string().into_bytes()),
                Col::Pid => p.as_ref().map(|p| p.pid.to_string().into_bytes()),
                Col::Ppid => p.as_ref().map(|p| p.ppid.to_string().into_bytes()),
                Col::Type => Some(
                    NS_NAMES
                        .get(n.ty)
                        .copied()
                        .unwrap_or("")
                        .as_bytes()
                        .to_vec(),
                ),
                Col::Nprocs => Some(n.nprocs.to_string().into_bytes()),
                Col::Command => p.as_ref().and_then(|p| {
                    strdup_procfs_file(p.pid, "cmdline")
                        .or_else(|| strdup_procfs_file(p.pid, "comm"))
                }),
                Col::Path => p.as_ref().map(|p| {
                    format!(
                        "/proc/{}/ns/{}",
                        p.pid,
                        NS_NAMES.get(n.ty).copied().unwrap_or("")
                    )
                    .into_bytes()
                }),
                Col::Uid => Some(
                    (p.as_ref().map_or(n.uid_fallback, |p| p.uid) as i32)
                        .to_string()
                        .into_bytes(),
                ),
                Col::User => Some(self.user_name(p.as_ref().map_or(n.uid_fallback, |p| p.uid))),
                Col::Netnsid => match &p {
                    Some(p) if n.ty == ID_NET => netnsid_text(p.netnsid),
                    _ => None,
                },
                Col::Nsfs => self.nsfs_text(&n, if self.no_wrap { b',' } else { b'\n' }),
                Col::Pns => Some(n.related_id.parent.to_string().into_bytes()),
                Col::Ons => Some(n.related_id.owner.to_string().into_bytes()),
            };
            if let (Some(t), Some(&id)) = (text, ids.get(i)) {
                // The line and column are this table's own.
                let _ = table.line_set_data(line, id, &t);
            }
        }
        if matches!(self.tree, Tree::Owner | Tree::Parent) {
            if let Some(n) = self.namespaces.get_mut(ns) {
                n.ns_outline = Some(line);
            }
        } else if let Some(p) = proc_.and_then(|i| self.processes.get_mut(i)) {
            p.outline = Some(line);
        }
    }

    /// `init_scols_table(ls)`.
    fn init_scols_table(&self) -> (Table, Vec<ColumnId>) {
        let mut tab = Table::new();
        tab.enable_raw(self.raw);
        tab.enable_json(self.json);
        tab.enable_noheadings(self.no_headings);
        if self.json {
            tab.set_name(b"namespaces");
        }
        let mut ids = Vec::with_capacity(self.columns.len());
        for &col in &self.columns {
            let ci = info(col);
            let mut flags = ci.flags;
            if self.no_trunc {
                flags &= !FL_TRUNC;
            }
            if self.tree == Tree::Process && col == Col::Command {
                flags |= FL_TREE;
            }
            if self.no_wrap {
                flags &= !FL_WRAP;
            }
            if matches!(self.tree, Tree::Owner | Tree::Parent) && col == Col::Ns {
                flags |= FL_TREE;
                flags &= !FL_RIGHT;
            }
            let cl = tab.new_column(ci.name.as_bytes(), ci.whint, flags);
            // The column is the table's own.
            if self.json {
                let _ = tab.column_set_json_type(cl, ci.json);
            }
            if !self.no_wrap && col == Col::Nsfs {
                let _ = tab.column_set_wrapnl(cl);
                let _ = tab.column_set_safechars(cl, b"\n");
            }
            ids.push(cl);
        }
        (tab, ids)
    }

    /// `show_namespace(ls, tab, ns, proc)`: the namespace, after the owner
    /// or parent it hangs from in a tree of those.
    fn show_namespace(
        &mut self,
        tab: &mut Table,
        ids: &[ColumnId],
        ns: usize,
        proc_: Option<usize>,
    ) {
        let Some(n) = self.namespaces.get(ns) else {
            return;
        };
        let (parent, owner) = (n.related_ns.parent, n.related_ns.owner);
        let unshown =
            |s: &Self, i: usize| s.namespaces.get(i).is_some_and(|x| x.ns_outline.is_none());
        let first = match self.tree {
            Tree::Owner => owner.filter(|&o| unshown(self, o)),
            Tree::Parent => match parent {
                Some(p) => Some(p).filter(|&p| unshown(self, p)),
                None => owner.filter(|&o| unshown(self, o)),
            },
            Tree::None | Tree::Process => None,
        };
        if let Some(r) = first {
            let rp = self.namespaces.get(r).and_then(|x| x.proc_);
            self.show_namespace(tab, ids, r, rp);
        }
        self.add_scols_line(tab, ids, ns, proc_);
    }

    /// `namespace_has_process(ns, pid)`.
    fn namespace_has_process(&self, ns: &Namespace, pid: i32) -> bool {
        ns.processes
            .iter()
            .any(|&p| self.processes.get(p).is_some_and(|p| p.pid == pid))
    }

    /// The table `show_namespaces` prints: each namespace the filters leave,
    /// in ID order, once.
    fn namespaces_table(&mut self) -> Table {
        let (mut tab, ids) = self.init_scols_table();
        for &ns in &self.order.clone() {
            let Some(n) = self.namespaces.get(ns) else {
                continue;
            };
            if self.fltr_pid != 0 && !self.namespace_has_process(n, self.fltr_pid) {
                continue;
            }
            if self.persist && n.nprocs != 0 {
                continue;
            }
            if n.ns_outline.is_none() {
                let proc_ = n.proc_;
                self.show_namespace(&mut tab, &ids, ns, proc_);
            }
        }
        tab
    }

    /// `show_namespaces(ls)`.
    fn show_namespaces(&mut self) -> Vec<u8> {
        let mut tab = self.namespaces_table();
        let mut text = Vec::new();
        // `scols_print_table`'s status is not looked at, as upstream does
        // not look at it.
        let _ = tab.print_into(&mut text);
        text
    }

    /// `show_process(ls, tab, proc, ns)`: the process, after its parent
    /// when that is in the same namespace and not shown yet.
    fn show_process(&mut self, tab: &mut Table, ids: &[ColumnId], proc_: usize, ns: usize) {
        let ty = self.namespaces.get(ns).map_or(0, |n| n.ty);
        if self.tree == Tree::Process
            && let Some(p) = self.processes.get(proc_)
            && let Some(parent) = p.parent
            && let Some(pp) = self.processes.get(parent)
            && pp.outline.is_none()
            && pp.ns_ids.get(ty) == p.ns_ids.get(ty)
        {
            self.show_process(tab, ids, parent, ns);
        }
        self.add_scols_line(tab, ids, ns, Some(proc_));
    }

    /// The table `show_namespace_processes` prints: the namespace's
    /// processes, in the order they were found.
    fn namespace_processes_table(&mut self, ns: usize) -> Table {
        let (mut tab, ids) = self.init_scols_table();
        let procs = self
            .namespaces
            .get(ns)
            .map(|n| n.processes.clone())
            .unwrap_or_default();
        for p in procs {
            if self.processes.get(p).is_some_and(|x| x.outline.is_none()) {
                self.show_process(&mut tab, &ids, p, ns);
            }
        }
        tab
    }

    /// `show_namespace_processes(ls, ns)`.
    fn show_namespace_processes(&mut self, ns: usize) -> Vec<u8> {
        let mut tab = self.namespace_processes_table(ns);
        let mut text = Vec::new();
        let _ = tab.print_into(&mut text);
        text
    }
}

/// `netnsid_xasputs(&str, netnsid)`.
fn netnsid_text(netnsid: i32) -> Option<Vec<u8>> {
    if netnsid >= 0 {
        Some(netnsid.to_string().into_bytes())
    } else if netnsid == NSID_NOT_ASSIGNED {
        Some(b"unassigned".to_vec())
    } else {
        None
    }
}

/// `clone_type_to_lsns_type(clone_type)`.
fn clone_type_to_lsns_type(clone_type: i32) -> Option<usize> {
    match clone_type {
        0x0002_0000 => Some(0),
        0x4000_0000 => Some(1),
        0x2000_0000 => Some(2),
        0x0400_0000 => Some(3),
        0x0800_0000 => Some(4),
        0x1000_0000 => Some(5),
        0x0200_0000 => Some(6),
        0x0000_0080 => Some(7),
        _ => None,
    }
}

/// The inode in an nsfs mount's root, `net:[4026532000]`: read by
/// `strtoumax` from after the first `[`, which must end at a `]`.
///
/// As `strtoumax`, a number past `u64` is refused (`ERANGE`), a sign is
/// allowed (a `-` negates modulo 2^64), and no digits at all is the number
/// 0 ending where it started -- so `net:[]` is inode 0.
fn nsfs_root_ino(root: &[u8]) -> Option<u64> {
    let open = root.iter().position(|&b| b == b'[')?;
    let digits = root.get(open.saturating_add(1)..).unwrap_or_default();
    let (ino, end) = match ulstrutils::scan_integer(digits, 10) {
        None => (0, 0),
        Some(sc) => {
            let m = u64::try_from(sc.magnitude).ok().filter(|_| !sc.saturated)?;
            (if sc.negative { m.wrapping_neg() } else { m }, sc.end)
        }
    };
    (digits.get(end) == Some(&b']')).then_some(ino)
}

/// `is_path_included(path_set, elt, sep)`: whether `elt` is one of the
/// `sep`-separated paths in `path_set`.
///
/// Upstream looks only where `strstr` first finds `elt`, so a path that
/// first occurs inside a longer one (`/a/b` in `/x/a/b,/a/b`) counts as not
/// included, and is listed again.
fn is_path_included(set: &[u8], elt: &[u8], sep: u8) -> bool {
    let found = if elt.is_empty() {
        Some(0)
    } else {
        set.windows(elt.len()).position(|w| w == elt)
    };
    let Some(at) = found else {
        return false;
    };
    // The byte after it; `None` is the string's terminating NUL.
    let after = set.get(at.saturating_add(elt.len())).copied();
    if at == 0 && (set.len() == elt.len() || after == Some(sep)) {
        return true;
    }
    // Upstream reads the byte before a match at the start too, which lies
    // outside the string; nothing there is a separator here.
    let before = at.checked_sub(1).and_then(|i| set.get(i)).copied();
    before == Some(sep) && (after == Some(sep) || after.is_none())
}

/// The option each parsed item stands for, as upstream's switch sees it.
fn option_code(opt: &Opt<'_>) -> Option<(i32, Option<OsString>)> {
    match opt {
        Opt::Short(c, value) => Some((i32::from(*c), value.clone())),
        Opt::Long(name, value) => {
            let i = LONGS.iter().position(|&(n, _)| n == *name)?;
            Some((*LONG_VALS.get(i)?, value.clone()))
        }
        Opt::Operand(_) => None,
    }
}

/// `option_to_longopt(c, opts)`.
fn option_to_longopt(c: i32) -> Option<&'static str> {
    LONG_VALS
        .iter()
        .position(|&v| v == c)
        .and_then(|i| LONGS.get(i))
        .map(|&(name, _)| name)
}

stdfdguard::guard_std_fds!();

fn main() -> ExitCode {
    stdfdguard::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let short = short_name(argv.first().map_or(OsStr::new("lsns"), OsString::as_os_str));
    let mut out = Stdout::new(1);
    let status = run(&argv, &short, &mut out);
    // `close_stdout`, which upstream registers with `atexit`.
    ExitCode::from(out.close(status, &short))
}

/// `main()`.
#[allow(
    clippy::too_many_lines,
    reason = "upstream's main, kept in one piece so it can be read against it"
)]
fn run(argv: &[OsString], short: &[u8], out: &mut Stdout) -> u8 {
    let arg0 = argv.first().map_or(OsStr::new("lsns"), OsString::as_os_str);
    let mut ls = Lsns {
        short: short.to_vec(),
        utf8: smartcols::tty::codeset_is_utf8(),
        ..Lsns::default()
    };
    let mut force_list = false;
    let mut outarg: Option<Vec<u8>> = None;
    let mut is_net = false;
    let mut operands: Vec<Vec<u8>> = Vec::new();
    let mut excl_st = [0i32; 3];
    let own = argv.get(1..).unwrap_or_default();
    for item in LSNS.parse(own, SHORTS, LONGS) {
        let opt = match item {
            Ok(opt) => opt,
            Err(e) => {
                // glibc names the program by argv[0] as given.
                stderr_write(format!("{}: {}\n", shown(&os_bytes(arg0)), e.sentence).as_bytes());
                return errtryhelp(short);
            }
        };
        let Some((c, value)) = option_code(&opt) else {
            if let Opt::Operand(o) = &opt {
                operands.push(os_bytes(o).into_owned());
            }
            continue;
        };
        if let Some(msg) =
            ulstrutils::err_exclusive_options(c, &EXCL, &mut excl_st, option_to_longopt, short)
        {
            stderr_write(msg.as_bytes());
            return 1;
        }
        let arg = value.as_deref().map(|v| os_bytes(v).into_owned());
        match c {
            OPT_OUTPUT_ALL => ls.columns = INFOS.iter().map(|i| i.id).collect(),
            _ => match u8::try_from(c).unwrap_or(0) {
                b'J' => ls.json = true,
                b'l' => force_list = true,
                b'o' => outarg = arg,
                b'P' => ls.persist = true,
                b'p' => {
                    let value = value.unwrap_or_default();
                    match ulstrutils::ul_strtos32(&os_bytes(&value), 10) {
                        Ok(p) => ls.fltr_pid = p,
                        Err(e) => {
                            warnx(short, &num_error_message("invalid PID argument", &value, e));
                            return 1;
                        }
                    }
                }
                b'n' => ls.no_headings = true,
                b'r' => {
                    ls.no_wrap = true;
                    ls.raw = true;
                }
                b'u' => ls.no_trunc = true,
                b't' => {
                    let a = arg.unwrap_or_default();
                    let Some(ty) = NS_NAMES.iter().position(|n| n.as_bytes() == a.as_slice())
                    else {
                        warnx(short, &format!("unknown namespace type: {}", shown(&a)));
                        return 1;
                    };
                    if let Some(f) = ls.fltr_types.get_mut(ty) {
                        *f = true;
                    }
                    ls.fltr_ntypes = ls.fltr_ntypes.saturating_add(1);
                    if ty == ID_NET {
                        is_net = true;
                    }
                }
                b'W' => ls.no_wrap = true,
                b'T' => {
                    ls.tree = Tree::Owner;
                    if let Some(a) = arg {
                        let a = a.strip_prefix(b"=").unwrap_or(&a);
                        match a {
                            b"parent" => ls.tree = Tree::Parent,
                            b"process" => ls.tree = Tree::Process,
                            b"owner" => {}
                            _ => {
                                warnx(short, &format!("unknown tree type: {}", shown(a)));
                                return 1;
                            }
                        }
                    }
                }
                b'h' => {
                    out.write(&usage(short));
                    return 0;
                }
                b'V' => {
                    let mut line = short.to_vec();
                    line.extend_from_slice(b" from util-linux 2.39.3\n");
                    out.write(&line);
                    return 0;
                }
                _ => return errtryhelp(short),
            },
        }
    }
    if ls.fltr_ntypes == 0 {
        ls.fltr_types = [true; 8];
    }
    if let Some(o) = operands.first() {
        if ls.fltr_pid != 0 {
            warnx(short, "--task is mutually exclusive with <namespace>");
            return 1;
        }
        let o_os = quoting::os_from_bytes(o);
        match ulstrutils::ul_strtou64(o, 10) {
            Ok(v) => ls.fltr_ns = v,
            Err(e) => {
                warnx(
                    short,
                    &num_error_message("invalid namespace argument", &o_os, e),
                );
                return 1;
            }
        }
        if ls.tree == Tree::None && !force_list {
            ls.tree = Tree::Process;
        }
        if ls.columns.is_empty() {
            ls.columns = vec![Col::Pid, Col::Ppid, Col::User, Col::Command];
        }
    }
    if ls.columns.is_empty() {
        ls.columns = vec![Col::Ns, Col::Type, Col::Nprocs, Col::Pid, Col::User];
        if is_net {
            ls.columns.extend([Col::Netnsid, Col::Nsfs]);
        }
        ls.columns.push(Col::Command);
        if ls.tree == Tree::None && !force_list {
            ls.tree = Tree::Process;
        }
    }
    if let Some(list) = &outarg {
        let parsed: Result<usize, IdListError> =
            string_add_to_idarray(list, &mut ls.columns, MAX_COLUMNS, |name, rest| {
                column_name_to_id(name, rest, short)
            });
        if parsed.is_err() {
            return 1;
        }
    }
    if ls.columns.contains(&Col::Netnsid) {
        ls.netlink = sys::Netlink::open();
    }
    // `mnt_new_table_from_file(_PATH_PROC_MOUNTINFO)`: a `stat`, then the
    // parse, whose errors on a line are all recoverable -- so only a file
    // that is not there, or will not open, fails it.
    let mi = ulmount::tab_parse::PATH_PROC_MOUNTINFO;
    let parsed = std::fs::metadata(quoting::os_from_bytes(mi))
        .map_err(|e| errno_of(&e))
        .and_then(|_| ulmount::tab_parse::parse_file(&mut ls.tab, mi, None));
    if let Err(errno) = parsed {
        let e = std::io::Error::from_raw_os_error(errno);
        warn(short, &format!("failed to parse {}", shown(mi)), &e);
        return MNT_EX_FAIL;
    }
    let mut rc = ls.read_processes();
    if rc.is_ok() {
        ls.read_namespaces();
    }
    if rc.is_ok() {
        if ls.fltr_ns != 0 {
            let Some(ns) = ls.get_namespace(ls.fltr_ns) else {
                warnx(short, &format!("not found namespace: {}", ls.fltr_ns));
                return 1;
            };
            out.write(&ls.show_namespace_processes(ns));
        } else {
            out.write(&ls.show_namespaces());
        }
        rc = Ok(());
    }
    match rc {
        Ok(()) => 0,
        Err(sys::ENOTTY) => EXIT_UNSUPPORTED_IOCTL,
        Err(_) => 1,
    }
}

#[cfg(test)]
mod tests;
