//! chown — change file owner and group.
//!
//! Usage: chown [OPTION]... OWNER[:GROUP] FILE...
//!   -R, --recursive        operate on directories recursively
//!   -h, --no-dereference   change the symlink itself, not what it points to
//!       --dereference      change what a symlink points to (the default)
//!   -H                     with -R, follow a symlink named on the command line
//!   -L                     with -R, follow every symlink encountered
//!   -P                     with -R, follow no symlinks (the default)
//!   -v, --verbose          report every file processed
//!   -f, --silent, --quiet  suppress most error messages
//!   --                     end of options
//!
//! OWNER and GROUP are numeric UIDs/GIDs (name lookup not yet supported; see
//! `design-decisions.md` §353 on `/etc/users.yaml`).
//!
//! # Why the symlink rules are the important part of this file
//!
//! `chown` hands a file to another user, so an implementation that can be
//! talked into handing over the *wrong* file is a way to take over an account.
//! Two rules keep that from happening, and both were once broken
//! (`known-issues.md` → `B-chown-FOLLOWS-SYMLINKS-WHILE-RECURSING`):
//!
//! 1. **`-R` does not follow symlinks unless asked.** It used to test
//!    `path.is_dir()`, which follows them, so `chown -R alice srv/` on a tree
//!    containing `srv/x -> /etc` walked into `/etc` and gave alice the lot.
//!    POSIX makes `-P` the default for exactly this reason; `-L` restores the
//!    old behaviour for anyone who genuinely wants it, and then only with
//!    symlink-loop protection.
//! 2. **A symlink met during traversal is changed, not its target.** The
//!    `chown(2)` call follows links, so `srv/x -> /etc/shadow` used to hand
//!    `/etc/shadow` to alice. Traversal now uses `lchown(2)`.
//!
//! `-r` is **not** accepted as a spelling of `-R`. It is not an option at all
//! in POSIX chown, and quietly treating a typo as "recurse" is how a change
//! meant for one file reaches a whole tree.
//!
//! Built only on unix-family targets (our x86_64-slateos presents as
//! linux-musl, so `cfg(unix)` matches).  On non-unix hosts (e.g.
//! Windows when running `cargo test --workspace`), a stub `main` keeps
//! the workspace compile-clean.
//!
//! The pure helpers (`parse_args`, `parse_owner_group`) are exposed on
//! all platforms so they can be unit-tested anywhere.

#![cfg_attr(not(unix), allow(dead_code))]

#[cfg(not(unix))]
fn main() {
    eprintln!("chown: unix-only utility; not supported on this platform");
    std::process::exit(1);
}

// `quote` is not `cfg(unix)`-gated because `parse_owner_group` is not: the
// pure helpers are compiled everywhere so they can be unit-tested on the
// host, and they are what report a bad `OWNER[:GROUP]`.
use coreutils::quote::quote;
#[cfg(unix)]
use coreutils::quote::quotef_os;
#[cfg(unix)]
use std::collections::HashSet;
#[cfg(unix)]
use std::env;
#[cfg(unix)]
use std::fs::{self, Metadata};
#[cfg(unix)]
use std::io;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(unix)]
use std::path::Path;
#[cfg(unix)]
use std::process;

// libc-level chown/lchown — our POSIX layer provides both. `lchown` is what
// makes rule 2 above enforceable: `chown` follows symlinks by definition, so
// there is no flag that makes it safe during traversal.
#[cfg(unix)]
unsafe extern "C" {
    fn chown(path: *const u8, owner: u32, group: u32) -> i32;
    fn lchown(path: *const u8, owner: u32, group: u32) -> i32;
}

/// POSIX's "leave this field alone" sentinel for `chown(2)`: `(uid_t)-1`.
///
/// Passing it is better than reading the current owner and passing that back:
/// the read-then-write version has a window in which the file can be replaced,
/// and it turns a no-op field into a real ownership write.
const UNCHANGED: u32 = u32::MAX;

/// Which symlinks a recursive run may walk through. POSIX's `-H`/`-L`/`-P`.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
enum Traverse {
    /// `-P`, and the default: follow none. A symlink is a thing to change,
    /// never a door to walk through.
    #[default]
    Never,
    /// `-H`: follow a symlink named on the command line, but none found
    /// inside the tree.
    CommandLine,
    /// `-L`: follow every symlink. Needs loop protection, which is why the
    /// traversal carries a visited set.
    Always,
}

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct ChownArgs {
    recursive: bool,
    /// `-h`: act on the link itself even for a command-line operand.
    no_dereference: bool,
    traverse: Traverse,
    verbose: bool,
    /// `-f`: keep going quietly. The exit status still reflects the failures;
    /// only the messages are suppressed.
    quiet: bool,
    owner: String,
    paths: Vec<String>,
}

/// Parse chown's argv. The first positional is the owner spec (`UID`,
/// `UID:GID`, `:GID`, or `UID:`); the rest are paths.
///
/// Unknown options are an error. They used to be silently accepted as
/// positionals, so `chown 1000 -v file` tried to change the owner of a file
/// literally named `-v` and reported "No such file or directory" about it.
fn parse_args(args: &[String]) -> Result<ChownArgs, String> {
    let mut out = ChownArgs::default();
    let mut positional: Vec<&str> = Vec::new();
    let mut end_of_options = false;

    for arg in args {
        if end_of_options {
            positional.push(arg);
            continue;
        }
        match arg.as_str() {
            // `--` is the only way to name a file that begins with a dash.
            "--" => end_of_options = true,
            "-R" | "--recursive" => out.recursive = true,
            "-h" | "--no-dereference" => out.no_dereference = true,
            "--dereference" => out.no_dereference = false,
            "-H" => out.traverse = Traverse::CommandLine,
            "-L" => out.traverse = Traverse::Always,
            "-P" => out.traverse = Traverse::Never,
            "-v" | "--verbose" => out.verbose = true,
            "-f" | "--silent" | "--quiet" => out.quiet = true,
            // A bare `-` is not an option; it falls through as a positional
            // and will fail as a bad owner spec or a missing file, which is
            // what GNU does with it.
            other if other.starts_with("--") => {
                return Err(format!("unrecognized option '{other}'"));
            }
            other if other.starts_with('-') && other.len() > 1 => {
                // Notably including `-r`. POSIX chown has no such option, and
                // reading it as `-R` would silently turn a one-file change
                // into a whole-tree one.
                return Err(format!(
                    "invalid option -- '{}'",
                    other.trim_start_matches('-')
                ));
            }
            other => positional.push(other),
        }
    }

    if positional.len() < 2 {
        return Err("missing operand".to_string());
    }

    out.owner = positional.first().copied().unwrap_or_default().to_string();
    out.paths = positional
        .get(1..)
        .unwrap_or(&[])
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    Ok(out)
}

/// Whether a **command-line operand** is resolved through a symlink
/// (`chown(2)`) or changed as the link itself (`lchown(2)`).
///
/// Pure, and compiled on every platform, because it is the decision the
/// symlink bug was made of and the `Runner` around it only exists under
/// `cfg(unix)` — an untestable security rule is one that regresses quietly.
///
/// * Without `-R` the default is to follow: `chown alice link` is
///   conventionally about the file, and `-h` is how you say otherwise.
/// * With `-R` the default is `-P`, so a command-line symlink is *changed*
///   rather than walked into unless `-H` or `-L` overrides it.
fn follow_operand(recursive: bool, no_dereference: bool, traverse: Traverse) -> bool {
    if no_dereference {
        return false;
    }
    !recursive || traverse != Traverse::Never
}

/// Whether an entry found **inside** a traversal is followed.
///
/// Only `-L` says yes. `-H`'s exception is the command line, and it has
/// already been spent by the time this is asked; `-P` never follows. This is
/// the rule whose absence let `chown -R` walk out of the tree it was given.
fn follow_child(traverse: Traverse) -> bool {
    traverse == Traverse::Always
}

/// Parse an `OWNER[:GROUP]` spec into (uid, gid).  `None` for a field
/// means "leave unchanged".  Empty owner with `:GROUP` is allowed (gid-
/// only change); empty group after `:` is allowed (uid-only).
fn parse_owner_group(spec: &str) -> Result<(Option<u32>, Option<u32>), String> {
    if let Some((owner_str, group_str)) = spec.split_once(':') {
        let uid = if owner_str.is_empty() {
            None
        } else {
            Some(owner_str.parse::<u32>().map_err(|_| {
                format!(
                    "invalid user: {} (only numeric UIDs supported)",
                    quote(owner_str.as_bytes())
                )
            })?)
        };
        let gid = if group_str.is_empty() {
            None
        } else {
            Some(group_str.parse::<u32>().map_err(|_| {
                format!(
                    "invalid group: {} (only numeric GIDs supported)",
                    quote(group_str.as_bytes())
                )
            })?)
        };
        Ok((uid, gid))
    } else {
        let uid = spec.parse::<u32>().map_err(|_| {
            format!(
                "invalid user: {} (only numeric UIDs supported)",
                quote(spec.as_bytes())
            )
        })?;
        Ok((Some(uid), None))
    }
}

#[cfg(unix)]
struct Runner {
    uid: u32,
    gid: u32,
    verbose: bool,
    quiet: bool,
    traverse: Traverse,
    no_dereference: bool,
    /// `(dev, ino)` of every directory already entered, so `-L` on a tree that
    /// links back to one of its own ancestors terminates instead of recursing
    /// until the stack runs out.
    seen: HashSet<(u64, u64)>,
    status: i32,
}

#[cfg(unix)]
impl Runner {
    fn fail(&mut self, path: &Path, e: &dyn std::fmt::Display) {
        if !self.quiet {
            eprintln!("chown: {}: {e}", quotef_os(path));
        }
        self.status = 1;
    }

    /// Change one file's owner. `follow` decides between `chown(2)` and
    /// `lchown(2)`; there is no third option, and getting it wrong is the
    /// whole bug this file exists to not have.
    fn apply(&mut self, path: &Path, follow: bool) {
        // Paths are bytes. The previous version went through `to_str()` and
        // refused anything that was not UTF-8, which on this OS is a legal
        // filename (`design.txt`: every byte but `/` and NUL).
        let bytes = path.as_os_str().as_bytes();
        if bytes.contains(&0) {
            self.fail(path, &"path contains a NUL byte");
            return;
        }
        let mut c_path: Vec<u8> = Vec::with_capacity(bytes.len().saturating_add(1));
        c_path.extend_from_slice(bytes);
        c_path.push(0);

        // SAFETY: `c_path` is NUL-terminated, contains no interior NUL, and
        // outlives the call. Both functions are provided by the POSIX layer
        // and take a borrowed C string they do not retain.
        let ret = unsafe {
            if follow {
                chown(c_path.as_ptr(), self.uid, self.gid)
            } else {
                lchown(c_path.as_ptr(), self.uid, self.gid)
            }
        };
        if ret != 0 {
            // The previous message was the literal string "chown failed
            // (errno)", which told the user nothing: permission denied, no
            // such file and read-only filesystem were indistinguishable.
            self.fail(path, &io::Error::last_os_error());
            return;
        }
        if self.verbose {
            println!("changed ownership of {}", quotef_os(path));
        }
    }

    /// Stat `path`, following symlinks or not as `follow` says.
    fn stat(&mut self, path: &Path, follow: bool) -> Option<Metadata> {
        let r = if follow {
            fs::metadata(path)
        } else {
            fs::symlink_metadata(path)
        };
        match r {
            Ok(m) => Some(m),
            Err(e) => {
                self.fail(path, &e);
                None
            }
        }
    }

    /// Process one command-line operand.
    fn run_operand(&mut self, path: &Path, recursive: bool) {
        let follow = follow_operand(recursive, self.no_dereference, self.traverse);
        if recursive {
            self.walk(path, follow);
        } else {
            self.apply(path, follow);
        }
    }

    /// Recursively change `path`. `follow` applies to `path` itself; whether
    /// its children are followed is decided by `-L` alone.
    fn walk(&mut self, path: &Path, follow: bool) {
        let Some(meta) = self.stat(path, follow) else {
            return;
        };

        // A symlink we are not following is a leaf: change the link, do not
        // look at what is on the other side of it. `is_dir()` on the old code
        // answered about the *target*, which is how `-R` escaped the tree.
        if !meta.is_dir() {
            self.apply(path, follow);
            return;
        }

        // Change the directory before descending, so an interrupted run
        // leaves the tree partly done from the top rather than the bottom.
        self.apply(path, follow);

        if !self.seen.insert((meta.dev(), meta.ino())) {
            // Only reachable under `-L`, where a symlink can point at an
            // ancestor. Without this the recursion is unbounded.
            if !self.quiet {
                eprintln!(
                    "chown: {}: directory loop detected; not descending again",
                    quotef_os(path)
                );
            }
            self.status = 1;
            return;
        }

        let entries = match fs::read_dir(path) {
            Ok(e) => e,
            Err(e) => {
                self.fail(path, &e);
                return;
            }
        };
        for entry in entries {
            match entry {
                Ok(entry) => {
                    let child_follow = follow_child(self.traverse);
                    self.walk(&entry.path(), child_follow);
                }
                Err(e) => self.fail(path, &e),
            }
        }
    }
}

#[cfg(unix)]
fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let parsed = match parse_args(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("chown: {e}");
            eprintln!("Usage: chown [-R] [-h] [-H|-L|-P] [-v] [-f] [--] OWNER[:GROUP] FILE...");
            process::exit(1);
        }
    };

    let (uid, gid) = match parse_owner_group(&parsed.owner) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("chown: {e}");
            process::exit(1);
        }
    };

    let mut runner = Runner {
        uid: uid.unwrap_or(UNCHANGED),
        gid: gid.unwrap_or(UNCHANGED),
        verbose: parsed.verbose,
        quiet: parsed.quiet,
        traverse: parsed.traverse,
        no_dereference: parsed.no_dereference,
        seen: HashSet::new(),
        status: 0,
    };

    for path_str in &parsed.paths {
        runner.run_operand(Path::new(path_str), parsed.recursive);
    }

    process::exit(runner.status);
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    // ---------------- parse_args ----------------

    #[test]
    fn parse_no_args_errors() {
        let err = parse_args(&s(&[])).unwrap_err();
        assert!(err.contains("missing operand"));
    }

    #[test]
    fn parse_owner_only_errors() {
        let err = parse_args(&s(&["1000"])).unwrap_err();
        assert!(err.contains("missing operand"));
    }

    #[test]
    fn parse_owner_and_file() {
        let a = parse_args(&s(&["1000", "f"])).unwrap();
        assert!(!a.recursive);
        assert_eq!(a.owner, "1000");
        assert_eq!(a.paths, vec!["f"]);
    }

    #[test]
    fn parse_recursive_dash_r_uppercase() {
        let a = parse_args(&s(&["-R", "1000:100", "dir"])).unwrap();
        assert!(a.recursive);
        assert_eq!(a.owner, "1000:100");
        assert_eq!(a.paths, vec!["dir"]);
    }

    /// POSIX chown has no `-r`. This used to set `recursive`, so a typo turned
    /// a single-file change into a whole-tree one -- and a test asserted that
    /// it did. See known-issues.md -> B-chown-FOLLOWS-SYMLINKS-WHILE-RECURSING.
    #[test]
    fn parse_lowercase_r_is_rejected_not_treated_as_recursive() {
        let err = parse_args(&s(&["-r", "1000", "f"])).unwrap_err();
        assert!(err.contains("invalid option"), "got: {err}");
        assert!(err.contains('r'));
    }

    #[test]
    fn parse_multiple_files() {
        let a = parse_args(&s(&["0:0", "a", "b", "c"])).unwrap();
        assert_eq!(a.paths, vec!["a", "b", "c"]);
    }

    #[test]
    fn parse_recursive_flag_position_independent() {
        let a = parse_args(&s(&["1000", "-R", "dir"])).unwrap();
        assert!(a.recursive);
        assert_eq!(a.owner, "1000");
        assert_eq!(a.paths, vec!["dir"]);
    }

    #[test]
    fn parse_long_recursive() {
        let a = parse_args(&s(&["--recursive", "1000", "dir"])).unwrap();
        assert!(a.recursive);
    }

    #[test]
    fn parse_no_dereference() {
        let a = parse_args(&s(&["-h", "1000", "link"])).unwrap();
        assert!(a.no_dereference);
        let a = parse_args(&s(&["--no-dereference", "1000", "link"])).unwrap();
        assert!(a.no_dereference);
    }

    #[test]
    fn parse_dereference_overrides_h() {
        // Last one wins, as with GNU.
        let a = parse_args(&s(&["-h", "--dereference", "1000", "link"])).unwrap();
        assert!(!a.no_dereference);
    }

    #[test]
    fn parse_traverse_defaults_to_never() {
        // -P is the default: without it, `-R` on a tree containing a symlink
        // to /etc would walk into /etc.
        let a = parse_args(&s(&["-R", "1000", "d"])).unwrap();
        assert_eq!(a.traverse, Traverse::Never);
    }

    #[test]
    fn parse_traverse_flags() {
        assert_eq!(
            parse_args(&s(&["-R", "-H", "1000", "d"])).unwrap().traverse,
            Traverse::CommandLine
        );
        assert_eq!(
            parse_args(&s(&["-R", "-L", "1000", "d"])).unwrap().traverse,
            Traverse::Always
        );
        assert_eq!(
            parse_args(&s(&["-R", "-L", "-P", "1000", "d"]))
                .unwrap()
                .traverse,
            Traverse::Never
        );
    }

    #[test]
    fn parse_verbose_and_quiet() {
        let a = parse_args(&s(&["-v", "-f", "1000", "f"])).unwrap();
        assert!(a.verbose);
        assert!(a.quiet);
    }

    #[test]
    fn parse_unknown_short_option_rejected() {
        let err = parse_args(&s(&["-z", "1000", "f"])).unwrap_err();
        assert!(err.contains("invalid option"));
    }

    #[test]
    fn parse_unknown_long_option_rejected() {
        let err = parse_args(&s(&["--frobnicate", "1000", "f"])).unwrap_err();
        assert!(err.contains("unrecognized option"));
    }

    #[test]
    fn parse_double_dash_ends_options() {
        // The only way to address a file called `-R`.
        let a = parse_args(&s(&["--", "1000", "-R", "-h"])).unwrap();
        assert!(!a.recursive);
        assert!(!a.no_dereference);
        assert_eq!(a.owner, "1000");
        assert_eq!(a.paths, vec!["-R", "-h"]);
    }

    #[test]
    fn parse_bare_dash_is_positional() {
        let a = parse_args(&s(&["1000", "-"])).unwrap();
        assert_eq!(a.paths, vec!["-"]);
    }

    // ---------------- symlink policy ----------------
    //
    // This is the security rule, stated once. `chown -R` used to test
    // `path.is_dir()`, which resolves symlinks, and then call `chown(2)`,
    // which also resolves them -- so a symlink inside the tree was both a
    // door out of it and a way to hand its target away. known-issues.md ->
    // B-chown-FOLLOWS-SYMLINKS-WHILE-RECURSING.

    #[test]
    fn follow_operand_without_r_dereferences_by_default() {
        // `chown alice link` is about the file, not the link.
        assert!(follow_operand(false, false, Traverse::Never));
    }

    #[test]
    fn follow_operand_h_wins_everywhere() {
        for recursive in [false, true] {
            for t in [Traverse::Never, Traverse::CommandLine, Traverse::Always] {
                assert!(!follow_operand(recursive, true, t), "{recursive} {t:?}");
            }
        }
    }

    #[test]
    fn follow_operand_with_r_defaults_to_not_following() {
        // The POSIX default is -P. This single `false` is what stops
        // `chown -R alice srv/` from walking into `/etc` via `srv/x -> /etc`.
        assert!(!follow_operand(true, false, Traverse::Never));
    }

    #[test]
    fn follow_operand_h_and_l_opt_back_in() {
        assert!(follow_operand(true, false, Traverse::CommandLine));
        assert!(follow_operand(true, false, Traverse::Always));
    }

    #[test]
    fn follow_child_only_under_dash_l() {
        assert!(!follow_child(Traverse::Never));
        // -H's exception is the command line only, and it is already spent.
        assert!(!follow_child(Traverse::CommandLine));
        assert!(follow_child(Traverse::Always));
    }

    #[test]
    fn default_args_never_follow_a_symlink_during_recursion() {
        // End to end through the parser: the plain, everyday invocation.
        let a = parse_args(&s(&["-R", "1000", "dir"])).unwrap();
        assert!(!follow_operand(a.recursive, a.no_dereference, a.traverse));
        assert!(!follow_child(a.traverse));
    }

    // ---------------- parse_owner_group ----------------

    #[test]
    fn pog_owner_only() {
        assert_eq!(parse_owner_group("1000").unwrap(), (Some(1000), None));
    }

    #[test]
    fn pog_owner_and_group() {
        assert_eq!(
            parse_owner_group("1000:100").unwrap(),
            (Some(1000), Some(100))
        );
    }

    #[test]
    fn pog_owner_with_empty_group() {
        // "1000:" — uid set, gid unchanged.
        assert_eq!(parse_owner_group("1000:").unwrap(), (Some(1000), None));
    }

    #[test]
    fn pog_empty_owner_with_group() {
        // ":100" — uid unchanged, gid set.
        assert_eq!(parse_owner_group(":100").unwrap(), (None, Some(100)));
    }

    #[test]
    fn pog_both_empty() {
        // ":" — both unchanged (no-op).
        assert_eq!(parse_owner_group(":").unwrap(), (None, None));
    }

    #[test]
    fn pog_root() {
        assert_eq!(parse_owner_group("0").unwrap(), (Some(0), None));
        assert_eq!(parse_owner_group("0:0").unwrap(), (Some(0), Some(0)));
    }

    #[test]
    fn pog_non_numeric_owner_errors() {
        let err = parse_owner_group("alice").unwrap_err();
        assert!(err.contains("invalid user"));
        assert!(err.contains("alice"));
    }

    #[test]
    fn pog_non_numeric_group_errors() {
        let err = parse_owner_group("1000:wheel").unwrap_err();
        assert!(err.contains("invalid group"));
        assert!(err.contains("wheel"));
    }

    #[test]
    fn pog_non_numeric_owner_with_group_errors() {
        let err = parse_owner_group("alice:100").unwrap_err();
        assert!(err.contains("invalid user"));
    }

    #[test]
    fn pog_max_u32() {
        // 4294967295 is `(uid_t)-1`, which the kernel reads as "leave this
        // field alone" -- so asking for it is indistinguishable from asking
        // for nothing. Parsing still accepts it; the collision is noted in
        // known-issues.md rather than papered over here.
        assert_eq!(
            parse_owner_group("4294967295:4294967295").unwrap(),
            (Some(u32::MAX), Some(u32::MAX)),
        );
    }

    #[test]
    fn pog_negative_errors() {
        let err = parse_owner_group("-1").unwrap_err();
        assert!(err.contains("invalid user"));
    }

    #[test]
    fn unchanged_sentinel_is_posix_minus_one() {
        // `(uid_t)-1`. The POSIX layer short-circuits a call where both
        // fields are this, so an omitted field costs nothing.
        assert_eq!(UNCHANGED, u32::MAX);
    }
}
