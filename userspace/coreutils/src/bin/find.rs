#![cfg_attr(not(unix), allow(dead_code))]

//! find — search for files in a directory hierarchy.
//!
//! A byte-path port of GNU findutils 4.9's `find`, measured against the real
//! thing rather than against the manual page.
//!
//! ## Why 4.9 and not 4.10
//!
//! Because 4.9 is what `scripts/find-diff.sh` compares against. That harness
//! runs inside WSL — as `du`'s does, and for the same reason: `find`'s answers
//! come from `st_dev`, `st_ino`, `st_nlink` and real symlinks, none of which
//! Windows has — and WSL's findutils is 4.9.0.
//!
//! The trap worth naming, because the two `find`s are one `wsl.exe` apart: the
//! build on the *host*'s `PATH` is MSYS2's **4.10.0**, so a `find --version`
//! typed into the wrong shell answers for a version this port is not aimed at.
//! The differences are not cosmetic. 4.10 rewrote the `-user`/`-group`
//! diagnostics into one sentence each, made a non-numeric `-links` argument a
//! fatal of its own, added an `isnan` check where 4.9 fails an assertion and
//! dumps core, and fixed `pred_xtype`'s broken-link fallback. All four are
//! reachable from a command line, and each would look like a bug here.
//!
//! ## Why this is a port and not a rewrite
//!
//! `find` is the one utility in this tree with no `getopt_long` at all:
//! findutils parses argv by hand through `parse_table[]`, and the *shape* of
//! that hand parse is the user interface. `find . -name f -o -name g -print`
//! prints one file, not two, because `-o` binds looser than the `-a` that
//! `find` silently interposes between adjacent predicates, and because the
//! default `-print` is suppressed by the presence of an explicit one. Neither
//! rule is expressible as "parse the flags, then walk"; both fall out of
//! building an expression tree the way `find/tree.c` builds one. So the parser
//! here is `get_expr`/`scan_rest` transliterated, with GNU's own diagnostics
//! attached to the same branches, because a `find` that accepts a different
//! set of command lines than `find` is not `find`.
//!
//! The previous implementation understood four options — `-name`, `-type`,
//! `-maxdepth`, `-print` — with no expression tree, no operators, and no
//! actions, and matched `-name` with a hand-rolled globber that worked on
//! `char`s (so it could not be handed a name that is not UTF-8), recursed once
//! per possible `*` split (so `find . -name '*x*x*x*x*x*y'` was a hang
//! reachable from a command line), and treated `[a-z]` as the three-character
//! set `a`, `-`, `z`.
//!
//! ## The rules a reader will not guess
//!
//! * **Every "global option" is still a predicate.** `-maxdepth`, `-depth`,
//!   `-follow`, `-xdev`, `-regextype` and friends insert a `---noop` node that
//!   always evaluates true. This is not decoration: `build_expression_tree`
//!   decides whether to append the default `-print` by asking whether the
//!   predicate list is *empty*, so `find . -maxdepth 1` prints and
//!   `find . -maxdepth 1 -print` does not print twice. `-daystart` is the one
//!   global option that inserts nothing.
//! * **`-prune` and `-quit` do not suppress the default `-print`**, though
//!   every other action does. `find . -prune` prints `.`; `find . -quit`
//!   prints nothing, because `-quit` exits before reaching the appended
//!   `-print`.
//! * **Primaries carry precedence `NO_PREC`**, not `MAX_PREC`, which looks as
//!   though it would stop `scan_rest`'s loop dead. It never does, because the
//!   implicit `-a` is interposed *before* a primary is ever appended after
//!   another one.
//! * **Traversal order is `readdir` order.** GNU opens fts with no compare
//!   function, so the output is whatever the directory hands back. Sorting
//!   would be a nicer `find` and a different one.
//! * **`-printf`'s numeric directives render through `%s`.** GNU pushes
//!   `%i %s %n %b %k %D %U %G` through `human_readable` and prints the
//!   resulting *string*, so `%.2s` on a size truncates it rather than setting
//!   a minimum digit count. Only `%d` is a `%d`, only `%m` is a `%o`, and only
//!   `%S` is a `%g`.
//! * **Diagnostics quote with `quote`, not `quoteaf`.** `find/util.c` sets
//!   `err_quoting_style = locale_quoting_style`, and the differential harness
//!   runs under `LC_ALL=C.UTF-8`, where that renders the curly `‘…’` pair.
//!
//! ## Deliberate omissions
//!
//! `-context` (SELinux) is parsed and rejected rather than silently accepted:
//! there is no SELinux on this system, and a predicate that always answers
//! false would be a wrong answer rather than a refusal. `%Z` renders empty for
//! the same reason. Tracked in `known-issues.md`.

use coreutils::errmsg::{self, strerror};
#[cfg(unix)]
use coreutils::quote::os_bytes;
use coreutils::quote::{self, os_from_bytes, quote};
use coreutils::{cfmt, extfloat, fnmatch, pathname};
#[cfg(unix)]
use std::collections::HashMap;
use std::io::{self, Write};
use std::process::ExitCode;

// ---------------------------------------------------------------------------
// Primitive values
// ---------------------------------------------------------------------------

/// A timestamp with nanosecond resolution, as `stat` reports one.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
struct Ts {
    sec: i64,
    nsec: u32,
}

/// The fields of `struct stat` this program actually reads.
///
/// A struct rather than `std::fs::Metadata` because the host's `Metadata` does
/// not expose `st_rdev` or nanosecond `ctime` portably, and because the fake
/// tree the unit tests run against has to be able to produce one.
#[derive(Clone, Copy, Debug)]
struct Meta {
    dev: u64,
    ino: u64,
    mode: u32,
    nlink: u64,
    uid: u32,
    gid: u32,
    size: u64,
    blocks: u64,
    rdev: u64,
    atime: Ts,
    mtime: Ts,
    ctime: Ts,
}

impl Meta {
    fn is_dir(&self) -> bool {
        self.mode & modechange::S_IFMT == modechange::S_IFDIR
    }

    fn is_symlink(&self) -> bool {
        self.mode & modechange::S_IFMT == modechange::S_IFLNK
    }

    fn is_reg(&self) -> bool {
        self.mode & modechange::S_IFMT == modechange::S_IFREG
    }
}

/// `find`'s own file-type letter, as `-type` and `%y` spell it.
///
/// Note this is *not* `modechange::file_type_letter`: `find` writes a door as
/// `D` and an unknown type as `U`, where `ls` writes `?`.
fn type_letter(mode: u32) -> u8 {
    match mode & modechange::S_IFMT {
        modechange::S_IFREG => b'f',
        modechange::S_IFDIR => b'd',
        modechange::S_IFLNK => b'l',
        modechange::S_IFSOCK => b's',
        modechange::S_IFBLK => b'b',
        modechange::S_IFCHR => b'c',
        modechange::S_IFIFO => b'p',
        0o150_000 => b'D', // S_IFDOOR, Solaris; GNU tests for it explicitly
        _ => b'U',
    }
}

// ---------------------------------------------------------------------------
// The filesystem, behind a trait
// ---------------------------------------------------------------------------

/// Everything `find` asks of the world outside itself.
///
/// Behind a trait for the reason `du`'s is: the parser and the expression
/// evaluator are the parts most likely to be subtly wrong, and they can only
/// be unit-tested on a Windows host against a filesystem that does not exist
/// there. `RealTree` is the `#[cfg(unix)]` implementation; `FakeTree` in the
/// test module is a tree of literals.
trait Tree {
    fn lstat(&self, path: &[u8]) -> io::Result<Meta>;
    fn stat(&self, path: &[u8]) -> io::Result<Meta>;
    /// One entry per name, paired with the `S_IFMT` bits `readdir` reported in
    /// `d_type` — or 0 where it reported `DT_UNKNOWN`.
    ///
    /// The pairing is not an optimisation. `fts` is opened `FTS_NOSTAT`, so
    /// upstream never calls `stat` on an entry that no predicate asked about,
    /// and that is *observable*: in a directory that is readable but not
    /// searchable, `find d` prints every name while `find d -printf '%s\n'`
    /// reports `Permission denied` for each of them. A walk that statted
    /// eagerly would fail the first command as well as the second.
    fn read_dir(&self, path: &[u8]) -> io::Result<Vec<(Vec<u8>, u32)>>;
    fn readlink(&self, path: &[u8]) -> io::Result<Vec<u8>>;
    /// `euidaccess(path, mode)`, where mode is 4/2/1 for read/write/execute.
    fn access(&self, path: &[u8], mode: i32) -> bool;
    fn remove_file(&self, path: &[u8]) -> io::Result<()>;
    fn remove_dir(&self, path: &[u8]) -> io::Result<()>;
    /// The filesystem type of the device a file sits on, for `-fstype`/`%F`.
    fn fstype(&self, dev: u64) -> Vec<u8>;
    fn user_name(&self, uid: u32) -> Option<Vec<u8>>;
    fn group_name(&self, gid: u32) -> Option<Vec<u8>>;
    fn uid_by_name(&self, name: &[u8]) -> Option<u32>;
    fn gid_by_name(&self, name: &[u8]) -> Option<u32>;
    /// Run a command, optionally with a working directory (`-execdir`).
    /// Returns whether it exited successfully.
    fn run(&self, argv: &[Vec<u8>], cwd: Option<&[u8]>) -> io::Result<bool>;
    /// Wall-clock now, read once at startup.
    fn now(&self) -> Ts;
    /// `$PATH`, for `-execdir`'s safety check.
    ///
    /// On the trait rather than read from the environment where it is used,
    /// because it is a question about the world outside the program — and
    /// because a test whose answer depended on the `$PATH` the developer
    /// happened to have would pass or fail by accident.
    fn path_env(&self) -> Option<Vec<u8>>;
    /// One line from the terminal, for `-ok`/`-okdir`.
    fn confirm(&self) -> bool;
}

#[cfg(not(unix))]
fn main() -> ExitCode {
    eprintln!("find: unix-only utility; not supported on this platform");
    ExitCode::from(1)
}

#[cfg(unix)]
unsafe extern "C" {
    fn euidaccess(path: *const u8, mode: i32) -> i32;
}

#[cfg(unix)]
struct RealTree {
    /// `st_dev` → filesystem type, read once from `/proc/self/mountinfo`.
    mounts: HashMap<u64, Vec<u8>>,
    users: pwdb::Db,
}

#[cfg(unix)]
impl RealTree {
    fn new() -> Self {
        Self {
            mounts: read_mountinfo(),
            users: pwdb::Db::load(),
        }
    }
}

/// Decode a glibc `dev_t` into the `major:minor` pair `/proc` prints.
///
/// glibc's encoding is not the naive `major<<8 | minor`: the high bits of both
/// numbers live above bit 32, so a device on a filesystem with many minors
/// decodes wrongly under the naive rule and `-fstype` then answers for the
/// wrong mount.
fn dev_major_minor(dev: u64) -> (u64, u64) {
    let major = ((dev >> 8) & 0xfff) | ((dev >> 32) & !0xfff);
    let minor = (dev & 0xff) | ((dev >> 12) & !0xff);
    (major, minor)
}

#[cfg(unix)]
fn read_mountinfo() -> HashMap<u64, Vec<u8>> {
    // mountinfo line: id parent major:minor root mount opts... - fstype src ...
    let mut map = HashMap::new();
    let Ok(text) = std::fs::read("/proc/self/mountinfo") else {
        return map;
    };
    for line in text.split(|&b| b == b'\n') {
        let fields: Vec<&[u8]> = line.split(|&b| b == b' ').collect();
        let Some(devstr) = fields.get(2) else {
            continue;
        };
        let Some(sep) = fields.iter().position(|f| *f == b"-") else {
            continue;
        };
        let Some(fstype) = fields.get(sep.saturating_add(1)) else {
            continue;
        };
        let mut halves = devstr.splitn(2, |&b| b == b':');
        let (Some(maj), Some(min)) = (halves.next(), halves.next()) else {
            continue;
        };
        let (Some(maj), Some(min)) = (parse_u64(maj), parse_u64(min)) else {
            continue;
        };
        // Re-encode with glibc's rule so the lookup key matches `st_dev`.
        let dev =
            ((maj & 0xfff) << 8) | ((maj & !0xfff) << 32) | (min & 0xff) | ((min & !0xff) << 12);
        map.entry(dev).or_insert_with(|| (*fstype).to_vec());
    }
    map
}

fn parse_u64(bytes: &[u8]) -> Option<u64> {
    if bytes.is_empty() {
        return None;
    }
    let mut n: u64 = 0;
    for &b in bytes {
        let d = u64::from(b.checked_sub(b'0')?);
        if d > 9 {
            return None;
        }
        n = n.checked_mul(10)?.checked_add(d)?;
    }
    Some(n)
}

/// The `S_IFMT` bits of a `DirEntry`'s type, which is `d_type` widened back
/// out into the shape `state.type` carries it in.
#[cfg(unix)]
fn file_type_bits(t: &std::fs::FileType) -> u32 {
    use std::os::unix::fs::FileTypeExt as _;
    if t.is_file() {
        modechange::S_IFREG
    } else if t.is_dir() {
        modechange::S_IFDIR
    } else if t.is_symlink() {
        modechange::S_IFLNK
    } else if t.is_socket() {
        modechange::S_IFSOCK
    } else if t.is_fifo() {
        modechange::S_IFIFO
    } else if t.is_block_device() {
        modechange::S_IFBLK
    } else if t.is_char_device() {
        modechange::S_IFCHR
    } else {
        0
    }
}

#[cfg(unix)]
fn meta_of(m: &std::fs::Metadata) -> Meta {
    use std::os::unix::fs::MetadataExt;
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let ts = |s: i64, n: i64| Ts {
        sec: s,
        nsec: (n as u64 & 0xffff_ffff) as u32,
    };
    Meta {
        dev: m.dev(),
        ino: m.ino(),
        mode: m.mode(),
        nlink: m.nlink(),
        uid: m.uid(),
        gid: m.gid(),
        size: m.size(),
        blocks: m.blocks(),
        rdev: m.rdev(),
        atime: ts(m.atime(), m.atime_nsec()),
        mtime: ts(m.mtime(), m.mtime_nsec()),
        ctime: ts(m.ctime(), m.ctime_nsec()),
    }
}

#[cfg(unix)]
impl Tree for RealTree {
    fn lstat(&self, path: &[u8]) -> io::Result<Meta> {
        Ok(meta_of(&std::fs::symlink_metadata(os_from_bytes(path))?))
    }

    fn stat(&self, path: &[u8]) -> io::Result<Meta> {
        Ok(meta_of(&std::fs::metadata(os_from_bytes(path))?))
    }

    fn read_dir(&self, path: &[u8]) -> io::Result<Vec<(Vec<u8>, u32)>> {
        let mut names = Vec::new();
        for entry in std::fs::read_dir(os_from_bytes(path))? {
            let entry = entry?;
            // `file_type()` on a `DirEntry` is the `d_type` field when the
            // filesystem filled it in, and a `lstat` when it did not — which
            // is `DT_UNKNOWN`'s only correct handling and is what `fts` does
            // too. The `S_IFMT` bits are all we keep; the permission bits are
            // not knowable without the `stat` we are avoiding.
            let mode = match entry.file_type() {
                Ok(t) => file_type_bits(&t),
                Err(_) => 0,
            };
            names.push((os_bytes(&entry.file_name()).into_owned(), mode));
        }
        Ok(names)
    }

    fn readlink(&self, path: &[u8]) -> io::Result<Vec<u8>> {
        let target = std::fs::read_link(os_from_bytes(path))?;
        Ok(os_bytes(target.as_os_str()).into_owned())
    }

    fn access(&self, path: &[u8], mode: i32) -> bool {
        let mut c_path = path.to_vec();
        if c_path.contains(&0) {
            return false;
        }
        c_path.push(0);
        // SAFETY: `c_path` is NUL-terminated, has no interior NUL, and outlives
        // the call. `euidaccess` reads it and does not retain it.
        unsafe { euidaccess(c_path.as_ptr(), mode) == 0 }
    }

    fn remove_file(&self, path: &[u8]) -> io::Result<()> {
        std::fs::remove_file(os_from_bytes(path))
    }

    fn remove_dir(&self, path: &[u8]) -> io::Result<()> {
        std::fs::remove_dir(os_from_bytes(path))
    }

    fn fstype(&self, dev: u64) -> Vec<u8> {
        self.mounts
            .get(&dev)
            .cloned()
            .unwrap_or_else(|| b"unknown".to_vec())
    }

    fn user_name(&self, uid: u32) -> Option<Vec<u8>> {
        self.users.user_by_uid(uid).map(|u| u.name.clone())
    }

    fn group_name(&self, gid: u32) -> Option<Vec<u8>> {
        self.users.group_by_gid(gid).map(|g| g.name.clone())
    }

    fn uid_by_name(&self, name: &[u8]) -> Option<u32> {
        self.users.user_by_name(name).map(|u| u.uid)
    }

    fn gid_by_name(&self, name: &[u8]) -> Option<u32> {
        self.users.group_by_name(name).map(|g| g.gid)
    }

    fn run(&self, argv: &[Vec<u8>], cwd: Option<&[u8]>) -> io::Result<bool> {
        let Some(prog) = argv.first() else {
            return Ok(false);
        };
        let mut cmd = std::process::Command::new(os_from_bytes(prog));
        for a in argv.iter().skip(1) {
            cmd.arg(os_from_bytes(a));
        }
        if let Some(dir) = cwd {
            cmd.current_dir(os_from_bytes(dir));
        }
        Ok(cmd.status()?.success())
    }

    fn now(&self) -> Ts {
        let d = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        Ts {
            sec: i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
            nsec: d.subsec_nanos(),
        }
    }

    fn path_env(&self) -> Option<Vec<u8>> {
        std::env::var_os("PATH").map(|p| os_bytes(&p).into_owned())
    }

    fn confirm(&self) -> bool {
        // GNU reads one line and accepts it if it matches the locale's yesexpr;
        // under C.UTF-8 that is `^[yY]`.
        let mut line = String::new();
        if io::stdin().read_line(&mut line).is_err() {
            return false;
        }
        matches!(line.as_bytes().first(), Some(b'y' | b'Y'))
    }
}

// ---------------------------------------------------------------------------
// Comparisons and predicate payloads
// ---------------------------------------------------------------------------

/// `+n` / `-n` / `n`, as every numeric predicate spells it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Comp {
    Gt,
    Lt,
    Eq,
}

#[derive(Clone, Copy, Debug)]
struct NumCmp {
    cmp: Comp,
    n: u64,
}

impl NumCmp {
    fn test(self, v: u64) -> bool {
        match self.cmp {
            Comp::Gt => v > self.n,
            Comp::Lt => v < self.n,
            Comp::Eq => v == self.n,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TimeField {
    Access,
    Modify,
    Change,
    Birth,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PermKind {
    /// `-perm mode` — every bit, and no other bit.
    Exact,
    /// `-perm -mode` — at least these bits.
    AtLeast,
    /// `-perm /mode` — any of these bits.
    Any,
}

/// One primary. Punctuation is not here; it lives in [`PKind`].
enum Prim {
    True,
    False,
    /// `---noop`: what every "global option" leaves behind so that the
    /// predicate list is non-empty and the default `-print` is not appended
    /// twice.
    Noop,
    Name {
        pat: Vec<u8>,
        ci: bool,
    },
    Path {
        pat: Vec<u8>,
        ci: bool,
    },
    LName {
        pat: Vec<u8>,
        ci: bool,
    },
    Regex(Box<ere::Regex>),
    Type(Vec<u8>),
    XType(Vec<u8>),
    Size {
        cmp: Comp,
        n: u64,
        unit: u64,
    },
    Perm {
        kind: PermKind,
        file_mode: u32,
        dir_mode: u32,
    },
    Empty,
    Links(NumCmp),
    Inum(NumCmp),
    Uid(NumCmp),
    Gid(NumCmp),
    User(u32),
    Group(u32),
    NoUser,
    NoGroup,
    SameFile {
        dev: u64,
        ino: u64,
    },
    /// `-atime`/`-mtime`/`-ctime`/`-amin`/`-mmin`/`-cmin`/`-used`.
    TimeWindow {
        field: Option<TimeField>,
        cmp: Comp,
        origin: Ts,
        window: f64,
    },
    /// `-newer`/`-anewer`/`-cnewer`/`-newerXY`: strictly newer than a fixed
    /// instant. Always `COMP_GT` upstream, so there is no comparison to carry.
    Newer {
        field: TimeField,
        ts: Ts,
    },
    FsType(Vec<u8>),
    /// `-readable`/`-writable`/`-executable`, as the `access(2)` bit.
    Access(i32),
    Print {
        sink: usize,
        terminator: u8,
    },
    Printf {
        sink: usize,
        segs: Vec<Seg>,
    },
    Ls {
        sink: usize,
    },
    Delete,
    Prune,
    Quit,
    Exec(usize),
}

/// A node of the flat predicate list, before it becomes a tree.
struct Node {
    kind: PKind,
    /// The token exactly as the user wrote it. Several of GNU's diagnostics
    /// quote it back, so it cannot be reconstructed from `kind`.
    name: Vec<u8>,
    /// True for the `(` and `)` and `-print` `find` inserts itself. The
    /// distinction is load-bearing: `get_expr` words the same syntax error
    /// differently depending on whether the paren it tripped over is one the
    /// user typed.
    artificial: bool,
    prim: Option<Prim>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PKind {
    Primary,
    Not,
    And,
    Or,
    Comma,
    Open,
    Close,
}

impl PKind {
    /// Upstream's `NO_PREC`=0, `COMMA_PREC`=1, `OR_PREC`=2, `AND_PREC`=3,
    /// `NEGATE_PREC`=4.
    ///
    /// A primary really is `NO_PREC`, which looks as though `scan_rest`'s
    /// `p_prec > prev_prec` loop could never advance past one. It always can,
    /// because an implicit `-a` (`AND_PREC`) is interposed before a primary is
    /// ever appended after another primary or a `)`.
    fn prec(self) -> u8 {
        match self {
            Self::Primary | Self::Open | Self::Close => 0,
            Self::Comma => 1,
            Self::Or => 2,
            Self::And => 3,
            Self::Not => 4,
        }
    }
}

/// The expression, after `get_expr` has shaped the list into a tree.
enum Expr {
    Prim(usize),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Comma(Box<Expr>, Box<Expr>),
}

/// A fatal diagnostic: one or more lines, each printed as `find: <line>`.
///
/// Several of GNU's failures print two lines — the "paths must precede
/// expression" case adds a second guessing at an unquoted glob — so this is a
/// list rather than a string.
struct Fatal(Vec<String>);

impl Fatal {
    fn new(line: impl Into<String>) -> Self {
        Self(vec![line.into()])
    }
}

type Parsed<T> = Result<T, Fatal>;

// ---------------------------------------------------------------------------
// Output sinks
// ---------------------------------------------------------------------------

/// Where an action writes.
///
/// Deduplicated by name at parse time — in `Parser::sink_names`, which is why
/// the name is not carried here. GNU opens one `FILE *` per distinct filename,
/// so `-fprint x -fprint x` interleaves through a single buffer; opening it
/// twice here would produce two buffers writing over each other at different
/// offsets.
enum Sink {
    Stdout,
    Stderr,
    File(std::fs::File),
    /// Test-only stand-in for [`Sink::Stdout`]: [`Ctx::flush`] leaves the bytes
    /// in the buffer instead of writing them, so `run_capture` can read them
    /// afterwards.
    ///
    /// A sink rather than a wrapper around the whole of stdout because the
    /// tests must exercise the *same* `run` the program does — a test harness
    /// that assembled its own `Ctx` would stop testing the argument handling,
    /// which is the half most likely to be wrong.
    #[cfg(test)]
    Capture,
}

/// `-exec` and its three relatives.
struct ExecSpec {
    /// The command, with `{}` still embedded for the `;` form.
    argv: Vec<Vec<u8>>,
    /// `+` form: accumulate names and run once per batch.
    multiple: bool,
    /// `-ok`/`-okdir`: ask before running.
    confirm: bool,
    /// `-execdir`/`-okdir`: run in the containing directory, with `./name`.
    dir_relative: bool,
    /// Names collected so far, for the `+` form.
    pending: Vec<Vec<u8>>,
    /// The directory the pending batch belongs to. `-execdir ... +` runs its
    /// batch when the walk leaves the directory, not at the end, because the
    /// names in it are relative to that directory and mean nothing outside it.
    pending_dir: Option<Vec<u8>>,
}

// ---------------------------------------------------------------------------
// Where the tokens come from
// ---------------------------------------------------------------------------

/// Upstream `looks_like_expression`.
///
/// This is what separates the start points from the expression, and it is
/// deliberately not "starts with a dash": `find - -name f` searches a file
/// literally called `-`, and `find . ) -name f` treats `)` as a *start point*
/// only while still in the leading run.
fn looks_like_expression(arg: &[u8], leading: bool) -> bool {
    match arg.first() {
        Some(b'-') => arg.len() > 1,
        Some(b')' | b',') => {
            if arg.len() > 1 {
                false
            } else {
                !leading
            }
        }
        Some(b'!' | b'(') => arg.len() == 1,
        _ => false,
    }
}

/// How symbolic links are treated: `-P` (never follow), `-L` (always),
/// `-H` (only for the start points).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Follow {
    Never,
    Always,
    CommandLine,
}

/// Upstream `process_leading_options`: the `-H`/`-L`/`-P`/`-D`/`-O` prefix,
/// which is the only part of `find`'s command line that is option-shaped in
/// the ordinary sense.
///
/// Returns the index of the first argument that is not a leading option.
fn process_leading_options(argv: &[Vec<u8>], follow: &mut Follow) -> Result<usize, Leading> {
    let mut i = 0usize;
    while i < argv.len() {
        let Some(arg) = argv.get(i) else { break };
        match arg.as_slice() {
            b"-H" => *follow = Follow::CommandLine,
            b"-L" => *follow = Follow::Always,
            b"-P" => *follow = Follow::Never,
            b"--" => {
                i = i.saturating_add(1);
                break;
            }
            b"-D" => {
                let Some(spec) = argv.get(i.saturating_add(1)) else {
                    return Err(Leading::Usage(
                        "Missing argument after the -D option.".to_string(),
                    ));
                };
                process_debug_options(spec)?;
                i = i.saturating_add(1);
            }
            a if a.starts_with(b"-O") => {
                process_optimisation_option(a.get(2..).unwrap_or_default())?;
            }
            _ => break,
        }
        i = i.saturating_add(1);
    }
    Ok(i)
}

/// The three ways `process_leading_options` can end the program, none of which
/// is a plain [`Fatal`]: two print the terse `Try 'find --help'` pointer that
/// `usage(EXIT_FAILURE)` adds and one succeeds.
enum Leading {
    /// `usage (EXIT_FAILURE)` — a `find: …` line, then the pointer, exit 1.
    Usage(String),
    /// `die (EXIT_FAILURE, …)` — a `find: …` line and nothing else, exit 1.
    Die(String),
    /// `-D help` — the debug-flag table on stdout, exit 0.
    DebugHelp,
}

/// The `-D` flag names, in the order `show_valid_debug_options` prints them.
const DEBUG_FLAGS: &[(&str, &str)] = &[
    (
        "exec",
        "Show diagnostic information relating to -exec, -execdir, -ok and -okdir",
    ),
    (
        "opt",
        "Show diagnostic information relating to optimisation",
    ),
    ("rates", "Indicate how often each predicate succeeded"),
    ("search", "Navigate the directory tree verbosely"),
    ("stat", "Trace calls to stat(2) and lstat(2)"),
    (
        "time",
        "Show diagnostic information relating to time-of-day and timestamp comparisons",
    ),
    ("tree", "Display the expression tree"),
    ("all", "Set all of the debug flags (but help)"),
    ("help", "Explain the various -D options"),
];

/// `process_debug_options`.
///
/// Every flag but `help` is accepted and then ignored: this port has no
/// debugging output to switch on, and refusing a flag `find` accepts would
/// break a command line that merely asked for tracing it did not read.
fn process_debug_options(spec: &[u8]) -> Result<(), Leading> {
    let mut empty = true;
    let mut help = false;
    for token in spec.split(|&b| b == b',').filter(|t| !t.is_empty()) {
        empty = false;
        help |= token == b"help";
        if !DEBUG_FLAGS.iter().any(|(n, _)| n.as_bytes() == token) {
            // Upstream quotes `arg`, not the token it failed on — and by this
            // point `strtok_r` has written a NUL over the delimiter that ended
            // the *first* token, so what gets quoted is that first token
            // however late in the list the offender is. `-D exec,bogus`
            // really does say `Ignoring unrecognised debug flag 'exec'`.
            let head = strtok_first(spec);
            eprintln!("find: Ignoring unrecognised debug flag {}", quote(head));
        }
    }
    if empty {
        return Err(Leading::Usage(
            "Empty argument to the -D option.".to_string(),
        ));
    }
    if help {
        return Err(Leading::DebugHelp);
    }
    Ok(())
}

/// What `arg` reads as after `strtok_r` has extracted its first token: the
/// leading delimiters are left alone, and the delimiter that *ends* the first
/// token has been overwritten with a NUL.
fn strtok_first(arg: &[u8]) -> &[u8] {
    let start = arg.iter().position(|&b| b != b',').unwrap_or(arg.len());
    match arg
        .get(start..)
        .and_then(|t| t.iter().position(|&b| b == b','))
    {
        Some(off) => arg.get(..start.saturating_add(off)).unwrap_or(arg),
        None => arg,
    }
}

/// `process_optimisation_option`. The level is parsed, validated and then
/// discarded — this port does not reorder predicates, so there is nothing for
/// it to select between. Validating it anyway is not ceremony: four of the
/// five refusals above are the only observable behaviour `-O` has, and a
/// `find` that accepted `-Ox` would differ from `find` on a command line
/// someone can type.
fn process_optimisation_option(arg: &[u8]) -> Result<(), Leading> {
    if arg.is_empty() {
        return Err(Leading::Die(
            "The -O option must be immediately followed by a decimal integer".to_string(),
        ));
    }
    if !arg.first().is_some_and(u8::is_ascii_digit) {
        return Err(Leading::Die(
            "Please specify a decimal number immediately after -O".to_string(),
        ));
    }
    let run = digit_run(arg);
    let digits = arg.get(..run).unwrap_or_default();
    let level = xstrtoumax(digits);
    if run != arg.len() {
        return Err(Leading::Die(format!(
            "Invalid optimisation level {}",
            String::from_utf8_lossy(arg)
        )));
    }
    // `strtoul` overflow, then the `USHRT_MAX` ceiling. Both refuse; only the
    // wording differs, and only the second one names the level.
    let Some(level) = level.filter(|l| *l <= u64::from(u16::MAX)) else {
        return Err(Leading::Die(match level {
            Some(l) => format!(
                "Optimisation level {l} is too high.  If you want to find files very quickly, \
                 consider using GNU locate."
            ),
            None => format!(
                "Invalid optimisation level {}: Numerical result out of range",
                String::from_utf8_lossy(arg)
            ),
        }));
    };
    let _ = level;
    Ok(())
}

// ---------------------------------------------------------------------------
// The predicate table
// ---------------------------------------------------------------------------

/// Every name `find_parser` will match, in upstream's declaration order.
///
/// The names carry no leading `-` because upstream strips exactly one before
/// comparing — which is why `--help` and `-help` both work, and why `!`, `(`,
/// `)` and `,` are in the same table as the tests.
const TABLE: &[&[u8]] = &[
    b"!",
    b"not",
    b"(",
    b")",
    b",",
    b"a",
    b"amin",
    b"and",
    b"anewer",
    b"atime",
    b"cmin",
    b"cnewer",
    b"ctime",
    b"context",
    b"daystart",
    b"delete",
    b"d",
    b"depth",
    b"empty",
    b"exec",
    b"executable",
    b"execdir",
    b"files0-from",
    b"fls",
    b"follow",
    b"fprint",
    b"fprint0",
    b"fprintf",
    b"fstype",
    b"gid",
    b"group",
    b"ignore_readdir_race",
    b"ilname",
    b"iname",
    b"inum",
    b"ipath",
    b"iregex",
    b"iwholename",
    b"links",
    b"lname",
    b"ls",
    b"maxdepth",
    b"mindepth",
    b"mmin",
    b"mount",
    b"mtime",
    b"name",
    b"newer",
    b"noleaf",
    b"nogroup",
    b"nouser",
    b"noignore_readdir_race",
    b"nowarn",
    b"warn",
    b"o",
    b"or",
    b"ok",
    b"okdir",
    b"path",
    b"perm",
    b"print",
    b"print0",
    b"printf",
    b"prune",
    b"quit",
    b"readable",
    b"regex",
    b"regextype",
    b"samefile",
    b"size",
    b"type",
    b"uid",
    b"used",
    b"user",
    b"wholename",
    b"writable",
    b"xdev",
    b"xtype",
    b"false",
    b"true",
    b"--noop",
    b"help",
    b"-help",
    b"version",
    b"-version",
];

/// Upstream `find_parser`: the token as typed → the table name, or `None` for
/// "unknown predicate".
fn find_parser(tok: &[u8]) -> Option<&'static [u8]> {
    // `-newerXY`: matched by shape rather than by name, before the dash strip.
    if tok.len() == 8 && tok.starts_with(b"-newer") {
        return Some(b"newerXY");
    }
    let stripped = if tok.first() == Some(&b'-') {
        tok.get(1..).unwrap_or(b"")
    } else {
        tok
    };
    TABLE.iter().copied().find(|n| *n == stripped)
}

// ---------------------------------------------------------------------------
// The parser
// ---------------------------------------------------------------------------

struct Parser<'a> {
    argv: &'a [Vec<u8>],
    /// Index of the next unconsumed token.
    i: usize,
    nodes: Vec<Node>,
    sinks: Vec<Sink>,
    sink_names: Vec<Vec<u8>>,
    execs: Vec<ExecSpec>,
    tree: &'a dyn Tree,

    // Global state the "options" set.
    follow: Follow,
    max_depth: Option<usize>,
    min_depth: usize,
    depth_first: bool,
    xdev: bool,
    ignore_readdir_race: bool,
    extended_regex: bool,
    files0_from: Option<Vec<u8>>,

    // Time origins, fixed at startup.
    now: Ts,
    cur_day_start: Ts,

    /// Set by any action that produces output, which is what suppresses the
    /// default `-print`. Deliberately *not* set by `-prune` or `-quit`.
    no_default_print: bool,
    /// Diagnostics that are warnings rather than failures, emitted in order.
    warnings: Vec<String>,
    /// `options.warnings`, whose default is `isatty(0)` and which `-warn` and
    /// `-nowarn` move. Three of the parser's warnings are gated on it and the
    /// rest are not, so it cannot simply suppress the whole list.
    warn: bool,
    /// `options.posixly_correct`: `POSIXLY_CORRECT` was in the environment.
    /// Silences the same three warnings and halves `-ls`'s block size.
    posixly_correct: bool,
    /// `first_nonoption_arg`: the first token that was neither a global option
    /// nor one of the four positional ones, remembered so that a global option
    /// appearing *after* it can be warned about.
    first_nonoption: Option<Vec<u8>>,
}

/// The `ARG_*` column of upstream's `parse_table`, to the extent
/// `found_parser` cares about it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ArgClass {
    /// `ARG_OPTION`: a global option, whose position on the command line does
    /// *not* limit what it affects.
    Option,
    /// `ARG_POSITIONAL_OPTION`: `-daystart`, `-follow`, `-warn`, `-nowarn`,
    /// `-regextype`. Exempt from the warning below because their position
    /// genuinely does matter — they affect what follows and not what precedes.
    Positional,
    /// Everything else: tests, actions and punctuation.
    Other,
}

/// `parse_table`'s `ARG_*` for one canonical name.
fn arg_class(canon: &[u8]) -> ArgClass {
    match canon {
        b"d"
        | b"depth"
        | b"files0-from"
        | b"ignore_readdir_race"
        | b"maxdepth"
        | b"mindepth"
        | b"mount"
        | b"noleaf"
        | b"noignore_readdir_race"
        | b"xdev" => ArgClass::Option,
        b"daystart" | b"follow" | b"nowarn" | b"warn" | b"regextype" => ArgClass::Positional,
        _ => ArgClass::Other,
    }
}

/// Seconds in a day, as `find` counts them.
const DAYSECS: f64 = 86400.0;

impl<'a> Parser<'a> {
    fn new(argv: &'a [Vec<u8>], tree: &'a dyn Tree, follow: Follow) -> Self {
        let now = tree.now();
        // `options.cur_day_start = start_time - DAYSECS`, before `-daystart`
        // has a chance to move it to local midnight.
        let cur_day_start = Ts {
            sec: now.sec.saturating_sub(86400),
            nsec: now.nsec,
        };
        Self {
            argv,
            i: 0,
            nodes: Vec::new(),
            sinks: vec![Sink::Stdout, Sink::Stderr],
            sink_names: vec![b"/dev/stdout".to_vec(), b"/dev/stderr".to_vec()],
            execs: Vec::new(),
            tree,
            follow,
            max_depth: None,
            min_depth: 0,
            depth_first: false,
            xdev: false,
            ignore_readdir_race: false,
            extended_regex: false,
            files0_from: None,
            now,
            cur_day_start,
            no_default_print: false,
            warnings: Vec::new(),
            // `options.warnings = isatty(0)`. Not `isatty(1)`: the question
            // upstream is asking is "is a person typing this", and a person
            // typing it has a terminal on standard *input* whether or not the
            // output is going to a pipe.
            warn: std::io::IsTerminal::is_terminal(&io::stdin()),
            posixly_correct: std::env::var_os("POSIXLY_CORRECT").is_some(),
            first_nonoption: None,
        }
    }

    /// `should_issue_warnings`.
    fn should_warn(&self) -> bool {
        !self.posixly_correct && self.warn
    }

    /// `found_parser`'s half: the warning for a global option written after a
    /// test, which reads as though it were conditional on that test and is not.
    fn found_parser(&mut self, tok: &[u8], canon: &[u8]) {
        match arg_class(canon) {
            ArgClass::Positional => {}
            ArgClass::Option => {
                if let Some(first) = self.first_nonoption.clone()
                    && self.should_warn()
                {
                    self.warnings.push(format!(
                        "warning: you have specified the global option {} after the argument {}, \
                         but global options are not positional, i.e., {} affects tests specified \
                         before it as well as those specified after it.  Please specify global \
                         options before other arguments.",
                        String::from_utf8_lossy(tok),
                        String::from_utf8_lossy(&first),
                        String::from_utf8_lossy(tok)
                    ));
                }
            }
            ArgClass::Other => {
                if self.first_nonoption.is_none() {
                    self.first_nonoption = Some(tok.to_vec());
                }
            }
        }
    }

    /// Upstream `collect_arg`: the next token, or the "missing argument"
    /// failure the driver would otherwise have produced.
    fn arg(&mut self, name: &[u8]) -> Parsed<Vec<u8>> {
        match self.argv.get(self.i) {
            Some(a) => {
                self.i = self.i.saturating_add(1);
                Ok(a.clone())
            }
            None => Err(Fatal::new(format!(
                "missing argument to `{}'",
                String::from_utf8_lossy(name)
            ))),
        }
    }

    fn bad_arg(name: &[u8], arg: &[u8]) -> Fatal {
        Fatal::new(format!(
            "invalid argument `{}' to `{}'",
            String::from_utf8_lossy(arg),
            String::from_utf8_lossy(name)
        ))
    }

    /// Upstream `get_new_pred_chk_op`: interpose the implicit `-a` when one
    /// operand would otherwise sit directly against another.
    ///
    /// Called for primaries, `!` and `(` — and *not* for `)`, `-a`, `-o`, `,`,
    /// which is what makes `find . -name f -o -name g -print` print one file.
    fn push_checked(&mut self, node: Node) {
        if matches!(node.kind, PKind::Primary | PKind::Not | PKind::Open)
            && matches!(
                self.nodes.last().map(|n| n.kind),
                Some(PKind::Primary | PKind::Close)
            )
        {
            self.nodes.push(Node {
                kind: PKind::And,
                name: b"-a".to_vec(),
                artificial: true,
                prim: None,
            });
        }
        self.nodes.push(node);
    }

    fn push_prim(&mut self, name: &[u8], prim: Prim) {
        self.push_checked(Node {
            kind: PKind::Primary,
            name: name.to_vec(),
            artificial: false,
            prim: Some(prim),
        });
    }

    /// A named output stream, opened at most once per distinct name.
    fn sink(&mut self, path: &[u8]) -> Parsed<usize> {
        if let Some(idx) = self.sink_names.iter().position(|n| n == path) {
            return Ok(idx);
        }
        let file = open_sink(path)
            .map_err(|e| Fatal::new(format!("{}: {}", quote(path), strerror(&e))))?;
        self.sinks.push(Sink::File(file));
        self.sink_names.push(path.to_vec());
        Ok(self.sinks.len().saturating_sub(1))
    }
}

#[cfg(unix)]
fn open_sink(path: &[u8]) -> io::Result<std::fs::File> {
    std::fs::File::create(os_from_bytes(path))
}

#[cfg(not(unix))]
fn open_sink(_path: &[u8]) -> io::Result<std::fs::File> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

// ---------------------------------------------------------------------------
// Argument grammars shared by several primaries
// ---------------------------------------------------------------------------

/// Upstream `get_comp_type`: strip a leading `+`/`-` and say what it meant.
fn get_comp_type(s: &[u8]) -> (Comp, &[u8]) {
    match s.first() {
        Some(b'+') => (Comp::Gt, s.get(1..).unwrap_or(b"")),
        Some(b'-') => (Comp::Lt, s.get(1..).unwrap_or(b"")),
        _ => (Comp::Eq, s),
    }
}

/// gnulib `xstrtoumax(str, &pend, 10, &n, "")`, reduced to the one answer
/// every caller here wants: `Some` exactly when the return is `LONGINT_OK`.
///
/// Three parts of that grammar are not the obvious ones. `strtoumax` skips
/// leading whitespace and accepts a leading `+`, so ` +5` is five. A leading
/// `-` is rejected outright rather than negated — gnulib special-cases it for
/// unsigned types precisely so that `-links --5` is an error instead of a
/// number near `UINTMAX_MAX`. And an empty `valid_suffixes` means *no* suffix
/// is valid rather than "suffixes are not checked", so any trailing byte
/// yields `LONGINT_INVALID_SUFFIX_CHAR`; overflow yields `LONGINT_OVERFLOW`.
/// All three are non-`OK`, so all three are `None`.
fn xstrtoumax(s: &[u8]) -> Option<u64> {
    let body = s
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .map_or(&[][..], |k| s.get(k..).unwrap_or(&[]));
    if body.first() == Some(&b'-') {
        return None;
    }
    let body = body.strip_prefix(b"+").unwrap_or(body);
    if body.is_empty() || !body.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let mut n: u64 = 0;
    for &b in body {
        n = n
            .checked_mul(10)?
            .checked_add(u64::from(b.wrapping_sub(b'0')))?;
    }
    Some(n)
}

/// `strspn(s, "0123456789")`: how much of `s` is a leading run of digits.
fn digit_run(s: &[u8]) -> usize {
    s.iter().take_while(|b| b.is_ascii_digit()).count()
}

/// `lib/safe-atoi.c`: `strtol` with four separate refusals, all fatal.
///
/// Worth its own function for one reason that is easy to miss: the range it
/// enforces is `int`'s, not `long`'s. `-user 3000000000` is a perfectly good
/// uid on Linux and a perfectly good `long`, and `find` still refuses it with
/// `Numerical result out of range`, because the value goes through this and
/// this returns an `int`.
fn safe_atoi(s: &[u8]) -> Parsed<i32> {
    let erange = || {
        Fatal::new(format!(
            "{}: {}",
            String::from_utf8_lossy(s),
            errmsg::strerror(&std::io::Error::from_raw_os_error(34))
        ))
    };
    let text = String::from_utf8_lossy(s).into_owned();
    let body = text.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let digits_at = body.strip_prefix(['+', '-']).map_or(0, |_| 1);
    let end = body
        .get(digits_at..)
        .unwrap_or("")
        .find(|c: char| !c.is_ascii_digit())
        .map_or(body.len(), |k| digits_at.saturating_add(k));
    let (num, rest) = (body.get(..end).unwrap_or(""), body.get(end..).unwrap_or(""));
    if num.is_empty() || num == "+" || num == "-" {
        // `end == s`: strtol consumed nothing.
        return Err(Fatal::new(format!("Expected an integer: {}", quote(s))));
    }
    let Ok(lval) = num.parse::<i64>() else {
        return Err(erange());
    };
    if i32::try_from(lval).is_err() {
        return Err(erange());
    }
    if !rest.is_empty() {
        return Err(Fatal::new(format!(
            "Unexpected suffix {} on {}",
            quote(rest.as_bytes()),
            quote(s)
        )));
    }
    i32::try_from(lval).map_err(|_| erange())
}

/// Upstream `get_num`: `+n` / `-n` / `n`.
fn get_num(s: &[u8]) -> Option<NumCmp> {
    let (cmp, rest) = get_comp_type(s);
    xstrtoumax(rest).map(|n| NumCmp { cmp, n })
}

/// gnulib `xstrtod` with a NULL end pointer: the whole string must be a
/// number.
fn strtod_full(s: &[u8]) -> Option<f64> {
    let text = std::str::from_utf8(s).ok()?;
    text.parse::<f64>().ok()
}

/// Upstream `get_relative_timestamp`.
///
/// The sense of the comparison is *inverted* on the way through: `-mtime +1`
/// means "modified more than a day ago", which as a comparison against a fixed
/// instant is "older than", i.e. `<`.
///
/// Everything past the parse is done in saturating arithmetic, which is not an
/// improvement on upstream but a reproduction of it. `-mtime +1e15` is 8.64e19
/// seconds, `origin - seconds` is computed in `double` and then converted to a
/// `time_t` that cannot hold it; on x86-64 that conversion yields `INT64_MIN`,
/// which is what `i64::MIN` gives here too. Upstream's own overflow check
/// (`(origin < result) != (seconds < 0)`) does not fire on that path — its
/// comment admits it "may be unreliable" — so no diagnostic is printed and the
/// predicate simply answers against a saturated instant. `-mtime -1e15`
/// therefore matches everything and `+1e15` nothing, which is measured
/// behaviour on glibc and is what this reproduces.
///
/// The one place we deliberately part company is NaN. `find . -mtime nan`
/// reaches `assert (nanosec < nanosec_per_sec)` in 4.9 and dumps core; we
/// refuse the argument instead. See `known-issues.md`.
fn get_relative_timestamp(s: &[u8], origin: Ts, sec_per_unit: f64) -> Option<(Comp, Ts)> {
    let (raw, rest) = get_comp_type(s);
    let cmp = match raw {
        Comp::Lt => Comp::Gt,
        Comp::Gt => Comp::Lt,
        Comp::Eq => Comp::Eq,
    };
    let offset = strtod_full(rest)?;
    if offset.is_nan() {
        return None;
    }
    let total = offset * sec_per_unit;
    let seconds = total.trunc();
    // `modf` of an infinity is a zero fraction, which `total - seconds` is not.
    let nanosec = if total.is_finite() {
        (total - seconds) * 1.0e9
    } else {
        0.0
    };
    // `as` on a float is saturating in Rust and UB in C; on x86-64 the UB
    // resolves to the same two values, which is why this matches.
    #[allow(clippy::cast_possible_truncation)]
    let secs = seconds as i64;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let ns = nanosec as i64;
    let mut sec = origin.sec.saturating_sub(secs);
    let mut nsec = i64::from(origin.nsec).saturating_sub(ns);
    if nsec < 0 {
        nsec = nsec.saturating_add(1_000_000_000);
        sec = sec.saturating_sub(1);
    }
    Some((
        cmp,
        Ts {
            sec,
            nsec: u32::try_from(nsec).unwrap_or(0),
        },
    ))
}

/// Upstream `insert_type`: the comma-separated letter list `-type`/`-xtype`
/// take, with its four distinct refusals.
fn parse_type_letters(name: &[u8], arg: &[u8]) -> Parsed<Vec<u8>> {
    let pname = String::from_utf8_lossy(name).into_owned();
    if arg.is_empty() {
        return Err(Fatal::new(format!(
            "Arguments to {pname} should contain at least one letter"
        )));
    }
    let mut letters: Vec<u8> = Vec::new();
    let mut idx = 0usize;
    loop {
        let Some(&c) = arg.get(idx) else {
            return Err(Fatal::new(format!(
                "Last file type in list argument to {pname} is missing, i.e., list is ending on: ','"
            )));
        };
        if !matches!(c, b'b' | b'c' | b'd' | b'f' | b'l' | b'p' | b's' | b'D') {
            return Err(Fatal::new(format!(
                "Unknown argument to {pname}: {}",
                c as char
            )));
        }
        if letters.contains(&c) {
            return Err(Fatal::new(format!(
                "Duplicate file type '{}' in the argument list to {pname}.",
                c as char
            )));
        }
        letters.push(c);
        idx = idx.saturating_add(1);
        match arg.get(idx) {
            None => break,
            Some(b',') => idx = idx.saturating_add(1),
            Some(_) => {
                return Err(Fatal::new(format!(
                    "Must separate multiple arguments to {pname} using: ','"
                )));
            }
        }
    }
    Ok(letters)
}

/// Upstream `lib/regextype.c`'s table, reduced to the one bit that changes
/// what a pattern means here: basic or extended.
///
/// The Emacs dialects are an approximation — `ere` has no Emacs syntax, so
/// they are treated as POSIX basic, which agrees on everything except Emacs's
/// own escapes (`\\|`, `\\(`…`\\)` are the same, but `\\w`, `\\b` and the
/// symbol classes are not). Documented in `known-issues.md`.
fn regex_is_extended(name: &[u8]) -> Option<bool> {
    match name {
        b"findutils-default"
        | b"ed"
        | b"emacs"
        | b"grep"
        | b"posix-basic"
        | b"posix-minimal-basic"
        | b"sed" => Some(false),
        b"gnu-awk" | b"posix-awk" | b"awk" | b"posix-egrep" | b"egrep" | b"posix-extended" => {
            Some(true)
        }
        _ => None,
    }
}

const REGEX_TYPES: &[&[u8]] = &[
    b"findutils-default",
    b"ed",
    b"emacs",
    b"gnu-awk",
    b"grep",
    b"posix-awk",
    b"awk",
    b"posix-basic",
    b"posix-egrep",
    b"egrep",
    b"posix-extended",
    b"posix-minimal-basic",
    b"sed",
];

fn compile_regex(pattern: &[u8], extended: bool, ci: bool) -> Parsed<ere::Regex> {
    let result = if extended {
        ere::Regex::new_flags(pattern, ci)
    } else {
        ere::bre::compile(pattern, ci)
    };
    result.map_err(|e| {
        Fatal::new(format!(
            "failed to compile regular expression '{}': {}",
            String::from_utf8_lossy(pattern),
            String::from_utf8_lossy(&e.0)
        ))
    })
}

// ---------------------------------------------------------------------------
// One token
// ---------------------------------------------------------------------------

/// `-help` and `-version` stop the parse dead and exit 0.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Halt {
    Help,
    Version,
}

impl Parser<'_> {
    /// Parse one expression token, appending whatever nodes it implies.
    ///
    /// `tok` is the token as typed; `canon` is the table name `find_parser`
    /// matched it to. Both are needed: the diagnostics quote the former and
    /// the dispatch switches on the latter.
    #[allow(clippy::too_many_lines)]
    fn parse_token(&mut self, tok: &[u8], canon: &'static [u8]) -> Parsed<Option<Halt>> {
        match canon {
            // ---- punctuation -------------------------------------------
            b"!" | b"not" => self.push_checked(Node {
                kind: PKind::Not,
                name: tok.to_vec(),
                artificial: false,
                prim: None,
            }),
            b"(" => self.push_checked(Node {
                kind: PKind::Open,
                name: tok.to_vec(),
                artificial: false,
                prim: None,
            }),
            b")" => self.nodes.push(Node {
                kind: PKind::Close,
                name: tok.to_vec(),
                artificial: false,
                prim: None,
            }),
            b"a" | b"and" => self.nodes.push(Node {
                kind: PKind::And,
                name: tok.to_vec(),
                artificial: false,
                prim: None,
            }),
            b"o" | b"or" => self.nodes.push(Node {
                kind: PKind::Or,
                name: tok.to_vec(),
                artificial: false,
                prim: None,
            }),
            b"," => self.nodes.push(Node {
                kind: PKind::Comma,
                name: tok.to_vec(),
                artificial: false,
                prim: None,
            }),

            // ---- name-ish tests ----------------------------------------
            b"name" | b"iname" => {
                let pat = self.arg(tok)?;
                self.check_name_arg(tok, &pat);
                self.push_prim(
                    tok,
                    Prim::Name {
                        pat,
                        ci: canon == b"iname",
                    },
                );
            }
            b"path" | b"ipath" | b"wholename" | b"iwholename" => {
                let pat = self.arg(tok)?;
                let ci = matches!(canon, b"ipath" | b"iwholename");
                self.push_prim(tok, Prim::Path { pat, ci });
            }
            b"lname" | b"ilname" => {
                let pat = self.arg(tok)?;
                self.push_prim(
                    tok,
                    Prim::LName {
                        pat,
                        ci: canon == b"ilname",
                    },
                );
            }
            b"regex" | b"iregex" => {
                let pat = self.arg(tok)?;
                let re = compile_regex(&pat, self.extended_regex, canon == b"iregex")?;
                self.push_prim(tok, Prim::Regex(Box::new(re)));
            }
            b"regextype" => {
                let name = self.arg(tok)?;
                let Some(extended) = regex_is_extended(&name) else {
                    let names: Vec<String> = REGEX_TYPES.iter().map(|n| quote(n)).collect();
                    return Err(Fatal::new(format!(
                        "Unknown regular expression type {}; valid types are {}.",
                        quote(&name),
                        names.join(", ")
                    )));
                };
                self.extended_regex = extended;
                self.push_prim(tok, Prim::Noop);
            }

            // ---- type ---------------------------------------------------
            b"type" => {
                let arg = self.arg(tok)?;
                let letters = parse_type_letters(tok, &arg)?;
                self.push_prim(tok, Prim::Type(letters));
            }
            b"xtype" => {
                let arg = self.arg(tok)?;
                let letters = parse_type_letters(tok, &arg)?;
                self.push_prim(tok, Prim::XType(letters));
            }

            // ---- size and mode ------------------------------------------
            b"size" => {
                let arg = self.arg(tok)?;
                let prim = parse_size(&arg)?;
                self.push_prim(tok, prim);
            }
            b"perm" => {
                let arg = self.arg(tok)?;
                let prim = self.parse_perm(&arg)?;
                self.push_prim(tok, prim);
            }

            // ---- numeric tests -------------------------------------------
            b"links" | b"inum" | b"uid" | b"gid" => {
                let arg = self.arg(tok)?;
                let num = get_num(&arg).ok_or_else(|| Self::bad_arg(tok, &arg))?;
                let prim = match canon {
                    b"links" => Prim::Links(num),
                    b"inum" => Prim::Inum(num),
                    b"uid" => Prim::Uid(num),
                    _ => Prim::Gid(num),
                };
                self.push_prim(tok, prim);
            }

            // ---- ownership ------------------------------------------------
            // The numeric fallback is `strspn(arg, "0123456789")` rather than a
            // number parser, which is why `-user +5` is *not* uid 5 but "not
            // the name of a known user": the `+` is not a digit, the run has
            // length zero, and the name branch is taken. `-user` and `-group`
            // then disagree about what a partial run means — `-user 5x` says
            // only "not the name of a known user", `-group 5x` explains the
            // suffix — because they are two hand-written copies upstream.
            b"user" => {
                let arg = self.arg(tok)?;
                let uid = match self.tree.uid_by_name(&arg) {
                    Some(u) => u,
                    None => {
                        let run = digit_run(&arg);
                        if run > 0 && run == arg.len() {
                            #[allow(clippy::cast_sign_loss)]
                            {
                                safe_atoi(&arg)? as u32
                            }
                        } else if arg.is_empty() {
                            return Err(Fatal::new("The argument to -user should not be empty"));
                        } else {
                            return Err(Fatal::new(format!(
                                "{} is not the name of a known user",
                                quote(&arg)
                            )));
                        }
                    }
                };
                self.push_prim(tok, Prim::User(uid));
            }
            b"group" => {
                let arg = self.arg(tok)?;
                let gid = match self.tree.gid_by_name(&arg) {
                    Some(g) => g,
                    None => {
                        let run = digit_run(&arg);
                        if run == 0 {
                            if arg.is_empty() {
                                return Err(Fatal::new(
                                    "argument to -group is empty, but should be a group name",
                                ));
                            }
                            return Err(Fatal::new(format!(
                                "{} is not the name of an existing group",
                                quote(&arg)
                            )));
                        }
                        if run != arg.len() {
                            return Err(Fatal::new(format!(
                                "{} is not the name of an existing group and it does not look \
                                 like a numeric group ID because it has the unexpected suffix {}",
                                quote(&arg),
                                quote(arg.get(run..).unwrap_or(b""))
                            )));
                        }
                        #[allow(clippy::cast_sign_loss)]
                        {
                            safe_atoi(&arg)? as u32
                        }
                    }
                };
                self.push_prim(tok, Prim::Group(gid));
            }
            b"nouser" => self.push_prim(tok, Prim::NoUser),
            b"nogroup" => self.push_prim(tok, Prim::NoGroup),

            // ---- whole-file tests -----------------------------------------
            b"empty" => self.push_prim(tok, Prim::Empty),
            b"true" => self.push_prim(tok, Prim::True),
            b"false" => self.push_prim(tok, Prim::False),
            b"fstype" => {
                let arg = self.arg(tok)?;
                self.push_prim(tok, Prim::FsType(arg));
            }
            b"readable" => self.push_prim(tok, Prim::Access(4)),
            b"writable" => self.push_prim(tok, Prim::Access(2)),
            b"executable" => self.push_prim(tok, Prim::Access(1)),
            b"samefile" => {
                let arg = self.arg(tok)?;
                let meta = self.stat_arg(&arg)?;
                self.push_prim(
                    tok,
                    Prim::SameFile {
                        dev: meta.dev,
                        ino: meta.ino,
                    },
                );
            }
            b"context" => {
                return Err(Fatal::new(
                    "invalid predicate -context: SELinux is not enabled.",
                ));
            }

            // ---- times -----------------------------------------------------
            b"atime" | b"mtime" | b"ctime" => {
                let arg = self.arg(tok)?;
                let field = match canon {
                    b"atime" => TimeField::Access,
                    b"mtime" => TimeField::Modify,
                    _ => TimeField::Change,
                };
                let prim = self.parse_time(tok, &arg, field)?;
                self.push_prim(tok, prim);
            }
            b"amin" | b"mmin" | b"cmin" => {
                let arg = self.arg(tok)?;
                let field = match canon {
                    b"amin" => TimeField::Access,
                    b"mmin" => TimeField::Modify,
                    _ => TimeField::Change,
                };
                // `-Xmin`'s origin is the start of the run, not midnight.
                let origin = Ts {
                    sec: self.cur_day_start.sec.saturating_add(86400),
                    nsec: self.cur_day_start.nsec,
                };
                let (cmp, ts) = get_relative_timestamp(&arg, origin, 60.0)
                    .ok_or_else(|| Self::bad_arg(tok, &arg))?;
                self.push_prim(
                    tok,
                    Prim::TimeWindow {
                        field: Some(field),
                        cmp,
                        origin: ts,
                        window: 60.0,
                    },
                );
            }
            b"used" => {
                let arg = self.arg(tok)?;
                let (cmp, ts) =
                    get_relative_timestamp(&arg, Ts::default(), DAYSECS).ok_or_else(|| {
                        Fatal::new(format!("Invalid argument {} to -used", quote(&arg)))
                    })?;
                self.push_prim(
                    tok,
                    Prim::TimeWindow {
                        field: None,
                        cmp,
                        origin: ts,
                        window: DAYSECS,
                    },
                );
            }
            b"newer" | b"anewer" | b"cnewer" => {
                let arg = self.arg(tok)?;
                let meta = self.stat_arg(&arg)?;
                let field = match canon {
                    b"anewer" => TimeField::Access,
                    b"cnewer" => TimeField::Change,
                    _ => TimeField::Modify,
                };
                self.push_prim(
                    tok,
                    Prim::Newer {
                        field,
                        ts: meta.mtime,
                    },
                );
            }
            b"newerXY" => {
                let prim = self.parse_newer_xy(tok)?;
                self.push_prim(tok, prim);
            }
            b"daystart" => {
                // The one global option that leaves no predicate behind.
                self.apply_daystart();
            }

            // ---- global options (each leaves a ---noop behind) -------------
            b"maxdepth" | b"mindepth" => {
                let arg = self.arg(tok)?;
                // `insert_depthspec` screens with `strspn` and only then calls
                // `safe_atoi`, so the two refusals are reachable in that order:
                // `-maxdepth 1x` is "Expected a positive decimal integer", but
                // `-maxdepth 99999999999` — which passes the screen — is
                // `safe_atoi`'s `Numerical result out of range`.
                let run = digit_run(&arg);
                let depth = if run > 0 && run == arg.len() {
                    let limit = safe_atoi(&arg)?;
                    usize::try_from(limit).ok()
                } else {
                    None
                };
                let depth = depth.ok_or_else(|| {
                    Fatal::new(format!(
                        "Expected a positive decimal integer argument to {}, but got {}",
                        String::from_utf8_lossy(tok),
                        quote(&arg)
                    ))
                })?;
                if canon == b"maxdepth" {
                    self.max_depth = Some(depth);
                } else {
                    self.min_depth = depth;
                }
                self.push_prim(tok, Prim::Noop);
            }
            b"depth" | b"d" => {
                if canon == b"d" && self.should_warn() {
                    self.warnings.push(
                        "warning: the -d option is deprecated; please use -depth instead, \
                         because the latter is a POSIX-compliant feature."
                            .to_string(),
                    );
                }
                self.depth_first = true;
                self.push_prim(tok, Prim::Noop);
            }
            b"follow" => {
                self.follow = Follow::Always;
                self.push_prim(tok, Prim::Noop);
            }
            b"xdev" | b"mount" => {
                self.xdev = true;
                self.push_prim(tok, Prim::Noop);
            }
            b"ignore_readdir_race" => {
                self.ignore_readdir_race = true;
                self.push_prim(tok, Prim::Noop);
            }
            b"noignore_readdir_race" => {
                self.ignore_readdir_race = false;
                self.push_prim(tok, Prim::Noop);
            }
            b"noleaf" => self.push_prim(tok, Prim::Noop),
            b"nowarn" | b"warn" => {
                self.warn = canon == b"warn";
                self.push_prim(tok, Prim::Noop);
            }
            b"files0-from" => {
                let arg = self.arg(tok)?;
                self.files0_from = Some(arg);
                self.push_prim(tok, Prim::Noop);
            }

            // ---- actions ---------------------------------------------------
            b"print" => {
                self.no_default_print = true;
                self.push_prim(
                    tok,
                    Prim::Print {
                        sink: 0,
                        terminator: b'\n',
                    },
                );
            }
            b"print0" => {
                self.no_default_print = true;
                self.push_prim(
                    tok,
                    Prim::Print {
                        sink: 0,
                        terminator: 0,
                    },
                );
            }
            b"fprint" | b"fprint0" => {
                let path = self.arg(tok)?;
                let sink = self.sink(&path)?;
                self.no_default_print = true;
                self.push_prim(
                    tok,
                    Prim::Print {
                        sink,
                        terminator: if canon == b"fprint0" { 0 } else { b'\n' },
                    },
                );
            }
            b"printf" => {
                let fmt = self.arg(tok)?;
                let segs = compile_format(&fmt, &mut self.warnings)?;
                self.no_default_print = true;
                self.push_prim(tok, Prim::Printf { sink: 0, segs });
            }
            b"fprintf" => {
                let path = self.arg(tok)?;
                let fmt = self.arg(tok)?;
                let sink = self.sink(&path)?;
                let segs = compile_format(&fmt, &mut self.warnings)?;
                self.no_default_print = true;
                self.push_prim(tok, Prim::Printf { sink, segs });
            }
            b"ls" => {
                self.no_default_print = true;
                self.push_prim(tok, Prim::Ls { sink: 0 });
            }
            b"fls" => {
                let path = self.arg(tok)?;
                let sink = self.sink(&path)?;
                self.no_default_print = true;
                self.push_prim(tok, Prim::Ls { sink });
            }
            b"delete" => {
                // `-delete` implies `-depth`: a directory cannot be removed
                // before the things inside it.
                self.depth_first = true;
                self.no_default_print = true;
                self.push_prim(tok, Prim::Delete);
            }
            b"prune" => self.push_prim(tok, Prim::Prune),
            b"quit" => self.push_prim(tok, Prim::Quit),
            b"exec" | b"execdir" | b"ok" | b"okdir" => {
                let idx = self.parse_exec(tok, canon)?;
                self.no_default_print = true;
                self.push_prim(tok, Prim::Exec(idx));
            }

            b"help" | b"-help" => return Ok(Some(Halt::Help)),
            b"version" | b"-version" => return Ok(Some(Halt::Version)),
            b"--noop" => self.push_prim(tok, Prim::Noop),

            _ => {
                return Err(Fatal::new(format!(
                    "unknown predicate `{}'",
                    String::from_utf8_lossy(tok)
                )));
            }
        }
        Ok(None)
    }

    /// `-name`'s one warning: a pattern with a `/` in it can never match,
    /// because the name it is matched against is a single component.
    fn check_name_arg(&mut self, tok: &[u8], pat: &[u8]) {
        if self.should_warn() && pat.contains(&b'/') {
            let alt = [b"-wholename".as_slice(), pat].concat();
            self.warnings.push(format!(
                "warning: {} matches against basenames only, but the given pattern contains a directory separator ({}), thus the expression will evaluate to false all the time.  Did you mean {}?",
                quote(tok),
                quote(b"/"),
                quote(&alt)
            ));
        }
    }

    /// `stat` a file named on the command line, for `-newer`/`-samefile`.
    fn stat_arg(&self, path: &[u8]) -> Parsed<Meta> {
        self.tree
            .stat(path)
            .map_err(|e| Fatal::new(format!("{}: {}", quote(path), strerror(&e))))
    }

    fn parse_time(&self, tok: &[u8], arg: &[u8], field: TimeField) -> Parsed<Prim> {
        // The origin moves to the end of "today" when the user wrote `-n`,
        // because the inverted comparison then means "newer than".
        let mut origin = self.cur_day_start;
        if get_comp_type(arg).0 == Comp::Lt {
            origin.sec = origin.sec.saturating_add(86399);
        }
        let (cmp, ts) =
            get_relative_timestamp(arg, origin, DAYSECS).ok_or_else(|| Self::bad_arg(tok, arg))?;
        Ok(Prim::TimeWindow {
            field: Some(field),
            cmp,
            origin: ts,
            window: DAYSECS,
        })
    }

    /// `-daystart`: move the origin from "24 hours ago" to "local midnight".
    fn apply_daystart(&mut self) {
        let zone = localtime::Zone::from_env();
        let base = self.cur_day_start.sec.saturating_add(86400);
        let tm = zone.local(base, 0);
        let since_midnight = i64::from(tm.second)
            .saturating_add(i64::from(tm.minute).saturating_mul(60))
            .saturating_add(i64::from(tm.hour).saturating_mul(3600));
        self.cur_day_start = Ts {
            sec: base.saturating_sub(since_midnight),
            nsec: 0,
        };
    }
}

/// Upstream `parse_size`: a comparison, a count, and an optional unit letter.
///
/// The unit is decided from the *last* byte before the number is looked at,
/// which is why `-size x` and `-size 1x` produce the same complaint about a
/// size *type* rather than one about a bad number.
fn parse_size(arg: &[u8]) -> Parsed<Prim> {
    let Some(&last) = arg.last() else {
        return Err(Fatal::new("invalid null argument to -size"));
    };
    let head = arg.get(..arg.len().saturating_sub(1)).unwrap_or(b"");
    let (body, unit, suffix): (&[u8], u64, &[u8]) = match last {
        b'b' => (head, 512, b"b"),
        b'c' => (head, 1, b"c"),
        b'k' => (head, 1024, b"k"),
        b'M' => (head, 1024 * 1024, b"M"),
        b'G' => (head, 1024 * 1024 * 1024, b"G"),
        b'w' => (head, 2, b"w"),
        d if d.is_ascii_digit() => (arg, 512, b""),
        other => {
            return Err(Fatal::new(format!(
                "invalid -size type `{}'",
                other as char
            )));
        }
    };
    let (cmp, digits) = get_comp_type(body);
    let n = xstrtoumax(digits).ok_or_else(|| {
        Fatal::new(format!(
            "Invalid argument `{}{}' to -size",
            String::from_utf8_lossy(body),
            String::from_utf8_lossy(suffix)
        ))
    })?;
    Ok(Prim::Size { cmp, n, unit })
}

// ---------------------------------------------------------------------------
// The three primaries with a grammar of their own
// ---------------------------------------------------------------------------

impl Parser<'_> {
    /// Upstream `parse_perm`.
    ///
    /// Two modes are computed rather than one because a symbolic mode can
    /// depend on whether the file is a directory — `-perm -X` means something
    /// different for a directory than for a regular file — so the answer is
    /// selected at match time.
    fn parse_perm(&mut self, arg: &[u8]) -> Parsed<Prim> {
        let (mut kind, spec): (PermKind, &[u8]) = match arg.first() {
            Some(b'-') => (PermKind::AtLeast, arg.get(1..).unwrap_or(b"")),
            Some(b'/') => (PermKind::Any, arg.get(1..).unwrap_or(b"")),
            _ => (PermKind::Exact, arg),
        };
        // `+NUMERICMODE` was a GNU extension that now contradicts what chmod
        // does with the same spelling, so it is refused rather than guessed at.
        let plus_numeric =
            arg.first() == Some(&b'+') && matches!(arg.get(1), Some(d) if (b'0'..b'8').contains(d));
        let changes = modechange::compile(spec).filter(|_| !plus_numeric);
        let Some(changes) = changes else {
            return Err(Fatal::new(format!("invalid mode {}", quote(arg))));
        };
        let file_mode = modechange::adjust(0, false, 0, &changes).mode;
        let dir_mode = modechange::adjust(0, true, 0, &changes).mode;
        if arg.first() == Some(&b'/') && file_mode == 0 && dir_mode == 0 {
            self.warnings.push(format!(
                "warning: you have specified a mode pattern {} (which is equivalent to /000). \
                 The meaning of -perm /000 has now been changed to be consistent with -perm -000; \
                 that is, while it used to match no files, it now matches all files.",
                String::from_utf8_lossy(arg)
            ));
            kind = PermKind::AtLeast;
        }
        Ok(Prim::Perm {
            kind,
            file_mode,
            dir_mode,
        })
    }

    /// Upstream `parse_newerXY`.
    ///
    /// This is the one `ARG_SPECIAL_PARSE` entry, which means the driver has
    /// *not* consumed the predicate token: refusing without consuming it is
    /// how `-newerqq` becomes "invalid predicate" rather than "invalid
    /// argument".
    fn parse_newer_xy(&mut self, tok: &[u8]) -> Parsed<Prim> {
        const VALID: &[u8] = b"aBcmt";
        let (Some(&x), Some(&y)) = (tok.get(6), tok.get(7)) else {
            return Err(Self::invalid_predicate(tok));
        };
        if !VALID.contains(&x) || !VALID.contains(&y) || x == b't' {
            return Err(Self::invalid_predicate(tok));
        }
        // Consume the predicate token now that it is known to be one.
        self.i = self.i.saturating_add(1);
        let arg = self
            .argv
            .get(self.i)
            .cloned()
            .ok_or_else(|| Fatal::new(format!("The {} test needs an argument", quote(tok))))?;
        self.i = self.i.saturating_add(1);

        let ts = if y == b't' {
            parse_datetime(&arg).ok_or_else(|| {
                Fatal::new(format!(
                    "I cannot figure out how to interpret {} as a date or time",
                    quote(&arg)
                ))
            })?
        } else {
            let meta = self.stat_arg(&arg)?;
            match y {
                b'a' => meta.atime,
                b'c' => meta.ctime,
                b'B' => {
                    return Err(Fatal::new(format!(
                        "The system does not provide a way to find the birth time of {}",
                        quote(&arg)
                    )));
                }
                _ => meta.mtime,
            }
        };
        let field = match x {
            b'a' => TimeField::Access,
            b'c' => TimeField::Change,
            b'B' => TimeField::Birth,
            _ => TimeField::Modify,
        };
        Ok(Prim::Newer { field, ts })
    }

    fn invalid_predicate(tok: &[u8]) -> Fatal {
        Fatal::new(format!(
            "invalid predicate `{}'",
            String::from_utf8_lossy(tok)
        ))
    }

    /// Upstream `insert_exec_ok`, shared by `-exec`, `-execdir`, `-ok` and
    /// `-okdir`.
    fn parse_exec(&mut self, tok: &[u8], canon: &[u8]) -> Parsed<usize> {
        let confirm = matches!(canon, b"ok" | b"okdir");
        let dir_relative = matches!(canon, b"execdir" | b"okdir");
        let allow_plus = !confirm;
        if self.argv.get(self.i).is_none() {
            return Err(Fatal::new(format!(
                "missing argument to `{}'",
                String::from_utf8_lossy(tok)
            )));
        }
        if dir_relative {
            check_path_safety(tok, self.tree.path_env().as_deref())?;
            // `-execdir`'s whole point is that the name it hands the child is
            // relative, so a racing rename must not be ignored.
            self.ignore_readdir_race = false;
        }

        let start = self.i;
        let mut end = start;
        let mut multiple = false;
        let mut saw_braces = false;
        let mut brace_count = 0usize;
        let mut brace_arg: Vec<u8> = Vec::new();
        while let Some(a) = self.argv.get(end) {
            if a.as_slice() == b";" {
                break;
            }
            if allow_plus && a.as_slice() == b"+" && saw_braces {
                multiple = true;
                break;
            }
            saw_braces = window_contains(a, b"{}");
            if saw_braces {
                brace_count = brace_count.saturating_add(1);
                brace_arg = a.clone();
            }
            end = end.saturating_add(1);
        }
        if end == start || self.argv.get(end).is_none() {
            self.i = end;
            return Err(Fatal::new(format!(
                "missing argument to `{}'",
                String::from_utf8_lossy(tok)
            )));
        }

        let suffix = if canon == b"execdir" { "dir" } else { "" };
        if multiple {
            if brace_count > 1 {
                return Err(Fatal::new(format!(
                    "Only one instance of {{}} is supported with -exec{suffix} ... +"
                )));
            }
            if brace_arg.len() != 2 {
                return Err(Fatal::new(format!(
                    "In {} the {} must appear by itself, but you specified {}",
                    quote(format!("-exec{suffix} ... {{}} +").as_bytes()),
                    quote(b"{}"),
                    quote(&brace_arg)
                )));
            }
        }

        // For the `+` form the trailing `{}` is dropped: the names are
        // appended to the initial arguments instead of replacing anything.
        let last = if multiple { end.saturating_sub(1) } else { end };
        let argv: Vec<Vec<u8>> = self
            .argv
            .get(start..last)
            .map_or_else(Vec::new, <[Vec<u8>]>::to_vec);
        self.i = end.saturating_add(1);
        self.execs.push(ExecSpec {
            argv,
            multiple,
            confirm,
            dir_relative,
            pending: Vec::new(),
            pending_dir: None,
        });
        Ok(self.execs.len().saturating_sub(1))
    }
}

/// `memmem`, for the `{}` scan.
fn window_contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.len() > haystack.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Upstream `check_path_safety`: `-execdir` runs a command from a directory
/// an attacker may control, so a `$PATH` that can resolve relative to the cwd
/// is refused outright rather than merely warned about.
fn check_path_safety(action: &[u8], path: Option<&[u8]>) -> Parsed<()> {
    let Some(path) = path else {
        return Ok(());
    };
    for entry in path.split(|&b| b == b':') {
        if entry.is_empty() || entry == b"." {
            return Err(Fatal::new(format!(
                "The current directory is included in the PATH environment variable, which is \
                 insecure in combination with the {} action of find.  Please remove the current \
                 directory from your $PATH (that is, remove \".\", doubled colons, or leading or \
                 trailing colons)",
                String::from_utf8_lossy(action)
            )));
        }
        if entry.first() != Some(&b'/') {
            return Err(Fatal::new(format!(
                "The relative path {} is included in the PATH environment variable, which is \
                 insecure in combination with the {} action of find.  Please remove that entry \
                 from $PATH",
                quote(entry),
                String::from_utf8_lossy(action)
            )));
        }
    }
    Ok(())
}

/// The slice of `parse_datetime` that `-newerXt` actually reaches for.
///
/// gnulib's parser accepts English relative phrases ("2 hours ago"); this
/// accepts `@SECONDS` and the ISO-ish absolute forms, which is what a script
/// writes. A phrase it cannot read is refused with GNU's own wording rather
/// than silently misread. Tracked in `known-issues.md`.
fn parse_datetime(s: &[u8]) -> Option<Ts> {
    if let Some(rest) = s.strip_prefix(b"@") {
        let (neg, digits) = match rest.strip_prefix(b"-") {
            Some(d) => (true, d),
            None => (false, rest),
        };
        let (secs, nanos) = split_fraction(digits)?;
        let sec = i64::try_from(secs).ok()?;
        return Some(Ts {
            sec: if neg { -sec } else { sec },
            nsec: nanos,
        });
    }
    let text = std::str::from_utf8(s).ok()?;
    let text = text.trim();
    let (date, time) = match text.split_once(['T', ' ']) {
        Some((d, t)) => (d, Some(t.trim())),
        None => (text, None),
    };
    let mut parts = date.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: u32 = parts.next()?.parse().ok()?;
    let day: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let (mut hour, mut min, mut sec) = (0u32, 0u32, 0u32);
    if let Some(t) = time {
        let t = t.trim_end_matches('Z');
        let mut hms = t.split(':');
        hour = hms.next()?.parse().ok()?;
        min = hms.next().map_or(Ok(0), str::parse).ok()?;
        sec = hms.next().map_or(Ok(0), str::parse).ok()?;
        if hms.next().is_some() {
            return None;
        }
    }
    let days = days_from_civil(year, month, day);
    let utc = days
        .checked_mul(86400)?
        .checked_add(i64::from(hour).checked_mul(3600)?)?
        .checked_add(i64::from(min).checked_mul(60)?)?
        .checked_add(i64::from(sec))?;
    // Interpret in the local zone, as parse_datetime does.
    let zone = localtime::Zone::from_env();
    let guess = zone.lookup(utc);
    let sec = utc.checked_sub(i64::from(guess.gmtoff))?;
    Some(Ts { sec, nsec: 0 })
}

fn split_fraction(digits: &[u8]) -> Option<(u64, u32)> {
    let (whole, frac) = match digits.iter().position(|&b| b == b'.') {
        Some(p) => (digits.get(..p)?, digits.get(p.saturating_add(1)..)?),
        None => (digits, &b""[..]),
    };
    let secs = parse_u64(whole)?;
    let mut nanos: u32 = 0;
    for i in 0..9 {
        let d = frac.get(i).copied().unwrap_or(b'0');
        if !d.is_ascii_digit() {
            return None;
        }
        nanos = nanos
            .checked_mul(10)?
            .checked_add(u32::from(d.wrapping_sub(b'0')))?;
    }
    Some((secs, nanos))
}

/// Howard Hinnant's `days_from_civil`: a proleptic Gregorian day number.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y.saturating_sub(1) } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = i64::from((m + 9) % 12);
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

// ---------------------------------------------------------------------------
// -printf: the format compiler
// ---------------------------------------------------------------------------

/// One piece of a compiled `-printf` format.
///
/// Upstream (`find/print.c`) keeps a linked list of `struct segment`, each of
/// which carries *both* the literal run that precedes a directive and the
/// directive itself, because it hands the pair straight to `printf`. The split
/// is preserved here — `lit` is the run, `spec` is the `%` and everything up to
/// but not including the conversion letter — because a width or precision in
/// `spec` has to be applied to the *rendered* directive and to nothing else.
enum Seg {
    /// Literal bytes, no directive.
    Plain(Vec<u8>),
    /// Literal bytes, and then stop: `\c` truncates the format and suppresses
    /// everything after it, including any trailing newline.
    Stop(Vec<u8>),
    Format {
        lit: Vec<u8>,
        /// `%`, flags, width, precision — never the conversion letter.
        spec: Vec<u8>,
        conv: u8,
        /// The second letter of a two-letter directive (`%A`/`%B`/`%C`/`%T`),
        /// which is a `strftime` conversion rather than a `find` one.
        aux: u8,
    },
}

/// `get_format_flags_length` — the length of `%`, flags, width and precision,
/// measured from the `%` at `at`. The returned index is that of the character
/// that *follows* them, which upstream then reads as the conversion letter.
fn format_flags_length(buf: &[u8], at: usize) -> usize {
    let mut n = 0usize;
    loop {
        n = n.saturating_add(1);
        match buf.get(at.saturating_add(n)) {
            Some(&b) if b == b'-' || b == b'+' || b == b' ' || b == b'#' => {}
            _ => break,
        }
    }
    while matches!(buf.get(at.saturating_add(n)), Some(b) if b.is_ascii_digit()) {
        n = n.saturating_add(1);
    }
    if buf.get(at.saturating_add(n)) == Some(&b'.') {
        n = n.saturating_add(1);
        while matches!(buf.get(at.saturating_add(n)), Some(b) if b.is_ascii_digit()) {
            n = n.saturating_add(1);
        }
    }
    n
}

/// `get_format_specifer_length` — 1 for the one-letter directives, 2 for the
/// four that take a `strftime` letter after them, 0 for anything else.
fn format_specifier_length(ch: u8) -> usize {
    if b"abcdDfFgGhHiklmMnpPsStuUyYZ%".contains(&ch) {
        1
    } else if b"ABCT".contains(&ch) {
        2
    } else {
        0
    }
}

/// One byte as it appears inside a diagnostic. A NUL renders as nothing, which
/// is what C's `%c` does with it once the message reaches a terminal.
fn chr(b: u8) -> String {
    if b == 0 {
        String::new()
    } else {
        String::from_utf8_lossy(&[b]).into_owned()
    }
}

/// `insert_fprintf` — compile a `-printf`/`-fprintf` format.
///
/// Escapes are decoded in place exactly as upstream does, which is why this
/// works on a copy of the argument: a `\n` becomes one byte *inside* the
/// literal run rather than a segment of its own, and an unrecognised escape is
/// left in the run verbatim after a warning. The warnings are collected rather
/// than printed because the caller prints them all once parsing has finished,
/// which keeps them ahead of any output the walk produces.
fn compile_format(fmt: &[u8], warnings: &mut Vec<String>) -> Parsed<Vec<Seg>> {
    let mut buf = fmt.to_vec();
    let mut segs: Vec<Seg> = Vec::new();
    let mut segstart: usize = 0;
    let mut i: usize = 0;

    while i < buf.len() {
        let c = buf.get(i).copied().unwrap_or(0);
        if c == b'\\' && buf.get(i.saturating_add(1)) == Some(&b'c') {
            segs.push(Seg::Stop(buf.get(segstart..i).unwrap_or(b"").to_vec()));
            return Ok(segs);
        }
        if c == b'\\' {
            let mut readpos: usize = 1;
            match buf.get(i.saturating_add(readpos)).copied() {
                None => {
                    warnings.push("warning: escape `\\' followed by nothing at all".to_string());
                    // The backslash is its own reasonable result; keep it.
                    readpos = 0;
                }
                Some(d) if (b'0'..=b'7').contains(&d) => {
                    let mut n: u32 = 0;
                    let mut k: usize = 0;
                    while k < 3 {
                        match buf
                            .get(i.saturating_add(readpos).saturating_add(k))
                            .copied()
                        {
                            Some(o) if (b'0'..=b'7').contains(&o) => {
                                n = n.wrapping_mul(8).wrapping_add(u32::from(o - b'0'));
                                k = k.saturating_add(1);
                            }
                            _ => break,
                        }
                    }
                    if let Some(slot) = buf.get_mut(i) {
                        *slot = u8::try_from(n & 0xff).unwrap_or(0);
                    }
                    // `parse_octal_escape` reports one fewer than it read, and
                    // the caller adds the backslash back on.
                    readpos = readpos.saturating_add(k.saturating_sub(1));
                }
                Some(d) => {
                    let v = match d {
                        b'a' => 0x07,
                        b'b' => 0x08,
                        b'f' => 0x0c,
                        b'n' => b'\n',
                        b'r' => b'\r',
                        b't' => b'\t',
                        b'v' => 0x0b,
                        b'\\' => b'\\',
                        _ => 0,
                    };
                    if v == 0 {
                        warnings.push(format!("warning: unrecognized escape `\\{}'", chr(d)));
                        i = i.saturating_add(readpos).saturating_add(1);
                        continue;
                    }
                    if let Some(slot) = buf.get_mut(i) {
                        *slot = v;
                    }
                }
            }
            let end = i.saturating_add(1);
            segs.push(Seg::Plain(buf.get(segstart..end).unwrap_or(b"").to_vec()));
            segstart = i.saturating_add(readpos).saturating_add(1);
            i = i.saturating_add(readpos).saturating_add(1);
            continue;
        }
        if c == b'%' {
            let pct = i;
            if i.saturating_add(1) >= buf.len() {
                return Err(Fatal::new("error: % at end of format string"));
            }
            let flen = if buf.get(i.saturating_add(1)) == Some(&b'%') {
                1
            } else {
                format_flags_length(&buf, i)
            };
            i = i.saturating_add(flen);
            let conv = buf.get(i).copied().unwrap_or(0);
            let speclen = format_specifier_length(conv);
            let complete = speclen > 0
                && buf
                    .get(i.saturating_add(speclen).saturating_sub(1))
                    .is_some();
            if complete {
                let aux = if speclen == 2 {
                    buf.get(i.saturating_add(1)).copied().unwrap_or(0)
                } else {
                    0
                };
                segs.push(Seg::Format {
                    lit: buf.get(segstart..pct).unwrap_or(b"").to_vec(),
                    spec: buf.get(pct..i).unwrap_or(b"").to_vec(),
                    conv,
                    aux,
                });
                i = i.saturating_add(speclen.saturating_sub(1));
            } else {
                if conv == b'{' || conv == b'[' || conv == b'(' {
                    return Err(Fatal::new(format!(
                        "error: the format directive `%{}' is reserved for future use",
                        chr(conv)
                    )));
                }
                if speclen == 2 {
                    warnings.push(format!(
                        "warning: format directive `%{}' should be followed by another character",
                        chr(conv)
                    ));
                } else {
                    warnings.push(format!(
                        "warning: unrecognized format directive `%{}'",
                        chr(conv)
                    ));
                }
                let end = i.saturating_add(1);
                segs.push(Seg::Plain(buf.get(segstart..end).unwrap_or(b"").to_vec()));
            }
            segstart = i.saturating_add(1);
        }
        i = i.saturating_add(1);
    }

    if i > segstart {
        segs.push(Seg::Plain(buf.get(segstart..i).unwrap_or(b"").to_vec()));
    }
    Ok(segs)
}

// ---------------------------------------------------------------------------
// The expression tree
// ---------------------------------------------------------------------------

impl Parser<'_> {
    /// `build_expression_tree`'s driver loop: everything from the first
    /// expression token to the end of `argv`.
    ///
    /// The leading `(` is pushed here rather than by the caller because the
    /// three-way wrap-up in [`Parser::finish`] is defined in terms of whether
    /// anything was appended *after* it.
    fn parse_expression(&mut self) -> Parsed<Option<Halt>> {
        self.nodes.push(Node {
            kind: PKind::Open,
            name: b"(".to_vec(),
            artificial: true,
            prim: None,
        });

        while self.i < self.argv.len() {
            let tok = self.argv.get(self.i).cloned().unwrap_or_default();
            if !looks_like_expression(&tok, false) {
                let mut fatal = Fatal::new(format!(
                    "paths must precede expression: `{}'",
                    String::from_utf8_lossy(&tok)
                ));
                // The second line is a guess, and upstream only offers it when
                // the token names something that exists — which is the shape a
                // forgotten quote around a glob leaves behind.
                if self.tree.access(&tok, 0) {
                    let last = self
                        .nodes
                        .last()
                        .map(|n| n.name.clone())
                        .unwrap_or_default();
                    fatal.0.push(format!(
                        "possible unquoted pattern after predicate `{}'?",
                        String::from_utf8_lossy(&last)
                    ));
                }
                return Err(fatal);
            }

            let Some(canon) = find_parser(&tok) else {
                return Err(Fatal::new(format!(
                    "unknown predicate `{}'",
                    String::from_utf8_lossy(&tok)
                )));
            };

            self.found_parser(&tok, canon);

            // `-newerXY` is the one `ARG_SPECIAL_PARSE` entry: it re-reads its
            // own name to find the two letters, so the driver must not eat it.
            if canon != b"newerXY" {
                self.i = self.i.saturating_add(1);
            }
            if let Some(halt) = self.parse_token(&tok, canon)? {
                return Ok(Some(halt));
            }
        }

        self.finish();
        Ok(None)
    }

    /// The wrap-up: `( … ) -print`, or one of the two ways of not doing that.
    fn finish(&mut self) {
        if self.nodes.len() == 1 {
            // Nothing but global options. Drop the `(` and print everything.
            self.nodes.clear();
            self.push_prim(
                b"-print",
                Prim::Print {
                    sink: 0,
                    terminator: b'\n',
                },
            );
        } else if self.no_default_print {
            // An action already produces output; drop the now-unmatched `(`.
            self.nodes.remove(0);
        } else {
            self.nodes.push(Node {
                kind: PKind::Close,
                name: b")".to_vec(),
                artificial: true,
                prim: None,
            });
            // Goes through `push_checked`, which is what supplies the `-a`
            // between the `)` and the `-print`.
            self.push_checked(Node {
                kind: PKind::Primary,
                name: b"-print".to_vec(),
                artificial: true,
                prim: Some(Prim::Print {
                    sink: 0,
                    terminator: b'\n',
                }),
            });
        }
    }
}

/// A cursor over the flat predicate list, which is upstream's `*input`.
struct Builder<'n> {
    nodes: &'n [Node],
    i: usize,
}

impl Builder<'_> {
    fn kind(&self) -> Option<PKind> {
        self.nodes.get(self.i).map(|n| n.kind)
    }

    fn artificial(&self, idx: usize) -> bool {
        self.nodes.get(idx).is_some_and(|n| n.artificial)
    }

    fn name(&self, idx: usize) -> String {
        self.nodes
            .get(idx)
            .map(|n| String::from_utf8_lossy(&n.name).into_owned())
            .unwrap_or_default()
    }

    /// `get_expr`. `prev_pred` is the node the caller was looking at, which
    /// several of the diagnostics quote — it is not the same thing as the
    /// node to the left in the tree.
    fn get_expr(&mut self, prev_prec: u8, prev_pred: Option<usize>) -> Parsed<Expr> {
        let this = self.i;
        let Some(kind) = self.kind() else {
            return Err(Fatal::new("invalid expression"));
        };

        let mut next: Expr = match kind {
            PKind::And | PKind::Or | PKind::Comma => {
                // e.g. `find . -a`
                return Err(Fatal::new(format!(
                    "invalid expression; you have used a binary operator '{}' with nothing before it.",
                    self.name(this)
                )));
            }
            PKind::Close => {
                let Some(prev) = prev_pred else {
                    return Err(Fatal::new(format!(
                        "invalid expression: expected expression before closing parentheses '{}'.",
                        self.name(this)
                    )));
                };
                let prev_is_op = matches!(
                    self.nodes.get(prev).map(|n| n.kind),
                    Some(PKind::Not | PKind::And | PKind::Or | PKind::Comma)
                );
                return Err(if prev_is_op && !self.artificial(this) {
                    // e.g. `find \( -not \)`
                    Fatal::new(format!(
                        "expected an expression between '{}' and ')'",
                        self.name(prev)
                    ))
                } else if self.artificial(this) {
                    // The user's predicates ran out inside the wrapper: the
                    // `)` we tripped over is one `find` added itself.
                    Fatal::new(format!(
                        "expected an expression after '{}'",
                        self.name(prev)
                    ))
                } else {
                    Fatal::new("invalid expression; you have too many ')'")
                });
            }
            PKind::Primary => {
                self.i = self.i.saturating_add(1);
                Expr::Prim(this)
            }
            PKind::Not => {
                self.i = self.i.saturating_add(1);
                let right = self.get_expr(PKind::Not.prec(), Some(this))?;
                Expr::Not(Box::new(right))
            }
            PKind::Open => {
                let after = this.saturating_add(1);
                if !matches!(self.nodes.get(after), Some(n) if !n.artificial) {
                    // The `)` in sight is the artificial one, so the user's
                    // `(` never got a partner.
                    return Err(Fatal::new(format!(
                        "invalid expression; expected to find a ')' but didn't see one. \
                         Perhaps you need an extra predicate after '{}'",
                        self.name(this)
                    )));
                }
                self.i = after;
                if self.kind() == Some(PKind::Close) {
                    if self.artificial(this) {
                        return Err(Fatal::new(format!(
                            "invalid expression: expected expression before closing parentheses '{}'.",
                            self.name(self.i)
                        )));
                    }
                    return Err(Fatal::new(
                        "invalid expression; empty parentheses are not allowed.",
                    ));
                }
                let inner = self.get_expr(PKind::Open.prec(), Some(this))?;
                if self.kind() != Some(PKind::Close) {
                    return Err(Fatal::new(
                        "invalid expression; I was expecting to find a ')' somewhere but did not see one.",
                    ));
                }
                self.i = self.i.saturating_add(1);
                inner
            }
        };

        if self.i >= self.nodes.len() {
            return Ok(next);
        }
        if self.nodes.get(self.i).map_or(0, |n| n.kind.prec()) > prev_prec {
            next = self
                .scan_rest(next, prev_prec)?
                .ok_or_else(|| Fatal::new("invalid expression"))?;
        }
        Ok(next)
    }

    /// `scan_rest`: fold the operators that bind more tightly than the
    /// caller's into the tree `head` already holds.
    fn scan_rest(&mut self, head: Expr, prev_prec: u8) -> Parsed<Option<Expr>> {
        if matches!(self.kind(), None | Some(PKind::Close)) {
            return Ok(None);
        }
        let mut tree = head;
        while let Some(kind) = self.kind() {
            if kind.prec() <= prev_prec {
                break;
            }
            match kind {
                // Unreachable in practice: an implicit `-a` is always
                // interposed before one operand can follow another.
                PKind::Primary | PKind::Not | PKind::Open => {
                    return Err(Fatal::new("invalid expression"));
                }
                PKind::Close => return Ok(Some(tree)),
                PKind::And | PKind::Or | PKind::Comma => {
                    let op = self.i;
                    self.i = self.i.saturating_add(1);
                    let right = self.get_expr(kind.prec(), Some(op))?;
                    tree = match kind {
                        PKind::Or => Expr::Or(Box::new(tree), Box::new(right)),
                        PKind::Comma => Expr::Comma(Box::new(tree), Box::new(right)),
                        _ => Expr::And(Box::new(tree), Box::new(right)),
                    };
                }
            }
        }
        Ok(Some(tree))
    }
}

/// Shape the flat list into a tree, and refuse what is left over.
fn build_tree(nodes: &[Node]) -> Parsed<Expr> {
    let mut builder = Builder { nodes, i: 0 };
    let tree = builder.get_expr(PKind::Open.prec(), None)?;
    if builder.i < nodes.len() {
        return Err(if builder.kind() == Some(PKind::Close) {
            // e.g. `find \( -true \) \)`
            Fatal::new("you have too many ')'")
        } else {
            Fatal::new(format!(
                "unexpected extra predicate '{}'",
                builder.name(builder.i)
            ))
        });
    }
    Ok(tree)
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// Everything the actions need to know about the file in hand.
struct Item {
    /// The path as it will be printed: the start point, then the components
    /// the walk descended through.
    path: Vec<u8>,
    /// `state.starting_path_length` — how much of `path` is the start point.
    /// `%H` prints exactly this prefix and `%P` strips it.
    start_len: usize,
    /// `state.curdepth`: 0 for a start point.
    depth: usize,
    /// `state.rel_pathname`: the name relative to the directory `find` would
    /// have been sitting in — the whole start point for a start point, the bare
    /// entry name for anything below one. `-delete` and `-execdir` are defined
    /// in terms of this rather than of `path`.
    rel: Vec<u8>,
    /// The directory `-execdir` runs its command in, or `None` for the
    /// directory `find` itself started in.
    dir: Option<Vec<u8>>,
    /// `state.type`: the `S_IFMT` bits `readdir` gave us for free, or 0 when
    /// it gave us nothing. Never has permission bits in it.
    type_mode: u32,
    /// `state.have_stat` and the `struct stat` behind it, in one: `Some` once
    /// [`Ctx::get_info`] has taken the `stat` some predicate asked for.
    ///
    /// A [`Cell`](std::cell::Cell) so that the evaluator can fill it while
    /// holding the item by shared reference, which is what lets the whole of
    /// [`Ctx::apply`] keep taking `&Item`.
    stat: std::cell::Cell<Option<Meta>>,
    /// `state.already_issued_stat_error_msg`: one diagnostic per file, however
    /// many predicates go on to ask for the `stat` that failed.
    reported: std::cell::Cell<bool>,
}

impl Item {
    /// The `stat` we have, or the type-only stand-in `state.type` amounts to.
    ///
    /// Callers reach this only after [`Ctx::get_info`] has agreed the
    /// predicate can run, so a zeroed `Meta` here means the predicate declared
    /// it needed nothing more than the type — which is exactly what it gets.
    fn meta(&self) -> Meta {
        self.stat.get().unwrap_or(Meta {
            dev: 0,
            ino: 0,
            mode: self.type_mode,
            nlink: 0,
            uid: 0,
            gid: 0,
            size: 0,
            blocks: 0,
            rdev: 0,
            atime: Ts { sec: 0, nsec: 0 },
            mtime: Ts { sec: 0, nsec: 0 },
            ctime: Ts { sec: 0, nsec: 0 },
        })
    }
}

/// What a predicate must know before it can run: upstream's `need_stat`,
/// `need_type` and `need_inum` triple, which is a total order in practice
/// because `get_pred_cost` reads them as one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Need {
    /// The name is enough — `-name`, `-print`, `-exec`, `-delete`.
    Nothing,
    /// `-type` and `%y`: `d_type` will do, if `readdir` supplied it.
    Type,
    /// `-inum` and `%i`. Upstream distrusts `d_ino` for directories, which may
    /// be mount points, and we have no `d_ino` at all, so this always stats.
    Inum,
    /// Everything else.
    Stat,
}

/// `make_segment`'s per-directive contribution to the format's `need_*` flags.
///
/// The list of directives that need *nothing* is short and worth reading as a
/// list rather than as a default, because it is the whole of what
/// `find -printf` can report about a file it cannot `stat`: the four names
/// (`%p %f %h %P`), the start point (`%H`), the depth (`%d`), a literal `%%`,
/// and the SELinux context (`%Z`), which upstream costs as `NeedsAccessInfo`
/// and does not stat for.
fn seg_need(s: &Seg) -> Need {
    match s {
        Seg::Plain(_) | Seg::Stop(_) => Need::Nothing,
        Seg::Format { conv, .. } => match conv {
            b'%' | b'f' | b'h' | b'p' | b'P' | b'H' | b'd' | b'Z' => Need::Nothing,
            b'y' => Need::Type,
            b'i' => Need::Inum,
            _ => Need::Stat,
        },
    }
}

/// The `need_stat`/`need_type`/`need_inum` flags upstream's parser attaches to
/// each predicate, transcribed from `find -D tree` rather than from the
/// source: several are the way they are only because `insert_primary` defaults
/// them on and the parse function never cleared them (`-prune` needs a `stat`
/// it does not read, and `-printf '%d'` needs one too, because *any* `%`
/// directive that is not on print.c's short list raises the flag).
fn prim_need(p: &Prim) -> Need {
    match p {
        Prim::True
        | Prim::False
        | Prim::Noop
        | Prim::Name { .. }
        | Prim::Path { .. }
        | Prim::Regex(_)
        | Prim::Access(_)
        | Prim::Print { .. }
        | Prim::Quit
        | Prim::Delete
        | Prim::Exec(_) => Need::Nothing,
        Prim::Type(_) => Need::Type,
        Prim::Inum(_) => Need::Inum,
        Prim::Printf { segs, .. } => segs
            .iter()
            .map(seg_need)
            .max_by_key(|n| match n {
                Need::Nothing => 0u8,
                Need::Type => 1,
                Need::Inum => 2,
                Need::Stat => 3,
            })
            .unwrap_or(Need::Nothing),
        _ => Need::Stat,
    }
}

/// C-locale day and month abbreviations. `find` carries its own arrays rather
/// than asking `strftime`, so the `ctime` format is locale-independent — which
/// is why `%a`/`%c`/`%t` do not move when `LC_TIME` does.
const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// `human_readable(n, human_ceiling, FROM, TO)` with neither autoscaling nor
/// SI suffixes, which is every call `find` makes: a rounded-up ratio and
/// nothing else. Done in `u128` because `blocks * 512` overflows a `u64` only
/// on a nonsense `st_blocks`, and a wrong answer there is worse than a slow
/// multiply.
fn scaled_ceil(n: u64, from: u64, to: u64) -> u64 {
    if to == 0 {
        return n;
    }
    let scaled = u128::from(n).saturating_mul(u128::from(from));
    u64::try_from(scaled.div_ceil(u128::from(to))).unwrap_or(u64::MAX)
}

/// The C conversion each `-printf` directive is ultimately rendered by.
/// Everything is `%s` — even the sizes, which go through `human_readable`
/// first — except the three upstream lists as honouring `#`, `0` and `+`.
fn printf_conv(conv: u8) -> u8 {
    match conv {
        b'd' => b'd',
        b'm' => b'o',
        b'S' => b'g',
        _ => b's',
    }
}

/// Split `%`, flags, width and precision into the shape [`cfmt`] takes.
fn parse_spec(spec: &[u8], conv: u8) -> extfloat::Spec {
    let mut out = extfloat::Spec {
        minus: false,
        plus: false,
        space: false,
        hash: false,
        zero: false,
        width: 0,
        precision: None,
        conv,
    };
    let mut i = 1usize; // skip the '%'
    while let Some(&b) = spec.get(i) {
        match b {
            b'-' => out.minus = true,
            b'+' => out.plus = true,
            b' ' => out.space = true,
            b'#' => out.hash = true,
            b'0' => out.zero = true,
            _ => break,
        }
        i = i.saturating_add(1);
    }
    let mut width: usize = 0;
    while let Some(&b) = spec.get(i) {
        if !b.is_ascii_digit() {
            break;
        }
        width = width
            .saturating_mul(10)
            .saturating_add(usize::from(b - b'0'));
        i = i.saturating_add(1);
    }
    out.width = width;
    if spec.get(i) == Some(&b'.') {
        i = i.saturating_add(1);
        let mut prec: usize = 0;
        while let Some(&b) = spec.get(i) {
            if !b.is_ascii_digit() {
                break;
            }
            prec = prec
                .saturating_mul(10)
                .saturating_add(usize::from(b - b'0'));
            i = i.saturating_add(1);
        }
        out.precision = Some(prec);
    }
    out
}

/// `print_quoted` when the destination is a terminal: replace anything that is
/// not printable with `?`, so that a file name cannot repaint the screen.
///
/// Deliberately not applied when the destination is a file or a pipe, which is
/// upstream's rule and the reason a script sees raw bytes.
fn qmark(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let text = String::from_utf8_lossy(bytes);
    if matches!(text, std::borrow::Cow::Borrowed(_)) {
        for ch in text.chars() {
            if ch.is_control() || (!ch.is_ascii() && ch.is_whitespace()) {
                out.push(b'?');
            } else {
                let mut buf = [0u8; 4];
                out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
            }
        }
    } else {
        // Not valid text in this locale; a byte at a time, ASCII only.
        for &b in bytes {
            if b.is_ascii_graphic() || b == b' ' {
                out.push(b);
            } else {
                out.push(b'?');
            }
        }
    }
    out
}

/// `ctime_format`: `%a`, `%c` and `%t`'s fixed 26-plus-nanoseconds layout.
fn ctime_format(ts: Ts, zone: &localtime::Zone) -> Vec<u8> {
    let tm = zone.local(ts.sec, ts.nsec);
    let wd = WEEKDAYS.get(tm.wday as usize).copied().unwrap_or("???");
    let mon = MONTHS
        .get(tm.month.saturating_sub(1) as usize)
        .copied()
        .unwrap_or("???");
    format!(
        "{wd} {mon} {:2} {:02}:{:02}:{:02}.{:09}0 {:04}",
        tm.day, tm.hour, tm.minute, tm.second, tm.nanos, tm.year
    )
    .into_bytes()
}

/// `scan_for_digit_differences`: are the two renderings identical apart from
/// one contiguous run of digits, and if so where is it?
fn digit_difference(p: &[u8], q: &[u8]) -> Option<(usize, usize)> {
    let mut seen: Option<(usize, usize)> = None;
    let mut i = 0usize;
    while i < p.len() && i < q.len() {
        let (a, b) = (p.get(i).copied()?, q.get(i).copied()?);
        if a != b {
            if !a.is_ascii_digit() || !b.is_ascii_digit() {
                return None;
            }
            match seen {
                None => seen = Some((i, 1)),
                Some((first, n)) => {
                    if i.saturating_sub(first) == n {
                        seen = Some((first, n.saturating_add(1)));
                    } else {
                        // More than one differing run: give up rather than
                        // guess which one is the seconds.
                        return None;
                    }
                }
            }
        }
        i = i.saturating_add(1);
    }
    if p.len() != q.len() {
        return None;
    }
    seen
}

/// `do_time_format`: `strftime`, with the nanoseconds spliced in after the
/// seconds if the seconds can be located.
///
/// Located by rendering the same instant twice with the seconds field moved by
/// eleven, which changes both of its digits and nothing else. That is upstream's
/// trick and it is the only way to find the seconds inside a format like `%X`
/// without reimplementing the format.
fn do_time_format(fmt: &[u8], tm: &localtime::Tm, ns: &[u8]) -> Vec<u8> {
    // The leading `_` stops a format that expands to nothing from looking like
    // a buffer failure; it is removed again below.
    let mut timefmt = vec![b'_'];
    timefmt.extend_from_slice(fmt);

    let buf = localtime::strftime(&timefmt, tm);
    let mut altered = *tm;
    altered.second = if tm.second >= 11 {
        tm.second.saturating_sub(11)
    } else {
        tm.second.saturating_add(11)
    };
    let altbuf = localtime::strftime(&timefmt, &altered);

    let mut out = buf;
    if let Some((i, n)) = digit_difference(&out, &altbuf) {
        let end = i.saturating_add(n);
        let next_is_digit = out.get(end).is_some_and(u8::is_ascii_digit);
        if n == 2 && !next_is_digit {
            let tail = out.split_off(end);
            out.extend_from_slice(ns);
            out.extend_from_slice(&tail);
        }
    }
    out.remove(0);
    out
}

/// `format_date`: the two-letter directives `%A`, `%B`, `%C` and `%T`.
fn format_date(ts: Ts, kind: u8, zone: &localtime::Zone) -> Vec<u8> {
    let (fmt, need_ns): (Vec<u8>, bool) = if kind == b'+' {
        (b"%Y-%m-%d+%T".to_vec(), true)
    } else {
        (vec![b'%', kind], matches!(kind, b'S' | b'T' | b'X' | b'@'))
    };
    // The trailing zero is upstream's, and it is deliberate: it stops scripts
    // slicing the fraction out by a fixed column offset.
    let ns = if need_ns {
        format!(".{:09}0", ts.nsec).into_bytes()
    } else {
        Vec::new()
    };

    if kind != b'@' {
        let tm = zone.local(ts.sec, ts.nsec);
        let out = do_time_format(&fmt, &tm, &ns);
        if !out.is_empty() {
            return out;
        }
    }

    let mut out = Vec::new();
    if ts.sec < 0 {
        out.push(b'-');
    }
    out.extend_from_slice(ts.sec.unsigned_abs().to_string().as_bytes());
    out.extend_from_slice(&ns);
    out
}

/// The result of expanding one `-printf` format against one file.
struct Rendered {
    bytes: Vec<u8>,
    /// `\c` was reached: stop expanding, and flush.
    stop: bool,
    /// Diagnostics, each paired with whether it makes the exit status
    /// non-zero. `%Y`'s is deliberately the one that does not — upstream has
    /// the assignment commented out, and a `-printf '%Y'` over a tree with one
    /// unreadable directory should still exit 0.
    errs: Vec<(String, bool)>,
}

/// `%h`: the leading directories of a path.
///
/// Not [`pathname::dir_name`]: upstream strips *all* trailing slashes first,
/// then cuts at the last remaining one, and answers `.` — not the whole name —
/// when there is no slash at all.
fn printf_dirname(path: &[u8]) -> Vec<u8> {
    let mut end = path.len();
    while end > 0 && path.get(end.saturating_sub(1)) == Some(&b'/') {
        end = end.saturating_sub(1);
    }
    // `pname < s`: a name that is nothing but slashes keeps them.
    let trimmed = if end > 1 {
        path.get(..end).unwrap_or(path)
    } else {
        path
    };
    match trimmed.iter().rposition(|&b| b == b'/') {
        None => b".".to_vec(),
        Some(pos) => trimmed.get(..pos).unwrap_or(b"").to_vec(),
    }
}

/// Expand a compiled `-printf` format.
fn render_printf(
    segs: &[Seg],
    it: &Item,
    tree: &dyn Tree,
    zone: &localtime::Zone,
    tty: bool,
) -> Rendered {
    let mut out = Rendered {
        bytes: Vec::new(),
        stop: false,
        errs: Vec::new(),
    };
    let meta = it.meta();
    let m = &meta;

    for seg in segs {
        match seg {
            Seg::Plain(text) => out.bytes.extend_from_slice(text),
            Seg::Stop(text) => {
                out.bytes.extend_from_slice(text);
                out.stop = true;
                return out;
            }
            Seg::Format {
                lit,
                spec,
                conv,
                aux,
            } => {
                out.bytes.extend_from_slice(lit);
                if *conv == b'%' {
                    out.bytes.push(b'%');
                    continue;
                }
                let parsed = parse_spec(spec, printf_conv(*conv));

                // The four two-letter directives are dispatched before the
                // one-letter table, because their meaning is entirely in the
                // second letter.
                if *aux != 0 {
                    let ts = match *conv {
                        b'A' => Some(m.atime),
                        b'C' => Some(m.ctime),
                        b'T' => Some(m.mtime),
                        // Birth time is not in our `struct stat`; upstream
                        // renders an unavailable timestamp as nothing at all
                        // while still honouring the surrounding literal text.
                        _ => None,
                    };
                    let text = ts.map_or_else(Vec::new, |t| format_date(t, *aux, zone));
                    out.bytes
                        .extend_from_slice(&cfmt::render(&parsed, cfmt::Value::Text(&text)));
                    continue;
                }

                // A directive whose value is text, quoted if it names a file
                // and the destination is a terminal.
                let mut text: Option<Vec<u8>> = None;
                let mut quoted = false;
                match *conv {
                    b'a' => text = Some(ctime_format(m.atime, zone)),
                    b'c' => text = Some(ctime_format(m.ctime, zone)),
                    b't' => text = Some(ctime_format(m.mtime, zone)),
                    b'b' => text = Some(scaled_ceil(m.blocks, 512, 512).to_string().into_bytes()),
                    b'k' => text = Some(scaled_ceil(m.blocks, 512, 1024).to_string().into_bytes()),
                    b'D' => text = Some(m.dev.to_string().into_bytes()),
                    b'i' => text = Some(m.ino.to_string().into_bytes()),
                    b'n' => text = Some(m.nlink.to_string().into_bytes()),
                    b's' => text = Some(m.size.to_string().into_bytes()),
                    b'U' => text = Some(m.uid.to_string().into_bytes()),
                    b'G' => text = Some(m.gid.to_string().into_bytes()),
                    b'u' => {
                        // Falls through to the number when the name is unknown,
                        // which is why this is not `unwrap_or`-with-a-name.
                        text = Some(
                            tree.user_name(m.uid)
                                .unwrap_or_else(|| m.uid.to_string().into_bytes()),
                        );
                    }
                    b'g' => {
                        text = Some(
                            tree.group_name(m.gid)
                                .unwrap_or_else(|| m.gid.to_string().into_bytes()),
                        );
                    }
                    b'M' => text = Some(modechange::mode_string(m.mode).into_bytes()),
                    b'y' => text = Some(vec![type_letter(m.mode)]),
                    b'Y' => {
                        if m.is_symlink() {
                            match tree.stat(&it.path) {
                                Ok(target) => text = Some(vec![type_letter(target.mode)]),
                                Err(e) => {
                                    let code = e.raw_os_error().unwrap_or(0);
                                    // ENOENT / ENOTDIR / ELOOP, as numbered by
                                    // Linux; anything else is reported.
                                    text = Some(match code {
                                        2 | 20 => b"N".to_vec(),
                                        40 => b"L".to_vec(),
                                        _ => {
                                            out.errs.push((
                                                format!(
                                                    "{}: {}",
                                                    quote::quote(&it.path),
                                                    errmsg::strerror(&e)
                                                ),
                                                false,
                                            ));
                                            b"?".to_vec()
                                        }
                                    });
                                }
                            }
                        } else {
                            text = Some(vec![type_letter(m.mode)]);
                        }
                    }
                    b'F' => {
                        text = Some(tree.fstype(m.dev));
                        quoted = true;
                    }
                    b'p' => {
                        text = Some(it.path.clone());
                        quoted = true;
                    }
                    b'f' => {
                        text = Some(pathname::base_name(&it.path).to_vec());
                        quoted = true;
                    }
                    b'h' => {
                        text = Some(printf_dirname(&it.path));
                        quoted = true;
                    }
                    b'H' => {
                        text = Some(it.path.get(..it.start_len).unwrap_or(b"").to_vec());
                    }
                    b'P' => {
                        let rest = if it.depth > 0 {
                            let mut cp = it.start_len;
                            if it.path.get(cp) == Some(&b'/') {
                                // The start point did not end in a slash, so
                                // the walk added one; step over it.
                                cp = cp.saturating_add(1);
                            }
                            it.path.get(cp..).unwrap_or(b"").to_vec()
                        } else {
                            Vec::new()
                        };
                        text = Some(rest);
                        quoted = true;
                    }
                    b'l' => {
                        let link = if m.is_symlink() {
                            match tree.readlink(&it.path) {
                                Ok(t) => Some(t),
                                Err(e) => {
                                    out.errs.push((
                                        format!(
                                            "{}: {}",
                                            quote::quote(&it.path),
                                            errmsg::strerror(&e)
                                        ),
                                        true,
                                    ));
                                    None
                                }
                            }
                        } else {
                            None
                        };
                        // Still rendered when empty: the width is honoured.
                        text = Some(link.unwrap_or_default());
                        quoted = true;
                    }
                    b'Z' => {
                        // No security contexts on this system, which is the
                        // same answer a Linux build without SELinux gives.
                        out.errs.push((
                            format!(
                                "getfilecon failed: {}: {}",
                                quote::quote(&it.path),
                                "Function not implemented"
                            ),
                            true,
                        ));
                        text = Some(Vec::new());
                    }
                    b'd' => {
                        let depth = i64::try_from(it.depth).unwrap_or(i64::MAX);
                        out.bytes
                            .extend_from_slice(&cfmt::render(&parsed, cfmt::Value::Signed(depth)));
                    }
                    b'm' => {
                        out.bytes.extend_from_slice(&cfmt::render(
                            &parsed,
                            cfmt::Value::Unsigned(u64::from(m.mode & 0o7777)),
                        ));
                    }
                    b'S' => {
                        let sparse = if m.size == 0 {
                            if m.blocks == 0 { 1.0 } else { f64::INFINITY }
                        } else {
                            #[allow(clippy::cast_precision_loss)]
                            {
                                (512.0 * m.blocks as f64) / m.size as f64
                            }
                        };
                        out.bytes.extend_from_slice(&cfmt::render(
                            &parsed,
                            cfmt::Value::Float(extfloat::ExtF80::from_f64(sparse)),
                        ));
                    }
                    // `insert_fprintf` refuses everything else, so this is
                    // unreachable rather than a silent drop.
                    _ => {}
                }
                if let Some(t) = text {
                    let t = if quoted && tty { qmark(&t) } else { t };
                    out.bytes
                        .extend_from_slice(&cfmt::render(&parsed, cfmt::Value::Text(&t)));
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// `-ls` / `-fls`
// ---------------------------------------------------------------------------

/// The column widths `list_file` carries between calls.
///
/// Upstream's are `static` and therefore process-global and sticky: a field
/// that overflows its width widens it *for every later line*, and never
/// narrows again. That is why `-ls` output is not a function of one file — the
/// first line of a listing can be narrower than the last. Reproducing it means
/// carrying the state rather than recomputing per line.
struct LsWidths {
    inode: usize,
    blocks: usize,
    nlink: usize,
    owner: usize,
    group: usize,
    major: usize,
    minor: usize,
    size: usize,
}

impl Default for LsWidths {
    fn default() -> Self {
        Self {
            inode: 9,
            blocks: 6,
            nlink: 3,
            owner: 8,
            group: 8,
            major: 3,
            minor: 3,
            size: 8,
        }
    }
}

/// `%*s`: right-align to `width` bytes, then widen `width` if it did not fit.
fn pad_left(out: &mut Vec<u8>, text: &[u8], width: &mut usize) {
    for _ in text.len()..*width {
        out.push(b' ');
    }
    out.extend_from_slice(text);
    if text.len() > *width {
        *width = text.len();
    }
}

/// `%-*s`: left-align to `width` bytes. Does not widen — upstream widens from
/// the *display* width before the call, not from the byte count after it.
fn pad_right(out: &mut Vec<u8>, text: &[u8], width: usize) {
    out.extend_from_slice(text);
    for _ in text.len()..width {
        out.push(b' ');
    }
}

/// gnulib `mbswidth(s, 0)`: the display width, or `None` where upstream
/// returns -1 — an invalid multibyte sequence or a non-printable character.
///
/// It matters that this is display width and the padding above is *bytes*:
/// upstream widens the column by one and pads by the other, so a user name
/// outside ASCII under-pads. Splitting the two functions is what reproduces
/// that rather than quietly fixing it.
fn mbswidth(s: &[u8]) -> Option<usize> {
    let text = core::str::from_utf8(s).ok()?;
    let mut total = 0usize;
    for c in text.chars() {
        total = total.saturating_add(charwidth::char_width(c)?);
    }
    Some(total)
}

/// `print_name_with_quoting`: the C-escaped rendering `-ls` always uses.
///
/// `list_file` still takes a `literal_control_chars` flag, but 4.10.0 `#if 0`s
/// out the only option that could set it (`-show-control-chars`), so the flag
/// is dead and the quoting is unconditional.
fn ls_quote(name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(name.len());
    for &c in name {
        match c {
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'\n' => out.extend_from_slice(b"\\n"),
            0x08 => out.extend_from_slice(b"\\b"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            0x0c => out.extend_from_slice(b"\\f"),
            b' ' => out.extend_from_slice(b"\\ "),
            b'"' => out.extend_from_slice(b"\\\""),
            _ => {
                if c > 0o40 && c < 0o177 {
                    out.push(c);
                } else {
                    out.extend_from_slice(format!("\\{c:03o}").as_bytes());
                }
            }
        }
    }
    out
}

/// The `-ls` timestamp: a clock for something recent, a year for anything
/// older than about six months or more than an hour into the future.
///
/// The slop into the future is upstream's and is for NFS, whose server and
/// client clocks disagree often enough that a freshly written file would
/// otherwise list with a year on it.
fn ls_time(mtime: Ts, start: Ts, zone: &localtime::Zone) -> Vec<u8> {
    const SIX_MONTHS: i64 = 6 * 30 * 24 * 60 * 60;
    let recent = start.sec.saturating_sub(SIX_MONTHS) <= mtime.sec
        && mtime.sec <= start.sec.saturating_add(3600);
    let fmt: &[u8] = if recent { b"%b %e %H:%M" } else { b"%b %e  %Y" };
    let tm = zone.local(mtime.sec, mtime.nsec);
    let out = localtime::strftime(fmt, &tm);
    if out.is_empty() {
        // The instant has no local representation. Upstream falls back to a
        // 12-column signed second count.
        let mut num = Vec::new();
        if mtime.sec < 0 {
            num.push(b'-');
        }
        num.extend_from_slice(mtime.sec.unsigned_abs().to_string().as_bytes());
        let mut buf = Vec::new();
        let mut width = 12;
        pad_left(&mut buf, &num, &mut width);
        return buf;
    }
    out
}

/// `list_file`: one `-ls` line, plus any diagnostic it produced.
///
/// The diagnostic — a `readlink` that failed on a symlink we are listing — is
/// returned rather than printed, but note that upstream deliberately does
/// *not* let it set the exit status: `list_file` has no way to tell its caller,
/// and the comment in `lib/listfile.c` says so in as many words.
fn render_ls(
    w: &mut LsWidths,
    it: &Item,
    tree: &dyn Tree,
    zone: &localtime::Zone,
    start: Ts,
    block_size: u64,
    literal: bool,
) -> (Vec<u8>, Option<String>) {
    let meta = it.meta();
    let m = &meta;
    let mut out = Vec::new();

    pad_left(&mut out, m.ino.to_string().as_bytes(), &mut w.inode);
    out.push(b' ');
    pad_left(
        &mut out,
        scaled_ceil(m.blocks, 512, block_size)
            .to_string()
            .as_bytes(),
        &mut w.blocks,
    );
    out.push(b' ');

    // `strmode` writes eleven characters: the ten everybody knows plus a
    // trailing space, the POSIX "optional alternate access method flag".
    // `modechange::mode_string` stops at ten because every other caller
    // strips it; `-ls` is the one that wants it, so it is added back here.
    out.extend_from_slice(modechange::mode_string(m.mode).as_bytes());
    out.push(b' ');

    pad_left(&mut out, m.nlink.to_string().as_bytes(), &mut w.nlink);
    out.push(b' ');

    match tree.user_name(m.uid) {
        Some(name) => {
            if let Some(len) = mbswidth(&name)
                && len > w.owner
            {
                w.owner = len;
            }
            pad_right(&mut out, &name, w.owner);
            out.push(b' ');
        }
        None => {
            // The literal 8 is upstream's, and is not `owner_width`: an
            // unknown uid is padded to eight columns however wide the column
            // has grown, and then widens it.
            let num = m.uid.to_string();
            let chars_out = num.len().max(8).saturating_add(1);
            pad_right(&mut out, num.as_bytes(), 8);
            out.push(b' ');
            if chars_out > w.owner {
                w.owner = chars_out;
            }
        }
    }

    match tree.group_name(m.gid) {
        Some(name) => {
            if let Some(len) = mbswidth(&name)
                && len > w.group
            {
                w.group = len;
            }
            pad_right(&mut out, &name, w.group);
            out.push(b' ');
        }
        None => {
            let num = m.gid.to_string();
            let chars_out = num.len().max(w.group);
            pad_right(&mut out, num.as_bytes(), w.group);
            if chars_out > w.group {
                w.group = chars_out;
            }
            out.push(b' ');
        }
    }

    let kind = m.mode & modechange::S_IFMT;
    if kind == modechange::S_IFCHR || kind == modechange::S_IFBLK {
        let (major, minor) = dev_major_minor(m.rdev);
        pad_left(&mut out, major.to_string().as_bytes(), &mut w.major);
        out.extend_from_slice(b", ");
        pad_left(&mut out, minor.to_string().as_bytes(), &mut w.minor);
    } else {
        pad_left(&mut out, m.size.to_string().as_bytes(), &mut w.size);
    }
    out.push(b' ');

    out.extend_from_slice(&ls_time(m.mtime, start, zone));
    out.push(b' ');

    let name = if literal {
        it.path.clone()
    } else {
        ls_quote(&it.path)
    };
    out.extend_from_slice(&name);

    let mut err = None;
    if m.is_symlink() {
        match tree.readlink(&it.path) {
            Ok(target) => {
                out.extend_from_slice(b" -> ");
                let target = if literal { target } else { ls_quote(&target) };
                out.extend_from_slice(&target);
            }
            Err(e) => {
                // Not quoted: upstream's `error (0, errno, "%s", name)` puts
                // the name through a plain `%s`.
                err = Some(format!(
                    "{}: {}",
                    String::from_utf8_lossy(&it.path),
                    errmsg::strerror(&e)
                ));
            }
        }
    }
    out.push(b'\n');
    (out, err)
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

/// Upstream `ts_difference`: `difftime(a.sec, b.sec)` plus the nanosecond
/// remainder, and deliberately not `a.as_f64() - b.as_f64()`. Keeping the two
/// terms apart is what stops the nanoseconds being rounded away by the
/// seconds' magnitude, which is the whole range that matters.
///
/// The seconds are subtracted *as `f64`* rather than as `i64`, which matters
/// only at the extremes: glibc's `difftime` widens both operands before
/// subtracting, so an argument big enough to overflow `time_t` — `-mtime
/// +1e15` — does not wrap round to the opposite answer. It does on platforms
/// whose `difftime` is `(double)(t1 - t2)`, and GNU find there really does
/// report every file as older than 10^15 days.
fn ts_difference(a: Ts, b: Ts) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    {
        (a.sec as f64 - b.sec as f64) + 1.0e-9 * (i64::from(a.nsec) - i64::from(b.nsec)) as f64
    }
}

/// Upstream `compare_ts`. Exact equality is tested on the fields, so two
/// instants a nanosecond apart never compare equal however far from the epoch
/// they are — which a comparison of two `f64` seconds would.
fn compare_ts(a: Ts, b: Ts) -> core::cmp::Ordering {
    if a.sec == b.sec && a.nsec == b.nsec {
        core::cmp::Ordering::Equal
    } else if ts_difference(a, b) < 0.0 {
        core::cmp::Ordering::Less
    } else {
        core::cmp::Ordering::Greater
    }
}

/// Upstream `pred_timewindow`.
///
/// The `Eq` arm is the surprising one and the comment upstream is longer than
/// the code: with the origin a whole day *before* the program started, a file
/// written this instant has a delta of exactly `window`, and one written
/// exactly 24h ago has a delta of 0 — so the interval is half-open at the
/// bottom, and `-mtime 0` is "changed since the same time yesterday".
fn timewindow(ts: Ts, cmp: Comp, origin: Ts, window: f64) -> bool {
    match cmp {
        Comp::Gt => compare_ts(ts, origin) == core::cmp::Ordering::Greater,
        Comp::Lt => compare_ts(ts, origin) == core::cmp::Ordering::Less,
        Comp::Eq => {
            let delta = ts_difference(ts, origin);
            delta > 0.0 && delta <= window
        }
    }
}

/// Everything the evaluator needs that is not the file being tested.
///
/// The predicate list is *not* here: it is lent to [`Ctx::eval`] instead, so
/// that a primary can mutate the context (an `-exec` batch, the exit status)
/// while the expression it came from is still being read.
struct Ctx<'a> {
    sinks: Vec<Sink>,
    /// Per sink: is the destination a terminal? Decides `-print`/`-printf`
    /// quoting, and nothing else.
    sink_tty: Vec<bool>,
    /// Buffered output per sink. Stdout is flushed before any child runs so
    /// that `find . -exec echo x \;` interleaves the way it does upstream.
    sink_buf: Vec<Vec<u8>>,
    execs: Vec<ExecSpec>,
    tree: &'a dyn Tree,
    zone: localtime::Zone,
    start: Ts,
    block_size: u64,
    ls_widths: LsWidths,
    ignore_readdir_race: bool,
    depth_first: bool,
    follow: Follow,
    /// `state.exit_status`.
    status: i32,
    /// `-prune` matched a directory: do not descend into this one.
    stop_at_current_level: bool,
    /// `-quit` ran: unwind out of the walk and exit.
    quit: bool,
}

impl Ctx<'_> {
    /// A diagnostic that also makes the exit status non-zero, which is what
    /// POSIX asks for and what upstream does everywhere except the two places
    /// noted at their call sites.
    fn fail(&mut self, msg: &str) {
        eprintln!("find: {msg}");
        self.status = 1;
    }

    /// `following_links()`: whether the walk dereferenced this item's own name.
    /// Under `-H` that is true only at depth 0, which is why it takes an item
    /// rather than being a field.
    fn following_links(&self, it: &Item) -> bool {
        match self.follow {
            Follow::Always => true,
            Follow::CommandLine => it.depth == 0,
            Follow::Never => false,
        }
    }

    fn write(&mut self, sink: usize, bytes: &[u8]) {
        if let Some(buf) = self.sink_buf.get_mut(sink) {
            buf.extend_from_slice(bytes);
        }
    }

    /// Push every buffer out. Called before running a child, and at the end.
    fn flush(&mut self) {
        for (i, buf) in self.sink_buf.iter_mut().enumerate() {
            if buf.is_empty() {
                continue;
            }
            let res = match self.sinks.get_mut(i) {
                Some(Sink::Stdout) => io::stdout()
                    .write_all(buf)
                    .and_then(|()| io::stdout().flush()),
                Some(Sink::Stderr) => io::stderr().write_all(buf),
                Some(Sink::File(f)) => f.write_all(buf).and_then(|()| f.flush()),
                // Accumulates rather than drains: the point of it is that the
                // bytes are still there when the walk has finished.
                #[cfg(test)]
                Some(Sink::Capture) => continue,
                None => Ok(()),
            };
            buf.clear();
            if let Err(e) = res {
                // Upstream dies here rather than carrying on: a `find` whose
                // output is going nowhere has nothing useful left to do.
                eprintln!("find: {}", errmsg::strerror(&e));
                self.status = 1;
            }
        }
    }

    fn eval(&mut self, nodes: &[Node], e: &Expr, it: &Item) -> bool {
        // `pred_quit` does not return: it calls `cleanup()` and `exit()`, so
        // nothing to its right in the same expression ever runs and no
        // enclosing operator ever sees its value. Exiting the process here
        // would take the unit tests with it, so the flag stands in for the
        // `exit` and this is the unwind — every node above and to the right of
        // the `-quit` folds to false without being applied, which is the whole
        // of what is observable about upstream's `exit`.
        if self.quit {
            return false;
        }
        match e {
            Expr::Prim(i) => match nodes.get(*i).and_then(|n| n.prim.as_ref()) {
                Some(p) => self.apply(p, it),
                // A node with no primary is one of the punctuation nodes, which
                // the tree builder never puts in a leaf position.
                None => true,
            },
            Expr::Not(a) => !self.eval(nodes, a, it),
            Expr::And(a, b) => self.eval(nodes, a, it) && self.eval(nodes, b, it),
            Expr::Or(a, b) => self.eval(nodes, a, it) || self.eval(nodes, b, it),
            // `,` runs both arms and takes the value of the right one. That is
            // the only operator here whose left arm's result is discarded.
            Expr::Comma(a, b) => {
                let _ = self.eval(nodes, a, it);
                self.eval(nodes, b, it)
            }
        }
    }
}

/// Replace every `{}` in `arg` with `prefix` + `target`.
fn substitute(arg: &[u8], prefix: &[u8], target: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(arg.len());
    let mut i = 0usize;
    while i < arg.len() {
        if arg.get(i..i.saturating_add(2)) == Some(b"{}".as_slice()) {
            out.extend_from_slice(prefix);
            out.extend_from_slice(target);
            i = i.saturating_add(2);
        } else {
            if let Some(&b) = arg.get(i) {
                out.push(b);
            }
            i = i.saturating_add(1);
        }
    }
    out
}

impl Ctx<'_> {
    /// Run one command, having first pushed everything buffered so that the
    /// child's output lands after ours rather than in the middle of it.
    fn spawn(&mut self, argv: &[Vec<u8>], cwd: Option<&[u8]>) -> bool {
        self.flush();
        match self.tree.run(argv, cwd) {
            Ok(ok) => ok,
            Err(e) => {
                let name = argv.first().map_or_else(Vec::new, Clone::clone);
                self.fail(&format!(
                    "{}: {}",
                    quote::quote(&name),
                    errmsg::strerror(&e)
                ));
                false
            }
        }
    }

    /// Run the accumulated `+`-form batch of one `-exec`, if it has one.
    fn flush_exec(&mut self, idx: usize) {
        let Some(spec) = self.execs.get_mut(idx) else {
            return;
        };
        if spec.pending.is_empty() {
            return;
        }
        let mut argv = spec.argv.clone();
        argv.append(&mut spec.pending);
        let cwd = spec.pending_dir.take();
        if !self.spawn(&argv, cwd.as_deref()) {
            // A failing `+` batch cannot report through the predicate's value
            // — the batch runs long after the predicate returned true — so the
            // only channel left is the exit status, which is what upstream
            // uses too.
            self.status = 1;
        }
    }

    /// Flush every `-execdir ... +` batch. Called when the walk leaves a
    /// directory, since the names in such a batch are relative to it.
    fn flush_execdirs(&mut self) {
        for idx in 0..self.execs.len() {
            if self.execs.get(idx).is_some_and(|e| e.dir_relative) {
                self.flush_exec(idx);
            }
        }
    }

    fn flush_all_execs(&mut self) {
        for idx in 0..self.execs.len() {
            self.flush_exec(idx);
        }
    }

    fn do_exec(&mut self, idx: usize, it: &Item) -> bool {
        // Everything the borrow of `self.execs` is needed for is taken here in
        // one go, because the prompt and the batch flush below both want
        // `self` mutably.
        let Some((confirm, dir_relative, multiple, prog)) = self.execs.get(idx).map(|s| {
            (
                s.confirm,
                s.dir_relative,
                s.multiple,
                s.argv.first().cloned().unwrap_or_default(),
            )
        }) else {
            return false;
        };

        // `-execdir` names the file relative to the directory holding it, and
        // prefixes `./` so that a name beginning with a dash cannot be read as
        // an option by the child.
        let (prefix, target, cwd): (&[u8], Vec<u8>, Option<Vec<u8>>) = if dir_relative {
            let base = pathname::base_name(&it.rel).to_vec();
            let prefix: &[u8] = if base.first() == Some(&b'/') {
                b""
            } else {
                b"./"
            };
            (prefix, base, it.dir.clone())
        } else {
            (b"", it.path.clone(), None)
        };

        if confirm {
            self.flush();
            eprint!(
                "< {} ... {} > ? ",
                String::from_utf8_lossy(&prog),
                String::from_utf8_lossy(&it.path)
            );
            let _ = io::stderr().flush();
            if !self.tree.confirm() {
                return false;
            }
        }

        if multiple {
            // A batch that has moved on to a new directory has to run before
            // the new name joins it, or the two sets of relative names would
            // be interpreted against one directory.
            if dir_relative && self.execs.get(idx).is_some_and(|e| e.pending_dir != cwd) {
                self.flush_exec(idx);
            }
            if let Some(spec) = self.execs.get_mut(idx) {
                let mut name = prefix.to_vec();
                name.extend_from_slice(&target);
                spec.pending.push(name);
                spec.pending_dir = cwd;
            }
            // POSIX: the `+` form always evaluates true.
            return true;
        }

        let argv: Vec<Vec<u8>> = self.execs.get(idx).map_or_else(Vec::new, |spec| {
            spec.argv
                .iter()
                .map(|a| substitute(a, prefix, &target))
                .collect()
        });
        self.spawn(&argv, cwd.as_deref())
    }

    #[allow(clippy::too_many_lines)]
    fn apply(&mut self, p: &Prim, it: &Item) -> bool {
        // `apply_predicate` calls `get_info` before every predicate, and a
        // predicate whose `stat` could not be taken is not run at all — it is
        // simply false. That is why `find d -type f` still lists the regular
        // files in a directory it cannot search, while `find d -size 1` lists
        // none of them and reports each failure instead.
        if self.get_info(prim_need(p), it).is_err() {
            return false;
        }
        let m = it.meta();
        match p {
            Prim::True | Prim::Noop => true,
            Prim::Prune => {
                // `-prune` is a no-op under `-depth`, because by then the
                // directory's contents have already been visited. It still
                // evaluates true: POSIX requires that.
                if !self.depth_first && m.is_dir() {
                    self.stop_at_current_level = true;
                }
                true
            }
            Prim::Quit => {
                self.quit = true;
                true
            }
            Prim::False => false,
            Prim::Name { pat, ci } => {
                let base = pathname::base_name(&it.path);
                fnmatch::fnmatch(pat, base, ci_flag(*ci))
            }
            Prim::Path { pat, ci } => fnmatch::fnmatch(pat, &it.path, ci_flag(*ci)),
            Prim::LName { pat, ci } => {
                if !m.is_symlink() {
                    return false;
                }
                match self.tree.readlink(&it.path) {
                    Ok(target) => fnmatch::fnmatch(pat, &target, ci_flag(*ci)),
                    Err(e) => {
                        self.fail(&format!(
                            "{}: {}",
                            quote::quote(&it.path),
                            errmsg::strerror(&e)
                        ));
                        false
                    }
                }
            }
            Prim::Regex(re) => {
                // `re_match` anchors at the start and must consume the whole
                // name, which is why this is not a search.
                matches!(re.find(&it.path), Ok(Some((0, end))) if end == it.path.len())
            }
            Prim::Type(letters) => letters.contains(&type_letter(m.mode)),
            Prim::XType(letters) => self.xtype(letters, it),
            Prim::Size { cmp, n, unit } => {
                let blocks = m.size.div_ceil(*unit);
                NumCmp { cmp: *cmp, n: *n }.test(blocks)
            }
            Prim::Perm {
                kind,
                file_mode,
                dir_mode,
            } => {
                let want = if m.is_dir() { *dir_mode } else { *file_mode };
                match kind {
                    PermKind::AtLeast => m.mode & want == want,
                    // An all-zero mask is true for everything: Savannah 14748,
                    // where the previous answer of `false` made `-perm /000`
                    // useless rather than merely odd.
                    PermKind::Any => want == 0 || m.mode & want != 0,
                    PermKind::Exact => m.mode & 0o7777 == want,
                }
            }
            Prim::Empty => self.empty(it),
            Prim::Links(c) => c.test(m.nlink),
            Prim::Inum(c) => c.test(m.ino),
            Prim::Uid(c) => c.test(u64::from(m.uid)),
            Prim::Gid(c) => c.test(u64::from(m.gid)),
            Prim::User(uid) => m.uid == *uid,
            Prim::Group(gid) => m.gid == *gid,
            Prim::NoUser => self.tree.user_name(m.uid).is_none(),
            Prim::NoGroup => self.tree.group_name(m.gid).is_none(),
            Prim::SameFile { dev, ino } => m.dev == *dev && m.ino == *ino,
            Prim::TimeWindow {
                field,
                cmp,
                origin,
                window,
            } => match field {
                Some(f) => match self.field_time(*f, m) {
                    Some(ts) => timewindow(ts, *cmp, *origin, *window),
                    None => false,
                },
                // `-used`: the gap between last access and last status change.
                // Always false when the file was accessed before it changed,
                // because a negative age is not an age.
                None => {
                    if compare_ts(m.atime, m.ctime) == core::cmp::Ordering::Less {
                        return false;
                    }
                    let mut sec = m.ctime.sec.saturating_sub(m.atime.sec);
                    let mut nsec = i64::from(m.ctime.nsec) - i64::from(m.atime.nsec);
                    if nsec < 0 {
                        nsec += 1_000_000_000;
                        sec = sec.saturating_sub(1);
                    }
                    let delta = Ts {
                        sec,
                        nsec: u32::try_from(nsec).unwrap_or(0),
                    };
                    timewindow(delta, *cmp, *origin, *window)
                }
            },
            Prim::Newer { field, ts } => match self.field_time(*field, m) {
                Some(f) => compare_ts(f, *ts) == core::cmp::Ordering::Greater,
                None => false,
            },
            Prim::FsType(want) => self.tree.fstype(m.dev) == *want,
            Prim::Access(mode) => self.tree.access(&it.path, *mode),
            Prim::Print { sink, terminator } => {
                let tty = self.sink_tty.get(*sink).copied().unwrap_or(false);
                // `-print0` never quotes: the whole point of a NUL terminator
                // is that the name travels unaltered.
                let mut line = if *terminator == 0 || !tty {
                    it.path.clone()
                } else {
                    qmark(&it.path)
                };
                line.push(*terminator);
                self.write(*sink, &line);
                true
            }
            Prim::Printf { sink, segs } => {
                let tty = self.sink_tty.get(*sink).copied().unwrap_or(false);
                let r = render_printf(segs, it, self.tree, &self.zone, tty);
                self.write(*sink, &r.bytes);
                if r.stop {
                    self.flush();
                }
                for (msg, sets_status) in r.errs {
                    if sets_status {
                        self.fail(&msg);
                    } else {
                        eprintln!("find: {msg}");
                    }
                }
                true
            }
            Prim::Ls { sink } => {
                let (line, err) = render_ls(
                    &mut self.ls_widths,
                    it,
                    self.tree,
                    &self.zone,
                    self.start,
                    self.block_size,
                    // `options.literal_control_chars`, which nothing can set in
                    // 4.10.0 — see `ls_quote`.
                    false,
                );
                self.write(*sink, &line);
                if let Some(msg) = err {
                    // Not `fail`: `list_file` has no way to tell its caller,
                    // so upstream leaves the exit status alone here.
                    eprintln!("find: {msg}");
                }
                true
            }
            Prim::Delete => self.delete(it),
            Prim::Exec(idx) => self.do_exec(*idx, it),
        }
    }

    fn field_time(&self, field: TimeField, m: Meta) -> Option<Ts> {
        match field {
            TimeField::Access => Some(m.atime),
            TimeField::Modify => Some(m.mtime),
            TimeField::Change => Some(m.ctime),
            // No birth time in our `stat`. Upstream also answers false when
            // the filesystem does not record one, so this is the same shape of
            // answer rather than a new one.
            TimeField::Birth => None,
        }
    }

    /// `-xtype`: the type of the *other* end of the link from the one `-type`
    /// would have looked at.
    /// `optionl_stat`: the `-L` stat, which is `stat` with `fallback_stat`
    /// behind it — on `ENOENT` or `ENOTDIR` it retries without following, so a
    /// dangling symlink comes back as a symlink rather than as an error. That
    /// fallback, not anything in `pred_xtype`, is why `find . -xtype l` lists a
    /// broken link with exit status 0.
    fn optionl_stat(&self, path: &[u8]) -> io::Result<Meta> {
        match self.tree.stat(path) {
            Ok(m) => Ok(m),
            // ENOENT / ENOTDIR as Linux numbers them. Every other errno —
            // EACCES, EIO, ELOOP, ENAMETOOLONG, EOVERFLOW — is returned as-is.
            Err(e) if matches!(e.raw_os_error(), Some(2 | 20)) => self.tree.lstat(path),
            Err(e) => Err(e),
        }
    }

    /// `options.xstat` — the one of the three stat functions the `-H`/`-L`/`-P`
    /// choice selected, applied to a file at a given depth.
    ///
    /// `optionh_stat` is the reason this takes a depth: under `-H` the start
    /// points are followed and everything below them is not, so the same
    /// function is `optionl_stat` at depth 0 and `optionp_stat` beneath.
    fn xstat(&self, path: &[u8], depth: usize) -> io::Result<Meta> {
        match self.follow {
            Follow::Always => self.optionl_stat(path),
            Follow::CommandLine if depth == 0 => self.optionl_stat(path),
            Follow::CommandLine | Follow::Never => self.tree.lstat(path),
        }
    }

    /// [`Self::xstat`] plus the mode-0000 complaint.
    ///
    /// The complaint belongs here rather than in [`Self::statinfo`] because
    /// upstream makes it in *two* places — `consider_visiting`, for the stats
    /// `fts` took of its own accord, and `get_statinfo`, for the ones a
    /// predicate asked for — and the two must word it identically.
    ///
    /// Savannah bug #16378: a mode of zero is indistinguishable from "we have
    /// no mode", so every `S_ISREG`-style test below would silently answer
    /// false. Upstream refuses to guess and says so instead.
    fn stat_at(&mut self, path: &[u8], depth: usize) -> io::Result<Meta> {
        let m = self.xstat(path, depth)?;
        if m.mode == 0 {
            let msg = format!(
                "WARNING: file {} appears to have mode 0000",
                quote::quote(path)
            );
            self.fail(&msg);
        }
        Ok(m)
    }

    /// `get_statinfo`: take the `stat` and remember it, or report the failure
    /// once and remember *that*.
    fn statinfo(&mut self, it: &Item) -> Result<(), ()> {
        match self.stat_at(&it.path, it.depth) {
            Ok(m) => {
                it.stat.set(Some(m));
                Ok(())
            }
            Err(e) => {
                // `-ignore_readdir_race` covers exactly this: a name `readdir`
                // handed us for a file that was gone by the time we asked
                // about it. Only `ENOENT` qualifies — a race cannot produce
                // `EACCES`.
                let raced = self.ignore_readdir_race && e.raw_os_error() == Some(2);
                if !raced && !it.reported.get() {
                    let msg = format!("{}: {}", quote::quote(&it.path), errmsg::strerror(&e));
                    self.fail(&msg);
                }
                it.reported.set(true);
                Err(())
            }
        }
    }

    /// `get_info`: the decision *whether* to `stat`, which is the whole of
    /// what `FTS_NOSTAT` buys and the reason `Item` carries a `d_type` at all.
    fn get_info(&mut self, need: Need, it: &Item) -> Result<(), ()> {
        if it.stat.get().is_some() {
            return Ok(());
        }
        let todo = match need {
            Need::Nothing => false,
            // `d_type` answers this one, when `readdir` supplied it.
            Need::Type => it.type_mode == 0,
            // Upstream can sometimes use `d_ino`, but distrusts it for
            // directories — a mount point's `d_ino` belongs to the covered
            // directory, not the covering one. `std::fs::DirEntry` does not
            // expose `d_ino` portably, so this always stats, which is the
            // conservative half of the same rule.
            Need::Inum | Need::Stat => true,
        };
        if todo { self.statinfo(it) } else { Ok(()) }
    }

    fn xtype(&mut self, letters: &[u8], it: &Item) -> bool {
        // `-xtype` asks the *other* question from `-type`: if the walk would
        // have followed the link, look at the link; if it would not, look
        // through it. `following_links()` is not a constant — under `-H` it is
        // true only for a start point.
        let following = self.following_links(it);
        let probe = if following {
            self.tree.lstat(&it.path)
        } else {
            self.optionl_stat(&it.path)
        };
        match probe {
            Ok(meta) => letters.contains(&type_letter(meta.mode)),
            Err(e) => {
                // Mimics `ls -lL`. Reachable only under `-L`/`-H`, where the
                // probe is a bare `lstat` and so has no fallback of its own.
                if following && e.raw_os_error() == Some(2) {
                    return letters.contains(&type_letter(it.meta().mode));
                }
                self.fail(&format!(
                    "{}: {}",
                    quote::quote(&it.path),
                    errmsg::strerror(&e)
                ));
                false
            }
        }
    }

    fn empty(&mut self, it: &Item) -> bool {
        let meta = it.meta();
        if meta.is_dir() {
            return match self.tree.read_dir(&it.path) {
                Ok(entries) => entries.is_empty(),
                Err(e) => {
                    self.fail(&format!(
                        "{}: {}",
                        quote::quote(&it.path),
                        errmsg::strerror(&e)
                    ));
                    false
                }
            };
        }
        if meta.is_reg() {
            return meta.size == 0;
        }
        false
    }

    fn delete(&mut self, it: &Item) -> bool {
        // `find . -delete` does not delete `.`; upstream tests the *relative*
        // name, so a start point spelled `.` is spared and one spelled `./.`
        // is not.
        if it.rel == b"." {
            return true;
        }
        // `state.have_stat && S_ISDIR(...)`, and not `it.meta().is_dir()`:
        // `-delete` declares that it needs no `stat`, so unless some earlier
        // predicate took one this is `unlink` first and `rmdir` only after the
        // `EISDIR` below. The `d_type` we may have is deliberately not
        // consulted, which is upstream's choice and not an oversight — it is
        // what makes the `EISDIR` retry reachable at all.
        let res = if it.stat.get().is_some_and(|m| m.is_dir()) {
            self.tree.remove_dir(&it.path)
        } else {
            self.tree.remove_file(&it.path)
        };
        let e = match res {
            Ok(()) => return true,
            Err(e) => e,
        };
        match e.raw_os_error() {
            // ENOENT on a file that vanished under us, when the caller has
            // said that is expected.
            Some(2) if self.ignore_readdir_race => return true,
            // EISDIR: `unlink` was the wrong call. Only reachable when the
            // type we had was stale, so retry rather than report.
            Some(21) if self.tree.remove_dir(&it.path).is_ok() => return true,
            _ => {}
        }
        self.fail(&format!(
            "cannot delete {}: {}",
            quote::quote(&it.path),
            errmsg::strerror(&e)
        ));
        false
    }
}

/// `-iname` and friends differ from their case-sensitive twins by exactly one
/// `fnmatch` flag.
fn ci_flag(ci: bool) -> fnmatch::Flags {
    if ci {
        fnmatch::Flags::CASEFOLD
    } else {
        fnmatch::Flags::NONE
    }
}

// ---------------------------------------------------------------------------
// The walk
// ---------------------------------------------------------------------------

/// `ftsfind.c`, which is `find`'s traversal: `fts` plus `consider_visiting`.
///
/// Written as a recursion rather than as a port of `fts` itself, because the
/// only parts of `fts` that are visible from outside are the order entries
/// come back in (`readdir` order, depth first) and the handful of `fts_info`
/// codes `consider_visiting` switches on. Each of those codes is a branch
/// below, named where it arises.
struct Walk<'a, 'b> {
    ctx: Ctx<'a>,
    nodes: &'b [Node],
    expr: &'b Expr,
    max_depth: Option<usize>,
    min_depth: usize,
    xdev: bool,
    /// `sp->fts_dev`: the device of the start point being walked. `-xdev` is
    /// measured against *this*, not against the containing directory, so a
    /// start point that is itself on another filesystem is searched normally.
    root_dev: u64,
    /// The active-directory set `FTS_TIGHT_CYCLE_CHECK` keeps: every directory
    /// on the path from the start point, by `(dev, ino)`, with the name it was
    /// reached by. A directory whose pair is already in here is `FTS_DC`.
    active: Vec<(u64, u64, Vec<u8>)>,
}

impl Walk<'_, '_> {
    /// `issue_loop_warning`.
    ///
    /// The first message is not reachable through `fts`: it tests
    /// `S_ISLNK(ent->fts_statp->st_mode)`, and the only walk that descends
    /// through a symlink is `-L`, under which `fts_statp` holds the *followed*
    /// stat and so never says `S_IFLNK`. It is transcribed anyway, and tested
    /// the same way, because the condition is upstream's and not ours to
    /// simplify away.
    fn issue_loop_warning(&mut self, it: &Item, ancestor: &[u8]) {
        let msg = if it.meta().is_symlink() {
            format!(
                "Symbolic link {} is part of a loop in the directory hierarchy; \
                 we have already visited the directory to which it points.",
                quote::quote(&it.path)
            )
        } else {
            format!(
                "File system loop detected; {} is part of the same file system loop as {}.",
                quote::quote(&it.path),
                quote::quote(ancestor)
            )
        };
        self.ctx.fail(&msg);
    }

    /// `visit`, guarded by `consider_visiting`'s `ignore` rules.
    ///
    /// The `-maxdepth` half of those rules is not here: upstream sets
    /// `ignore` when `fts_level > options.maxdepth`, which a walk that stops
    /// descending at the limit can never produce. Upstream cannot rely on that
    /// either — `fts_set(FTS_SKIP)` is advisory and only takes effect on the
    /// next `fts_read` — so the test is defensive there and absent here.
    fn visit(&mut self, it: &Item) {
        if it.depth < self.min_depth || self.ctx.quit {
            return;
        }
        // `state.already_issued_stat_error_msg = false` at the top of the
        // `fts_read` loop: the "one message per file" rule is really one per
        // *visit*, so a directory seen both ways under `-depth` may complain
        // twice.
        it.reported.set(false);
        let _ = self.ctx.eval(self.nodes, self.expr, it);
    }

    /// One entry, from `consider_visiting` through to the descent `fts_read`
    /// would have driven on the next call.
    fn node(&mut self, it: &Item) {
        if self.ctx.quit {
            return;
        }
        // `isdir`. For anything below a start point this is `d_type` unless a
        // directory forced the `stat` below, which is the whole of what
        // `FTS_NOSTAT` changes.
        if it.type_mode & modechange::S_IFMT != modechange::S_IFDIR {
            self.visit(it);
            return;
        }

        if !self.ctx.depth_first {
            self.visit(it);
        }

        // `fts_set(p, ent, FTS_SKIP)` — from `-prune`, from `-maxdepth`, or
        // from `FTS_XDEV`. The `-prune` flag is read *after* the preorder
        // visit because that is the visit that can set it.
        let pruned = self.ctx.stop_at_current_level;
        self.ctx.stop_at_current_level = false;
        // `FTS_XDEV` is checked on the *child* side upstream; here it is the
        // same question asked one level earlier, so it reads as "this directory
        // is on another filesystem than the start point".
        let crossed = self.xdev && it.meta().dev != self.root_dev;
        let descend =
            !pruned && !self.ctx.quit && !crossed && self.max_depth.is_none_or(|md| it.depth < md);

        if descend {
            match self.ctx.tree.read_dir(&it.path) {
                Ok(entries) => self.children(it, entries),
                Err(e) => {
                    // `FTS_DNR`. The diagnostic comes first either way; what
                    // changes is that without `-depth` the entry has already
                    // had its one visit, whereas with `-depth` this *is* it —
                    // a directory that could not be opened gets no `FTS_DP`.
                    let msg = format!("{}: {}", quote::quote(&it.path), errmsg::strerror(&e));
                    self.ctx.fail(&msg);
                    if self.ctx.depth_first {
                        self.visit(it);
                    }
                    return;
                }
            }
        }

        if self.ctx.depth_first {
            self.visit(it);
        }
    }

    /// The children of one directory, in `readdir` order.
    fn children(&mut self, parent: &Item, entries: Vec<(Vec<u8>, u32)>) {
        // `enter_dir`: this directory joins the active set for as long as we
        // are inside it, which is what makes a descendant that points back at
        // it detectable as `FTS_DC`.
        let pm = parent.meta();
        self.active.push((pm.dev, pm.ino, parent.path.clone()));
        // A level change, which is upstream's trigger for running any
        // `-execdir ... +` batch: the names in it are relative to the
        // directory we are leaving and mean nothing in the one we are
        // entering.
        self.ctx.flush_execdirs();

        for (name, dtype) in entries {
            if self.ctx.quit {
                break;
            }
            self.child(parent, &name, dtype);
        }

        self.ctx.flush_execdirs();
        self.active.pop();
        // `state.stop_at_current_level = false` on `FTS_DP`: a `-prune` inside
        // this directory does not carry out of it.
        self.ctx.stop_at_current_level = false;
    }

    /// One name from a `readdir`, turned into an [`Item`] and handed to
    /// [`Walk::node`].
    fn child(&mut self, parent: &Item, name: &[u8], dtype: u32) {
        let mut path = parent.path.clone();
        // `NAPPEND`: `fts` drops one trailing slash before joining, so
        // `find dir/` prints `dir/f` rather than `dir//f`.
        if path.last() != Some(&b'/') {
            path.push(b'/');
        }
        path.extend_from_slice(name);
        let depth = parent.depth.saturating_add(1);

        // `skip_stat` in `fts_build`, negated: we must `stat` when `readdir`
        // did not say what this is, when it says a directory (the device and
        // inode are needed for the cycle check and for `-xdev`), and — under
        // `-L` only — when it says a symlink, since the type that matters then
        // is the target's.
        let must_stat = dtype == 0
            || dtype == modechange::S_IFDIR
            || (self.ctx.follow == Follow::Always && dtype == modechange::S_IFLNK);

        let mut it = Item {
            path,
            start_len: parent.start_len,
            depth,
            rel: name.to_vec(),
            dir: Some(parent.path.clone()),
            type_mode: dtype,
            stat: std::cell::Cell::new(None),
            reported: std::cell::Cell::new(false),
        };

        if !must_stat {
            self.node(&it);
            return;
        }

        match self.ctx.stat_at(&it.path, depth) {
            Ok(m) => {
                it.stat.set(Some(m));
                it.type_mode = m.mode & modechange::S_IFMT;
                if it.meta().is_dir()
                    && let Some((_, _, anc)) = self
                        .active
                        .iter()
                        .find(|(d, i, _)| *d == m.dev && *i == m.ino)
                {
                    // `FTS_DC`: `enter_dir` found this `(dev, ino)` already in
                    // the active set. The entry is diagnosed and then dropped
                    // entirely — no visit, in either order.
                    let anc = anc.clone();
                    self.issue_loop_warning(&it, &anc);
                    return;
                }
                self.node(&it);
            }
            Err(e) => {
                let msg = format!("{}: {}", quote::quote(&it.path), errmsg::strerror(&e));
                self.ctx.fail(&msg);
                if self.ctx.follow == Follow::Always && e.raw_os_error() == Some(40) {
                    // `FTS_SLNONE` or `FTS_NS` where `symlink_loop()` says
                    // yes: `-L` walked into `ln -s a b; ln -s b a`. Reported
                    // as `ELOOP` and skipped entirely.
                    return;
                }
                // `FTS_NS` below a start point: upstream reports the failure
                // and carries on with no type at all, on the grounds that a
                // name without stat information beats losing the name — and,
                // for a directory, beats silently not searching it.
                it.type_mode = 0;
                it.reported.set(true);
                self.node(&it);
            }
        }
    }

    /// Upstream `find (char *arg)`: one start point, `fts_open` to
    /// `fts_close`.
    fn start(&mut self, arg: &[u8]) {
        self.active.clear();
        // `FTS_COMFOLLOW` is set for both `-L` and `-H`, which is exactly what
        // `xstat` at depth 0 already does.
        let meta = match self.ctx.stat_at(arg, 0) {
            Ok(m) => m,
            Err(e) => {
                // `FTS_NS` at `fts_level == 0` — a nonexistent start point.
                // `symlink_loop()` is not consulted here; upstream returns on
                // the level-0 branch before reaching it, so `find -L a` where
                // `a` is a symlink loop reports `ELOOP` because that is what
                // the failed `stat` said, not because anything checked.
                let msg = format!("{}: {}", quote::quote(arg), errmsg::strerror(&e));
                self.ctx.fail(&msg);
                return;
            }
        };
        self.root_dev = meta.dev;
        let it = Item {
            path: arg.to_vec(),
            start_len: arg.len(),
            depth: 0,
            rel: arg.to_vec(),
            dir: None,
            type_mode: meta.mode & modechange::S_IFMT,
            stat: std::cell::Cell::new(Some(meta)),
            reported: std::cell::Cell::new(false),
        };
        self.node(&it);
        // Leaving the last level of this start point.
        self.ctx.flush_execdirs();
    }
}

// ---------------------------------------------------------------------------
// The two texts
// ---------------------------------------------------------------------------

/// `usage`'s body, transcribed from 4.9.0 rather than regenerated.
///
/// It lists predicates this port refuses (`-context`) and one it does not
/// implement (`-D`'s tracing), which is deliberate: the text is part of the
/// output being matched, and a `find --help` that differs from `find --help`
/// would be the first thing the differential harness reported. The refusals
/// are documented where they are made, not by editing the manual out from
/// under the reader.
const HELP: &str = "\
Usage: find [-H] [-L] [-P] [-Olevel] [-D debugopts] [path...] [expression]

Default path is the current directory; default expression is -print.
Expression may consist of: operators, options, tests, and actions.

Operators (decreasing precedence; -and is implicit where no others are given):
      ( EXPR )   ! EXPR   -not EXPR   EXPR1 -a EXPR2   EXPR1 -and EXPR2
      EXPR1 -o EXPR2   EXPR1 -or EXPR2   EXPR1 , EXPR2

Positional options (always true):
      -daystart -follow -nowarn -regextype -warn

Normal options (always true, specified before other expressions):
      -depth -files0-from FILE -maxdepth LEVELS -mindepth LEVELS
       -mount -noleaf -xdev -ignore_readdir_race -noignore_readdir_race

Tests (N can be +N or -N or N):
      -amin N -anewer FILE -atime N -cmin N -cnewer FILE -context CONTEXT
      -ctime N -empty -false -fstype TYPE -gid N -group NAME -ilname PATTERN
      -iname PATTERN -inum N -iwholename PATTERN -iregex PATTERN
      -links N -lname PATTERN -mmin N -mtime N -name PATTERN -newer FILE
      -nouser -nogroup -path PATTERN -perm [-/]MODE -regex PATTERN
      -readable -writable -executable
      -wholename PATTERN -size N[bcwkMG] -true -type [bcdpflsD] -uid N
      -used N -user NAME -xtype [bcdpfls]

Actions:
      -delete -print0 -printf FORMAT -fprintf FILE FORMAT -print\x20
      -fprint0 FILE -fprint FILE -ls -fls FILE -prune -quit
      -exec COMMAND ; -exec COMMAND {} + -ok COMMAND ;
      -execdir COMMAND ; -execdir COMMAND {} + -okdir COMMAND ;

Other common options:
      --help                   display this help and exit
      --version                output version information and exit

";

/// The tail of `usage`, which is the same list `-D help` prints followed by
/// the bug-reporting block.
const HELP_TAIL: &str = "\
Use '-D help' for a description of the options, or see find(1)

Please see also the documentation at https://www.gnu.org/software/findutils/.
You can report (and track progress on fixing) bugs in the \"find\"
program via the GNU findutils bug-reporting page at
https://savannah.gnu.org/bugs/?group=findutils or, if
you have no web access, by sending email to <bug-findutils@gnu.org>.
";

/// `--version`.
///
/// The `Features enabled:` line is transcribed rather than derived. Three of
/// the four claims are true of this port — `d_type` is read, `O_NOFOLLOW` is
/// available, and the child-order optimisation level is the same 2 that is
/// then discarded. `LEAF_OPTIMISATION` is not implemented here; it is a
/// promise about how many `stat` calls happen, which nothing observable
/// depends on, and the line is matched rather than corrected for the same
/// reason [`HELP`] is. Recorded in `known-issues.md`.
const VERSION: &str = "\
find (GNU findutils) 4.9.0
Copyright (C) 2022 Free Software Foundation, Inc.
License GPLv3+: GNU GPL version 3 or later <https://gnu.org/licenses/gpl.html>.
This is free software: you are free to change and redistribute it.
There is NO WARRANTY, to the extent permitted by law.

Written by Eric B. Decker, James Youngman, and Kevin Dalley.
Features enabled: D_TYPE O_NOFOLLOW(enabled) LEAF_OPTIMISATION FTS(FTS_CWDFD) CBO(level=2) \n";

/// `show_valid_debug_options`, which appears twice: on its own for `-D help`
/// and embedded in [`HELP`] between the option list and the tail.
fn debug_option_list(verbose: bool) -> String {
    let mut out = String::from("Valid arguments for -D:\n");
    if verbose {
        for (name, desc) in DEBUG_FLAGS {
            out.push_str(&format!("{name:<10} {desc}\n"));
        }
    } else {
        let names: Vec<&str> = DEBUG_FLAGS.iter().map(|(n, _)| *n).collect();
        out.push_str(&names.join(", "));
        out.push('\n');
    }
    out
}

/// `usage(stdout)` — the whole of `--help`.
fn help_text() -> String {
    format!("{HELP}{}{HELP_TAIL}", debug_option_list(false))
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

/// Print a [`Fatal`]'s lines, each with the `find: ` prefix `error(0, 0, …)`
/// supplies.
fn report(f: &Fatal) {
    for line in &f.0 {
        eprintln!("find: {line}");
    }
}

/// Point every stdout sink at a buffer instead. A no-op outside the tests.
#[cfg(test)]
fn divert_stdout(sinks: &mut [Sink]) {
    for s in sinks {
        if matches!(s, Sink::Stdout) {
            *s = Sink::Capture;
        }
    }
}

#[cfg(not(test))]
#[allow(clippy::missing_const_for_fn)]
fn divert_stdout(_sinks: &mut [Sink]) {}

/// Collect what [`divert_stdout`]'s sinks kept. A no-op outside the tests.
///
/// Every diverted sink is concatenated into the one buffer, for the reason the
/// real ones share one `FILE *`: `-print -fprint /dev/stdout` interleaves.
#[cfg(test)]
fn harvest(ctx: &Ctx<'_>, out: &mut Vec<u8>) {
    for (i, buf) in ctx.sink_buf.iter().enumerate() {
        if matches!(ctx.sinks.get(i), Some(Sink::Capture)) {
            out.extend_from_slice(buf);
        }
    }
}

#[cfg(not(test))]
#[allow(clippy::missing_const_for_fn)]
fn harvest(_ctx: &Ctx<'_>, _out: &mut Vec<u8>) {}

/// Everything after `main` has turned the environment into bytes: taken as a
/// function so the unit tests can drive it against a `FakeTree`.
fn run(argv: &[Vec<u8>], tree: &dyn Tree) -> i32 {
    run_inner(argv, tree, &mut None)
}

/// [`run`], with somewhere to put the bytes `-print` would have sent to stdout.
///
/// `capture` is `None` for the real program and `Some` under test; when it is
/// `Some` the stdout sink is swapped for [`Sink::Capture`] and its buffer is
/// moved out at the end. Nothing else differs, deliberately: a test that took a
/// different path through the parser would not be testing this program.
#[allow(clippy::too_many_lines)]
fn run_inner(argv: &[Vec<u8>], tree: &dyn Tree, capture: &mut Option<Vec<u8>>) -> i32 {
    let mut follow = Follow::Never;
    let leading = match process_leading_options(argv, &mut follow) {
        Ok(i) => i,
        Err(Leading::Usage(msg)) => {
            eprintln!("find: {msg}");
            eprintln!("Try 'find --help' for more information.");
            return 1;
        }
        Err(Leading::Die(msg)) => {
            eprintln!("find: {msg}");
            return 1;
        }
        Err(Leading::DebugHelp) => {
            print!("{}", debug_option_list(true));
            return 0;
        }
    };

    // `build_expression_tree`'s first act: skip the start points, which are
    // every remaining argument up to the first that `looks_like_expression`
    // recognises *in leading position* — a laxer test than the one applied
    // inside the expression, so that `find - -name f` searches a file called
    // `-` and `find . ) -print` treats `)` as a path.
    let rest = argv.get(leading..).unwrap_or_default();
    let n_start = rest
        .iter()
        .position(|a| looks_like_expression(a, true))
        .unwrap_or(rest.len());
    let start_points = rest.get(..n_start).unwrap_or_default();
    let expr_args = rest.get(n_start..).unwrap_or_default();

    let mut parser = Parser::new(expr_args, tree, follow);
    let parsed = parser.parse_expression();
    // The parser's warnings are emitted in the order they were raised, and
    // before whatever the parse produced — which is where upstream emits them,
    // since it prints each one at the moment it decides on it and nothing else
    // has reached the output yet.
    for w in &parser.warnings {
        eprintln!("find: {w}");
    }
    match parsed {
        Ok(None) => {}
        Ok(Some(Halt::Help)) => {
            print!("{}", help_text());
            return 0;
        }
        Ok(Some(Halt::Version)) => {
            print!("{VERSION}");
            return 0;
        }
        Err(f) => {
            report(&f);
            return 1;
        }
    }

    let expr = match build_tree(&parser.nodes) {
        Ok(e) => e,
        Err(f) => {
            report(&f);
            return 1;
        }
    };

    let mut sinks = parser.sinks;
    if capture.is_some() {
        divert_stdout(&mut sinks);
    }
    let sink_tty = sinks
        .iter()
        .map(|s| match s {
            Sink::Stdout => std::io::IsTerminal::is_terminal(&io::stdout()),
            Sink::Stderr => std::io::IsTerminal::is_terminal(&io::stderr()),
            // `stream_is_tty` would answer for a `-fprint /dev/tty` too. It is
            // answered false here because [`Parser::sink`] creates the file
            // with `File::create`, and a port that opened `/dev/tty` for
            // truncation would have bigger problems than its quoting.
            Sink::File(_) => false,
            // The tests are not a terminal, and must not be: quoting that
            // depended on where the harness was run from would be untestable.
            #[cfg(test)]
            Sink::Capture => false,
        })
        .collect();
    let sink_buf = vec![Vec::new(); sinks.len()];
    let start = parser.now;
    let ctx = Ctx {
        sinks,
        sink_tty,
        sink_buf,
        execs: parser.execs,
        tree,
        zone: localtime::Zone::from_env(),
        start,
        // `POSIXLY_CORRECT` halves it; `FIND_BLOCK_SIZE` is refused outright
        // below rather than honoured.
        block_size: if parser.posixly_correct { 512 } else { 1024 },
        ls_widths: LsWidths::default(),
        ignore_readdir_race: parser.ignore_readdir_race,
        depth_first: parser.depth_first,
        follow: parser.follow,
        status: 0,
        stop_at_current_level: false,
        quit: false,
    };

    let mut walk = Walk {
        ctx,
        nodes: &parser.nodes,
        expr: &expr,
        max_depth: parser.max_depth,
        min_depth: parser.min_depth,
        xdev: parser.xdev,
        root_dev: 0,
        active: Vec::new(),
    };

    let ok_prompt = walk.ctx.execs.iter().any(|e| e.confirm);
    let status = process_all_startpoints(
        &mut walk,
        start_points,
        parser.files0_from.as_deref(),
        ok_prompt,
    );

    // `cleanup()`: the outstanding `-exec … +` batches, then the buffers.
    walk.ctx.flush_all_execs();
    walk.ctx.flush();
    if let Some(out) = capture.as_mut() {
        harvest(&walk.ctx, out);
    }
    if status != 0 { status } else { walk.ctx.status }
}

/// Upstream `process_all_startpoints`.
///
/// Returns the exit status of the *fatal* failures only — the ones that end
/// the program where they stand. Everything else it reports goes through
/// [`Ctx::fail`], which is what carries `state.exit_status` out.
fn process_all_startpoints(
    walk: &mut Walk<'_, '_>,
    start_points: &[Vec<u8>],
    files0_from: Option<&[u8]>,
    ok_prompt: bool,
) -> i32 {
    let names: Vec<Vec<u8>> = if let Some(from) = files0_from {
        // `-files0-from` and start points on the command line are mutually
        // exclusive, and the refusal is two lines: the operand, then the rule.
        if let Some(first) = start_points.first() {
            eprintln!("find: extra operand {}", quote(first));
            eprintln!("find: file operands cannot be combined with -files0-from");
            return 1;
        }
        if from == b"-" {
            if ok_prompt {
                // The prompt and the name list would be reading the same
                // stream, so one would eat the other's input.
                eprintln!(
                    "find: option -files0-from reading from standard input cannot be combined with -ok, -okdir"
                );
                eprintln!();
                return 1;
            }
            let mut buf = Vec::new();
            if let Err(e) = io::Read::read_to_end(&mut io::stdin(), &mut buf) {
                eprintln!(
                    "find: {}: read error: {}",
                    files0_name(from),
                    errmsg::strerror(&e)
                );
                return 1;
            }
            split_nul(&buf)
        } else {
            match std::fs::read(os_from_bytes(from)) {
                Ok(buf) => split_nul(&buf),
                Err(e) => {
                    eprintln!(
                        "find: cannot open {} for reading: {}",
                        files0_name(from),
                        errmsg::strerror(&e)
                    );
                    return 1;
                }
            }
        }
    } else if start_points.is_empty() {
        // No start points: `.`, supplied as a real argument rather than as a
        // default inside the walk, because `%H` prints it.
        vec![b".".to_vec()]
    } else {
        start_points.to_vec()
    };

    let quoted_from = files0_from.map(files0_name);

    for (n, name) in names.iter().enumerate() {
        if name.is_empty() {
            // fts fails immediately on an empty name without looking at the
            // rest, so these are reported and skipped before it sees them.
            match &quoted_from {
                // The record number is 1-based and counts the empty record
                // itself, which is why an empty *first* record is `:1:`.
                Some(q) => eprintln!(
                    "find: {q}:{}: invalid zero-length file name",
                    n.saturating_add(1)
                ),
                None => eprintln!("find: {}: No such file or directory", quote(name)),
            }
            walk.ctx.status = 1;
            continue;
        }
        walk.start(name);
        if walk.ctx.quit {
            break;
        }
    }
    0
}

/// How `-files0-from`'s operand is named in a diagnostic, quoted ready for the
/// message.
///
/// Upstream renders the `-` that means standard input as `(standard input)`
/// rather than as the dash the user typed, so the name in the message is not
/// always the name in the argument. Called at each format site rather than
/// hoisted into a local because the quoting has to be visible where the message
/// is written — a pre-quoted local reads exactly like an unquoted one.
fn files0_name(from: &[u8]) -> String {
    if from == b"-" {
        quote(b"(standard input)")
    } else {
        quote(from)
    }
}

/// The records of a NUL-separated list.
///
/// A trailing NUL terminates the last record rather than introducing an empty
/// one, which is why this is not a plain `split`: `argv_iter_init_stream`
/// stops at EOF, and a file ending in NUL is at EOF straight afterwards.
fn split_nul(buf: &[u8]) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = buf.split(|&b| b == 0).map(<[u8]>::to_vec).collect();
    if buf.last() == Some(&0) {
        out.pop();
    }
    out
}

#[cfg(unix)]
fn main() -> ExitCode {
    if std::env::var_os("FIND_BLOCK_SIZE").is_some() {
        eprintln!(
            "find: The environment variable FIND_BLOCK_SIZE is not supported, the only thing \
             that affects the block size is the POSIXLY_CORRECT environment variable"
        );
        return ExitCode::from(1);
    }
    let argv: Vec<Vec<u8>> = std::env::args_os()
        .skip(1)
        .map(|a| os_bytes(&a).into_owned())
        .collect();
    let tree = RealTree::new();
    ExitCode::from(u8::try_from(run(&argv, &tree)).unwrap_or(1))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    /// One file in a [`FakeTree`].
    struct FakeFile {
        meta: Meta,
        /// The symlink's target, verbatim, for the ones that have one.
        link: Option<Vec<u8>>,
    }

    /// A whole filesystem as a literal.
    ///
    /// Keyed by the *exact* path the walk builds, which is the start point
    /// joined to each component with one `/`. That is deliberately literal
    /// rather than clever: a fake tree that normalised its keys could not tell
    /// `find .` from `find ./`, and the difference between those two —
    /// `NAPPEND`'s one-slash rule — is the sort of thing these tests exist to
    /// pin.
    ///
    /// Entries come back from [`Tree::read_dir`] in sorted order because the
    /// map is a `BTreeMap`. A real `readdir` promises no order at all and the
    /// port passes on whatever it is handed; sorting here only makes the
    /// assertions writable.
    /// One command a `-exec` family action started: its argv, and the directory
    /// it was to run in (`None` for the ones that do not chdir).
    type Spawned = (Vec<Vec<u8>>, Option<Vec<u8>>);

    struct FakeTree {
        files: BTreeMap<Vec<u8>, FakeFile>,
        /// Directories `read_dir` refuses with `EACCES`, to reach `FTS_DNR`.
        unreadable: Vec<Vec<u8>>,
        /// `readdir` reported `DT_UNKNOWN` for everything, so every type
        /// question has to be answered by a `stat`. Both branches of the
        /// `FTS_NOSTAT` decision have to be reachable from a test.
        no_dtype: bool,
        /// What `-ok` is told.
        answer: bool,
        /// Every command an `-exec` started, with its working directory.
        spawned: RefCell<Vec<Spawned>>,
        /// Every name `-delete` removed.
        removed: RefCell<Vec<Vec<u8>>>,
        /// The next inode to hand out.
        next_ino: u64,
    }

    /// A fixed instant, so a `-newer` test means the same thing in June as in
    /// December.
    const NOW: i64 = 1_787_486_400;

    impl FakeTree {
        fn new() -> Self {
            Self {
                files: BTreeMap::new(),
                unreadable: Vec::new(),
                no_dtype: false,
                answer: true,
                spawned: RefCell::new(Vec::new()),
                removed: RefCell::new(Vec::new()),
                next_ino: 100,
            }
        }

        fn add(&mut self, path: &str, mode: u32, size: u64, link: Option<&str>) {
            let ino = self.next_ino;
            self.next_ino += 1;
            let ts = Ts {
                sec: NOW - 3600,
                nsec: 0,
            };
            let is_dir = mode & modechange::S_IFMT == modechange::S_IFDIR;
            self.files.insert(
                path.as_bytes().to_vec(),
                FakeFile {
                    meta: Meta {
                        dev: 1,
                        ino,
                        mode,
                        nlink: if is_dir { 2 } else { 1 },
                        uid: 1000,
                        gid: 1000,
                        size,
                        blocks: size.div_ceil(512),
                        rdev: 0,
                        atime: ts,
                        mtime: ts,
                        ctime: ts,
                    },
                    link: link.map(|t| t.as_bytes().to_vec()),
                },
            );
        }

        fn dir(mut self, path: &str) -> Self {
            self.add(path, modechange::S_IFDIR | 0o755, 4096, None);
            self
        }

        fn file(mut self, path: &str, size: u64) -> Self {
            self.add(path, modechange::S_IFREG | 0o644, size, None);
            self
        }

        fn file_mode(mut self, path: &str, mode: u32, size: u64) -> Self {
            self.add(path, modechange::S_IFREG | mode, size, None);
            self
        }

        fn symlink(mut self, path: &str, target: &str) -> Self {
            self.add(path, modechange::S_IFLNK | 0o777, 7, Some(target));
            self
        }

        /// Move a file onto another device, for `-xdev` and `-fstype`.
        fn on_dev(mut self, path: &str, dev: u64) -> Self {
            if let Some(f) = self.files.get_mut(path.as_bytes()) {
                f.meta.dev = dev;
            }
            self
        }

        fn mtime(mut self, path: &str, sec: i64) -> Self {
            if let Some(f) = self.files.get_mut(path.as_bytes()) {
                f.meta.mtime = Ts { sec, nsec: 0 };
            }
            self
        }

        fn unreadable(mut self, path: &str) -> Self {
            self.unreadable.push(path.as_bytes().to_vec());
            self
        }

        fn blind(mut self) -> Self {
            self.no_dtype = true;
            self
        }

        fn says_no(mut self) -> Self {
            self.answer = false;
            self
        }

        /// One step of symlink resolution: the target, made relative to the
        /// directory the link itself sits in.
        fn resolve(path: &[u8], target: &[u8]) -> Vec<u8> {
            if target.first() == Some(&b'/') {
                return target.to_vec();
            }
            let mut out = pathname::dir_name(path).to_vec();
            if out.last() != Some(&b'/') {
                out.push(b'/');
            }
            out.extend_from_slice(target);
            out
        }

        /// Resolve a name to the key of the file it finally denotes, following
        /// a symlink wherever one appears — including part-way along, which is
        /// what `find -L` needs the moment it descends through one.
        fn follow(&self, path: &[u8]) -> io::Result<Vec<u8>> {
            let mut cur: Vec<u8> = Vec::new();
            // Deep enough to reach `ELOOP` on a cycle and never on a chain any
            // test writes.
            let mut hops = 0u32;
            for comp in path.split(|&b| b == b'/') {
                if !cur.is_empty() {
                    cur.push(b'/');
                }
                cur.extend_from_slice(comp);
                while let Some(t) = self.files.get(&cur).and_then(|f| f.link.clone()) {
                    hops += 1;
                    if hops > 40 {
                        return Err(io::Error::from_raw_os_error(40));
                    }
                    cur = Self::resolve(&cur, &t);
                }
            }
            if self.files.contains_key(&cur) {
                Ok(cur)
            } else {
                Err(enoent())
            }
        }

        fn ino_of(&self, path: &str) -> u64 {
            self.files.get(path.as_bytes()).map_or(0, |f| f.meta.ino)
        }

        fn commands(&self) -> Vec<Vec<Vec<u8>>> {
            self.spawned
                .borrow()
                .iter()
                .map(|(argv, _)| argv.clone())
                .collect()
        }
    }

    fn enoent() -> io::Error {
        io::Error::from_raw_os_error(2)
    }

    impl Tree for FakeTree {
        fn lstat(&self, path: &[u8]) -> io::Result<Meta> {
            if let Some(f) = self.files.get(path) {
                return Ok(f.meta);
            }
            // Not a key of its own: some component before the last one was a
            // symlink. `lstat` follows those and stops at the last, so resolve
            // the directory part and look the name up inside it.
            let mut real = self.follow(pathname::dir_name(path))?;
            if real.last() != Some(&b'/') {
                real.push(b'/');
            }
            real.extend_from_slice(pathname::base_name(path));
            self.files.get(&real).map(|f| f.meta).ok_or_else(enoent)
        }

        fn stat(&self, path: &[u8]) -> io::Result<Meta> {
            let real = self.follow(path)?;
            self.files.get(&real).map(|f| f.meta).ok_or_else(enoent)
        }

        fn read_dir(&self, path: &[u8]) -> io::Result<Vec<(Vec<u8>, u32)>> {
            // `opendir` follows the last component, which is why `find -L` can
            // descend through a symlink to a directory at all.
            let path = &self.follow(path)?;
            if !self.files.contains_key(path) {
                return Err(enoent());
            }
            if self.unreadable.iter().any(|u| u == path) {
                return Err(io::Error::from_raw_os_error(13));
            }
            let mut prefix = path.to_vec();
            if prefix.last() != Some(&b'/') {
                prefix.push(b'/');
            }
            let mut out = Vec::new();
            for (k, f) in &self.files {
                let Some(rest) = k.strip_prefix(prefix.as_slice()) else {
                    continue;
                };
                if rest.is_empty() || rest.contains(&b'/') {
                    continue;
                }
                let dtype = if self.no_dtype {
                    0
                } else {
                    f.meta.mode & modechange::S_IFMT
                };
                out.push((rest.to_vec(), dtype));
            }
            Ok(out)
        }

        fn readlink(&self, path: &[u8]) -> io::Result<Vec<u8>> {
            match self.files.get(path).and_then(|f| f.link.clone()) {
                Some(t) => Ok(t),
                // EINVAL, which is what `readlink` says about a non-symlink.
                None => Err(io::Error::from_raw_os_error(22)),
            }
        }

        fn access(&self, path: &[u8], mode: i32) -> bool {
            // The tests run as nobody in particular, so the "other" bits decide.
            self.stat(path).is_ok_and(|m| {
                let want = u32::try_from(mode).unwrap_or(0);
                m.mode & want == want
            })
        }

        fn remove_file(&self, path: &[u8]) -> io::Result<()> {
            let f = self.files.get(path).ok_or_else(enoent)?;
            if f.meta.is_dir() {
                // EISDIR, so `Ctx::delete`'s retry is reachable.
                return Err(io::Error::from_raw_os_error(21));
            }
            self.removed.borrow_mut().push(path.to_vec());
            Ok(())
        }

        fn remove_dir(&self, path: &[u8]) -> io::Result<()> {
            let f = self.files.get(path).ok_or_else(enoent)?;
            if f.meta.is_dir() {
                self.removed.borrow_mut().push(path.to_vec());
                Ok(())
            } else {
                // ENOTDIR.
                Err(io::Error::from_raw_os_error(20))
            }
        }

        fn fstype(&self, dev: u64) -> Vec<u8> {
            if dev == 1 {
                b"ext4".to_vec()
            } else {
                b"tmpfs".to_vec()
            }
        }

        fn user_name(&self, uid: u32) -> Option<Vec<u8>> {
            match uid {
                0 => Some(b"root".to_vec()),
                1000 => Some(b"user".to_vec()),
                _ => None,
            }
        }

        fn group_name(&self, gid: u32) -> Option<Vec<u8>> {
            self.user_name(gid)
        }

        fn uid_by_name(&self, name: &[u8]) -> Option<u32> {
            match name {
                b"root" => Some(0),
                b"user" => Some(1000),
                _ => None,
            }
        }

        fn gid_by_name(&self, name: &[u8]) -> Option<u32> {
            self.uid_by_name(name)
        }

        fn run(&self, argv: &[Vec<u8>], cwd: Option<&[u8]>) -> io::Result<bool> {
            self.spawned
                .borrow_mut()
                .push((argv.to_vec(), cwd.map(<[u8]>::to_vec)));
            Ok(true)
        }

        fn now(&self) -> Ts {
            Ts { sec: NOW, nsec: 0 }
        }

        fn path_env(&self) -> Option<Vec<u8>> {
            // A `$PATH` `-execdir` will accept. The host's own would not be:
            // on Windows it is `;`-separated and full of drive letters, so
            // every entry reads as a relative path and `check_path_safety`
            // refuses before the walk starts.
            Some(b"/bin:/usr/bin".to_vec())
        }

        fn confirm(&self) -> bool {
            self.answer
        }
    }

    /// The tree every test that does not need a special shape uses.
    ///
    /// ```text
    /// .            d 0755
    /// ./d          d 0755
    /// ./d/g        f 0644   3 bytes
    /// ./d/sub      d 0755
    /// ./d/sub/h    f 0644   5 bytes
    /// ./dangling   l -> nosuch
    /// ./empty      d 0755   (no entries)
    /// ./f          f 0644   0 bytes
    /// ./link       l -> f
    /// ```
    fn sample() -> FakeTree {
        FakeTree::new()
            .dir(".")
            .dir("./d")
            .file("./d/g", 3)
            .dir("./d/sub")
            .file("./d/sub/h", 5)
            .symlink("./dangling", "nosuch")
            .dir("./empty")
            .file("./f", 0)
            .symlink("./link", "f")
    }

    /// Drive the real [`run_inner`] and return `(status, stdout)`.
    fn find(tree: &FakeTree, args: &[&str]) -> (i32, String) {
        let argv: Vec<Vec<u8>> = args.iter().map(|a| a.as_bytes().to_vec()).collect();
        let mut cap = Some(Vec::new());
        let status = run_inner(&argv, tree, &mut cap);
        (
            status,
            String::from_utf8_lossy(&cap.unwrap_or_default()).into_owned(),
        )
    }

    /// The lines `find` printed, which is what most of these assertions are.
    fn lines(tree: &FakeTree, args: &[&str]) -> Vec<String> {
        find(tree, args).1.lines().map(str::to_owned).collect()
    }

    fn none() -> Vec<String> {
        Vec::new()
    }

    // -- the walk ----------------------------------------------------------

    #[test]
    fn bare_find_prints_the_whole_tree_preorder() {
        assert_eq!(
            lines(&sample(), &["."]),
            [
                ".",
                "./d",
                "./d/g",
                "./d/sub",
                "./d/sub/h",
                "./dangling",
                "./empty",
                "./f",
                "./link",
            ]
        );
    }

    #[test]
    fn no_start_point_means_dot() {
        assert_eq!(lines(&sample(), &[]), lines(&sample(), &["."]));
    }

    #[test]
    fn depth_puts_a_directory_after_its_contents() {
        assert_eq!(
            lines(&sample(), &[".", "-depth"]),
            [
                "./d/g",
                "./d/sub/h",
                "./d/sub",
                "./d",
                "./dangling",
                "./empty",
                "./f",
                "./link",
                ".",
            ]
        );
    }

    #[test]
    fn maxdepth_limits_descent_and_mindepth_hides_the_start_point() {
        assert_eq!(
            lines(&sample(), &[".", "-maxdepth", "1"]),
            [".", "./d", "./dangling", "./empty", "./f", "./link"]
        );
        assert_eq!(
            lines(&sample(), &[".", "-mindepth", "1", "-maxdepth", "1"]),
            ["./d", "./dangling", "./empty", "./f", "./link"]
        );
    }

    #[test]
    fn a_trailing_slash_on_the_start_point_is_not_doubled() {
        assert_eq!(
            lines(&FakeTree::new().dir("d/").file("d/g", 1), &["d/"]),
            ["d/", "d/g"]
        );
    }

    #[test]
    fn a_missing_start_point_is_reported_and_sets_the_status() {
        let (status, out) = find(&sample(), &["nosuch"]);
        assert_eq!(status, 1);
        assert_eq!(out, "");
    }

    #[test]
    fn a_missing_start_point_does_not_stop_the_others() {
        let (status, out) = find(&sample(), &["nosuch", "./f"]);
        assert_eq!(status, 1);
        assert_eq!(out, "./f\n");
    }

    #[test]
    fn an_unreadable_directory_is_reported_but_still_listed() {
        let tree = sample().unreadable("./d");
        let (status, out) = find(&tree, &["."]);
        assert_eq!(status, 1);
        // `./d` itself is visited; only its contents are missing.
        assert!(out.contains("./d\n"), "{out}");
        assert!(!out.contains("./d/g"), "{out}");
    }

    #[test]
    fn xdev_does_not_cross_a_mount_point() {
        let tree = sample().on_dev("./d", 2);
        assert_eq!(
            lines(&tree, &[".", "-xdev"]),
            [".", "./d", "./dangling", "./empty", "./f", "./link"]
        );
    }

    #[test]
    fn a_dtype_less_readdir_gives_the_same_answers() {
        // The whole of what `FTS_NOSTAT` changes is *when* the `stat` happens,
        // never what the walk concludes.
        assert_eq!(lines(&sample().blind(), &["."]), lines(&sample(), &["."]));
        assert_eq!(
            lines(&sample().blind(), &[".", "-type", "f"]),
            lines(&sample(), &[".", "-type", "f"])
        );
    }

    // -- the expression ----------------------------------------------------

    #[test]
    fn name_matches_the_last_component_only() {
        assert_eq!(lines(&sample(), &[".", "-name", "g"]), ["./d/g"]);
        assert_eq!(lines(&sample(), &[".", "-name", "d/g"]), none());
    }

    #[test]
    fn name_takes_a_real_glob() {
        // The matcher this replaced read `[a-z]` as three literal characters.
        assert_eq!(
            lines(&sample(), &[".", "-name", "[a-z]"]),
            ["./d", "./d/g", "./d/sub/h", "./f"]
        );
        // `.` matches too: `-name` is `fnmatch` without `FNM_PERIOD`, so a
        // leading dot is an ordinary character.
        assert_eq!(
            lines(&sample(), &[".", "-name", "[!fg]*"]),
            [
                ".",
                "./d",
                "./d/sub",
                "./d/sub/h",
                "./dangling",
                "./empty",
                "./link",
            ]
        );
    }

    #[test]
    fn iname_folds_case() {
        assert_eq!(lines(&sample(), &[".", "-iname", "F"]), ["./f"]);
    }

    #[test]
    fn path_matches_the_whole_name_and_star_crosses_slashes() {
        assert_eq!(
            lines(&sample(), &[".", "-path", "./d/*"]),
            ["./d/g", "./d/sub", "./d/sub/h"]
        );
    }

    #[test]
    fn regex_must_consume_the_whole_path() {
        assert_eq!(lines(&sample(), &[".", "-regex", ".*/f"]), ["./f"]);
        // Not a search: `f` alone matches nothing, because the pattern has to
        // account for the whole of `./f`.
        assert_eq!(lines(&sample(), &[".", "-regex", "f"]), none());
    }

    #[test]
    fn type_selects_one_kind() {
        assert_eq!(
            lines(&sample(), &[".", "-type", "f"]),
            ["./d/g", "./d/sub/h", "./f"]
        );
        assert_eq!(
            lines(&sample(), &[".", "-type", "l"]),
            ["./dangling", "./link"]
        );
    }

    #[test]
    fn type_takes_a_comma_separated_list() {
        assert_eq!(
            lines(&sample(), &[".", "-type", "l,f"]),
            ["./d/g", "./d/sub/h", "./dangling", "./f", "./link"]
        );
    }

    #[test]
    fn xtype_asks_about_the_other_end_of_the_link() {
        // For a dangling link `-xtype` falls back to the link itself, which is
        // why `-xtype l` is how a broken one is found.
        assert_eq!(lines(&sample(), &[".", "-xtype", "l"]), ["./dangling"]);
        assert!(lines(&sample(), &[".", "-xtype", "f"]).contains(&"./link".to_owned()));
    }

    #[test]
    fn lname_matches_the_target_text() {
        assert_eq!(lines(&sample(), &[".", "-lname", "nosuch"]), ["./dangling"]);
    }

    #[test]
    fn empty_covers_a_zero_length_file_and_a_childless_directory() {
        assert_eq!(lines(&sample(), &[".", "-empty"]), ["./empty", "./f"]);
    }

    #[test]
    fn size_counts_512_byte_blocks_by_default() {
        assert_eq!(
            lines(&sample(), &[".", "-type", "f", "-size", "1"]),
            ["./d/g", "./d/sub/h"]
        );
        assert_eq!(
            lines(&sample(), &[".", "-type", "f", "-size", "0"]),
            ["./f"]
        );
    }

    #[test]
    fn size_takes_a_unit_suffix() {
        assert_eq!(lines(&sample(), &[".", "-size", "3c"]), ["./d/g"]);
        // Rounded *up*, so three bytes is one kibibyte-block and `-1k` — fewer
        // than one — is the empty file alone.
        assert_eq!(
            lines(&sample(), &[".", "-type", "f", "-size", "-1k"]),
            ["./f"]
        );
    }

    #[test]
    fn perm_distinguishes_exact_from_at_least_from_any() {
        let tree = sample().file_mode("./x", 0o755, 1);
        // `-type f` because the sample's directories are 0755 as well, and
        // `-perm` says nothing about the file type.
        assert_eq!(lines(&tree, &[".", "-type", "f", "-perm", "755"]), ["./x"]);
        assert!(lines(&tree, &[".", "-perm", "-644"]).contains(&"./x".to_owned()));
        assert!(lines(&tree, &[".", "-perm", "/111"]).contains(&"./x".to_owned()));
        assert!(!lines(&tree, &[".", "-perm", "/111"]).contains(&"./f".to_owned()));
    }

    #[test]
    fn user_resolves_through_the_database() {
        assert_eq!(lines(&sample(), &[".", "-user", "user"]).len(), 9);
        assert_eq!(lines(&sample(), &[".", "-user", "root"]), none());
    }

    #[test]
    fn an_unknown_user_is_fatal() {
        let (status, out) = find(&sample(), &[".", "-user", "nosuchuser"]);
        assert_eq!(status, 1);
        assert_eq!(out, "");
    }

    #[test]
    fn newer_compares_modification_times() {
        let tree = sample().mtime("./f", NOW - 10);
        assert_eq!(lines(&tree, &[".", "-newer", "./d/g"]), ["./f"]);
    }

    #[test]
    fn samefile_compares_dev_and_ino() {
        assert_eq!(lines(&sample(), &[".", "-samefile", "./f"]), ["./f"]);
    }

    #[test]
    fn fstype_reads_the_device() {
        let tree = sample().on_dev("./f", 2);
        assert_eq!(lines(&tree, &[".", "-fstype", "tmpfs"]), ["./f"]);
    }

    #[test]
    fn inum_and_links_read_the_stat() {
        let want = sample().ino_of("./f").to_string();
        assert_eq!(lines(&sample(), &[".", "-inum", &want]), ["./f"]);
        assert_eq!(
            lines(&sample(), &[".", "-links", "2"]),
            [".", "./d", "./d/sub", "./empty"]
        );
    }

    // -- operators ---------------------------------------------------------

    #[test]
    fn and_binds_tighter_than_or() {
        // `-name f -o -name g -print` is `(f) -o (g -print)`: only `g` prints,
        // and the implicit `-print` is suppressed by the explicit one.
        assert_eq!(
            lines(
                &sample(),
                &[".", "-name", "f", "-o", "-name", "g", "-print"]
            ),
            ["./d/g"]
        );
    }

    #[test]
    fn parentheses_regroup() {
        assert_eq!(
            lines(
                &sample(),
                &[".", "(", "-name", "f", "-o", "-name", "g", ")", "-print"]
            ),
            ["./d/g", "./f"]
        );
    }

    #[test]
    fn not_negates() {
        let all = lines(&sample(), &["."]);
        let not_f = lines(&sample(), &[".", "!", "-name", "f"]);
        assert_eq!(not_f.len(), all.len() - 1);
        assert!(!not_f.contains(&"./f".to_owned()));
        assert_eq!(not_f, lines(&sample(), &[".", "-not", "-name", "f"]));
    }

    #[test]
    fn comma_evaluates_both_and_takes_the_right_hand_value() {
        assert_eq!(
            lines(
                &sample(),
                &[".", "-name", "f", "-print", ",", "-name", "g", "-print"]
            ),
            ["./d/g", "./f"]
        );
    }

    #[test]
    fn prune_stops_the_descent_but_is_true() {
        assert_eq!(
            lines(&sample(), &[".", "-name", "d", "-prune", "-o", "-print"]),
            [".", "./dangling", "./empty", "./f", "./link"]
        );
    }

    #[test]
    fn prune_does_nothing_under_depth() {
        // Upstream documents this: by the time `-prune` runs, the contents have
        // already been visited.
        assert!(
            lines(
                &sample(),
                &[".", "-depth", "-name", "d", "-prune", "-o", "-print"]
            )
            .contains(&"./d/g".to_owned())
        );
    }

    #[test]
    fn quit_ends_the_walk_where_it_stands() {
        assert_eq!(
            lines(&sample(), &[".", "-name", "d", "-print", "-quit"]),
            ["./d"]
        );
    }

    #[test]
    fn true_and_false_are_predicates() {
        assert_eq!(lines(&sample(), &[".", "-false"]), none());
        assert_eq!(lines(&sample(), &[".", "-true"]).len(), 9);
    }

    // -- actions -----------------------------------------------------------

    #[test]
    fn print0_separates_with_nul() {
        let (_, out) = find(&sample(), &[".", "-name", "f", "-print0"]);
        assert_eq!(out, "./f\u{0}");
    }

    #[test]
    fn printf_renders_the_path_directives() {
        let (_, out) = find(
            &sample(),
            &[".", "-name", "g", "-printf", "%p|%f|%h|%d|%y\n"],
        );
        assert_eq!(out, "./d/g|g|./d|2|f\n");
    }

    #[test]
    fn printf_renders_sizes_and_the_filesystem() {
        let (_, out) = find(&sample(), &[".", "-name", "g", "-printf", "%s %y %F\n"]);
        assert_eq!(out, "3 f ext4\n");
    }

    #[test]
    fn printf_reads_its_own_escapes() {
        let (_, out) = find(&sample(), &[".", "-name", "f", "-printf", "%%|%p\\n"]);
        assert_eq!(out, "%|./f\n");
    }

    #[test]
    fn exec_substitutes_the_braces() {
        let tree = sample();
        let (status, _) = find(&tree, &[".", "-name", "f", "-exec", "echo", "A", "{}", ";"]);
        assert_eq!(status, 0);
        assert_eq!(
            tree.commands(),
            [vec![b"echo".to_vec(), b"A".to_vec(), b"./f".to_vec()]]
        );
    }

    #[test]
    fn exec_plus_batches_into_one_command() {
        let tree = sample();
        let (status, _) = find(&tree, &[".", "-type", "f", "-exec", "echo", "{}", "+"]);
        assert_eq!(status, 0);
        assert_eq!(
            tree.commands(),
            [vec![
                b"echo".to_vec(),
                b"./d/g".to_vec(),
                b"./d/sub/h".to_vec(),
                b"./f".to_vec(),
            ]]
        );
    }

    #[test]
    fn execdir_runs_in_the_containing_directory_with_a_relative_name() {
        let tree = sample();
        let _ = find(&tree, &[".", "-name", "g", "-execdir", "echo", "{}", ";"]);
        let spawned = tree.spawned.borrow();
        let (argv, cwd) = spawned.first().expect("one command");
        assert_eq!(argv.as_slice(), [b"echo".to_vec(), b"./g".to_vec()]);
        assert_eq!(cwd.as_deref(), Some(b"./d".as_slice()));
    }

    #[test]
    fn ok_does_not_run_the_command_when_the_answer_is_no() {
        let tree = sample().says_no();
        let _ = find(&tree, &[".", "-name", "f", "-ok", "echo", "{}", ";"]);
        assert!(tree.commands().is_empty());
    }

    #[test]
    fn delete_removes_contents_before_their_directory() {
        let tree = sample();
        let (status, _) = find(&tree, &["./d", "-delete"]);
        assert_eq!(status, 0);
        // `-delete` turns on `-depth`, which is what makes it able to work.
        assert_eq!(
            *tree.removed.borrow(),
            [
                b"./d/g".to_vec(),
                b"./d/sub/h".to_vec(),
                b"./d/sub".to_vec(),
                b"./d".to_vec(),
            ]
        );
    }

    #[test]
    fn an_explicit_action_suppresses_the_implicit_print() {
        let tree = sample();
        let (_, out) = find(&tree, &[".", "-name", "f", "-exec", "echo", "{}", ";"]);
        assert_eq!(out, "");
    }

    // -- diagnostics -------------------------------------------------------

    #[test]
    fn a_missing_argument_is_fatal() {
        for args in [
            vec![".", "-name"],
            vec![".", "-type"],
            vec![".", "-maxdepth"],
            vec![".", "-size"],
            vec![".", "-perm"],
            vec![".", "-exec"],
        ] {
            let (status, out) = find(&sample(), &args);
            assert_eq!(status, 1, "{args:?}");
            assert_eq!(out, "", "{args:?}");
        }
    }

    #[test]
    fn a_bad_argument_is_fatal() {
        for args in [
            vec![".", "-type", "q"],
            vec![".", "-maxdepth", "x"],
            vec![".", "-maxdepth", "-1"],
            vec![".", "-size", "1x"],
            vec![".", "-perm", "zzz"],
            vec![".", "-newer", "nosuch"],
        ] {
            assert_eq!(find(&sample(), &args).0, 1, "{args:?}");
        }
    }

    #[test]
    fn an_unknown_predicate_is_fatal() {
        assert_eq!(find(&sample(), &[".", "-zzz"]).0, 1);
        // `-newerXY` is the one entry the driver does not consume before
        // parsing, which is what makes this "invalid predicate" rather than
        // "invalid argument".
        assert_eq!(find(&sample(), &[".", "-newerqq", "x"]).0, 1);
    }

    #[test]
    fn unbalanced_parentheses_are_fatal() {
        assert_eq!(find(&sample(), &[".", "("]).0, 1);
        assert_eq!(find(&sample(), &[".", "(", "-name", "f"]).0, 1);
        assert_eq!(find(&sample(), &[".", ")"]).0, 1);
    }

    #[test]
    fn a_dangling_operator_is_fatal() {
        assert_eq!(find(&sample(), &[".", "-name", "f", "-o"]).0, 1);
        assert_eq!(find(&sample(), &[".", "-o", "-name", "f"]).0, 1);
        assert_eq!(find(&sample(), &[".", "!"]).0, 1);
    }

    #[test]
    fn context_is_refused_rather_than_answered() {
        assert_eq!(find(&sample(), &[".", "-context", "x"]).0, 1);
    }

    // -- leading options ---------------------------------------------------

    #[test]
    fn the_three_link_options_are_accepted() {
        for opt in ["-P", "-L", "-H"] {
            assert_eq!(find(&sample(), &[opt, ".", "-name", "f"]).0, 0, "{opt}");
        }
    }

    #[test]
    fn dash_l_descends_through_a_link_to_a_directory() {
        let tree = sample().symlink("./dl", "d");
        // Under `-P` the link is a leaf.
        assert_eq!(lines(&tree, &[".", "-path", "./dl/*"]), none());
        assert_eq!(
            lines(&tree, &["-L", ".", "-path", "./dl/*"]),
            ["./dl/g", "./dl/sub", "./dl/sub/h"]
        );
    }

    #[test]
    fn optimisation_levels_are_accepted_and_validated() {
        assert_eq!(find(&sample(), &["-O3", ".", "-name", "f"]).0, 0);
        assert_eq!(find(&sample(), &["-O", ".", "-name", "f"]).0, 1);
        assert_eq!(find(&sample(), &["-O1x", "."]).0, 1);
        assert_eq!(find(&sample(), &["-O65536", "."]).0, 1);
    }

    #[test]
    fn a_double_dash_ends_the_leading_options() {
        assert_eq!(lines(&sample(), &["--", ".", "-name", "f"]), ["./f"]);
    }

    // -- the pure helpers --------------------------------------------------

    #[test]
    fn split_nul_does_not_invent_a_trailing_record() {
        assert_eq!(split_nul(b"a\0b\0"), [b"a".to_vec(), b"b".to_vec()]);
        assert_eq!(split_nul(b"a\0b"), [b"a".to_vec(), b"b".to_vec()]);
        assert_eq!(split_nul(b""), [Vec::<u8>::new()]);
        assert_eq!(split_nul(b"\0"), [Vec::<u8>::new()]);
    }

    #[test]
    fn strtok_first_reproduces_the_upstream_truncation() {
        // `process_debug_options` quotes the whole argument, but `strtok_r` has
        // already written a NUL over the first delimiter — so the message names
        // the first token rather than the offending one.
        assert_eq!(strtok_first(b"exec,bogus"), b"exec");
        assert_eq!(strtok_first(b"bogus"), b"bogus");
        // Leading delimiters are skipped rather than overwritten, so nothing is
        // truncated at all.
        assert_eq!(strtok_first(b",bogus"), b",bogus");
        assert_eq!(strtok_first(b""), b"");
    }

    #[test]
    fn type_letter_is_finds_alphabet_not_ls_s() {
        assert_eq!(type_letter(modechange::S_IFREG), b'f');
        assert_eq!(type_letter(modechange::S_IFDIR), b'd');
        assert_eq!(type_letter(modechange::S_IFLNK), b'l');
        assert_eq!(type_letter(0o150_000), b'D');
        assert_eq!(type_letter(0), b'U');
    }

    #[test]
    fn arg_class_matches_the_parse_table() {
        assert!(arg_class(b"maxdepth") == ArgClass::Option);
        assert!(arg_class(b"daystart") == ArgClass::Positional);
        assert!(arg_class(b"name") == ArgClass::Other);
        assert!(arg_class(b"print") == ArgClass::Other);
    }

    #[test]
    fn the_two_texts_are_present() {
        let h = help_text();
        assert!(h.starts_with("Usage: find"), "{h}");
        assert!(h.contains("-name"), "{h}");
        assert!(VERSION.starts_with("find (GNU findutils)"), "{VERSION}");
    }
}
