//! `chown-core.c`: the walk and the reporting that `chown` and `chgrp` share.
//!
//! Upstream builds both programs over one `chown_files`, so they cannot
//! disagree about which files a recursive run reaches, whether a symlink or its
//! target is changed, or how `-v` words a change. Until `chgrp` existed this was
//! the private half of `chown.rs`; a second copy in `chgrp.rs` would have been
//! a second answer to the same question -- the one question in this family
//! where a wrong answer hands a file to the wrong owner.
//!
//! # The two settings a walk turns on
//!
//! Upstream's `main` reduces the symlink options to two independent things, and
//! this module takes them in that form ([`walk_policy`] is the reduction):
//!
//! * **Which symlinks the walk goes *through*** ([`Traverse`], `-P`/`-H`/`-L`,
//!   the last one given winning). Only meaningful with `-R`; without it the
//!   walk is `-P`.
//! * **Whether a symlink it meets is changed, or its target is**
//!   (`affect_referent`, `--dereference`/`-h`). This applies to *every* entry,
//!   on the command line or inside the tree.
//!
//! The defaults are what keep `chown -R` from being talked into handing over a
//! file outside the tree it was given (`known-issues.md` →
//! `B-chown-FOLLOWS-SYMLINKS-WHILE-RECURSING`): with `-R` and no `-H`/`-L`,
//! nothing is walked through *and* every symlink is changed as a link, and
//! `-R --dereference` without `-H`/`-L` is refused outright. With `-H` or `-L`
//! the caller has asked for symlinks to be followed, and then -- measured
//! against GNU 9.4 -- a symlink met inside the tree has its *target* changed
//! unless `-h` is also given, and `-L -h` changes links even while walking
//! through them. The two settings are kept apart here because conflating them
//! was exactly how an earlier version got both of those last two wrong.
//!
//! # The walk is `fts`'s
//!
//! Upstream walks with gnulib's `fts`, and several of its properties are
//! visible in what `chown` prints:
//!
//! * a directory is changed **after** its contents under `-R` (the post-order
//!   visit), which is also the order `-v` reports in;
//! * a directory that cannot be read is reported and **not** changed;
//! * a directory already on the current path -- a symlink loop under `-L` --
//!   is changed but not entered again, and silently: `fts` calls that a cycle
//!   only when no symlinks were being followed, where it means a corrupted
//!   filesystem, and then it prints its long warning;
//! * a command-line name that cannot be stat'd is tried once more before it is
//!   reported, since the run itself may have made it reachable.
//!
//! # One deliberate difference
//!
//! When the target of a symlink cannot be looked up (`cannot dereference`),
//! upstream goes on to report `-v`'s `from` half out of a `struct stat` the
//! failed `fstatat` never filled -- uninitialised stack memory, which reads
//! `from 30186:root` on one run and `from 3526354685` on the next (measured).
//! Here that line has no `from` half, as it does when the name itself could
//! not be stat'd.

use crate::quote::{escape_unprintable, quoteaf_os};
use std::ffi::OsStr;

/// How much a run says about each file it visits.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Verbosity {
    /// `-v`: a line for every file, changed or not.
    High,
    /// `-c`: a line only for a file whose ownership actually moved.
    ChangesOnly,
    /// The default: nothing.
    #[default]
    Off,
}

/// Which symlinks to directories a recursive walk goes through: POSIX's
/// `-P`, `-H` and `-L`.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Traverse {
    /// `-P`, and the default: none.
    #[default]
    Physical,
    /// `-H`: one named on the command line, and none found inside the tree.
    CommandLine,
    /// `-L`: every one.
    Logical,
}

/// `--dereference` and `-h` as given -- the last one wins -- or neither.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Dereference {
    /// Neither was given.
    #[default]
    Unspecified,
    /// `--dereference`: change what a symlink points at.
    Referent,
    /// `-h`, `--no-dereference`: change the symlink itself.
    Link,
}

/// Upstream's refusal of `-R --dereference` with nothing to walk through.
pub const DEREFERENCE_NEEDS_TRAVERSAL: &str = "-R --dereference requires either -H or -L";

/// Upstream `main`'s reduction of `-R`, `-H`/`-L`/`-P` and
/// `--dereference`/`-h` to the two settings the walk consumes: the traversal
/// actually in effect, and whether a symlink's target is changed rather than
/// the link.
///
/// # Errors
///
/// [`DEREFERENCE_NEEDS_TRAVERSAL`] for `-R --dereference` under `-P`: asked
/// to change every symlink's target while walking through none of them.
pub fn walk_policy(
    recursive: bool,
    traverse: Traverse,
    dereference: Dereference,
) -> Result<(Traverse, bool), &'static str> {
    if !recursive {
        // Without -R nothing is walked, so the traversal is moot and forced to
        // -P; a symlink operand is still changed through unless -h.
        return Ok((Traverse::Physical, dereference != Dereference::Link));
    }
    if traverse == Traverse::Physical {
        if dereference == Dereference::Referent {
            return Err(DEREFERENCE_NEEDS_TRAVERSAL);
        }
        // `dereference = 0`: under -R -P every symlink is changed as a link.
        return Ok((Traverse::Physical, false));
    }
    Ok((traverse, dereference != Dereference::Link))
}

/// `chown-core.c`'s `user_group_str`: `USER:GROUP`, or whichever half exists.
///
/// `None` when neither does, which is a distinct case rather than an empty
/// string -- it selects a different sentence in [`describe_change`].
#[must_use]
pub fn user_group_str(user: Option<&[u8]>, group: Option<&[u8]>) -> Option<Vec<u8>> {
    match (user, group) {
        (Some(user), Some(group)) => {
            let mut out = user.to_vec();
            out.push(b':');
            out.extend_from_slice(group);
            Some(out)
        }
        (Some(one), None) | (None, Some(one)) => Some(one.to_vec()),
        (None, None) => None,
    }
}

/// What happened to one file, as `chown-core.c`'s `enum Change_status`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChangeStatus {
    Succeeded,
    Failed,
    /// The ownership was already what was asked for -- or `--from` did not
    /// match, which upstream reports the same way and does not call an error.
    NoChangeRequested,
    /// `lchown` on a symlink was refused for lack of support. POSIX requires
    /// that this *not* be an error, so it is a fourth outcome rather than a
    /// failure: nothing changed, and nothing was wrong.
    NotApplied,
}

/// `chown-core.c`'s `describe_change`, verbatim in behaviour.
///
/// `user`/`group` are the *new* names -- the resolved ones, or the plain
/// number when no name was resolved -- and `old_user`/`old_group` are the
/// file's current ones, or `None` when the file could not be stat'd at all.
///
/// Which of the four sentences is used turns on whether `user` is present,
/// not on what actually changed: `chown :staff f` has a user half (the empty
/// string `chown` puts there so the line reads `:staff`), and so says
/// "ownership", while `chgrp staff f` has none and says "group".
#[must_use]
pub fn describe_change(
    file: &OsStr,
    status: ChangeStatus,
    old_user: Option<&[u8]>,
    old_group: Option<&[u8]>,
    user: Option<&[u8]>,
    group: Option<&[u8]>,
) -> String {
    let mut spec = user_group_str(user, group);
    // The old spec names only the fields the new one does: `chown 1000 f`
    // reports `from root`, not `from root:root`, because the group is not part
    // of what was asked.
    let mut old_spec = user_group_str(user.and(old_user), group.and(old_group));

    let text =
        |value: &Option<Vec<u8>>| -> String { value.as_deref().map(name_text).unwrap_or_default() };

    let name = quoteaf_os(file);
    match status {
        // Names neither what was asked for nor what is there, because nothing
        // moved and nothing was wrong.
        ChangeStatus::NotApplied => {
            format!("neither symbolic link {name} nor referent has been changed")
        }
        ChangeStatus::Succeeded => {
            if user.is_some() {
                format!(
                    "changed ownership of {name} from {} to {}",
                    text(&old_spec),
                    text(&spec)
                )
            } else if group.is_some() {
                format!(
                    "changed group of {name} from {} to {}",
                    text(&old_spec),
                    text(&spec)
                )
            } else {
                format!("no change to ownership of {name}")
            }
        }
        ChangeStatus::Failed => {
            if old_spec.is_some() {
                if user.is_some() {
                    format!(
                        "failed to change ownership of {name} from {} to {}",
                        text(&old_spec),
                        text(&spec)
                    )
                } else if group.is_some() {
                    format!(
                        "failed to change group of {name} from {} to {}",
                        text(&old_spec),
                        text(&spec)
                    )
                } else {
                    format!("failed to change ownership of {name}")
                }
            } else {
                // No stat, so there is no "from". Upstream shifts the *new*
                // spec into the first slot rather than printing an empty one,
                // which is why `chown -v 1234 nosuch` says
                // `failed to change ownership of 'nosuch' to 1234`.
                old_spec = spec.take();
                if user.is_some() {
                    format!(
                        "failed to change ownership of {name} to {}",
                        text(&old_spec)
                    )
                } else if group.is_some() {
                    format!("failed to change group of {name} to {}", text(&old_spec))
                } else {
                    format!("failed to change ownership of {name}")
                }
            }
        }
        ChangeStatus::NoChangeRequested => {
            if user.is_some() {
                format!("ownership of {name} retained as {}", text(&old_spec))
            } else if group.is_some() {
                format!("group of {name} retained as {}", text(&old_spec))
            } else {
                format!("ownership of {name} retained")
            }
        }
    }
}

/// Render a name for a message. Names come from `/etc/passwd`, which is bytes,
/// and a byte that is not text must not become a raw control character in a
/// diagnostic -- the same argument as for file names.
fn name_text(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) if !text.bytes().any(|b| b < 0x20 || b == 0x7f) => text.to_string(),
        _ => escape_unprintable(bytes),
    }
}

/// A uid and a gid, either of which may be absent: POSIX's `(uid_t) -1`,
/// "leave this field alone" when setting and "anything matches" when
/// requiring.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Ids {
    pub uid: Option<u32>,
    pub gid: Option<u32>,
}

/// Everything about a run that is the same for every file: upstream's
/// `struct Chown_option`, plus the program name for diagnostics.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Options {
    /// `chown` or `chgrp`, the prefix of every diagnostic.
    pub program: &'static str,
    /// `-R`.
    pub recursive: bool,
    /// The traversal in effect, from [`walk_policy`].
    pub traverse: Traverse,
    /// Change a symlink's target rather than the link, from [`walk_policy`].
    pub affect_referent: bool,
    pub verbosity: Verbosity,
    /// `-f`: keep quiet about most failures. The status still reflects them.
    pub force_silent: bool,
    /// `(dev, ino)` of `/`, when `-R --preserve-root`.
    pub root_dev_ino: Option<(u64, u64)>,
    /// The name `-v` reports as the new owner; the number when `None`.
    pub user_name: Option<Vec<u8>>,
    /// The name `-v` reports as the new group; the number when `None`.
    pub group_name: Option<Vec<u8>>,
}

#[cfg(unix)]
pub use walk::chown_files;

#[cfg(unix)]
mod walk {
    use super::{ChangeStatus, Ids, Options, Traverse, Verbosity, describe_change};
    use crate::diag;
    use crate::errmsg::strerror;
    use crate::quote::{quoteaf_os, quotef_os};
    use crate::userspec::{gid_to_name, uid_to_name};
    use pwdb::Db;
    use std::ffi::OsString;
    use std::fs::{self, Metadata};
    use std::io::{self, Write};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::io::AsRawFd;
    use std::path::{Path, PathBuf};

    // libc-level chown/lchown/fchown -- our POSIX layer provides all three.
    unsafe extern "C" {
        fn chown(path: *const u8, owner: u32, group: u32) -> i32;
        fn lchown(path: *const u8, owner: u32, group: u32) -> i32;
        /// The descriptor-based form, which is what makes [`restricted_chown`]
        /// possible: it names an inode we already hold rather than a path
        /// someone else could re-point.
        fn fchown(fd: i32, owner: u32, group: u32) -> i32;
    }

    /// POSIX's "leave this field alone" sentinel for `chown(2)`: `(uid_t)-1`.
    ///
    /// Passing it is better than reading the current owner and passing that
    /// back: the read-then-write version has a window in which the file can be
    /// replaced, and it turns a no-op field into a real ownership write.
    const UNCHANGED: u32 = u32::MAX;

    /// What `fts` says about an entry: `fts_info`, with the stat it made.
    enum Info {
        /// `FTS_D`: a directory, before its contents.
        Dir(Metadata),
        /// `FTS_F`/`FTS_DEFAULT`: anything that is neither a directory nor a
        /// symlink left unfollowed.
        Other(Metadata),
        /// `FTS_SL`: a symlink, not followed; the stat is the link's own.
        Symlink(Metadata),
        /// `FTS_SLNONE`: a symlink that was to be followed and points nowhere;
        /// the stat is the link's own.
        Dangling(Metadata),
        /// `FTS_NS`: no stat at all.
        NoStat(io::Error),
    }

    /// `fts_stat`: `stat` when following, else `lstat`, and a dangling link
    /// told apart from a missing name.
    fn fts_stat(path: &Path, follow: bool) -> Info {
        if follow {
            match fs::metadata(path) {
                Ok(meta) => classify(meta),
                Err(e) => {
                    // Only `ENOENT` means "the link is there, its target is
                    // not"; `ELOOP` and the rest are a stat that failed.
                    if e.kind() == io::ErrorKind::NotFound
                        && let Ok(link) = fs::symlink_metadata(path)
                    {
                        return Info::Dangling(link);
                    }
                    Info::NoStat(e)
                }
            }
        } else {
            match fs::symlink_metadata(path) {
                Ok(meta) => classify(meta),
                Err(e) => Info::NoStat(e),
            }
        }
    }

    fn classify(meta: Metadata) -> Info {
        if meta.is_dir() {
            Info::Dir(meta)
        } else if meta.file_type().is_symlink() {
            Info::Symlink(meta)
        } else {
            Info::Other(meta)
        }
    }

    /// One run over the operands.
    struct Walk<'a, W: Write> {
        opts: &'a Options,
        db: &'a Db,
        /// The ids to set; `None` fields are left alone.
        set: Ids,
        /// `--from`; `None` fields match anything.
        required: Ids,
        out: &'a mut W,
        /// `(dev, ino)` of every directory on the current path, which is what
        /// `fts` checks a directory against before entering it.
        active: Vec<(u64, u64)>,
    }

    impl<W: Write> Walk<'_, W> {
        /// A diagnostic, unless `-f` said not to.
        fn warn(&self, message: &str) {
            if !self.opts.force_silent {
                diag!("{}: {message}", self.opts.program);
            }
        }

        /// A diagnostic `-f` does not silence: upstream's `ROOT_DEV_INO_WARN`
        /// and cycle warning call `error` directly.
        fn warn_always(&self, message: &str) {
            diag!("{}: {message}", self.opts.program);
        }

        /// `ROOT_DEV_INO_WARN`: two lines, and a `(same as '/')` when the name
        /// is not `/` itself.
        fn refuse_root(&self, path: &Path) {
            if path.as_os_str().as_bytes() == b"/" {
                self.warn_always(&format!(
                    "it is dangerous to operate recursively on {}",
                    quoteaf_os(path)
                ));
            } else {
                self.warn_always(&format!(
                    "it is dangerous to operate recursively on {} (same as {})",
                    quoteaf_os(path),
                    quoteaf_os("/")
                ));
            }
            self.warn_always("use --no-preserve-root to override this failsafe");
        }

        fn is_root(&self, meta: &Metadata) -> bool {
            self.opts.root_dev_ino == Some((meta.dev(), meta.ino()))
        }

        /// A command-line operand: `fts`'s level 0.
        fn operand(&mut self, file: &OsString) -> bool {
            let path = PathBuf::from(file);
            // `FTS_COMFOLLOW` (-H) and `FTS_LOGICAL` (-L) both follow an
            // operand; -P does not.
            let follow = self.opts.traverse != Traverse::Physical;
            let mut info = fts_stat(&path, follow);
            if matches!(info, Info::NoStat(_)) {
                // `FTS_AGAIN`: a top-level name that could not be stat'd is
                // tried once more, since a chown or chgrp earlier in the run
                // may have made it reachable.
                info = fts_stat(&path, follow);
            }
            self.entry(&path, 0, info)
        }

        /// `change_file_owner`'s switch on `fts_info`, and what follows it.
        fn entry(&mut self, path: &Path, level: usize, info: Info) -> bool {
            match info {
                Info::Dir(meta) => self.directory(path, level, meta),
                Info::NoStat(e) => {
                    self.warn(&format!(
                        "cannot access {}: {}",
                        quoteaf_os(path),
                        strerror(&e)
                    ));
                    self.change(path, None, false, false);
                    false
                }
                Info::Other(meta) | Info::Symlink(meta) | Info::Dangling(meta) => {
                    self.change(path, Some(meta), true, false)
                }
            }
        }

        /// `FTS_D`, the contents, then `FTS_DP` (or `FTS_DNR`, or `FTS_DC`).
        fn directory(&mut self, path: &Path, level: usize, meta: Metadata) -> bool {
            if !self.opts.recursive {
                // Not recursing: the directory is an ordinary entry, changed
                // now and not entered.
                return self.change(path, Some(meta), true, true);
            }
            if self.is_root(&meta) {
                // `chown -R --preserve-root 0 /`, or `-RH` through a symlink
                // to it: neither the tree nor the directory is touched.
                self.refuse_root(path);
                return false;
            }
            let id = (meta.dev(), meta.ino());
            if self.active.contains(&id) {
                // `FTS_DC`: already on the path, so entering it would never
                // end. A serious problem only when no symlinks were being
                // followed -- or only the operand's, and this is not an
                // operand -- because then the filesystem itself has a loop.
                let physical_cycle = match self.opts.traverse {
                    Traverse::Physical => true,
                    Traverse::CommandLine => level != 0,
                    Traverse::Logical => false,
                };
                if physical_cycle {
                    self.warn_always(&format!(
                        "WARNING: Circular directory structure.\n\
                         This almost certainly means that you have a corrupted file system.\n\
                         NOTIFY YOUR SYSTEM MANAGER.\n\
                         The following directory is part of the cycle:\n  {}\n",
                        quotef_os(path)
                    ));
                    return false;
                }
                // Otherwise the entry is changed like any other, but not
                // entered again.
                return self.change(path, Some(meta), true, true);
            }

            let entries = match fs::read_dir(path) {
                Ok(entries) => entries,
                Err(e) => {
                    // `FTS_DNR`: reported, and the directory is not changed
                    // either -- it reaches the reporting with `ok` already
                    // false, so `-v` calls it a failure and nothing is written.
                    self.warn(&format!(
                        "cannot read directory {}: {}",
                        quoteaf_os(path),
                        strerror(&e)
                    ));
                    self.change(path, None, false, true);
                    return false;
                }
            };
            let mut ok = true;
            self.active.push(id);
            // Inside the tree only -L follows; -H's exception was the operand.
            let follow = self.opts.traverse == Traverse::Logical;
            for entry in entries {
                match entry {
                    Ok(entry) => {
                        let child = entry.path();
                        let info = fts_stat(&child, follow);
                        ok &= self.entry(&child, level.saturating_add(1), info);
                    }
                    Err(e) => {
                        // A read that failed part-way: `FTS_ERR` on the
                        // directory, reported with `quotef`.
                        self.warn(&format!("{}: {}", quotef_os(path), strerror(&e)));
                        ok = false;
                    }
                }
            }
            self.active.pop();
            // `FTS_DP`: the directory is changed after its contents. The order
            // is load-bearing: handing a directory to another owner first can
            // cost the search permission still needed to reach what is inside.
            ok &= self.change(path, Some(meta), true, true);
            ok
        }

        /// `change_file_owner` from the switch onwards: decide, chown, report.
        ///
        /// `stats` is the entry's own stat (`None` when there is none), `ok`
        /// whether the switch left it true, `is_directory` upstream's
        /// `FTSENT_IS_DIRECTORY`.
        fn change(
            &mut self,
            path: &Path,
            stats: Option<Metadata>,
            mut ok: bool,
            is_directory: bool,
        ) -> bool {
            let opts = self.opts;
            let mut file_stats = stats;
            let mut do_chown = ok;
            if !ok {
                file_stats = None;
            } else if self.required == Ids::default()
                && opts.verbosity == Verbosity::Off
                && opts.root_dev_ino.is_none()
                && !opts.affect_referent
            {
                // Nothing needs the file's stat: chown it as it is.
            } else {
                // Changing a symlink's target: what matters is the target's
                // stat, for --from and for the report.
                if opts.affect_referent
                    && file_stats
                        .as_ref()
                        .is_some_and(|meta| meta.file_type().is_symlink())
                {
                    match fs::metadata(path) {
                        Ok(target) => file_stats = Some(target),
                        Err(e) => {
                            self.warn(&format!(
                                "cannot dereference {}: {}",
                                quoteaf_os(path),
                                strerror(&e)
                            ));
                            ok = false;
                            // Upstream goes on to report out of a stat buffer
                            // the failed call never filled; see the module
                            // documentation. There is no honest "from".
                            file_stats = None;
                        }
                    }
                }
                do_chown = ok
                    && file_stats.as_ref().is_some_and(|meta| {
                        self.required.uid.is_none_or(|uid| uid == meta.uid())
                            && self.required.gid.is_none_or(|gid| gid == meta.gid())
                    });
            }

            // `chown -LR --preserve-root` meeting a symlink to `/`.
            if ok && is_directory && file_stats.as_ref().is_some_and(|meta| self.is_root(meta)) {
                self.refuse_root(path);
                return false;
            }

            let mut symlink_changed = true;
            if do_chown {
                let uid = self.set.uid.unwrap_or(UNCHANGED);
                let gid = self.set.gid.unwrap_or(UNCHANGED);
                let result = if opts.affect_referent {
                    match file_stats
                        .as_ref()
                        .map(|meta| restricted_chown(path, meta, uid, gid, self.required))
                    {
                        Some(Restricted::Done) => Ok(()),
                        Some(Restricted::ByName) | None => chown_path(path, uid, gid, true).map_err(Some),
                        Some(Restricted::Failed(e)) => Err(Some(e)),
                        Some(Restricted::Excluded) => {
                            // RC_inode_changed and RC_excluded: not changed,
                            // and a failure, with no diagnostic of its own.
                            do_chown = false;
                            Err(None)
                        }
                    }
                } else {
                    match chown_path(path, uid, gid, false) {
                        Ok(()) => Ok(()),
                        // POSIX requires that a system unable to change a
                        // symlink's own ownership not treat that as an error.
                        Err(e) if e.raw_os_error() == Some(EOPNOTSUPP) => {
                            symlink_changed = false;
                            Ok(())
                        }
                        Err(e) => Err(Some(e)),
                    }
                };
                if let Err(error) = result {
                    ok = false;
                    if let (true, Some(error)) = (do_chown, error) {
                        let what = if self.set.uid.is_some() {
                            "changing ownership of"
                        } else {
                            "changing group of"
                        };
                        self.warn(&format!("{what} {}: {}", quoteaf_os(path), strerror(&error)));
                    }
                }
            }

            if opts.verbosity != Verbosity::Off {
                let changed = do_chown
                    && ok
                    && symlink_changed
                    && file_stats.as_ref().is_some_and(|meta| {
                        !(self.set.uid.is_none_or(|uid| uid == meta.uid())
                            && self.set.gid.is_none_or(|gid| gid == meta.gid()))
                    });
                if changed || opts.verbosity == Verbosity::High {
                    let status = if !ok {
                        ChangeStatus::Failed
                    } else if !symlink_changed {
                        ChangeStatus::NotApplied
                    } else if !changed {
                        ChangeStatus::NoChangeRequested
                    } else {
                        ChangeStatus::Succeeded
                    };
                    self.report(path, status, file_stats.as_ref());
                }
            }
            ok
        }

        /// Print a `-v`/`-c` line.
        fn report(&mut self, path: &Path, status: ChangeStatus, meta: Option<&Metadata>) {
            let old_user = meta.map(|m| uid_to_name(self.db, m.uid()));
            let old_group = meta.map(|m| gid_to_name(self.db, m.gid()));
            // The new name, or the plain number when nothing was resolved --
            // `chopt->user_name ? … : uid_to_str (uid)`.
            let user = self
                .opts
                .user_name
                .clone()
                .or_else(|| self.set.uid.map(|uid| uid.to_string().into_bytes()));
            let group = self
                .opts
                .group_name
                .clone()
                .or_else(|| self.set.gid.map(|gid| gid.to_string().into_bytes()));
            let mut line = describe_change(
                path.as_os_str(),
                status,
                old_user.as_deref(),
                old_group.as_deref(),
                user.as_deref(),
                group.as_deref(),
            )
            .into_bytes();
            line.push(b'\n');
            // Deliberately unread: a failed write is the sink's to remember
            // and the caller's `close_stdout` to report, once.
            let _ = self.out.write_all(&line);
        }
    }

    /// `EOPNOTSUPP`: 95 on Linux, and `posix::errno::EOPNOTSUPP` is 95.
    const EOPNOTSUPP: i32 = 95;

    /// `chown(2)` when `follow`, else `lchown(2)`, on a path given as bytes.
    fn chown_path(path: &Path, uid: u32, gid: u32, follow: bool) -> io::Result<()> {
        let bytes = path.as_os_str().as_bytes();
        // A NUL cannot arrive from argv or a directory read -- both are
        // NUL-terminated at the source -- so this guards a future caller.
        if bytes.contains(&0) {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        let mut c_path: Vec<u8> = Vec::with_capacity(bytes.len().saturating_add(1));
        c_path.extend_from_slice(bytes);
        c_path.push(0);
        // SAFETY: `c_path` is NUL-terminated with no interior NUL and outlives
        // the call; both functions take a borrowed C string they do not keep.
        let ret = unsafe {
            if follow {
                chown(c_path.as_ptr(), uid, gid)
            } else {
                lchown(c_path.as_ptr(), uid, gid)
            }
        };
        if ret == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// What [`restricted_chown`] managed to do.
    enum Restricted {
        /// The descriptor was chowned.
        Done,
        /// Not worth protecting, or not openable in a way that would help.
        ByName,
        /// The open, the re-stat or the `fchown` failed.
        Failed(io::Error),
        /// The file changed identity under us, or stopped matching `--from`
        /// between the two stats. Upstream gives no diagnostic for the first
        /// -- its source carries a FIXME asking whether it should -- but fails
        /// the run; for the second it reports success without changing
        /// anything, which is only reachable in a race and is not copied: a
        /// run that changed nothing it was asked to does not say it did.
        Excluded,
    }

    /// `chown-core.c`'s `restricted_chown`: change the file through an open
    /// descriptor rather than by name.
    ///
    /// Reachable only with `--from` *and* a symlink's target being changed,
    /// which is exactly the attackable combination: the stat says the owner
    /// matches, and anyone who can write the directory could swap the name
    /// for a symlink before a chown *by name*. A descriptor, once open,
    /// cannot be redirected.
    fn restricted_chown(
        path: &Path,
        orig: &Metadata,
        uid: u32,
        gid: u32,
        required: Ids,
    ) -> Restricted {
        if required == Ids::default() {
            return Restricted::ByName;
        }
        let is_regular = orig.file_type().is_file();
        if !is_regular && !orig.is_dir() {
            // Opening a FIFO would block for a writer, and opening a device can
            // have side effects on the device.
            return Restricted::ByName;
        }
        let opened = fs::File::open(path).or_else(|e| {
            // A file we may not read may still be one we may write, and either
            // descriptor pins the inode equally well.
            if e.kind() == io::ErrorKind::PermissionDenied && is_regular {
                fs::OpenOptions::new().write(true).open(path)
            } else {
                Err(e)
            }
        });
        let file = match opened {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => return Restricted::ByName,
            Err(e) => return Restricted::Failed(e),
        };
        let now = match file.metadata() {
            Ok(now) => now,
            Err(e) => return Restricted::Failed(e),
        };
        if (now.dev(), now.ino()) != (orig.dev(), orig.ino()) {
            return Restricted::Excluded;
        }
        if !(required.uid.is_none_or(|uid| uid == now.uid())
            && required.gid.is_none_or(|gid| gid == now.gid()))
        {
            return Restricted::Excluded;
        }
        // SAFETY: `file` owns the descriptor and is alive for the call.
        if unsafe { fchown(file.as_raw_fd(), uid, gid) } == 0 {
            Restricted::Done
        } else {
            Restricted::Failed(io::Error::last_os_error())
        }
    }

    /// `chown_files`: apply `set` to every operand (and, under `-R`, what is
    /// below it), reporting `-v`/`-c` lines to `out`. `true` if nothing
    /// failed.
    pub fn chown_files<W: Write>(
        files: &[OsString],
        set: Ids,
        required: Ids,
        opts: &Options,
        db: &Db,
        out: &mut W,
    ) -> bool {
        let mut walk = Walk {
            opts,
            db,
            set,
            required,
            out,
            active: Vec::new(),
        };
        let mut ok = true;
        for file in files {
            ok &= walk.operand(file);
        }
        ok
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn without_r_the_walk_is_physical_and_follows_unless_h() {
        for traverse in [Traverse::Physical, Traverse::CommandLine, Traverse::Logical] {
            assert_eq!(
                walk_policy(false, traverse, Dereference::Unspecified),
                Ok((Traverse::Physical, true))
            );
            assert_eq!(
                walk_policy(false, traverse, Dereference::Link),
                Ok((Traverse::Physical, false))
            );
            assert_eq!(
                walk_policy(false, traverse, Dereference::Referent),
                Ok((Traverse::Physical, true))
            );
        }
    }

    #[test]
    fn r_alone_changes_links_and_refuses_dereference() {
        assert_eq!(
            walk_policy(true, Traverse::Physical, Dereference::Unspecified),
            Ok((Traverse::Physical, false))
        );
        assert_eq!(
            walk_policy(true, Traverse::Physical, Dereference::Link),
            Ok((Traverse::Physical, false))
        );
        assert_eq!(
            walk_policy(true, Traverse::Physical, Dereference::Referent),
            Err(DEREFERENCE_NEEDS_TRAVERSAL)
        );
    }

    #[test]
    fn h_and_l_change_targets_unless_h_is_given() {
        for traverse in [Traverse::CommandLine, Traverse::Logical] {
            assert_eq!(
                walk_policy(true, traverse, Dereference::Unspecified),
                Ok((traverse, true))
            );
            assert_eq!(
                walk_policy(true, traverse, Dereference::Referent),
                Ok((traverse, true))
            );
            assert_eq!(walk_policy(true, traverse, Dereference::Link), Ok((traverse, false)));
        }
    }

    #[test]
    fn user_group_str_joins_what_is_there() {
        assert_eq!(user_group_str(Some(b"a"), Some(b"b")), Some(b"a:b".to_vec()));
        assert_eq!(user_group_str(Some(b"a"), None), Some(b"a".to_vec()));
        assert_eq!(user_group_str(None, Some(b"b")), Some(b"b".to_vec()));
        assert_eq!(user_group_str(Some(b""), Some(b"b")), Some(b":b".to_vec()));
        assert_eq!(user_group_str(None, None), None);
    }

    #[test]
    fn a_group_only_change_says_group() {
        let f = OsStr::new("f");
        assert_eq!(
            describe_change(f, ChangeStatus::Succeeded, Some(b"u"), Some(b"old"), None, Some(b"new")),
            "changed group of 'f' from old to new"
        );
        assert_eq!(
            describe_change(f, ChangeStatus::NoChangeRequested, Some(b"u"), Some(b"g"), None, Some(b"g")),
            "group of 'f' retained as g"
        );
        assert_eq!(
            describe_change(f, ChangeStatus::Failed, None, None, None, Some(b"g")),
            "failed to change group of 'f' to g"
        );
        assert_eq!(
            describe_change(f, ChangeStatus::NoChangeRequested, Some(b"u"), Some(b"g"), None, None),
            "ownership of 'f' retained"
        );
    }

    #[test]
    fn a_name_that_is_not_text_is_escaped() {
        assert_eq!(name_text(b"ok"), "ok");
        assert_ne!(name_text(b"a\nb"), "a\nb");
    }
}
