//! The parts of "delete a tree" that `rm` and `mv` must agree on.
//!
//! `mv` acquired a recursive delete on 2026-09-03, when a move across a disk
//! boundary stopped being refused: such a move is a copy followed by a
//! recursive delete of the original, and there was nothing to call. That left
//! two walks in the zone — `rm.rs`'s `Rm::remove_tree` and `mv.rs`'s
//! `remove_tree` — which is what `known-issues.md` →
//! `TD-B-TWO-RECURSIVE-REMOVERS-NOW-EXIST-IN-COREUTILS` is about. GNU has one,
//! `remove.c`, linked by both `rm.c` and `mv.c`; this module is the beginning
//! of the same arrangement here.
//!
//! It started with the rule the two provably disagreed about rather than with
//! the walk, because the disagreement is what proved the entry's point. `mv`
//! reproduced upstream's substitution of an uninformative `rmdir` errno;
//! `rm` did not, and as a result `rm -r` refused to remove an empty directory
//! it could not *read*, where GNU removes it. Sharing [`is_uninformative`] is
//! what made that one rule, in one place, for both.
//!
//! # The walk
//!
//! [`Remover`] is now the whole of it, and both binaries call it. The shape is
//! `copy.rs`'s after the copy-engine extraction: [`Opts`] is the knobs, the
//! streams and the answer source hang off the struct, and each binary's own
//! parsed command line becomes a *producer* of an `Opts` rather than something
//! the walk knows about. `rm`'s `Rm` keeps only what happens above the walk —
//! `--preserve-root`, `-I`'s single up-front question, the `.`/`..` refusal —
//! because those are properties of a command-line operand and `mv` has no
//! command-line operands to apply them to.
//!
//! Two things the caller supplies that are not knobs:
//!
//! * [`Remover::program`], because upstream's diagnostic goes through
//!   `error()`, which prefixes `program_name` — so the identical failure reads
//!   `rm: cannot remove …` or `mv: cannot remove …` depending only on who is
//!   asking. It is not cosmetic: `scripts/rm-diff.sh` and `scripts/mv-diff.sh`
//!   both compare the whole line.
//! * [`Remover::answers`], which is an `Option` rather than a required field.
//!   `mv`'s removal never prompts (`mv.c:87` fixes `x.interactive` to
//!   `RMI_NEVER`), so requiring it to name an answer source would be requiring
//!   it to name one that must never be consulted — and the natural thing to
//!   hand over, standard input, is the one that would do real damage if the
//!   invariant ever broke. `None` says the fact instead of hiding it.
//!
//! # What `mv` gains by the move, beyond not being written twice
//!
//! `mv`'s walk resolved by path: `fs::read_dir`, `fs::remove_file` and
//! `fs::remove_dir` on joined strings, so a directory swapped for a symlink
//! mid-walk was followed. `rm`'s had already been converted to resolve through
//! an open parent descriptor
//! (`TD-B-RM-WALKS-BY-PATH-SO-A-SYMLINK-SWAP-CAN-REDIRECT-A-REMOVAL`), and
//! sharing that walk is what fixes `mv`'s copy of the same weakness. The
//! extraction is therefore a security fix, not only a tidiness one.

use crate::dirfd::{Dir, Kind, Stat};
use crate::errmsg::strerror;
use crate::quote::{os_from_bytes, quoteaf};
use crate::yesno::{Answers, yesno};
use std::io::{self, Write};

/// Whether a failed `rmdir`'s errno is one upstream throws away in favour of an
/// earlier `opendir` failure (`remove.c:424`).
///
/// # Why a directory that cannot be read is still worth an `rmdir`
///
/// Reading a directory needs `r`; removing an empty one needs only `w`+`x` on
/// its *parent*. So `chmod 300 d` on an empty `d` is a directory nobody can
/// list and anybody can delete, and GNU deletes it: `fts` hands the entry over
/// as `FTS_DNR` and `remove.c:571` calls `excise` on it anyway. Measured
/// against GNU tar's sibling `rm` (coreutils 9.x) on 2026-09-03:
///
/// ```text
/// $ mkdir d && chmod 300 d && rm -rv d
/// removed directory 'd'
/// ```
///
/// The read error is therefore *held*, not reported — it only becomes the
/// diagnostic if the `rmdir` also fails, and then only when the `rmdir`'s own
/// errno says less than the read's did. `ENOTEMPTY` on a directory nobody could
/// open says less: it is the mechanical consequence of the entry that could not
/// be enumerated, and upstream's comment is that such errnos "would be
/// meaningless in a diagnostic" (`remove.c:420`). So `rm -r` on an unreadable
/// *non*-empty directory says `Permission denied`, not `Directory not empty`.
///
/// # The list
///
/// Upstream's verbatim, oddities included: `EISDIR` and `ENOTDIR` are there
/// because kernels have been observed to return them from `rmdir` on an
/// unreadable directory, and `EEXIST` because Solaris 10 spells `ENOTEMPTY`
/// that way.
///
/// The numbers are open-coded rather than taken from a `libc` binding because
/// this crate has none, and they are Linux's — which is also SlateOS's, since
/// `posix/src/errno.rs` is derived from the same table. The `ErrorKind` arm
/// below is what answers on the Windows development host, where the raw numbers
/// are the C runtime's and mean other things entirely.
#[must_use]
pub fn is_uninformative(err: &io::Error) -> bool {
    /// `ENOTEMPTY`, `EISDIR`, `ENOTDIR`, `EEXIST` — in the order `remove.c`
    /// lists them.
    const UNINFORMATIVE_CODES: &[i32] = &[
        39, // ENOTEMPTY
        21, // EISDIR
        20, // ENOTDIR
        17, // EEXIST
    ];
    if cfg!(unix)
        && err
            .raw_os_error()
            .is_some_and(|n| UNINFORMATIVE_CODES.contains(&n))
    {
        return true;
    }
    matches!(
        err.kind(),
        io::ErrorKind::DirectoryNotEmpty
            | io::ErrorKind::IsADirectory
            | io::ErrorKind::NotADirectory
            | io::ErrorKind::AlreadyExists
    )
}

/// Which error to print when an `rmdir` failed and an earlier read had too.
///
/// The whole of the substitution rule in one place, so that neither caller has
/// to remember which way round it goes. `held` is the read failure, if there
/// was one; `failure` is what the `rmdir` answered.
///
/// The `rmdir` error wins whenever it says anything specific — `EACCES`,
/// `EBUSY`, `EROFS` are all more informative than a stale `EACCES` from the
/// listing — and loses only to [`is_uninformative`].
#[must_use]
pub fn blame<'a>(held: Option<&'a io::Error>, failure: &'a io::Error) -> &'a io::Error {
    match held {
        Some(earlier) if is_uninformative(failure) => earlier,
        _ => failure,
    }
}

// ------------------------------------------------------------- addressing --

/// Where an entry is — both for the kernel and for the reader of a message.
///
/// The two are deliberately different things, and keeping them apart is the
/// whole of the fix recorded in `rm.rs`'s "The walk resolves descriptors, not
/// paths". `path` is the string GNU prints and the differential harnesses
/// certify; `dir` and `name` are what actually reach a syscall.
///
/// `dir` is `None` for exactly one entry per operand — the operand itself,
/// which has no descriptor above it because nothing has been opened yet. Every
/// other entry in the walk has one, and reaching an entry through its parent's
/// descriptor is what a swapped component cannot redirect.
pub struct Loc<'a> {
    /// The open parent directory, or `None` at a command-line operand.
    dir: Option<&'a Dir>,
    /// The single component naming this entry inside `dir`. Not read when
    /// `dir` is `None`, where the operand may be any number of components.
    name: &'a [u8],
    /// The spelling to print, and — at an operand only — the path to resolve.
    path: &'a [u8],
}

impl<'a> Loc<'a> {
    /// A command-line operand: reached by the name the user typed.
    #[must_use]
    pub fn top(path: &'a [u8]) -> Self {
        Self {
            dir: None,
            name: path,
            path,
        }
    }

    /// An entry inside an open directory.
    #[must_use]
    pub fn in_dir(dir: &'a Dir, name: &'a [u8], path: &'a [u8]) -> Self {
        Self {
            dir: Some(dir),
            name,
            path,
        }
    }

    /// The spelling to print, which is the only part a caller outside the walk
    /// has any business with.
    #[must_use]
    pub fn path(&self) -> &'a [u8] {
        self.path
    }

    /// What this entry is, without following it if it is a link.
    ///
    /// # Errors
    ///
    /// Whatever the `fstatat`/`lstat` answered.
    pub fn stat(&self) -> io::Result<Stat> {
        match self.dir {
            Some(dir) => dir.stat(self.name),
            None => Stat::of_path(&as_path(self.path)),
        }
    }

    /// Remove a non-directory.
    fn unlink(&self) -> io::Result<()> {
        match self.dir {
            Some(dir) => dir.unlink(self.name),
            None => std::fs::remove_file(as_path(self.path)),
        }
    }

    /// Remove an empty directory.
    fn rmdir(&self) -> io::Result<()> {
        match self.dir {
            Some(dir) => dir.rmdir(self.name),
            None => std::fs::remove_dir(as_path(self.path)),
        }
    }

    /// Open this entry as a directory, so the walk can go on below it.
    ///
    /// `st` is the lookup that decided it *was* a directory, and the descriptor
    /// is checked against it — a name that resolved somewhere else since is
    /// refused rather than descended into. See [`crate::dirfd`].
    fn open_dir(&self, st: &Stat) -> io::Result<Dir> {
        match self.dir {
            Some(dir) => dir.open_child(self.name, st),
            None => Dir::open_root(&as_path(self.path), st),
        }
    }

    /// Whether the prompt should say `write-protected`.
    ///
    /// GNU distinguishes `EACCES` (write-protected) from any other failure (an
    /// error in its own right, reported and the entry skipped). This treats
    /// everything that is not a plain success as "not write-protected"
    /// instead: the removal itself is about to run and will report the real
    /// problem with the real errno, and inventing a second diagnostic here
    /// could only turn a removable file into a refused one.
    fn write_protected(&self) -> bool {
        let writable = match self.dir {
            Some(dir) => dir.writable(self.name),
            None => path_writable(self.path),
        };
        writable == Some(false)
    }
}

// ----------------------------------------------------------------- knobs --

/// When to ask before removing.
///
/// Four states rather than a bool because `-i`, `-I`, `--interactive=WHEN` and
/// the default are four different behaviours, and only two of them are decided
/// inside the walk: [`Self::Always`] asks about everything, [`Self::Never`]
/// asks about nothing, and the other two defer to whether standard input is a
/// terminal. [`Self::Once`]'s *own* question is asked above the walk, once for
/// the whole command line, which is why the walk treats it as the tty case.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Interactive {
    /// `-I`, and the walk's behaviour is [`Self::WhenTty`]'s.
    Once,
    /// `-i` or `--interactive=always`.
    Always,
    /// `-f` or `--interactive=never`.
    Never,
    /// The default: ask about a write-protected entry, but only on a terminal.
    #[default]
    WhenTty,
}

/// The knobs the walk itself reads.
///
/// Deliberately not "everything `rm` parses". `--preserve-root`, `-I`'s
/// up-front question and the `.`/`..` refusal are all decided *per
/// command-line operand*, above the walk, and a caller that has no command line
/// has nothing to apply them to.
#[derive(Clone, Copy, Debug, Default)]
pub struct Opts {
    /// `-r`: descend, and remove the directory afterwards.
    pub recursive: bool,
    /// `-d`: remove a directory, but only if it is already empty.
    pub dir: bool,
    /// `--one-file-system`: below the operand, skip anything on another device.
    pub one_file_system: bool,
    /// `-v`: name each entry as it goes.
    pub verbose: bool,
    /// When to ask.
    pub interactive: Interactive,
    /// `-f`: a thing that is already absent is the outcome asked for.
    pub ignore_missing_files: bool,
}

/// What became of one entry, which is not the same question as the exit status.
///
/// Three states because a directory has to summarise its children, and "a child
/// was declined" and "a child failed" lead to different behaviour at the parent.
/// This is GNU's `mark_ancestor_dirs`, and getting it wrong changes both the
/// output and the exit status.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// Gone. The parent may go too.
    Removed,
    /// The user declined to remove *this* entry. The parent is still asked
    /// about, still attempts its own removal, and still fails with
    /// `Directory not empty` — which is an error, and does set the status.
    /// Measured; it is not what a "declined means skip the parent" reading
    /// predicts.
    Declined,
    /// A failure was reported, or a *descend* was declined. Every enclosing
    /// directory is skipped in silence: no prompt, no message, no second
    /// error. The status was already set by whatever caused this, if anything
    /// was — a declined descend exits 0.
    Abandoned,
}

/// Which question is being asked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Question {
    /// `descend into [write-protected ]directory 'x'? `
    Descend,
    /// `remove [write-protected ]<type> 'x'? `
    Remove,
    /// `attempt removal of inaccessible directory 'x'? `
    ///
    /// The third of upstream's three, asked only about a directory whose
    /// contents could not be read, and only when `-r` is *off* — under `-r` the
    /// question would have to be `Descend` or `Remove`, and choosing between
    /// them needs the listing that just failed. It takes no `write-protected`
    /// variant: measured, `rm -d -i` on a directory that is both unreadable and
    /// unwritable (mode 0100) asks this sentence unmodified.
    AttemptInaccessible,
}

// ------------------------------------------------------------------ walk --

/// One removal, from the walk's point of view: the knobs, the two output
/// streams, the answer source, and the exit status being earned.
pub struct Remover<'a> {
    /// The knobs.
    pub opts: Opts,
    /// What `error()` would prefix — `"rm"` or `"mv"`. See the module header.
    pub program: &'static str,
    /// Verbose output. Standard output, as upstream.
    pub out: &'a mut dyn Write,
    /// Diagnostics **and prompts**. Both go to standard error, which is why
    /// `rm -i ... 2>/dev/null` swallows the question and not the answer.
    pub err: &'a mut dyn Write,
    /// Where a prompt's answer comes from, or `None` for a caller that never
    /// prompts. See the module header for why this is not required.
    pub answers: Option<&'a mut dyn Answers>,
    /// Whether standard input is a terminal, which is what the two
    /// tty-conditional interactivity states turn on.
    pub stdin_tty: bool,
    /// Set by every reported failure. This, not [`Verdict`], is the exit
    /// status: declining a prompt is not a failure.
    pub failed: bool,
}

impl Remover<'_> {
    /// One entry, at `level` below its command-line operand.
    ///
    /// `top` is the device the operand itself is on, for `--one-file-system`;
    /// pass the operand's own `Stat::dev()` at `level` 0.
    pub fn entry(&mut self, loc: &Loc<'_>, st: &Stat, level: u32, top: Option<u64>) -> Verdict {
        if !st.is_dir() {
            return self.remove_nondirectory(loc, st);
        }

        // `--one-file-system` skips a directory below the operand that is on
        // another filesystem. The operand itself is never skipped: it is what
        // defines the filesystem to stay on.
        if level > 0 && self.opts.one_file_system && top.is_some() && st.dev() != top {
            self.diagnose(&format!(
                "skipping {}, since it's on a different device",
                quoteaf(loc.path)
            ));
            return Verdict::Abandoned;
        }

        if self.opts.recursive {
            self.remove_tree(loc, st, level, top)
        } else if self.opts.dir {
            self.remove_empty_directory(loc, st)
        } else {
            // Not `-r` and not `-d`: the directory is not read at all, which
            // is why an unreadable one still answers `Is a directory`.
            self.cannot_remove(loc.path, &io::Error::from(io::ErrorKind::IsADirectory));
            Verdict::Abandoned
        }
    }

    /// A file, symlink, fifo, socket or device node: prompt, then unlink.
    fn remove_nondirectory(&mut self, loc: &Loc<'_>, st: &Stat) -> Verdict {
        if !self.prompt(loc, st, Question::Remove) {
            return Verdict::Declined;
        }
        match loc.unlink() {
            Ok(()) => {
                self.verbose("removed", loc.path);
                Verdict::Removed
            }
            // Vanished between the stat and the unlink. Under `-f` that is the
            // outcome asked for.
            Err(e) if self.opts.ignore_missing_files && e.kind() == io::ErrorKind::NotFound => {
                Verdict::Removed
            }
            Err(e) => {
                self.cannot_remove(loc.path, &e);
                Verdict::Abandoned
            }
        }
    }

    /// `-d` without `-r`: only an empty directory may go.
    fn remove_empty_directory(&mut self, loc: &Loc<'_>, st: &Stat) -> Verdict {
        let children = match list(loc, st) {
            Ok((_, names)) => names,
            Err(e) => {
                return self.remove_inaccessible_directory(loc, st, e);
            }
        };
        if !children.is_empty() {
            // No prompt: measured, `rm -i -d nonempty` asks nothing and goes
            // straight to the error, because the `rmdir` cannot succeed.
            self.cannot_remove(loc.path, &io::Error::from(io::ErrorKind::DirectoryNotEmpty));
            return Verdict::Abandoned;
        }
        if !self.prompt(loc, st, Question::Remove) {
            return Verdict::Declined;
        }
        self.rmdir(loc)
    }

    /// `-r`: the directory, its contents, and the two prompts around them.
    fn remove_tree(&mut self, loc: &Loc<'_>, st: &Stat, level: u32, top: Option<u64>) -> Verdict {
        // Open and read first, which is the natural order anyway: whether the
        // directory is empty decides *which* of the two questions gets asked,
        // so there is nothing to ask until the listing has answered. A
        // directory that cannot be read therefore never reaches a prompt here
        // — see [`Remover::remove_inaccessible_directory`] for what happens to
        // it instead, which is not the same as failing.
        let (dir, children) = match list(loc, st) {
            Ok(pair) => pair,
            Err(e) => return self.remove_inaccessible_directory(loc, st, e),
        };

        if children.is_empty() {
            // One question, not two: there is nothing to descend into.
            if !self.prompt(loc, st, Question::Remove) {
                return Verdict::Declined;
            }
            drop(dir);
            return self.rmdir(loc);
        }

        if !self.prompt(loc, st, Question::Descend) {
            // A declined *descend* abandons the enclosing directories in
            // silence, and is not an error.
            return Verdict::Abandoned;
        }

        let mut worst = Verdict::Removed;
        for name in children {
            // `join` builds the string that gets *printed*; `Loc::in_dir` is
            // what the syscalls see, and it carries the open parent rather than
            // the string. That split is the fix — see `rm.rs`'s header.
            let child_path = join(loc.path, &name);
            let child = Loc::in_dir(&dir, &name, &child_path);
            let verdict = match child.stat() {
                Ok(child_st) => self.entry(&child, &child_st, level.saturating_add(1), top),
                Err(e) if e.kind() == io::ErrorKind::NotFound && self.opts.ignore_missing_files => {
                    Verdict::Removed
                }
                Err(e) => {
                    self.cannot_remove(&child_path, &e);
                    Verdict::Abandoned
                }
            };
            worst = worse(worst, verdict);
        }

        if worst == Verdict::Abandoned {
            // Silence, deliberately: the child already said what went wrong,
            // and a second message about the parent would be noise.
            return Verdict::Abandoned;
        }

        if !self.prompt(loc, st, Question::Remove) {
            return Verdict::Declined;
        }
        // Closed before the `rmdir`, not left to fall out of scope after it.
        // Unix does not mind removing a directory somebody still has open, but
        // the host build does, and a descriptor whose only remaining purpose is
        // to be dropped is worth dropping where the reason is visible.
        drop(dir);
        // With a declined child still in it this fails with `Directory not
        // empty`, which is exactly what GNU prints. The failure is not
        // special-cased into silence.
        self.rmdir(loc)
    }

    /// A directory whose contents could not be read — and which is therefore
    /// still worth one `rmdir`.
    ///
    /// # Why this is not simply an error
    ///
    /// Listing a directory needs `r`; removing an empty one needs `w`+`x` on
    /// its *parent* and nothing at all on the directory itself. `chmod 300 d`
    /// on an empty `d` is thus a directory nobody can read and anybody can
    /// delete, and GNU deletes it — `fts` hands the entry over as `FTS_DNR` and
    /// `remove.c:571` calls `excise` on it regardless. `rm` used to report the
    /// read failure and stop, so `rm -rv d` printed
    /// `cannot remove 'd': Permission denied` and left the directory standing
    /// where GNU printed `removed directory 'd'`. Measured against GNU 9.4 on
    /// 2026-09-03; `mv`'s younger walk had the rule and `rm`'s older one did
    /// not, which is the drift
    /// `TD-B-TWO-RECURSIVE-REMOVERS-NOW-EXIST-IN-COREUTILS` was filed to catch.
    ///
    /// # Why `-r` and `-d` part company here
    ///
    /// Only when a question is due. Under `-d` the whole operation *is* the
    /// `rmdir`, so upstream asks a third question — `attempt removal of
    /// inaccessible directory 'd'? ` — and proceeds on a yes. Under `-r` the
    /// question would have to be `descend into …?` or `remove …?`, and which
    /// one it is depends on whether the directory is empty, which is exactly
    /// what the failed listing was going to say. There is nothing to ask, so
    /// the read failure becomes the diagnostic after all. Both halves are
    /// measured: `rm -d -i` on mode 0300 asks and removes, `rm -r -i` on the
    /// same directory reports `Permission denied`, and `rm -r` without `-i`
    /// removes it.
    ///
    /// The read error is carried into the `rmdir` rather than dropped, because
    /// a non-empty unreadable directory earns `ENOTEMPTY` there and upstream
    /// prints the earlier `EACCES` instead — see [`blame`].
    fn remove_inaccessible_directory(
        &mut self,
        loc: &Loc<'_>,
        st: &Stat,
        why: io::Error,
    ) -> Verdict {
        if self.opts.recursive {
            if self.asking_about(loc, st).is_some() {
                self.cannot_remove(loc.path, &why);
                return Verdict::Abandoned;
            }
        } else if !self.prompt(loc, st, Question::AttemptInaccessible) {
            return Verdict::Declined;
        }
        self.rmdir_blaming(loc, Some(why))
    }

    fn rmdir(&mut self, loc: &Loc<'_>) -> Verdict {
        self.rmdir_blaming(loc, None)
    }

    /// `rmdir`, reporting `held` instead of the failure when the failure is one
    /// of the errnos upstream considers uninformative.
    fn rmdir_blaming(&mut self, loc: &Loc<'_>, held: Option<io::Error>) -> Verdict {
        match loc.rmdir() {
            Ok(()) => {
                self.verbose("removed directory", loc.path);
                Verdict::Removed
            }
            Err(e) if self.opts.ignore_missing_files && e.kind() == io::ErrorKind::NotFound => {
                Verdict::Removed
            }
            Err(e) => {
                self.cannot_remove(loc.path, blame(held.as_ref(), &e));
                Verdict::Abandoned
            }
        }
    }

    // ------------------------------------------------------------ prompts --

    /// Whether a question is going to be asked about this entry at all, and if
    /// so whether it will carry the words `write-protected`.
    ///
    /// Split out of [`Remover::prompt`] because the *answer to this* — not the
    /// answer to the question itself — is what decides whether an unreadable
    /// directory is an error. See [`Remover::remove_inaccessible_directory`].
    fn asking_about(&mut self, loc: &Loc<'_>, st: &Stat) -> Option<bool> {
        if self.opts.interactive == Interactive::Never {
            return None;
        }

        // The write-protection probe is itself conditional: it costs a syscall
        // per entry, and upstream only pays it when the answer could change
        // anything. A symlink is never probed — the bit that matters would be
        // the target's.
        let write_protected = !self.opts.ignore_missing_files
            && (self.opts.interactive == Interactive::Always || self.stdin_tty)
            && !st.is_symlink()
            && loc.write_protected();

        if write_protected || self.opts.interactive == Interactive::Always {
            Some(write_protected)
        } else {
            None
        }
    }

    fn prompt(&mut self, loc: &Loc<'_>, st: &Stat, question: Question) -> bool {
        let Some(write_protected) = self.asking_about(loc, st) else {
            return true;
        };
        let name = quoteaf(loc.path);
        let program = self.program;
        let sentence = match (question, write_protected) {
            (Question::Descend, true) => {
                format!("{program}: descend into write-protected directory {name}? ")
            }
            (Question::Descend, false) => format!("{program}: descend into directory {name}? "),
            (Question::Remove, true) => {
                format!(
                    "{program}: remove write-protected {} {name}? ",
                    file_type(st)
                )
            }
            (Question::Remove, false) => {
                format!("{program}: remove {} {name}? ", file_type(st))
            }
            (Question::AttemptInaccessible, _) => {
                format!("{program}: attempt removal of inaccessible directory {name}? ")
            }
        };
        self.ask(&sentence)
    }

    /// Put a question and read the answer. Public because `-I`'s single
    /// up-front question is asked by the caller, above the walk, and has to
    /// reach the same stream through the same answer source.
    pub fn ask(&mut self, sentence: &str) -> bool {
        let Some(answers) = self.answers.as_deref_mut() else {
            // Unreachable by construction: a caller with no answer source sets
            // `Interactive::Never`, and `asking_about` returns `None` for that
            // before any sentence is built. If it is ever reached anyway, going
            // ahead is the outcome `Never` asked for — whereas printing a
            // question nobody can answer, or inventing a "no", would silently
            // leave behind a source `mv` has already copied.
            return true;
        };
        let _ = self.err.write_all(sentence.as_bytes());
        let _ = self.err.flush();
        yesno(answers)
    }

    // ------------------------------------------------------------- output --

    fn verbose(&mut self, what: &str, path: &[u8]) {
        if self.opts.verbose {
            let _ = writeln!(self.out, "{what} {}", quoteaf(path));
        }
    }

    /// GNU's `cannot remove %s` (`remove.c:430`), and the exit status with it.
    ///
    /// The same sentence for a file and for a directory — only the `-v` line
    /// distinguishes them.
    ///
    /// `strerror`, not `{e}`: why it failed has to read the same wherever it
    /// is printed. On a Windows *host* `{e}` says `The system cannot find the
    /// file specified. (os error 2)`, which is neither POSIX's wording nor
    /// what this utility prints on the target it ships on.
    pub fn cannot_remove(&mut self, path: &[u8], e: &io::Error) {
        self.diagnose(&format!("cannot remove {}: {}", quoteaf(path), strerror(e)));
    }

    /// One diagnostic, prefixed as `error()` would prefix it, and the exit
    /// status with it.
    pub fn diagnose(&mut self, sentence: &str) {
        self.failed = true;
        let program = self.program;
        let _ = writeln!(self.err, "{program}: {sentence}");
    }
}

// -------------------------------------------------------------- helpers --

/// Open a directory the walk is about to act on, and read its names.
///
/// The two are returned together because they must not be separated: the names
/// are only meaningful as names *inside that descriptor*, and a caller that
/// kept the list but dropped the handle would be back to resolving them by
/// path — which is the bug `rm.rs`'s header is about.
///
/// The whole listing is read before anything is removed, as `fts` does, so the
/// order is `readdir`'s — which is observable through `-v`.
fn list(loc: &Loc<'_>, st: &Stat) -> io::Result<(Dir, Vec<Vec<u8>>)> {
    let dir = loc.open_dir(st)?;
    let names = dir.names()?;
    Ok((dir, names))
}

/// The more serious of two verdicts, for a directory summarising its children.
fn worse(a: Verdict, b: Verdict) -> Verdict {
    match (a, b) {
        (Verdict::Abandoned, _) | (_, Verdict::Abandoned) => Verdict::Abandoned,
        (Verdict::Declined, _) | (_, Verdict::Declined) => Verdict::Declined,
        _ => Verdict::Removed,
    }
}

/// gnulib's `file_type()`, whose words appear in the prompts verbatim.
///
/// It reads the [`Stat`] the walk already took rather than taking one of its
/// own — the prompt has to describe the same file the removal is about to act
/// on, and a second lookup by path could describe a different one.
fn file_type(st: &Stat) -> &'static str {
    match st.kind() {
        Kind::SymbolicLink => "symbolic link",
        Kind::Directory => "directory",
        Kind::BlockDevice => "block special file",
        Kind::CharDevice => "character special file",
        Kind::Fifo => "fifo",
        Kind::Socket => "socket",
        // The empty/non-empty split is upstream's, and visible: `rm -i` on a
        // zero-length file says `regular empty file`.
        Kind::Regular if st.size() == 0 => "regular empty file",
        Kind::Regular => "regular file",
        Kind::Other => "weird file",
    }
}

/// gnulib `fts`'s `NAPPEND`: a parent path ending in `/` loses that one slash
/// before the separator goes on, so `tree/` yields `tree/a.txt` and not
/// `tree//a.txt`. An interior double slash is left alone — `tree//sub` yields
/// `tree//sub/b.txt` — because only the *last* character is examined.
fn join(parent: &[u8], name: &[u8]) -> Vec<u8> {
    let trimmed = if parent.last() == Some(&b'/') {
        parent
            .get(..parent.len().saturating_sub(1))
            .unwrap_or(parent)
    } else {
        parent
    };
    let mut out = trimmed.to_vec();
    out.push(b'/');
    out.extend_from_slice(name);
    out
}

/// A byte path as something a syscall will take. See `quote::os_from_bytes`
/// for why the round trip is the only correct one on this OS.
#[must_use]
pub fn as_path(path: &[u8]) -> std::path::PathBuf {
    std::path::PathBuf::from(os_from_bytes(path))
}

#[cfg(unix)]
unsafe extern "C" {
    /// `euidaccess(path, mode)`, where mode 2 is `W_OK`. The *effective* uid
    /// is the one that matters: `access(2)` asks about the real one, which for
    /// a setuid `rm` would answer a question nobody asked.
    fn euidaccess(path: *const u8, mode: i32) -> i32;
}

/// Whether a command-line operand may be written by the effective user, or
/// `None` if the question could not be answered.
///
/// The by-path twin of [`crate::dirfd::Dir::writable`], and used only where
/// that one cannot be: at an operand, which has no descriptor above it. Below
/// one, the probe goes through the parent's handle like everything else.
#[cfg(unix)]
fn path_writable(path: &[u8]) -> Option<bool> {
    let mut c_path = path.to_vec();
    if c_path.contains(&0) {
        return None;
    }
    c_path.push(0);
    // SAFETY: `c_path` is NUL-terminated, has no interior NUL, and outlives
    // the call. `euidaccess` reads it and does not retain it.
    let rc = unsafe { euidaccess(c_path.as_ptr(), 2) };
    Some(rc == 0)
}

/// Off unix there is no `euidaccess`, so the question is unanswerable, nothing
/// is ever reported as write-protected, and the default interactivity never
/// prompts. That is the conservative direction only in the sense that it
/// matches `--interactive=never`; the host build is a test vehicle, not a
/// shipping one.
#[cfg(not(unix))]
fn path_writable(_path: &[u8]) -> Option<bool> {
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    /// The four codes, by number, which is the arm that runs on the target.
    #[cfg(unix)]
    #[test]
    fn the_four_upstream_codes_are_uninformative_by_number() {
        for code in [39, 21, 20, 17] {
            assert!(
                is_uninformative(&io::Error::from_raw_os_error(code)),
                "errno {code} should be uninformative"
            );
        }
    }

    /// ...and the ones that are not, which is what stops the rule swallowing a
    /// diagnostic that had something to say. `EACCES` is the important one: an
    /// `rmdir` that fails with it has found a real obstacle, and substituting
    /// the identical-looking earlier error would be harmless, but `EBUSY` and
    /// `EROFS` name obstacles the listing never saw.
    #[cfg(unix)]
    #[test]
    fn a_specific_errno_is_not_thrown_away() {
        for code in [13, 16, 30, 2] {
            assert!(
                !is_uninformative(&io::Error::from_raw_os_error(code)),
                "errno {code} should be kept"
            );
        }
    }

    /// The host arm, which has no errno numbers to match on. Portable, so it is
    /// the one test in here that runs on both targets.
    #[test]
    fn the_kinds_answer_where_the_numbers_cannot() {
        assert!(is_uninformative(&io::Error::from(
            io::ErrorKind::DirectoryNotEmpty
        )));
        assert!(is_uninformative(&io::Error::from(
            io::ErrorKind::AlreadyExists
        )));
        assert!(!is_uninformative(&io::Error::from(
            io::ErrorKind::PermissionDenied
        )));
        assert!(!is_uninformative(&io::Error::from(io::ErrorKind::NotFound)));
    }

    /// `join`'s one rule, which is `fts`'s and is observable through `-v`.
    #[test]
    fn joining_drops_one_trailing_slash_only() {
        assert_eq!(join(b"tree", b"a"), b"tree/a");
        assert_eq!(join(b"tree/", b"a"), b"tree/a");
        assert_eq!(join(b"tree//sub", b"a"), b"tree//sub/a");
        assert_eq!(join(b"/", b"a"), b"/a");
        assert_eq!(join(b"//", b"a"), b"//a".to_vec(), "only the last slash");
    }

    /// `worse` is a max over a three-valued lattice, and the property that
    /// matters is that it is order-independent: a directory folds its children
    /// in `readdir` order, so a `worse` that answered differently for
    /// `(Declined, Abandoned)` than for `(Abandoned, Declined)` would make a
    /// parent's fate depend on the order the filesystem happened to list its
    /// contents.
    #[test]
    fn worse_is_a_symmetric_maximum() {
        use Verdict::{Abandoned, Declined, Removed};
        for (a, b, want) in [
            (Removed, Removed, Removed),
            (Removed, Declined, Declined),
            (Removed, Abandoned, Abandoned),
            (Declined, Declined, Declined),
            (Declined, Abandoned, Abandoned),
            (Abandoned, Abandoned, Abandoned),
        ] {
            assert_eq!(worse(a, b), want, "{a:?} {b:?}");
            assert_eq!(worse(b, a), want, "{b:?} {a:?} (reversed)");
        }
    }

    /// `blame` substitutes only when there is something to substitute *and* the
    /// failure is one of the empty ones. Four combinations, all four asserted:
    /// a test that only checked the substituting case would pass against a
    /// `blame` that always substituted.
    #[test]
    fn blame_substitutes_only_the_uninformative_failure() {
        let held = io::Error::from(io::ErrorKind::PermissionDenied);
        let empty = io::Error::from(io::ErrorKind::DirectoryNotEmpty);
        let busy = io::Error::from(io::ErrorKind::ResourceBusy);

        assert_eq!(
            blame(Some(&held), &empty).kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            blame(Some(&held), &busy).kind(),
            io::ErrorKind::ResourceBusy
        );
        assert_eq!(blame(None, &empty).kind(), io::ErrorKind::DirectoryNotEmpty);
        assert_eq!(blame(None, &busy).kind(), io::ErrorKind::ResourceBusy);
    }
}
