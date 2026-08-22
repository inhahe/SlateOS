//! Turning a name into *the* name: gnulib's `canonicalize_filename_mode`.
//!
//! `readlink -f`, `readlink -e`, `readlink -m` and every mode of `realpath` are
//! one function with a three-valued parameter. This is that function. It takes
//! a name, follows every symbolic link in it, resolves every `.` and `..`, and
//! returns an absolute name with no link, no dot component and no repeated
//! slash left in it.
//!
//! # Why this is not `std::fs::canonicalize`
//!
//! `std::fs::canonicalize` is one of the three modes — the strictest, the one
//! that fails unless the *whole* name already exists. Before this module,
//! `readlink` called it for all three of `-f`, `-e` and `-m` and `realpath`
//! did the same, so two of the three options were wrong by construction:
//!
//! | Asked | GNU answers | `std::fs::canonicalize` answers |
//! |---|---|---|
//! | `readlink -e d/sub/missing` | *(fails)* | fails |
//! | `readlink -f d/sub/missing` | `/tmp/d/sub/missing` | **fails** |
//! | `readlink -m d/nodir/missing` | `/tmp/d/nodir/missing` | **fails** |
//! | `readlink -m a` *(a symlink cycle)* | `/tmp/a` | **fails** |
//!
//! `-m` in particular is the mode a script reaches for when it is *about* to
//! create the file — `mkdir -p "$(dirname "$(realpath -m "$out")")"` — so a
//! `-m` that fails on a name that does not exist yet fails at the one job it
//! has. It also returns `\\?\C:\…` on the Windows development host, which is
//! not a name any of this OS's utilities can use.
//!
//! Two further reasons this cannot be `std`'s: `std::fs::canonicalize` takes a
//! `Path` and gives a `PathBuf`, and everything here is `&[u8]`, because a
//! path on this system may hold any byte but `/` and NUL; and it offers no way
//! to observe *which* component failed, which is the difference between
//! `readlink -fv` printing `No such file or directory` and printing
//! `Not a directory`.
//!
//! # The three modes, and the one clause that separates them
//!
//! [`Mode`] is gnulib's `canonicalize_mode_t` minus the flags nothing here
//! uses. The whole difference between the three lives in [`accept`], which is
//! called when `readlink(2)` on a component fails — either because the
//! component is not a symbolic link (`EINVAL`, which is how "it exists and is
//! an ordinary file" is spelled) or because it could not be examined at all.
//!
//! # What was measured, and against what
//!
//! Every rule below was checked against GNU coreutils 9.4 under WSL, and the
//! implementation is a direct translation of gnulib's `canonicalize.c` rather
//! than a reconstruction from the manual — because the manual does not
//! describe any of the four behaviours that are easiest to get wrong:
//!
//! | Name | `-f` | `-e` | `-m` |
//! |---|---|---|---|
//! | `d/sub/missing` (last component absent) | `…/d/sub/missing` | fails `ENOENT` | `…/d/sub/missing` |
//! | `d/sub/missing/` (**and a trailing slash**) | `…/d/sub/missing` | fails `ENOENT` | `…/d/sub/missing` |
//! | `d/sub/real/` (a *file* with a trailing slash) | fails `ENOTDIR` | fails `ENOTDIR` | `…/d/sub/real` |
//! | `d/sub/real/..` (a *file* with `..` after it) | fails `ENOTDIR` | fails `ENOTDIR` | `…/d/sub` |
//! | `plainfile/x` (a file used as a directory) | fails `ENOTDIR` | fails `ENOTDIR` | `…/plainfile/x` |
//! | `d/nodir/missing` (absent *middle* component) | fails `ENOENT` | fails `ENOENT` | `…/d/nodir/missing` |
//! | `dangling` → `/nonexistent/x` | fails `ENOENT` | fails `ENOENT` | `/nonexistent/x` |
//! | `dang2` → `nothere`, in a real directory | `…/d/sub/nothere` | fails `ENOENT` | `…/d/sub/nothere` |
//! | `a` → `b` → `a` (a cycle) | fails `ELOOP` | fails `ELOOP` | `…/a` |
//!
//! Rows two and three are the pair worth staring at. Both names end in a
//! slash; one succeeds under `-f` and the other does not, and the reason is
//! not "one exists". It is that a trailing slash is an assertion that the name
//! is a **directory**, and `-f`'s licence to tolerate a missing component does
//! not extend to tolerating a component that exists and is the wrong kind. See
//! [`suffix_requires_dir_check`], which is where gnulib puts that rule.
//!
//! # There is no depth limit — only cycle detection
//!
//! A chain of 300 symbolic links resolves here, and under GNU, even though the
//! *kernel* refuses it: measured, `readlink -f l300` prints the target while
//! `cat l300` says `Too many levels of symbolic links`. That is deliberate
//! upstream — a userspace canonicaliser expands one link at a time and never
//! hands the kernel a name with more than one link left in it, so `SYMLOOP_MAX`
//! never applies. Refusing at 40 to imitate the kernel would reject names GNU
//! accepts.
//!
//! What replaces the limit is a cycle check, and its exact shape is
//! *observable*, so it is copied rather than approximated. gnulib expands the
//! first 20 links with no bookkeeping at all; from the 21st it records the pair
//! (directory the component was reached in, component name) and stops when a
//! pair repeats. With a 3-cycle `p → q → r → p`, that lands on a specific
//! member of the cycle, and which one depends on where you entered it:
//!
//! | Command | GNU prints | why |
//! |---|---|---|
//! | `readlink -m p` | `…/r` | expansions are `p q r p q r …`; the 21st is `r` |
//! | `readlink -m q` | `…/p` | expansions are `q r p q r p …`; the 21st is `p` |
//! | `readlink -m x/y/z` (`z` → `../../p`) | `…/q` | `z` is expansion 1, so the 21st is `q` |
//!
//! All three are reproduced by this module. That is the point of copying the
//! constant: any other number gives a different, equally arbitrary-looking
//! answer, and a test written against GNU would fail on it.
//!
//! The one deliberate divergence is the key. gnulib records the parent
//! directory's `(device, inode)`; this records its **canonical path**. They
//! differ only when one directory is reachable at two names — a bind mount —
//! where gnulib detects the cycle and this does not. That case cannot arise on
//! SlateOS today (there are no bind mounts), and the fallback is not
//! unbounded: [`MAX_EXPANSIONS`] stops it. See `known-issues.md` →
//! `TD-B-CANON-DETECTS-CYCLES-BY-PATH-NOT-BY-INODE`.
//!
//! # Why the filesystem is a trait
//!
//! [`Fs`] has three methods and [`RealFs`] implements them in eleven lines.
//! Everything else here — the component walk, the `..` arithmetic, the
//! link splicing, the three-mode acceptance rule, the cycle detection — is
//! ordinary byte manipulation that never touches a disk, and with the
//! filesystem behind a trait it can all be tested against a table of made-up
//! entries, on a host that has neither `/tmp/rl5/l300` nor symbolic links that
//! behave like POSIX's.
//!
//! That is the same split `touch` uses and for the same reason: the part that
//! can only be exercised on the target is reduced to the part that is too
//! small to hide a bug in.

use std::collections::HashSet;
use std::io;

use crate::quote::{os_bytes, os_from_bytes};

/// How much of the name has to exist already.
///
/// gnulib's `canonicalize_mode_t`, minus `CAN_NOLINKS` (which `realpath -s`
/// wants and which is not a canonicalisation at all — it is pure text
/// manipulation, so it lives in `realpath` itself).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// `CAN_EXISTING` — `readlink -e`, `realpath -e`, and what
    /// `std::fs::canonicalize` does. Every component must exist.
    Existing,
    /// `CAN_ALL_BUT_LAST` — `readlink -f`, and `realpath` with no mode flag.
    /// The final component may be absent; nothing else may be.
    AllButLast,
    /// `CAN_MISSING` — `readlink -m`, `realpath -m`. Nothing has to exist, and
    /// no failure to examine a component is an error.
    Missing,
}

/// The filesystem, as this algorithm sees it.
///
/// Three operations, each a direct counterpart of the C call gnulib makes, so
/// that the errors this module branches on are the errors the C branches on.
pub trait Fs {
    /// The working directory, as an absolute path. Called at most once, and
    /// only when the name being canonicalised is relative.
    ///
    /// # Errors
    ///
    /// Whatever `getcwd(3)` failed with.
    fn cwd(&self) -> io::Result<Vec<u8>>;

    /// `readlink(2)`: the bytes of a symbolic link's target.
    ///
    /// The error is load-bearing and is not merely "it did not work":
    ///
    /// | Kind | means |
    /// |---|---|
    /// | [`io::ErrorKind::InvalidInput`] (`EINVAL`) | it exists and is **not** a symbolic link |
    /// | [`io::ErrorKind::NotFound`] (`ENOENT`) | no such name |
    /// | [`io::ErrorKind::NotADirectory`] (`ENOTDIR`) | something used as a directory is not one |
    ///
    /// The first row is how existence is proved without a second syscall, so
    /// an implementation that reports a non-symlink as anything else will make
    /// [`Mode::Existing`] accept names that do not exist.
    ///
    /// # Errors
    ///
    /// See above.
    fn read_link(&self, path: &[u8]) -> io::Result<Vec<u8>>;

    /// gnulib's `dir_check`: is `path` a searchable directory?
    ///
    /// Upstream this is `faccessat(path + "/./", F_OK)`, whose whole purpose is
    /// to make the kernel produce the right `errno` for a name that is not a
    /// directory. Only ever called on a path [`read_link`](Fs::read_link) has
    /// just refused, so it never has a symbolic link at its end.
    ///
    /// # Errors
    ///
    /// [`io::ErrorKind::NotADirectory`] if it exists and is not a directory;
    /// otherwise whatever prevented it being examined.
    fn dir_check(&self, path: &[u8]) -> io::Result<()>;
}

/// The number of symbolic links expanded before cycle bookkeeping starts.
///
/// gnulib's literal `20`. Not a safety limit — see the module docs; it is the
/// point at which recording begins, and it is observable, because it decides
/// which member of a cycle `readlink -m` names.
const CYCLE_WATCH_AFTER: usize = 20;

/// A hard stop, for the case the path-keyed cycle check cannot see.
///
/// gnulib keys on the parent directory's inode and so cannot be fooled; this
/// keys on the parent's canonical path, which a bind mount could make unique
/// forever. Nothing on SlateOS can do that today, and if something ever can,
/// the failure should be `ELOOP` rather than a process that never returns.
///
/// Deliberately far above any real name: a 300-link chain is a measured, legal
/// input that must still resolve.
const MAX_EXPANSIONS: usize = 100_000;

/// Canonicalise `name`: absolute, symlink-free, `.`- and `..`-free.
///
/// This is gnulib's `canonicalize_filename_mode` over bytes. See the module
/// documentation for the measured behaviour of each [`Mode`].
///
/// # Errors
///
/// An empty `name` is [`io::ErrorKind::NotFound`], matching GNU
/// (`readlink -mv ''` prints `'': No such file or directory`). Otherwise the
/// error is the one the filesystem gave for the component that failed, which
/// is what makes `-v` able to distinguish `No such file or directory` from
/// `Not a directory`.
pub fn canonicalize<F: Fs + ?Sized>(fs: &F, name: &[u8], mode: Mode) -> io::Result<Vec<u8>> {
    if name.is_empty() {
        return Err(io::Error::from(io::ErrorKind::NotFound));
    }

    // `rname` is the answer so far. Invariant: it always begins with `/`, and
    // it never ends with one unless it is exactly `/`. gnulib keeps a trailing
    // slash around and trims it at the end; holding the invariant instead
    // means `..` and the final result need no special case.
    let mut rname: Vec<u8> = if name.first() == Some(&b'/') {
        vec![b'/']
    } else {
        let mut here = fs.cwd()?;
        if here.first() != Some(&b'/') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the working directory is not an absolute path",
            ));
        }
        while here.len() > 1 && here.last() == Some(&b'/') {
            here.pop();
        }
        here
    };

    // The text still to be walked, and where in it we are. Expanding a link
    // replaces the whole of `pending`, which is why this is an owned buffer
    // rather than a slice of `name`.
    let mut pending: Vec<u8> = name.to_vec();
    let mut at: usize = 0;

    let mut links: usize = 0;
    let mut expansions: usize = 0;
    let mut seen: HashSet<(Vec<u8>, Vec<u8>)> = HashSet::new();

    loop {
        while pending.get(at) == Some(&b'/') {
            at = at.saturating_add(1);
        }
        let start = at;
        while pending.get(at).is_some_and(|&c| c != b'/') {
            at = at.saturating_add(1);
        }
        if at == start {
            // A zero-length component is the end of the name — either its true
            // end or a run of trailing slashes. gnulib breaks here too, which
            // is why `a/` and `a` canonicalise alike.
            break;
        }

        let comp: Vec<u8> = pending.get(start..at).unwrap_or_default().to_vec();
        // Everything after the component, *including* the separator. Its
        // emptiness is what distinguishes a last component from one followed
        // by a slash, and both acceptance rules below depend on that.
        let rest: Vec<u8> = pending.get(at..).unwrap_or_default().to_vec();

        if comp == b"." {
            continue;
        }
        if comp == b".." {
            pop_component(&mut rname);
            continue;
        }

        // Kept for the cycle key, which is (the directory the component was
        // reached in, the component's name) — see the module docs.
        let parent = rname.clone();
        push_component(&mut rname, &comp);

        match fs.read_link(&rname) {
            Ok(target) => {
                expansions = expansions.saturating_add(1);
                if links < CYCLE_WATCH_AFTER {
                    links = links.saturating_add(1);
                } else if !seen.insert((parent, comp)) || expansions > MAX_EXPANSIONS {
                    if mode == Mode::Missing {
                        // Leave the link unresolved in the answer and carry on
                        // with whatever follows it. gnulib's `goto
                        // next_component`; measured, `readlink -m a/x` on a
                        // cyclic `a` prints `…/a/x`.
                        continue;
                    }
                    // `ErrorKind::FilesystemLoop` is still unstable, so the
                    // condition is carried as its own POSIX wording instead;
                    // see `errmsg::filesystem_loop`.
                    return Err(crate::errmsg::filesystem_loop());
                }

                if target.first() == Some(&b'/') {
                    rname = vec![b'/'];
                } else {
                    // A relative target is relative to the link's *directory*,
                    // so the link's own name comes back off first.
                    pop_component(&mut rname);
                }
                // The target is walked before the rest of the original name.
                pending = target;
                pending.extend_from_slice(&rest);
                at = 0;
            }
            Err(why) => accept(fs, &rname, &rest, mode, why)?,
        }
    }

    Ok(rname)
}

/// Decide whether a component that `readlink` refused is acceptable anyway.
///
/// This is the whole of the difference between the three modes, and it is a
/// transcription of one `if` in gnulib:
///
/// ```c
/// else if (! (can_exist == CAN_MISSING
///             || (suffix_requires_dir_check (end)
///                 ? dir_check (rname, dest)
///                 : errno == EINVAL)
///             || (can_exist == CAN_ALL_BUT_LAST
///                 && errno == ENOENT
///                 && !end[strspn (end, SLASHES)])))
///   goto error;
/// ```
///
/// Read it as three chances to be accepted:
///
/// 1. [`Mode::Missing`] accepts unconditionally — nothing has to exist.
/// 2. Otherwise the component must be shown to be *usable*. Which question
///    that is depends on what follows it: if the suffix demands a directory
///    (see [`suffix_requires_dir_check`]) then it must be one; if not, it is
///    enough that `readlink` failed with `EINVAL`, which means the name exists
///    and simply is not a link.
/// 3. [`Mode::AllButLast`] gets one extra chance: a missing component is fine
///    if nothing but slashes follows it.
///
/// The error reported is the *last* one asked for, not the first: when the
/// directory check runs, its `ENOTDIR` replaces `readlink`'s `EINVAL`, and
/// that replacement is why `readlink -f d/sub/real/` says `Not a directory`.
/// It also feeds rule 3, which is why `readlink -f d/sub/missing/` succeeds —
/// the directory check turned the tolerated `ENOENT` into… `ENOENT`.
///
/// # Errors
///
/// The failure that none of the three rules excused.
fn accept<F: Fs + ?Sized>(
    fs: &F,
    rname: &[u8],
    rest: &[u8],
    mode: Mode,
    why: io::Error,
) -> io::Result<()> {
    if mode == Mode::Missing {
        return Ok(());
    }

    let mut err = why;
    let usable = if suffix_requires_dir_check(rest) {
        match fs.dir_check(rname) {
            Ok(()) => true,
            Err(e) => {
                err = e;
                false
            }
        }
    } else {
        err.kind() == io::ErrorKind::InvalidInput
    };
    if usable {
        return Ok(());
    }

    if mode == Mode::AllButLast
        && err.kind() == io::ErrorKind::NotFound
        && rest.iter().all(|&c| c == b'/')
    {
        return Ok(());
    }

    Err(err)
}

/// Does what follows a component oblige that component to be a directory?
///
/// gnulib's function of the same name, transcribed. `rest` is everything after
/// the component, so it is either empty or starts with a slash.
///
/// The rule is not "does it end in a slash". It is: after the slash, is there
/// *nothing*, or a `.` at the end, or a `..`? Those are the three suffixes the
/// main walk will never check on its own — a trailing slash produces no
/// component at all, `.` is skipped, and `..` is arithmetic on the answer
/// rather than a lookup — so if the component is not a directory, nothing else
/// would ever notice.
///
/// | `rest` | result | because |
/// |---|---|---|
/// | `""` | false | an ordinary last component |
/// | `"/"`, `"//"` | true | a trailing slash asserts "directory" |
/// | `"/."`, `"/./"` | true | `.` is skipped by the walk |
/// | `"/.."`, `"/../x"` | true | `..` is popped by the walk |
/// | `"/x"` | false | `x` will be looked up, and will fail then |
/// | `"/.x"` | false | `.x` is an ordinary name that starts with a dot |
/// | `"/./x"` | false | `x` will be looked up |
#[must_use]
fn suffix_requires_dir_check(rest: &[u8]) -> bool {
    let mut i: usize = 0;
    while rest.get(i) == Some(&b'/') {
        // Two or more slashes act like one.
        while rest.get(i) == Some(&b'/') {
            i = i.saturating_add(1);
        }
        match rest.get(i) {
            // Nothing after the slash: the name ended in `/`.
            None => return true,
            // Possibly `.` or `..`; step past the dot and find out.
            Some(&b'.') => i = i.saturating_add(1),
            // An ordinary component, which the walk will check for itself.
            Some(_) => return false,
        }
        match rest.get(i) {
            // The component was exactly `.`, at the end.
            None => return true,
            // The component was `..`, wherever it is.
            Some(&b'.') if matches!(rest.get(i.saturating_add(1)), None | Some(&b'/')) => {
                return true;
            }
            // A name beginning with a dot, e.g. `.config`. Not a dot component;
            // the outer `while` will now see a non-slash and stop.
            _ => {}
        }
    }
    false
}

/// Append one component, keeping the "no trailing slash except at the root"
/// invariant.
fn push_component(rname: &mut Vec<u8>, comp: &[u8]) {
    if rname.last() != Some(&b'/') {
        rname.push(b'/');
    }
    rname.extend_from_slice(comp);
}

/// Drop the last component. At the root this does nothing, which is what makes
/// `/..` be `/`.
fn pop_component(rname: &mut Vec<u8>) {
    if rname.len() <= 1 {
        return;
    }
    match rname.iter().rposition(|&c| c == b'/') {
        // The component sits directly under the root: `/a` becomes `/`, not ``.
        Some(0) => rname.truncate(1),
        Some(cut) => rname.truncate(cut),
        None => {}
    }
}

/// The real filesystem.
///
/// Deliberately tiny. Everything that could be wrong about canonicalisation is
/// in [`canonicalize`], which is tested against a table instead.
#[derive(Clone, Copy, Debug, Default)]
pub struct RealFs;

impl Fs for RealFs {
    fn cwd(&self) -> io::Result<Vec<u8>> {
        Ok(os_bytes(std::env::current_dir()?.as_os_str()).into_owned())
    }

    #[cfg(unix)]
    fn read_link(&self, path: &[u8]) -> io::Result<Vec<u8>> {
        // One syscall, exactly as gnulib does it. `EINVAL` for a non-symlink
        // arrives as `InvalidInput`, which is what `accept` tests.
        let target = std::fs::read_link(os_from_bytes(path))?;
        Ok(os_bytes(target.as_os_str()).into_owned())
    }

    #[cfg(not(unix))]
    fn read_link(&self, path: &[u8]) -> io::Result<Vec<u8>> {
        // Windows answers a non-symlink with ERROR_NOT_A_REPARSE_POINT, which
        // std leaves uncategorised — so the `EINVAL` the algorithm needs would
        // never appear and `Mode::Existing` would reject every ordinary file.
        // Ask `lstat` first and synthesise it.
        let name = os_from_bytes(path);
        let meta = std::fs::symlink_metadata(&name)?;
        if !meta.file_type().is_symlink() {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        let target = std::fs::read_link(&name)?;
        Ok(os_bytes(target.as_os_str()).into_owned())
    }

    fn dir_check(&self, path: &[u8]) -> io::Result<()> {
        // `metadata` follows symlinks, as `faccessat(…, F_OK)` does.
        if std::fs::metadata(os_from_bytes(path))?.is_dir() {
            Ok(())
        } else {
            Err(io::Error::from(io::ErrorKind::NotADirectory))
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// What a name in [`FakeFs`] is.
    #[derive(Clone, Debug)]
    enum Entry {
        Dir,
        File,
        Link(&'static [u8]),
    }

    /// A filesystem made of a table, so the walk can be tested on a host that
    /// has no POSIX symbolic links and no `/tmp/rl5`.
    ///
    /// Keys are absolute, canonical, and carry no trailing slash — which is
    /// exactly the shape [`canonicalize`] asks about, since it only ever looks
    /// up a path it has already resolved.
    struct FakeFs {
        here: &'static [u8],
        entries: BTreeMap<&'static [u8], Entry>,
    }

    impl FakeFs {
        /// The tree every test below uses:
        ///
        /// ```text
        /// /home/u              cwd
        ///   d/                 a directory
        ///     sub/             a directory
        ///       real           a file
        ///       link    -> real
        ///       dang2   -> nothere
        ///     toplink   -> sub/link
        ///     dangling  -> /nowhere/x
        ///     absdir    -> /home/u/d
        ///     up        -> ../..
        ///   plainfile          a file
        ///   a -> b, b -> a     a two-cycle
        ///   p -> q, q -> r, r -> p     a three-cycle
        /// ```
        fn new() -> Self {
            let mut entries: BTreeMap<&'static [u8], Entry> = BTreeMap::new();
            for dir in [
                &b"/"[..],
                b"/home",
                b"/home/u",
                b"/home/u/d",
                b"/home/u/d/sub",
            ] {
                entries.insert(dir, Entry::Dir);
            }
            entries.insert(b"/home/u/d/sub/real", Entry::File);
            entries.insert(b"/home/u/plainfile", Entry::File);
            entries.insert(b"/home/u/d/sub/link", Entry::Link(b"real"));
            entries.insert(b"/home/u/d/sub/dang2", Entry::Link(b"nothere"));
            entries.insert(b"/home/u/d/toplink", Entry::Link(b"sub/link"));
            entries.insert(b"/home/u/d/dangling", Entry::Link(b"/nowhere/x"));
            entries.insert(b"/home/u/d/absdir", Entry::Link(b"/home/u/d"));
            entries.insert(b"/home/u/d/up", Entry::Link(b"../.."));
            entries.insert(b"/home/u/a", Entry::Link(b"b"));
            entries.insert(b"/home/u/b", Entry::Link(b"a"));
            entries.insert(b"/home/u/p", Entry::Link(b"q"));
            entries.insert(b"/home/u/q", Entry::Link(b"r"));
            entries.insert(b"/home/u/r", Entry::Link(b"p"));
            entries.insert(b"/home/u/x", Entry::Dir);
            entries.insert(b"/home/u/x/y", Entry::Dir);
            entries.insert(b"/home/u/x/y/z", Entry::Link(b"../../p"));
            Self {
                here: b"/home/u",
                entries,
            }
        }

        /// Look a canonical path up, failing the way the kernel would: a proper
        /// ancestor that is not a directory is `ENOTDIR`, an absent name is
        /// `ENOENT`.
        fn lookup(&self, path: &[u8]) -> io::Result<Entry> {
            for cut in 1..path.len() {
                if path.get(cut) != Some(&b'/') {
                    continue;
                }
                let ancestor = path.get(..cut).unwrap_or_default();
                match self.entries.get(ancestor) {
                    Some(Entry::Dir) => {}
                    Some(_) => return Err(io::Error::from(io::ErrorKind::NotADirectory)),
                    None => return Err(io::Error::from(io::ErrorKind::NotFound)),
                }
            }
            self.entries
                .get(path)
                .cloned()
                .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
        }
    }

    impl Fs for FakeFs {
        fn cwd(&self) -> io::Result<Vec<u8>> {
            Ok(self.here.to_vec())
        }

        fn read_link(&self, path: &[u8]) -> io::Result<Vec<u8>> {
            match self.lookup(path)? {
                Entry::Link(target) => Ok(target.to_vec()),
                // `EINVAL`: it is there, and it is not a link.
                _ => Err(io::Error::from(io::ErrorKind::InvalidInput)),
            }
        }

        fn dir_check(&self, path: &[u8]) -> io::Result<()> {
            match self.lookup(path)? {
                Entry::Dir => Ok(()),
                _ => Err(io::Error::from(io::ErrorKind::NotADirectory)),
            }
        }
    }

    /// Canonicalise, and render the answer as text a table can hold: the path,
    /// or `!` and the POSIX wording of the failure.
    ///
    /// The wording rather than the `ErrorKind` for two reasons. It is what a
    /// user actually sees, so the tables below assert the sentence GNU prints
    /// rather than an internal name that merely correlates with it. And one of
    /// the three failures — the loop — has no stable kind to be named by (see
    /// `errmsg::filesystem_loop`), so a kind-keyed table would have to spell it
    /// `!Other` and would then assert nothing at all.
    fn go(name: &str, mode: Mode) -> String {
        match canonicalize(&FakeFs::new(), name.as_bytes(), mode) {
            Ok(p) => String::from_utf8_lossy(&p).into_owned(),
            Err(e) => format!("!{}", crate::errmsg::strerror(&e)),
        }
    }

    /// The nine rows of the module doc's measured table, all three modes each.
    /// This is the test the whole module exists to pass.
    #[test]
    fn every_measured_row_of_the_gnu_table() {
        // (name, -f, -e, -m)
        let rows: &[(&str, &str, &str, &str)] = &[
            (
                "d/sub/link",
                "/home/u/d/sub/real",
                "/home/u/d/sub/real",
                "/home/u/d/sub/real",
            ),
            (
                "d/sub/missing",
                "/home/u/d/sub/missing",
                "!No such file or directory",
                "/home/u/d/sub/missing",
            ),
            (
                "d/sub/missing/",
                "/home/u/d/sub/missing",
                "!No such file or directory",
                "/home/u/d/sub/missing",
            ),
            (
                "d/sub/real/",
                "!Not a directory",
                "!Not a directory",
                "/home/u/d/sub/real",
            ),
            (
                "d/sub/real/..",
                "!Not a directory",
                "!Not a directory",
                "/home/u/d/sub",
            ),
            (
                "plainfile/x",
                "!Not a directory",
                "!Not a directory",
                "/home/u/plainfile/x",
            ),
            (
                "d/nodir/missing",
                "!No such file or directory",
                "!No such file or directory",
                "/home/u/d/nodir/missing",
            ),
            (
                "d/dangling",
                "!No such file or directory",
                "!No such file or directory",
                "/nowhere/x",
            ),
            (
                "d/sub/dang2",
                "/home/u/d/sub/nothere",
                "!No such file or directory",
                "/home/u/d/sub/nothere",
            ),
            (
                "a",
                "!Too many levels of symbolic links",
                "!Too many levels of symbolic links",
                "/home/u/a",
            ),
        ];
        for (name, f, e, m) in rows {
            assert_eq!(&go(name, Mode::AllButLast), f, "readlink -f {name}");
            assert_eq!(&go(name, Mode::Existing), e, "readlink -e {name}");
            assert_eq!(&go(name, Mode::Missing), m, "readlink -m {name}");
        }
    }

    /// Where a cycle stops is decided by gnulib's literal `20`, and it is
    /// visible in the answer. All three of these were measured from GNU.
    #[test]
    fn a_cycle_stops_where_gnu_stops() {
        assert_eq!(go("p", Mode::Missing), "/home/u/r");
        assert_eq!(go("q", Mode::Missing), "/home/u/p");
        assert_eq!(go("r", Mode::Missing), "/home/u/q");
        // Entering the cycle one expansion late shifts the answer by one.
        assert_eq!(go("x/y/z", Mode::Missing), "/home/u/q");
        // The two-cycle lands on whichever member you named.
        assert_eq!(go("a", Mode::Missing), "/home/u/a");
        assert_eq!(go("b", Mode::Missing), "/home/u/b");
        // And whatever followed the link is still walked.
        assert_eq!(go("a/k", Mode::Missing), "/home/u/a/k");
    }

    /// A long chain is not a cycle. GNU resolves 300 links where the kernel
    /// refuses at 40, so a depth limit would reject a legal name.
    #[test]
    fn three_hundred_links_is_not_a_loop() {
        // Built here rather than in `FakeFs::new` because it needs owned keys.
        struct Chain {
            depth: usize,
        }
        impl Fs for Chain {
            fn cwd(&self) -> io::Result<Vec<u8>> {
                Ok(b"/c".to_vec())
            }
            fn read_link(&self, path: &[u8]) -> io::Result<Vec<u8>> {
                let name = path.rsplit(|&c| c == b'/').next().unwrap_or_default();
                if name == b"end" {
                    return Err(io::Error::from(io::ErrorKind::InvalidInput));
                }
                let Some(n) = name
                    .strip_prefix(b"l")
                    .and_then(|d| std::str::from_utf8(d).ok())
                    .and_then(|d| d.parse::<usize>().ok())
                else {
                    return Err(io::Error::from(io::ErrorKind::NotFound));
                };
                if n > self.depth {
                    return Err(io::Error::from(io::ErrorKind::NotFound));
                }
                Ok(if n == 1 {
                    b"end".to_vec()
                } else {
                    format!("l{}", n.saturating_sub(1)).into_bytes()
                })
            }
            fn dir_check(&self, _path: &[u8]) -> io::Result<()> {
                Err(io::Error::from(io::ErrorKind::NotADirectory))
            }
        }
        let fs = Chain { depth: 300 };
        for mode in [Mode::Existing, Mode::AllButLast, Mode::Missing] {
            let got = canonicalize(&fs, b"l300", mode).unwrap();
            assert_eq!(got, b"/c/end", "a 300-link chain must resolve in {mode:?}");
        }
    }

    #[test]
    fn dots_and_repeated_slashes_disappear() {
        assert_eq!(
            go("d/./sub/../sub/real", Mode::Missing),
            "/home/u/d/sub/real"
        );
        assert_eq!(go("d//sub///real", Mode::Missing), "/home/u/d/sub/real");
        assert_eq!(go("///a//b", Mode::Missing), "/a/b");
        assert_eq!(go(".", Mode::AllButLast), "/home/u");
        assert_eq!(go("./", Mode::AllButLast), "/home/u");
        assert_eq!(go("/", Mode::Existing), "/");
        assert_eq!(go("//", Mode::Existing), "/");
    }

    /// `..` is popped off the *resolved* answer, not off the text the user
    /// typed, so a symbolic link changes what `..` means.
    #[test]
    fn dotdot_is_physical_and_stops_at_the_root() {
        // `d/sub/link` resolves to `d/sub/real`, so `..` from it is `d/sub`.
        assert_eq!(go("d/sub/link/..", Mode::Missing), "/home/u/d/sub");
        assert_eq!(go("/..", Mode::Missing), "/");
        assert_eq!(go("/../../..", Mode::Missing), "/");
        assert_eq!(go("../..", Mode::Missing), "/");
        // `up` is a link to `../..`, and the `..`s count from the *link's own
        // directory* — `/home/u/d` — not from where the user typed the name and
        // not from the cwd. Two levels up from there is `/home`. Getting this
        // wrong is the classic canonicaliser bug: it is why the link's name has
        // to come off `rname` (`pop_component`) before the target is spliced in.
        assert_eq!(go("d/up", Mode::AllButLast), "/home");
        assert_eq!(go("d/up/k", Mode::Missing), "/home/k");
    }

    #[test]
    fn an_absolute_link_target_restarts_at_the_root() {
        assert_eq!(go("d/absdir", Mode::Existing), "/home/u/d");
        assert_eq!(
            go("d/absdir/sub/real", Mode::Existing),
            "/home/u/d/sub/real"
        );
        assert_eq!(go("d/toplink", Mode::Existing), "/home/u/d/sub/real");
    }

    /// The empty name is `ENOENT`, not an empty answer — measured:
    /// `readlink -mv ''` prints `'': No such file or directory` even though
    /// `-m` excuses everything else.
    #[test]
    fn the_empty_name_is_not_found_in_every_mode() {
        for mode in [Mode::Existing, Mode::AllButLast, Mode::Missing] {
            assert_eq!(go("", mode), "!No such file or directory", "in {mode:?}");
        }
    }

    /// A name is bytes. Nothing here may assume UTF-8, and a component that is
    /// not valid UTF-8 must survive the round trip unchanged.
    #[test]
    fn a_component_that_is_not_utf8_survives() {
        struct OneBadName;
        impl Fs for OneBadName {
            fn cwd(&self) -> io::Result<Vec<u8>> {
                Ok(b"/\xff\xfe".to_vec())
            }
            fn read_link(&self, path: &[u8]) -> io::Result<Vec<u8>> {
                if path == b"/\xff\xfe/l" {
                    return Ok(b"\x80target".to_vec());
                }
                Err(io::Error::from(io::ErrorKind::InvalidInput))
            }
            fn dir_check(&self, _path: &[u8]) -> io::Result<()> {
                Ok(())
            }
        }
        let got = canonicalize(&OneBadName, b"l", Mode::Existing).unwrap();
        assert_eq!(got, b"/\xff\xfe/\x80target");
    }

    /// The rule that decides whether a trailing slash is fatal. Every row here
    /// is one line of gnulib's `suffix_requires_dir_check`.
    #[test]
    fn the_dir_check_rule_matches_gnulib() {
        for (rest, want) in [
            (&b""[..], false),
            (b"/", true),
            (b"//", true),
            (b"/.", true),
            (b"/./", true),
            (b"/..", true),
            (b"/../", true),
            (b"/../x", true),
            (b"//..//", true),
            (b"/x", false),
            (b"/.x", false),
            (b"/./x", false),
            (b"/..x", false),
            (b"/x/..", false),
        ] {
            assert_eq!(
                suffix_requires_dir_check(rest),
                want,
                "suffix {:?}",
                String::from_utf8_lossy(rest)
            );
        }
    }

    #[test]
    fn a_relative_cwd_is_refused_rather_than_pasted_in() {
        struct BadCwd;
        impl Fs for BadCwd {
            fn cwd(&self) -> io::Result<Vec<u8>> {
                Ok(b"not/absolute".to_vec())
            }
            fn read_link(&self, _path: &[u8]) -> io::Result<Vec<u8>> {
                Err(io::Error::from(io::ErrorKind::InvalidInput))
            }
            fn dir_check(&self, _path: &[u8]) -> io::Result<()> {
                Ok(())
            }
        }
        let e = canonicalize(&BadCwd, b"x", Mode::Missing).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
        // An absolute name never asks for the working directory, so it still
        // works.
        assert_eq!(
            canonicalize(&BadCwd, b"/x", Mode::Missing).unwrap(),
            b"/x".to_vec()
        );
    }

    #[test]
    fn pushing_and_popping_keep_the_no_trailing_slash_invariant() {
        let mut p = b"/".to_vec();
        push_component(&mut p, b"a");
        assert_eq!(p, b"/a".to_vec());
        push_component(&mut p, b"b");
        assert_eq!(p, b"/a/b".to_vec());
        pop_component(&mut p);
        assert_eq!(p, b"/a".to_vec());
        pop_component(&mut p);
        assert_eq!(p, b"/".to_vec());
        pop_component(&mut p);
        assert_eq!(p, b"/".to_vec(), "the root has no parent");
    }
}
