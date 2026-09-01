//! cp — copy files and directories.
//!
//! # Why this was rewritten
//!
//! It read argv as `String`, so it *panicked* on a filename holding a byte that
//! is not valid UTF-8 — which on this OS is a legal filename, by design
//! (`design.txt`: a path may hold every byte but `/` and NUL). See
//! `known-issues.md` → `B-COREUTILS-PANIC-ON-A-NON-UTF-8-ARGUMENT`; `cp` is the
//! third of the 49 bins listed there, after `rm` and `mv`.
//!
//! Argv is now `OsString` and stays that way to the syscall. Options go through
//! [`coreutils::getopt`], which is byte-based.
//!
//! # Six further bugs, in the lines this rewrite replaced
//!
//! 1. **A symlink inside a recursive copy was followed, so `cp -r` could not
//!    terminate.** `copy_dir_recursive` asked `src_path.is_dir()`, which follows
//!    symlinks, and then `fs::copy`, which also follows. A directory containing
//!    a link to any of its own ancestors — `ln -s .. loop`, which is ordinary —
//!    made `cp -r` descend for ever, writing an ever-deepening tree until the
//!    disk filled or the path length gave out. Even without a loop, every
//!    symlink in the tree silently became a full copy of whatever it pointed at,
//!    so copying a tree of a hundred links to one big file produced a hundred
//!    big files. GNU's `-r` does not dereference; neither does this one now.
//!    `DirEntry::file_type` is the non-following call and is what the walk uses.
//!
//! 2. **`cp -r` would copy a directory into itself, without limit.** `cp -r a a`
//!    and `cp -r a .` both resolve the destination to a path *inside* the
//!    source, so the walk copied what it had just written, for ever. GNU refuses
//!    with `cannot copy a directory into itself`; so does this, after resolving
//!    both paths as far as they exist.
//!
//! 3. **A copied directory came out world-readable.** `fs::create_dir_all` makes
//!    a directory with the process umask's default mode, so `cp -r private dst`
//!    — where `private` is mode 0700 — produced a 0755 `dst`, publishing every
//!    file inside it. POSIX says the new directory takes the source's
//!    permission bits. The mode is now applied *after* the contents are copied,
//!    because applying a mode like 0500 first would lock out the copy itself.
//!    (Regular files were never affected: `fs::copy` carries the mode over.)
//!
//! 4. **`--` was not an end-of-options marker.** `cp -- -foo bar` answered
//!    `unknown option: --`, so a file whose name begins with a dash could not be
//!    copied at all.
//!
//! 5. **A source ending in `.`, `..` or `/` copied into the wrong place.** The
//!    target was `dest.join(src.file_name().unwrap_or_default())`, and
//!    `Path::file_name` answers a *normalised* question rather than a textual
//!    one. It is `None` for `a/..`, so `unwrap_or_default()` gave an empty name
//!    and `dest.join("")` collapsed back to `dest` itself; and it is
//!    `Some("a")` for `a/.`, so `cp -r a/. dst` created `dst/a` instead of
//!    filling `dst`. GNU's last component is the bytes after the last slash,
//!    kept verbatim — `.` stays `.`.
//!
//!    This one was fixed twice, and the first fix was itself a divergence. It
//!    refused every such source with an invented diagnostic, on the reasoning
//!    that a path naming no new entry names nowhere to create one. Measurement
//!    says otherwise: `cp -r a/. dst` is the ordinary idiom for "copy `a`'s
//!    *contents* into `dst`", GNU performs it, and the refusal broke a working
//!    command. The rule GNU actually applies is four lines of `do_copy`
//!    (`cp.c:734`) and is now [`compute_target`]'s whole body — including its
//!    one special case, `arg_base += STREQ (arg_base, "..")`, which turns a
//!    trailing `..` into `.` so that `cp -r a/.. dst` writes into `dst` and
//!    never into the destination's *parent*.
//!
//! 6. **One unreadable file abandoned the rest of the copy.** `copy_dir_recursive`
//!    propagated the first error with `?`, so a single permission denial part-way
//!    through a large tree stopped the walk, reported one message, and left a
//!    partial copy that looked complete to anything that did not check the exit
//!    status. Each entry is now attempted, each failure reported, and the worst
//!    outcome returned — which is what `cp` is specified to do and what makes the
//!    diagnostics worth reading.
//!
//! # A seventh, found later, by measurement rather than by reading
//!
//! 7. **`cp a a` emptied `a`, silently, and exited 0.** The destination is
//!    opened with `O_TRUNC` before the source is read, so naming one file
//!    twice truncated it to nothing and then copied the nothing back over
//!    itself. It said nothing and reported success, so a shell loop that did
//!    it could destroy a directory's worth of files without a single
//!    diagnostic. Every one of these reached it — `cp a a`, `cp a ./a`,
//!    `cp a dir/../a`, `cp a hard-link-to-a`, `cp a symlink-to-a`, and
//!    `cp -r a .` — because there is no string comparison that catches the
//!    last four, and GNU does not attempt one: it compares the two `stat`
//!    results, device and inode, and so does [`is_same_file`] now.
//!
//!    This one is worth separating from bugs 1–6 for a reason that has nothing
//!    to do with `cp`. Those six were found by *reading* the code it replaced.
//!    This was found by `scripts/cp-diff.sh` on its first run, against a file
//!    that had already been rewritten once with the defect in it — the rewrite
//!    swapped a hand-rolled walk for `fs::copy` and never asked what `fs::copy`
//!    does when handed one file twice. Reading finds the bugs you thought to
//!    look for. See `known-issues.md` ->
//!    `B-CP-COPYING-A-FILE-ONTO-ITSELF-EMPTIED-IT`.
//!
//! # An eighth, from the same harness: three wrong answers about permissions
//!
//! 8. **`fs::copy` gave the destination the source's mode, exactly, in every
//!    case.** That is wrong three separate ways, and two of them publish a file
//!    that was private:
//!
//!    * *A new file ignored the umask.* `fs::copy` creates the destination and
//!      then `chmod`s it to the source's mode, so a 0777 source produced a 0777
//!      copy. GNU passes the mode to `open` and lets the kernel subtract the
//!      umask, so under the ordinary 022 the copy is 0755. Measured, both ways,
//!      across three umasks — see [`mode_of_a_new_file_is_narrowed_by_umask`].
//!    * *An existing destination had its mode overwritten.* `cp public private`
//!      — a 0777 source over somebody's 0600 file — left that file 0777. GNU
//!      reopens an existing destination **without** a mode argument, so its
//!      permissions are not touched at all; only its contents are. This is the
//!      one that is a security bug rather than a cosmetic one, and no amount of
//!      reading the old code would have suggested looking for it, because the
//!      old code mentioned modes for files nowhere at all.
//!    * *A directory ignored the umask too, and had a window.* `cp -r` of a
//!      1777 directory produced 1777 where GNU produces 1755, and the copy was
//!      made group- and other-writable *before* its contents were written —
//!      a window in which anyone could add a file to a directory that is about
//!      to look like a faithful copy. [`copy_tree`] now does GNU's dance:
//!      withhold group/other write at `mkdir`, force owner-rwx on if the source
//!      lacked it, and put both back at the end, less the umask.
//!
//!    Bug 3 above was the same subject and did not go far enough: it noticed
//!    that a copied directory came out *wider* than its source and fixed that
//!    by copying the mode over verbatim, which is a different wrong answer. The
//!    lesson is bug 7's — a fix derived by reading is worth what the reading
//!    was worth, and only measurement says whether it was right.
//!
//! # A ninth: one operand quietly destroying another's work
//!
//! 9. **Nothing remembered what this command had already written.** `cp a
//!    other/a d` copied `a` to `d/a`, then copied `other/a` over it. Exit 0,
//!    nothing printed, and the copy the user asked for was gone. Neither
//!    operand is wrong on its own, which is why no amount of checking one
//!    operand at a time could have found it; GNU keeps three tables for exactly
//!    this question and this had none. [`Seen`] is that record, and it answers
//!    three refusals — the collision above, the same collision reached through
//!    a symlink this command created (where the damage lands on a file nobody
//!    named), and a source given twice, which is a warning rather than an
//!    error because the file the user asked for is in fact there.
//!
//!    The last of those is where the identity question gets sharp: `cp a ./a d`
//!    is one file named twice, but `cp a hard-link-to-a d` is two directory
//!    entries that share an inode and a perfectly reasonable request for two
//!    copies. Telling them apart needs the entry — the directory it is in plus
//!    the final component — and not the inode alone. See [`entry_id`].
//!
//! # A tenth: the record the walk could not read
//!
//! 10. **One directory copied twice, in silence.** `cp -r parent/child parent
//!     d` copied `parent/child` to `d/child`, then walked into `parent`, found
//!     that same directory a second time, and copied the whole subtree again to
//!     `d/parent/child` — exit 0, nothing printed. GNU refuses the repeat
//!     (`will not create hard link 'd/parent/child' to directory 'd/child'`)
//!     and exits 1, because a directory appearing twice in the destination
//!     could only be one directory if it were hard-linked, and hard-linked
//!     directories are what it will not make.
//!
//!     The refusal *existed* here; it was in [`copy_one`], reading a table on
//!     [`Seen`] that only the operand loop can reach. So it fired when two
//!     operands named one directory and never when a walk arrived at one. Two
//!     tables were kept where GNU keeps one, on the reasoning that a directory
//!     and a file can never collide in a shared table — true, and beside the
//!     point: the cost of the split was not a collision but that half the
//!     question lived behind an interface half the code had no route to. They
//!     are now one table, [`Copied`], hung where the walk can read it.
//!
//!     What makes it a tenth bug rather than a missing feature is the comment
//!     that stood where the check now is, asserting that none of `copy_one`'s
//!     refusals "can arise" in a walk. It was reasoning about *files*, and it
//!     was right about them. See design-decisions.md 736.
//!
//! # Options this implementation does not have
//!
//! Everything except `-r`/`-R`/`--recursive`, `-t`/`--target-directory`,
//! `-T`/`--no-target-directory`, `-v`/`--verbose`, the three symlink policies
//! `-P`/`--no-dereference`, `-H` and `-L`/`--dereference`, the four
//! overwrite policies `-f`/`--force`, `-n`/`--no-clobber`,
//! `-i`/`--interactive` and `--remove-destination`,
//! `-p`/`--preserve`/`--no-preserve` for the five attributes this `cp` can
//! carry, `-d`, which is two of them together, and `-a`/`--archive`, which is
//! all of them and `-dR`. The rest are recognised and rejected with a message
//! saying they are not implemented, rather than ignored, and they are listed in
//! [`LONG_OPTIONS`] anyway because the table is what decides whether an
//! abbreviation is ambiguous.
//!
//! Ignoring them would be worse than refusing in almost every case: `-l` and
//! `-s` ask for a link rather than a copy, and `--sparse=always` asks for a
//! file whose holes survive. Every one of those, ignored, produces a
//! destination that looks right and is not.
//!
//! `--preserve` is the one option that is *partly* here, so it is refused a
//! word at a time rather than whole: six of GNU's seven words work, and
//! `--preserve=context` does not. There are no security contexts on this
//! system to carry, and a `--preserve` that silently carried nothing would
//! report success for a copy that dropped the one thing it was told to keep.
//! A whole-option refusal would instead send a user who asked for the six
//! attributes that do exist to look for another `cp`.
//! `--no-preserve=context` is taken rather than refused: refusing to *stop*
//! doing something this `cp` never does would be a refusal with no meaning
//! behind it.
//!
//! Note that `--preserve=all` is among the six. That is not this port
//! rounding up — GNU's own `PRESERVE_ALL` guards the security-context line
//! with `if (selinux_enabled)`, so on a machine without SELinux `all` does not
//! ask for a context either.
//!
//! # The four overwrite policies are four different options
//!
//! `-f`, `--remove-destination` and `-n` are routinely confused for two
//! options, or one. They are three, and each does something the others do not
//! — and `-i` is a fourth, which is not "`-n` but polite":
//!
//! | Option | When | What | `cp -v` order |
//! |---|---|---|---|
//! | `-f` | the open for writing **failed** | unlink and create a new file | `'a' -> 'b'` then `removed 'b'` |
//! | `--remove-destination` | always | unlink before opening at all | `removed 'b'` then `'a' -> 'b'` |
//! | `-n` | the destination exists | refuse, and **exit 1** | neither line |
//! | `-i` | the destination exists | ask; a non-`y` answer is **exit 1**, silently | neither line, if declined |
//!
//! `-i` and `-n` are two values of *one* field — GNU's `x.interactive` — so
//! the last of them on the command line wins and `cp -in` is `-n`. But they are
//! not the same refusal: `-n` prints `not replacing 'b'`, `-i` prints nothing
//! beyond the question, and `-n` also *suppresses* the "are the same file"
//! check that `-i` still makes. See [`overwrite_allowed`] and [`overwrite_ok`].
//!
//! Three consequences that fall out of the table and are each measurable:
//!
//! * `cp -f a b` on a writable `b` unlinks *nothing* — the truncating open
//!   succeeds, so `-f` never runs. Anything holding `b` open, and any hard link
//!   to it, sees the new contents. `cp --remove-destination a b` breaks both.
//! * `cp -f a link-to-b` writes through the link and leaves it a link;
//!   `cp --remove-destination a link-to-b` replaces the link with a file and
//!   leaves `b` alone.
//! * `cp -f a dangling-link` still fails, because the open that fails there is
//!   the `O_EXCL` one and `-f` acts on the other one. See
//!   [`create_destination`], which is the single place all three meet.
//!
//! `-n` is the one with a surprise in it: it writes `cp: not replacing 'b'` to
//! **stderr** and exits **1**. It is not a quiet skip — `copy.h`'s comment on
//! the value it sets reads "Skip and fail". Ubuntu's `cp` disagrees, because
//! Debian patches it; that patch is why `scripts/cp-diff.sh` builds its own
//! reference rather than comparing against the installed binary. See
//! `design-decisions.md` §726.
//!
//! Why these are three mechanisms here rather than one three-valued policy —
//! and the five behaviours a collapsed version could not produce — is
//! `design-decisions.md` §727. The cases are `scripts/cp-diff.sh` section 16.

use coreutils::backup::{self, BackupType, source_is_dst_backup, src_base_is_dot_or_dotdot};
use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::fileid::{
    EntryId, FileId, entry_id, file_id, is_same_file, same_entry, split_entry,
};
use coreutils::fsattr::{
    self, GroupRetry, Link, On, Ownership, chown_privileges, is_denied_ownership, owner_differs,
    owner_of, permission_bits, times_of,
};
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::overwrite::{self, Interactive};
use coreutils::quote::{os_bytes, quoteaf, quoteaf_os, quotef_os};
use coreutils::stdfd::{self, Stream};
use coreutils::yesno::{Answers, StdinAnswers};
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// `cp`'s usage status is 1, like almost every utility's; see
/// [`coreutils::getopt::Error`] for the two that differ and why.
const CP: Program = Program::new("cp", 1);

/// GNU `cp`'s `long_opts[]`, **in its declaration order**, which is observable:
/// `getopt_long` lists an ambiguous prefix's candidates in table order. Every
/// entry is here whether or not this implementation acts on it — see the module
/// docs for why leaving one out is a silent wrong answer rather than a missing
/// feature.
///
/// Measured with `cp --=x`, which an empty prefix makes print the whole table.
/// It once also held `("keep-directory-symlink", …)`, which is a `tar` option
/// and has never been a `cp` one; `scripts/getopt-ambiguity-check.py` now
/// compares this list against that readout on every run, because the same
/// mistake had independently reached `mv` as `--exchange`.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("archive", Takes::Nothing),
    ("attributes-only", Takes::Nothing),
    ("backup", Takes::Optional),
    ("copy-contents", Takes::Nothing),
    ("debug", Takes::Nothing),
    ("dereference", Takes::Nothing),
    ("force", Takes::Nothing),
    ("interactive", Takes::Nothing),
    ("link", Takes::Nothing),
    ("no-clobber", Takes::Nothing),
    ("no-dereference", Takes::Nothing),
    ("no-preserve", Takes::Required),
    ("no-target-directory", Takes::Nothing),
    ("one-file-system", Takes::Nothing),
    ("parents", Takes::Nothing),
    // Deprecated upstream but still in the table. It is the *same option* as
    // `--parents` — same `val` in GNU's `struct option` — which is why it is
    // named in [`ALIASES`] below and why `cp --pa` resolves rather than being
    // ambiguous. An earlier revision of this file asserted the opposite in a
    // comment here; measuring it (`cp --pa=1` answers `option '--parents'
    // doesn't allow an argument`) settled it the other way.
    ("path", Takes::Nothing),
    ("preserve", Takes::Optional),
    ("recursive", Takes::Nothing),
    ("remove-destination", Takes::Nothing),
    ("sparse", Takes::Required),
    ("reflink", Takes::Optional),
    ("strip-trailing-slashes", Takes::Nothing),
    ("suffix", Takes::Required),
    ("symbolic-link", Takes::Nothing),
    ("target-directory", Takes::Required),
    ("update", Takes::Optional),
    ("verbose", Takes::Nothing),
    ("context", Takes::Optional),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// The one pair of spellings in [`LONG_OPTIONS`] that name a single option.
///
/// See [`Program::resolve_long_aliased`]: without this, `--path` would count as
/// a second candidate for the prefix `--pa` and make `--parents` impossible to
/// abbreviate — which GNU allows. It does **not** make `--p` unambiguous, and a
/// test below pins that: `--p` still matches `--preserve`, which is a genuinely
/// different option.
const ALIASES: &[(&str, &str)] = &[("path", "parents")];

/// GNU `cp`'s `getopt_long` string, verbatim (`cp.c:992`).
///
/// The two colons are the part that cannot be left out. `-t` and `-S` take a
/// value, so `cp -t d a` is a target directory and one source, not three
/// operands, and `cp -S` is `option requires an argument -- 'S'` rather than a
/// copy of nothing. A table that merely listed the letters would parse both of
/// those into silently wrong operand lists.
const SHORT_OPTIONS: &str = "abdfHilLnprst:uvxPRS:TZ";

/// Whether `cp` copies a symbolic link, or copies whatever it points at.
///
/// GNU's `enum Dereference_symlink` (`copy.h`), spelled the same way and with
/// the same four members, including the one that is not a policy: `Undefined`
/// means none of `-P`, `-H`, `-L` was given, and is resolved by
/// [`CpFlags::resolved_deref`] rather than acted on.
///
/// Two policies and not one, because "follow a link" is answered differently
/// depending on *where the link was found*. That distinction is the whole of
/// `-H`, and it is invisible in any single boolean.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
enum Deref {
    /// None of the three options was given. Never observed outside
    /// [`CpFlags::resolved_deref`].
    #[default]
    Undefined,
    /// `-P` / `--no-dereference`: copy the link itself, wherever it was found.
    Never,
    /// `-H`: follow a link named as an operand; copy links found by walking a
    /// directory.
    CommandLine,
    /// `-L` / `--dereference`: follow every link, wherever it was found.
    Always,
}

/// One word of a `--preserve=` or `--no-preserve=` list.
///
/// GNU's `enum File_attribute`, declared inside `decode_preserve_arg`
/// (`cp.c:874`). The values matter to [`Program::argmatch`], which judges an
/// ambiguous prefix **by value rather than by spelling**: two words meaning the
/// same thing would not be ambiguous. None of these seven mean the same thing,
/// so every prefix that matches more than one is refused, exactly as GNU's
/// `XARGMATCH` refuses it.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
enum Attribute {
    Mode,
    Timestamps,
    Ownership,
    Links,
    Context,
    Xattr,
    /// Everything above, and in GNU also the SELinux context when the system
    /// has one. What `-a` means by `--preserve=all`.
    All,
}

/// The words `--preserve=` takes, **in GNU's table order** (`cp.c:preserve_args`).
///
/// The order is observable and not merely tidy: an invalid word makes gnulib
/// print `Valid arguments are:` followed by the table, one word per line, in
/// this order. A list sorted differently would produce a diagnostic no script
/// matching GNU's output could recognise.
const PRESERVE_WORDS: &[(&str, Attribute)] = &[
    ("mode", Attribute::Mode),
    ("timestamps", Attribute::Timestamps),
    ("ownership", Attribute::Ownership),
    ("links", Attribute::Links),
    ("context", Attribute::Context),
    ("xattr", Attribute::Xattr),
    ("all", Attribute::All),
];

/// Which of a source's attributes are put back onto the copy.
///
/// Five of GNU's seven words: the three POSIX names as the attributes `cp -p`
/// must restore — permission bits with the set-user-ID, set-group-ID and sticky
/// bits, the owner and group, and the two timestamps — plus `links`, which is
/// not one of them and is the only one that changes what ends up on disk rather
/// than what is attached to it, and `xattr`. Of the two that are not here,
/// `all` is not an attribute but the other six at once, and `context` is
/// refused at parse time rather than represented — see [`decode_preserve`] — so
/// no code downstream has to ask what to do about an attribute it cannot write.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
struct Preserve {
    /// `--preserve=mode`: the whole of `07777`, not just `0777`. Restoring the
    /// set-user-ID bit is the reason [`preserve_attributes`] takes an
    /// [`On`](fsattr::On) and prefers a descriptor to a name.
    mode: bool,
    /// `--preserve=timestamps`: the access and modification times. Not the
    /// change time, which no interface can set.
    timestamps: bool,
    /// `--preserve=ownership`: the owner and the group. Almost always fails for
    /// a non-root user, and that failure is *silent* — see [`chown_privileges`].
    ownership: bool,
    /// `--preserve=links`: when two sources turn out to be one inode, make the
    /// second destination a **hard link** to the first rather than a second
    /// copy of the bytes. See [`Copied`].
    ///
    /// The odd one out, and in two ways. It is not part of `-p` — GNU's `-p` is
    /// the three POSIX attributes and nothing else — and it is not an attribute
    /// of a file at all but a relationship between two of them, which is why it
    /// is the one word whose effect is visible in `ls -i` rather than in
    /// `ls -l`.
    links: bool,
    /// `--preserve=xattr`: the extended attributes — *except* the two that are
    /// the file's permissions rather than data about it.
    ///
    /// Those two, `system.posix_acl_access` and `system.posix_acl_default`, go
    /// with [`Self::mode`] instead, which is not a tidying but gnulib's own
    /// split: `qcopy_acl` chmods and then copies the permission-class names,
    /// while `copy_attr` copies everything else. See [`fsattr::Xattrs`].
    /// Carrying ACLs here would make `--preserve=xattr` change a file's
    /// permissions, which is not what its name says it does.
    xattr: bool,
}

impl Preserve {
    /// Nothing preserved: what `cp` does when `-p` was not given, and what
    /// `--no-preserve=all` returns it to.
    ///
    /// The same value as `Preserve::default()`, spelled as a `const` because
    /// `Default::default` is not callable in a constant and both call sites —
    /// the `--no-preserve=all` arm and the tests' `OFF` — want one.
    const NONE: Self = Preserve {
        mode: false,
        timestamps: false,
        ownership: false,
        links: false,
        xattr: false,
    };

    /// Every attribute this `cp` can carry: what `--preserve=all` and `-a` ask
    /// for.
    ///
    /// All five and not GNU's seven, which is GNU's own arithmetic rather than
    /// a shortfall — see the `Attribute::All` arm of [`decode_preserve`] for
    /// why `context` is not among them on a machine without SELinux, and
    /// [`Preserve`] for why the seventh word is not an attribute at all.
    const ALL: Self = Preserve {
        mode: true,
        timestamps: true,
        ownership: true,
        links: true,
        xattr: true,
    };

    /// What `-p`, and a bare `--preserve` with no list, ask for (`cp.c:1092`).
    ///
    /// The two `false`s are not omissions. GNU's `-p` is `preserve_mode`,
    /// `preserve_timestamps` and `preserve_ownership`, and `--preserve=links`
    /// and `--preserve=xattr` have to be asked for by name or through
    /// `-d`/`-a`.
    const fn posix() -> Self {
        Preserve {
            mode: true,
            timestamps: true,
            ownership: true,
            links: false,
            xattr: false,
        }
    }

    /// Add what `-p` asks for, leaving the attributes it does not name alone.
    ///
    /// **Not `*self = Self::posix()`.** GNU's `case 'p'` (`cp.c:1104`) is three
    /// assignments and no fourth: it never mentions `preserve_links`, so a `-p`
    /// that follows a `-d` still has it. Overwriting the whole value reads the
    /// same in every test that gives one option at a time, and turns `cp -d -p`
    /// from a command that hard-links two hard-linked sources together into one
    /// that gives the second its own copy of the bytes — with nothing said,
    /// which is the kind of wrong answer this file's docs are otherwise about.
    ///
    /// Spelled as an or-in of [`Self::posix`] rather than as three assignments
    /// so the list of what `-p` means stays in one place. The `false` fields of
    /// `posix()` or in as no-ops, which is exactly what "never mentions it"
    /// does.
    fn add_posix(&mut self) {
        let p = Self::posix();
        self.mode |= p.mode;
        self.timestamps |= p.timestamps;
        self.ownership |= p.ownership;
        self.links |= p.links;
        self.xattr |= p.xattr;
    }
}

// `Clone` for exactly one caller — [`same_name_backup_rewrite`], which is
// upstream's `x_tmp = *x` and needs a second options value that differs from
// the user's in one field.
#[derive(Clone, Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct CpFlags {
    recursive: bool,
    /// `-t DIR` / `--target-directory=DIR`: the destination, named by the
    /// option instead of by the last operand, so that every operand is a
    /// source. What `xargs cp -t dir` exists for.
    target_directory: Option<OsString>,
    /// `-T` / `--no-target-directory`: the destination is a name to create or
    /// replace, never a directory to copy *into*. `cp -T a d` overwrites `d`
    /// rather than writing `d/a` — or refuses to, when `d` is a directory.
    no_target_directory: bool,
    /// `-v` / `--verbose`: name every copy as it is made. See [`announce`] for
    /// where those lines come out and why they are not diagnostics.
    verbose: bool,
    /// `-P` / `-H` / `-L`: what to do with a symbolic link. Stored exactly as
    /// given, including "not given"; ask [`CpFlags::follow_operand`] and
    /// [`CpFlags::follow_walked`] rather than reading it.
    dereference: Deref,
    /// `-n` / `--no-clobber` and, later, `-i`. See [`Interactive`].
    interactive: Interactive,
    /// `-f` / `--force`: GNU's `unlink_dest_after_failed_open`, and the field
    /// name is the whole of the semantics. `-f` does **not** mean "remove the
    /// destination"; it means "if opening it for writing fails, remove it and
    /// create a new one". So `cp -f a b` on a writable `b` truncates `b` in
    /// place and never unlinks anything, `cp -f a b` on a 0400 `b` unlinks and
    /// recreates, and `cp -f a dangling-link` still refuses — the open that
    /// fails there is the `O_EXCL` one, which is not this one.
    /// See [`create_destination`], which is where the distinction lives.
    force: bool,
    /// `--remove-destination`: GNU's `unlink_dest_before_opening`. The option
    /// `-f` is commonly mistaken for. It unlinks unconditionally, before the
    /// copy is attempted at all, so it replaces a symlink with a file rather
    /// than writing through it.
    remove_destination: bool,
    /// `-p` / `--preserve[=LIST]` / `--no-preserve=LIST`. See [`Preserve`].
    preserve: Preserve,
    /// `--no-preserve=mode` (or `=all`) was given: GNU's
    /// `explicit_no_preserve_mode`, whose effect is not "leave the mode alone"
    /// but "give a **newly created** destination the mode it would have had if
    /// nobody had asked" — 0666 for a file, 0777 for a directory, each less the
    /// umask. So `cp --no-preserve=mode 0700-file b` writes a 0644 `b` where
    /// plain `cp` would write a 0600 one.
    ///
    /// A field of its own rather than `!preserve.mode`, because the two differ:
    /// plain `cp` also does not preserve the mode, and it does *not* do this.
    explicit_no_preserve_mode: bool,
    /// GNU's `require_preserve`: whether a failure to restore an attribute is
    /// an error rather than a warning.
    ///
    /// Set by `-p` and by any `--preserve=`, and **not** by `--no-preserve=`
    /// (`cp.c:1085`) — asking to stop preserving something cannot fail. The
    /// distinction is the whole difference between `cp -p a b` exiting 1 when
    /// the times could not be set and `cp --no-preserve=xattr a b` exiting 0,
    /// and it is why this is a flag rather than a constant `true`.
    require_preserve: bool,
    /// GNU's `require_preserve_xattr`: whether a failure to carry an *extended
    /// attribute* is an error rather than a warning.
    ///
    /// A second flag beside [`Self::require_preserve`] because GNU sets them in
    /// different places. `--preserve=xattr` sets this one inside
    /// `decode_preserve_arg` (`cp.c:PRESERVE_XATTR`); `--preserve=all` and `-a`
    /// turn extended attributes *on* without setting it, so they carry them
    /// best-effort. That is the whole difference between `cp --preserve=xattr`
    /// on a destination filesystem with no attribute support — which fails —
    /// and `cp -a` onto the same place, which succeeds.
    require_preserve_xattr: bool,
    /// GNU's `reduce_diagnostics`: say nothing at all about an extended
    /// attribute that could not be carried.
    ///
    /// Set only by `-a`, whose own comment in GNU is "like `-dR --preserve=all`
    /// with reduced failure diagnostics" (`cp.c:1063`). `cp -a` onto a FAT
    /// stick would otherwise print a line per file about attributes the user
    /// never mentioned, on a copy that succeeded.
    reduce_diagnostics: bool,
    /// `-b` / `--backup[=CONTROL]` and `-S`/`--suffix=SUFFIX`: what happens to
    /// a destination that is about to be replaced. GNU's `x.backup_type`
    /// together with the `simple_backup_suffix` global; see
    /// [`coreutils::backup`] for why the two are one value here.
    ///
    /// Reaches further into this file than an option that renames one file has
    /// any right to. Backups turn the destination `stat` into an `lstat`,
    /// suppress the "specified more than once" warning, suppress two of the
    /// three just-created guards, and are the only reason a failed copy has
    /// anything to undo. Each of those sites names the `copy.c` line it comes
    /// from.
    backup: backup::Backup,
}

impl CpFlags {
    /// [`Self::dereference`] with `Undefined` replaced by what it means.
    ///
    /// GNU does this once, after the option loop (`cp.c:1239`), and calls the
    /// default "compatible with FreeBSD": recursive copies keep links, flat
    /// copies follow them. That is why plain `cp link dst` writes a *file* and
    /// plain `cp -r link dst` writes a *link* — one option that was never
    /// given changing meaning because another one was.
    ///
    /// Resolved on demand here rather than written back into the struct, so
    /// that the parse tests can see `-r` and `-rP` as the different command
    /// lines they are, and so that there is no window in which an unresolved
    /// value could be read. GNU's `x.hard_link` also takes part in the rule
    /// (`x.recursive && ! x.hard_link`); `-l` is not implemented here, so its
    /// half of the condition is not yet expressible and is noted rather than
    /// guessed at.
    fn resolved_deref(&self) -> Deref {
        match self.dereference {
            Deref::Undefined if self.recursive => Deref::Never,
            Deref::Undefined => Deref::Always,
            given => given,
        }
    }

    /// Whether a source *named on the command line* is stat'd through.
    ///
    /// `copy.c:2250` picks `AT_SYMLINK_NOFOLLOW` exactly when the policy is
    /// `DEREF_NEVER`, so `-H` follows here and `-P` does not.
    fn follow_operand(&self) -> bool {
        self.resolved_deref() != Deref::Never
    }

    /// Whether a source *found by walking a directory* is stat'd through.
    ///
    /// GNU expresses this by handing the recursion a modified copy of the
    /// options: `copy.c:845` sets `non_command_line_options.dereference =
    /// DEREF_NEVER` when the policy is `DEREF_COMMAND_LINE_ARGUMENTS`. So only
    /// `-L` follows in here, which is what makes `cp -Hr` and `cp -Lr` differ
    /// at all — they agree about the operand and disagree about everything
    /// underneath it.
    fn follow_walked(&self) -> bool {
        self.resolved_deref() == Deref::Always
    }

    /// GNU's `should_dereference` (`copy.c:2148`), which is the same question
    /// as the two above asked once with the *place* as a parameter rather than
    /// baked into the name.
    ///
    /// `follow_operand()` is this with `true` and `follow_walked()` is this
    /// with `false`; they stay as they are because their call sites read better
    /// for it, and because each is asked where only one answer is possible.
    /// This one exists for [`place_entity`], which is reached from both places
    /// and so has to carry the distinction as data.
    fn should_dereference(&self, command_line_arg: bool) -> bool {
        match self.resolved_deref() {
            Deref::Always => true,
            Deref::CommandLine => command_line_arg,
            Deref::Never | Deref::Undefined => false,
        }
    }
}

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    /// The flags, and every operand in order. The last operand is the
    /// destination.
    Run(CpFlags, Vec<OsString>),
}

/// The funnel. A diagnostic that could not be written turns the earned
/// status into `exit_failure`, which is what upstream's `atexit
/// (close_stdout)` does on every exit path at once. See
/// [`stdfd::close_stderr`].
fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match parse_args(&args) {
        Ok(Request::Help) => {
            print!("{}", help_text());
            ExitCode::SUCCESS
        }
        Ok(Request::Version) => {
            println!("cp (SlateOS coreutils) 0.1.0");
            ExitCode::SUCCESS
        }
        Ok(Request::Run(flags, paths)) => {
            // `Stream` and not `io::stderr()`, whose failures the runtime hides: a
            // diagnostic that never arrived has to reach `close_stderr`'s flag.
            let mut out = Stream::stdout();
            let mut err = Stream::stderr();
            // Held for the whole run, not opened per prompt: `cp -i a b c d`
            // asks three questions and reads three lines of one stream.
            let mut answers = StdinAnswers::new();
            // One table for the whole command, which is what both of its
            // readers mean by "the same inode twice". See [`Copied`].
            let mut copied = Copied::default();
            let earned = {
                let mut job = Job {
                    flags: &flags,
                    copied: &mut copied,
                    out: &mut out,
                    err: &mut err,
                    answers: &mut answers,
                };
                if copy_all(&mut job, &paths) {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(1)
                }
            };
            // `--verbose` is the only thing `cp` ever writes to stdout, and a
            // line of it that never arrived has to change the status the same
            // way a lost diagnostic does — otherwise `cp -v … | head -1`
            // reports success for output nobody received.
            stdfd::close_stdout("cp", out, earned)
        }
        Err(e) => {
            diag!("cp: {e}");
            ExitCode::from(u8::try_from(e.status).unwrap_or(1))
        }
    }
}

fn help_text() -> String {
    "\
Usage: cp [OPTION]... SOURCE DEST
  or:  cp [OPTION]... SOURCE... DIRECTORY
Copy SOURCE to DEST, or multiple SOURCE(s) to DIRECTORY.

  -a, --archive         same as -dR --preserve=all
      --backup[=CONTROL]  make a backup of each existing destination file
  -b                    like --backup but does not accept an argument
  -d                    same as --no-dereference --preserve=links
  -f, --force           if an existing destination file cannot be
                          opened, remove it and try again
  -H                    follow command-line symbolic links in SOURCE
  -i, --interactive     prompt before overwrite (overrides a previous -n
                          option)
  -L, --dereference     always follow symbolic links in SOURCE
  -n, --no-clobber      do not overwrite an existing file (overrides a
                          previous -i option); exit status 1
  -P, --no-dereference  never follow symbolic links in SOURCE
  -p                    same as --preserve=mode,ownership,timestamps
      --preserve[=ATTR_LIST]  preserve the specified attributes
      --no-preserve=ATTR_LIST  don't preserve the specified attributes
      --remove-destination  remove each existing destination file before
                          attempting to open it (contrast with --force)
  -r, -R, --recursive   copy directories recursively.  Symbolic links are
                          copied as symbolic links, not followed.
  -S, --suffix=SUFFIX   override the usual backup suffix
  -t, --target-directory=DIRECTORY
                        copy all SOURCE arguments into DIRECTORY
  -T, --no-target-directory
                        treat DEST as a normal file
  -v, --verbose         explain what is being done
      --help            display this help and exit
      --version         output version information and exit

ATTR_LIST is a comma-separated list of attributes: 'mode' for the
permission bits together with any setuid, setgid and sticky bits,
'ownership' for the owner and group, 'timestamps' for the access and
modification times, and 'links' to make a hard link where two sources
turn out to be one file.  GNU's other two words -- 'context' and
'xattr' -- and 'all', which includes them, are accepted only by
--no-preserve.

The backup suffix is '~', unless set with --suffix or SIMPLE_BACKUP_SUFFIX.
The version control method may be selected via the --backup option or through
the VERSION_CONTROL environment variable.  Here are the values:

  none, off       never make backups (even if --backup is given)
  numbered, t     make numbered backups
  existing, nil   numbered if numbered backups exist, simple otherwise
  simple, never   always make simple backups

As a special case, cp makes a backup of SOURCE when the force and backup
options are given and SOURCE and DEST are the same name for an existing,
regular file.

To copy a file whose name starts with a '-', for example '-foo',
use one of these commands:
  cp -- -foo bar
  cp ./-foo bar
"
    .to_string()
}

// ---------------------------------------------------------------- parsing ---

/// Parse `cp`'s argv into `(flags, operands)`.
///
/// Options and operands may be interleaved — `cp a -r b` is `cp -r a b` — which
/// is `getopt_long`'s default permuting behaviour and what [`getopt::Parser`]
/// does.
///
/// # Errors
///
/// An unknown option, a recognised option this implementation does not have, a
/// long option given a value it does not take, or an option missing a value it
/// requires.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut flags = CpFlags::default();
    let mut paths: Vec<OsString> = Vec::new();

    // The three halves of the backup family, kept as locals through the loop
    // and resolved into [`CpFlags::backup`] after it — which is where GNU
    // resolves them too (`cp.c:1233`), and not merely for tidiness. Whether a
    // backup is made and *what it is named* are two separate questions with two
    // separate answers, and both can be settled by options given in either
    // order: `cp -S .bak --backup=simple` and `cp --backup=simple -S .bak` are
    // the same command. Resolving as we go would make the first `-b` read a
    // suffix a later `-S` was about to replace.
    //
    // `make_backups` is the one that is not simply "was an option given": both
    // `-b` and `-S` set it, so `cp -S .bak a b` backs up without `-b` ever
    // appearing. See the `-S` arm.
    let mut make_backups = false;
    let mut version_control: Option<OsString> = None;
    let mut backup_suffix: Option<OsString> = None;

    for item in CP.parse_aliased(args, SHORT_OPTIONS, LONG_OPTIONS, ALIASES) {
        match item? {
            Opt::Operand(name) => paths.push(name.clone()),
            Opt::Short(b'r' | b'R', _) | Opt::Long("recursive", _) => flags.recursive = true,
            Opt::Short(b't', value) | Opt::Long("target-directory", value) => {
                // A second `-t` is refused even when it names the same
                // directory as the first — GNU compares nothing, it just asks
                // whether one was already given. Measured: `cp -t d -t d a`
                // fails. And it is a plain diagnostic, with no "Try 'cp
                // --help'" after it, because GNU raises it with `error
                // (EXIT_FAILURE, …)` rather than through `usage`.
                if flags.target_directory.is_some() {
                    return Err(CP.usage("multiple target directories specified".into()));
                }
                // Unreachable: `t:` in [`SHORT_OPTIONS`] and `Takes::Required`
                // in [`LONG_OPTIONS`] both make the parser supply a value or
                // fail before this point.
                let Some(dir) = value else {
                    return Err(CP.short_missing_argument(b't'));
                };
                flags.target_directory = Some(dir);
            }
            Opt::Short(b'T', _) | Opt::Long("no-target-directory", _) => {
                flags.no_target_directory = true;
            }
            Opt::Short(b'v', _) | Opt::Long("verbose", _) => flags.verbose = true,
            // Plain assignment, so the last of several wins: `cp -LP` copies
            // the link and `cp -PL` follows it. GNU's three `case` arms do the
            // same, and there is no diagnostic for giving two of them — unlike
            // `-t`, where a repeat is an error.
            //
            Opt::Short(b'P', _) | Opt::Long("no-dereference", _) => {
                flags.dereference = Deref::Never;
            }
            // `-d` is `-P` *plus* `--preserve=links` (`cp.c:1044`), which is
            // why it is a separate arm rather than a second spelling of `-P`:
            // honouring only the dereference half would turn two hard-linked
            // sources into two independent copies with nothing said, which is
            // the silent wrong answer the module docs are about.
            //
            // It sets the two fields directly and leaves `require_preserve`
            // alone, because GNU's `case 'd'` does. That is what makes `cp -d`
            // and `cp -P --preserve=links` differ in one observable way: only
            // the second promises to fail if an attribute cannot be carried.
            Opt::Short(b'd', _) => {
                flags.dereference = Deref::Never;
                flags.preserve.links = true;
            }
            Opt::Short(b'L', _) | Opt::Long("dereference", _) => {
                flags.dereference = Deref::Always;
            }
            // No long spelling: GNU gives `-H` none either.
            Opt::Short(b'H', _) => flags.dereference = Deref::CommandLine,
            Opt::Short(b'f', _) | Opt::Long("force", _) => flags.force = true,
            Opt::Long("remove-destination", _) => flags.remove_destination = true,
            // Plain assignment again, and for the same reason as the symlink
            // policies: GNU keeps one `x.interactive` and lets the last option
            // win, so `cp -in` is `-n` and `cp -ni` is `-i` (measured against
            // 9.4 — the second prompts).
            Opt::Short(b'n', _) | Opt::Long("no-clobber", _) => {
                flags.interactive = Interactive::AlwaysNo;
            }
            Opt::Short(b'i', _) | Opt::Long("interactive", _) => {
                flags.interactive = Interactive::AskUser;
            }
            // `-p` and a bare `--preserve` are literally the same `case` in
            // GNU (`--preserve` with no value falls through to `'p'`), so a
            // bare `--preserve` is not "preserve nothing" — it is all three.
            Opt::Short(b'p', _) | Opt::Long("preserve", None) => {
                flags.preserve.add_posix();
                flags.require_preserve = true;
            }
            Opt::Long("preserve", Some(list)) => {
                decode_preserve(&list, true, &mut flags)?;
                // *After* the decode, and only for `--preserve`: GNU sets this
                // in the option loop rather than inside `decode_preserve_arg`,
                // which is what makes `--no-preserve=` leave it alone.
                flags.require_preserve = true;
            }
            Opt::Long("no-preserve", value) => {
                // Unreachable: `Takes::Required` in [`LONG_OPTIONS`] makes the
                // parser supply a value or fail before this point.
                let Some(list) = value else {
                    return Err(CP.long_missing_argument("no-preserve"));
                };
                decode_preserve(&list, false, &mut flags)?;
            }
            // `-a` is not a synonym for `-dR --preserve=all` — GNU's own
            // comment calls it "like" that, and the difference is the word
            // *like*. `case 'a'` (`cp.c:1063`) sets the same seven fields and
            // one more: `reduce_diagnostics`, which nothing else in `cp` sets.
            // So `cp -a` and `cp -dR --preserve=all` copy the same bytes with
            // the same attributes and differ in what they say when an extended
            // attribute cannot be carried — the first says nothing, the second
            // complains. See [`CpFlags::reduce_diagnostics`].
            //
            // [`Preserve::ALL`] rather than [`Preserve::add_posix`]: this sets
            // five where `-p` sets three, and `require_preserve` is the `-p`
            // half of it.
            Opt::Short(b'a', _) | Opt::Long("archive", _) => {
                flags.dereference = Deref::Never;
                flags.recursive = true;
                flags.preserve = Preserve::ALL;
                flags.require_preserve = true;
                flags.reduce_diagnostics = true;
            }
            // The short spelling's slot is always empty — `b` has no colon in
            // [`SHORT_OPTIONS`], as in GNU, and the help line says so: "like
            // --backup but does not accept an argument". It is bound anyway
            // because upstream's `case 'b'` reads `optarg` for both spellings
            // and gets a null for one of them; writing that out is cheaper than
            // a second arm that would have to be kept in step with this one.
            //
            // The `if let` rather than a plain assignment is upstream's `if
            // (optarg)` (`cp.c:1028`), and it is what makes `cp
            // --backup=numbered -b` stay numbered: a later bare `-b` turns
            // backups on again without erasing the word an earlier one chose.
            Opt::Short(b'b', value) | Opt::Long("backup", value) => {
                make_backups = true;
                if let Some(word) = value {
                    version_control = Some(word);
                }
            }
            // `-S` sets `make_backups` **as well as** the suffix (`cp.c:1190`),
            // which is the surprising half: `cp -S .bak a b` makes a backup
            // although `-b` was never given. Omitting that line would make
            // `-S` alone silently do nothing, which is the shape of wrong
            // answer this file's module docs are about — the copy succeeds and
            // the file the user meant to keep is gone.
            Opt::Short(b'S', value) | Opt::Long("suffix", value) => {
                // Unreachable: `S:` in [`SHORT_OPTIONS`] and `Takes::Required`
                // in [`LONG_OPTIONS`] both make the parser supply a value or
                // fail before this point.
                let Some(given) = value else {
                    return Err(CP.short_missing_argument(b'S'));
                };
                make_backups = true;
                backup_suffix = Some(given);
            }
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            // Everything else in the two tables is an option GNU has and this
            // one does not. Refused rather than ignored: see the module docs —
            // every one of them, ignored, produces a destination that looks
            // right and is not. The `Parser` has already turned a byte that is
            // in *no* table into `invalid option`, so nothing that reaches here
            // is a typo.
            Opt::Long(other, _) => return Err(unimplemented_long(other)),
            Opt::Short(other, _) => return Err(unimplemented_short(other)),
        }
    }

    // GNU's `cp.c:1220`, and it is a real contradiction rather than a tidiness
    // rule: `-n` says "leave the destination exactly as it is" and `-b` says
    // "move the destination aside", so a command with both has asked for the
    // file to stay and to go. Refused rather than resolved, because either
    // resolution silently ignores half of what was typed.
    //
    // Reachable through `-S` as well as `-b`, since `-S` sets `make_backups`:
    // `cp -n -S .bak a b` fails with this. Measured.
    //
    // The wording is upstream's verbatim, including the long spellings for
    // options that may have been given short — GNU names the concepts, not the
    // letters the user typed.
    //
    // `usage_referring` and not `usage`: upstream reaches this through
    // `usage (EXIT_FAILURE)` (`cp.c:1223`) rather than through `die`, so the
    // sentence is followed by `Try 'cp --help' for more information.` It is the
    // only diagnostic this program has that carries the referral, which is why
    // it is worth a comment — the neighbouring ones deliberately do not.
    if make_backups && flags.interactive == Interactive::AlwaysNo {
        return Err(
            CP.usage_referring("options --backup and --no-clobber are mutually exclusive".into())
        );
    }

    // The type is asked for only when an option asked for backups; the suffix
    // is settled unconditionally, exactly as upstream does it. That asymmetry
    // is load-bearing in one direction only: `$VERSION_CONTROL` alone must
    // never enable backups (which is why [`backup::control`] is not called
    // here without `make_backups`), while `$SIMPLE_BACKUP_SUFFIX` alone is
    // harmless because nothing reads the suffix unless backups are on.
    flags.backup = if make_backups {
        backup::Backup::new(
            backup::control(CP, version_control.as_deref())?,
            backup::suffix(backup_suffix.as_deref()),
        )
    } else {
        backup::Backup::disabled()
    };

    Ok(Request::Run(flags, paths))
}

/// The diagnostic for an option that GNU `cp` has and this one does not.
///
/// Deliberately not [`Program::invalid_option`]: `-p` is not a typo, and telling
/// the user it is invalid sends them to check their spelling of a flag they
/// spelled correctly.
fn unimplemented_short(flag: u8) -> getopt::Error {
    CP.usage_referring(format!(
        "option -{} is not implemented by this cp",
        char::from(flag)
    ))
}

fn unimplemented_long(name: &str) -> getopt::Error {
    CP.usage_referring(format!("option '--{name}' is not implemented by this cp"))
}

/// The refusal for a `--preserve=` word this implementation cannot honour.
///
/// A *word* and not the option, which is the point: `--preserve` is partly
/// implemented, so `option '--preserve' is not implemented` would be a lie to
/// anyone who wrote `--preserve=mode`. `because` names what is missing when the
/// word the user wrote is not itself the missing thing — `all` is refused
/// because of `links`, and saying so is the difference between a user reaching
/// for `--preserve=mode,timestamps,ownership` and reaching for another `cp`.
fn unimplemented_attribute(word: &str, because: &str) -> getopt::Error {
    CP.usage_referring(format!(
        "option '--preserve={word}' is not implemented by this cp{because}"
    ))
}

/// GNU's `decode_preserve_arg` (`cp.c:872`): apply one comma-separated list of
/// attribute words, either switching them on (`--preserve=`) or off
/// (`--no-preserve=`).
///
/// The split on commas is GNU's own and is why `--preserve=mode,ownership` is
/// one option rather than two: each word is resolved separately, so a bad word
/// in the middle names *itself* in the diagnostic rather than the whole list.
///
/// The asymmetry between the two directions is deliberate and is the module
/// docs' point about refusing a word at a time. Switching an attribute **on**
/// can be refused, because this `cp` might not be able to do it. Switching one
/// **off** never can: `--no-preserve=context` asks this `cp` to stop doing
/// something it has never done, and answering "not implemented" to that would
/// be refusing to obey an instruction that has already been obeyed. `context`
/// is the one word left that this applies to.
///
/// # Errors
///
/// A word that names nothing, a prefix that names several things that disagree
/// — both from [`Program::argmatch`], in gnulib's own wording — or a word this
/// implementation does not have, in the `on` direction.
fn decode_preserve(list: &OsString, on: bool, flags: &mut CpFlags) -> Result<(), getopt::Error> {
    let option = if on { "--preserve" } else { "--no-preserve" };
    // Split the *bytes*: an option value is OS data, and a list containing a
    // byte that is not UTF-8 must reach `argmatch` to be refused by name rather
    // than be rejected here with a different sentence.
    let bytes = os_bytes(list.as_os_str());
    for word in bytes.split(|&b| b == b',') {
        let attribute = CP.argmatch(word, option, PRESERVE_WORDS)?;
        // The spelling the user actually wrote, for the refusals below. It is a
        // prefix of one of the table's words, so it is ASCII whenever
        // `argmatch` resolved it at all.
        let spelling = String::from_utf8_lossy(word);
        match attribute {
            Attribute::Mode => {
                flags.preserve.mode = on;
                flags.explicit_no_preserve_mode = !on;
            }
            Attribute::Timestamps => flags.preserve.timestamps = on,
            Attribute::Ownership => flags.preserve.ownership = on,
            Attribute::Links => flags.preserve.links = on,
            // GNU's `PRESERVE_XATTR` sets *two* fields, and the second is why
            // `--preserve=xattr` is not just `--preserve=all` narrowed: naming
            // the word promises to fail if the attributes cannot be carried,
            // where `all` carries them best-effort. See
            // [`CpFlags::require_preserve_xattr`].
            Attribute::Xattr => {
                flags.preserve.xattr = on;
                flags.require_preserve_xattr = on;
            }
            // The SELinux security context. Alone among the seven in still
            // being refused, and for the reason the module docs give: this
            // kernel has no security contexts to read, and a `--preserve` that
            // silently carried nothing would report success for a copy that
            // dropped the thing it was asked to keep.
            Attribute::Context if on => {
                return Err(unimplemented_attribute(&spelling, ""));
            }
            // `all` is the other six words at once (`cp.c`'s `PRESERVE_ALL`).
            // It is *not* `context` as well on this system, and that is GNU's
            // own rule rather than a shortcut: its `PRESERVE_ALL` arm guards
            // the security-context line with `if (selinux_enabled)`, so on a
            // machine without SELinux `--preserve=all` does not ask for one
            // either. That is what makes `all` implementable while `context`
            // by name is not.
            //
            // An assignment rather than a delegation to [`Preserve::add_posix`]
            // because this arm is the `off` direction too, and `add_posix` only
            // turns things on. `all` and `no-preserve=all` are the two ends of
            // the range, so they are the two constants.
            Attribute::All => {
                flags.preserve = if on { Preserve::ALL } else { Preserve::NONE };
                flags.explicit_no_preserve_mode = !on;
                // Deliberately not `require_preserve_xattr`: GNU's
                // `PRESERVE_ALL` sets `preserve_xattr` and stops there.
            }
            // The `off` direction for `context`, which is the one word left
            // that this `cp` cannot do. Accepted rather than refused —
            // `--no-preserve=context` asks it to stop doing something it has
            // never done, and answering "not implemented" to that would be
            // refusing an instruction that is already obeyed.
            Attribute::Context => {}
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- copying ---

/// Everything a copy needs that is not the two paths: what was asked for, and
/// the two places it can say something.
///
/// One value rather than three parameters, because it is the *recursion* that
/// needs them. [`copy_tree`] and [`copy_entry`] could reach neither the flags
/// nor stdout, so no option that changes what happens inside a directory could
/// be written at all — and `--verbose`, `-p`, `-x`, `-L`/`-H` and
/// `--copy-contents` are all of them that. Two of those now exist: `--verbose`
/// reads `job.out`, and `-L` reads `job.flags` from inside [`copy_entry`].
///
/// Both sinks are parameters rather than `stdout()`/`stderr()` taken directly,
/// so that a test can assert on what a copy said. The old file had no test of
/// that path at all, which is how bugs 1–3 and 6 in the module docs survived.
struct Job<'a, O: Write, E: Write> {
    flags: &'a CpFlags,
    /// The record of which inode went where, read by `--preserve=links` and by
    /// the directory-named-twice refusal alike.
    ///
    /// On `Job` and not on [`Seen`] because the *walk* needs it, twice over:
    /// two hard-linked files inside one source directory must come out linked
    /// too, and a directory reached by walking has to be checked against the
    /// directories already copied. [`copy_entry`] can reach `Job` and cannot
    /// reach `Seen`, so anything it must consult lives here.
    copied: &'a mut Copied,
    /// Where `--verbose` announces. Measured: GNU's `emit_verbose` uses
    /// `printf`, so the line is on stdout and is *not* a diagnostic.
    out: &'a mut O,
    err: &'a mut E,
    /// Where `-i`'s prompts are answered. See [`overwrite_ok`].
    ///
    /// `dyn` rather than a third generic parameter: eleven signatures name
    /// `Job`, and none of them but [`overwrite_ok`] cares what the answers come
    /// from. The one indirect call is per *prompt*, which is per human keypress.
    answers: &'a mut dyn Answers,
}

/// `--verbose`'s one line about one copy: `'src' -> 'dst'`, and with `-b`
/// `'src' -> 'dst' (backup: 'dst~')`.
///
/// Three measured facts are packed into four lines of code, and each of them is
/// a way the obvious implementation would be wrong:
///
/// * **It goes to stdout, not stderr.** GNU's `emit_verbose` (`copy.c:2082`) is
///   a `printf`. So `cp -v a b > log` captures the line and `cp -v a b
///   2>/dev/null` does not silence it — the reverse of what a diagnostic does.
///   That is also why `run_main` has to route stdout through
///   [`stdfd::close_stdout`]: with `-v` this utility finally *has* stdout
///   output whose loss must change the exit status.
/// * **Both names are quoted, in the same style as a diagnostic's.** GNU writes
///   `quoteaf_n (0, src)` and `quoteaf_n (1, dst)` — two slots of one style, not
///   two styles — so `cp -v 'a b' c` prints `'a b' -> c` and the reader can tell
///   a space in a name from a space between names.
/// * **There is no flush here.** The line is buffered like any other stdout
///   write and lands in order with respect to nothing else, because `cp` writes
///   nothing else to stdout. Interleaving with stderr is not a property GNU has
///   either — piping the two together reorders them there too.
///
/// *When* it is called is the part that is not local to this function, and is
/// documented at each of the two call sites: after every refusal and after the
/// backup but before the copy for a non-directory, and only on the `mkdir`
/// actually happening for a directory.
///
/// `backup` is `None` at the directory call site and always will be: `cp` backs
/// a destination up only when it is *not* a directory (`copy.c:2524`), and a
/// directory source onto a non-directory destination is refused earlier with
/// `cannot overwrite non-directory`. `mv` is the utility for which that
/// combination exists, and it does not share this function.
fn announce<O: Write, E: Write>(
    job: &mut Job<'_, O, E>,
    src: &Path,
    dst: &Path,
    backup: Option<&Path>,
) {
    if !job.flags.verbose {
        return;
    }
    match backup {
        // One `writeln!` and not two, because the parenthesis is part of *this*
        // line rather than a note after it: GNU's `emit_verbose` prints the
        // arrow with `printf` and only then the suffix, with the newline last.
        // Two writes would let a `cp -v … | head` truncate between them.
        Some(name) => {
            let _ = writeln!(
                job.out,
                "{} -> {} (backup: {})",
                quoteaf_os(src),
                quoteaf_os(dst),
                quoteaf_os(name)
            );
        }
        None => {
            let _ = writeln!(job.out, "{} -> {}", quoteaf_os(src), quoteaf_os(dst));
        }
    }
}

/// Copy every source onto the destination.
///
/// Returns `true` if everything asked for was copied.
fn copy_all<O: Write, E: Write>(job: &mut Job<'_, O, E>, paths: &[OsString]) -> bool {
    let flags = job.flags;
    // GNU's `n_files <= !target_directory`. With `-t` the destination came from
    // the option, so one operand is enough; without it the last operand *is* the
    // destination and two are needed. Zero and one are distinct diagnostics —
    // "missing operand" alone left the user to work out which.
    let least = usize::from(flags.target_directory.is_none());
    if paths.len() <= least {
        let message = match paths.first() {
            None => "missing file operand".to_string(),
            Some(first) => format!(
                "missing destination file operand after {}",
                quoteaf_os(first)
            ),
        };
        let _ = writeln!(job.err, "cp: {}", CP.usage_referring(message));
        return false;
    }

    // Both `-T` checks come before `-t`'s directory is even looked at, which is
    // GNU's order and is observable: `cp -t nosuch -T a b` reports the
    // combination rather than the missing directory.
    if flags.no_target_directory {
        if flags.target_directory.is_some() {
            let _ = writeln!(
                job.err,
                "cp: cannot combine --target-directory (-t) and --no-target-directory (-T)"
            );
            return false;
        }
        // `-T` is "the destination is one name", so a third operand is not a
        // source that went to the wrong place — it is an operand with nowhere
        // to go at all.
        if let Some(extra) = paths.get(2) {
            let _ = writeln!(
                job.err,
                "cp: {}",
                CP.usage_referring(format!("extra operand {}", quoteaf_os(extra)))
            );
            return false;
        }
    }

    let (sources, dest, dest_is_dir) = match &flags.target_directory {
        // Every operand is a source. The directory is checked once, here, and
        // the failure names it as a *target directory* — a different sentence
        // from the one below, because the user named it as one.
        Some(dir) => {
            if let Some(e) = dest_directory_error(Path::new(dir)) {
                let why = strerror(&e);
                let _ = writeln!(job.err, "cp: target directory {}: {why}", quoteaf_os(dir));
                return false;
            }
            (paths, dir, true)
        }
        None => {
            // Unreachable: the operand count was checked above.
            let Some((dest, sources)) = paths.split_last() else {
                return false;
            };
            if flags.no_target_directory {
                // `-T` asks for the destination to be treated as a name and
                // never as a directory to copy *into*, so it is not stat'd for
                // that question at all and `cp -T a d` goes on to report that a
                // directory cannot be overwritten with a non-directory.
                (sources, dest, false)
            } else {
                // The destination is followed, which is right here: `cp a
                // link-to-dir/` puts `a` inside the directory.
                let not_a_dir = dest_directory_error(Path::new(dest));

                // GNU reports *why* the last operand is not a directory, and
                // the two reasons read differently: `cp a b nosuch` says "No
                // such file or directory" while `cp a b afile` says "Not a
                // directory". One fixed sentence for both loses the distinction
                // that tells a user whether they mistyped the name or forgot to
                // make the directory.
                if sources.len() > 1
                    && let Some(e) = not_a_dir
                {
                    let why = strerror(&e);
                    let _ = writeln!(job.err, "cp: target {}: {why}", quoteaf_os(dest));
                    return false;
                }
                (sources, dest, not_a_dir.is_none())
            }
        }
    };
    // `cp -f -b foo foo`, which GNU answers by rewriting the command rather
    // than by refusing it. See [`same_name_backup_rewrite`].
    let rewritten = same_name_backup_rewrite(sources, dest, dest_is_dir, flags);
    let dest_path = rewritten
        .as_ref()
        .map_or(Path::new(dest), |(name, _)| Path::new(name));
    // Upstream's `x = &x_tmp`: from here on the copy runs under the rewritten
    // options, and everything above ran under the ones the user gave. A
    // reborrow rather than a second `Job`-shaped branch, so that the loop below
    // stays the one loop.
    let job = &mut Job {
        flags: rewritten.as_ref().map_or(flags, |(_, f)| f),
        copied: &mut *job.copied,
        out: &mut *job.out,
        err: &mut *job.err,
        answers: &mut *job.answers,
    };

    // Both "named twice" problems need two sources to exist at all, so GNU
    // builds the tables only in that case and this follows it — not to save the
    // allocation, but because the tables also decide whether a *repeat* is
    // possible, and with one source it never is. Counted after `-t` has been
    // resolved, as GNU counts it: `cp -t d a a` is two sources and does warn.
    let mut seen = (sources.len() > 1).then(Seen::default);

    let mut ok = true;
    for src in sources {
        if !copy_one(src, dest_path, dest_is_dir, seen.as_mut(), job) {
            ok = false;
        }
    }
    ok
}

/// GNU's `cp --force --backup foo foo` conversion (`cp.c:797`), which turns the
/// command into `cp --force foo fooSUFFIX` and switches backups **off**.
///
/// Worth understanding as a rewrite rather than as a special case, because the
/// alternative reading gets the result wrong in a way that looks right. Left
/// alone, `cp -fb foo foo` would reach the ordinary path, back `foo` up to
/// `foo~`, find the source gone, and fail — having moved the user's file to a
/// name they did not ask for. Upstream instead observes that "back this up and
/// then copy it onto itself" is precisely "copy it to its backup name", and
/// issues that copy. The result is a `foo~` holding `foo`'s bytes with `foo`
/// still in place, which is what `cp -fb foo foo` is *for*.
///
/// Four conditions, all upstream's:
///
/// * **`-f`.** Without it the command is refused as "the same file" instead.
///   Upstream gives no reason and there is no obvious one beyond backwards
///   compatibility, but it is measurable: `cp -b foo foo` fails.
/// * **Backups are on**, or there is no name to rewrite to.
/// * **The two operands are byte-identical.** `STREQ`, not "same file" — so
///   `cp -fb foo ./foo` is *not* rewritten and does fail. Faithful, and the
///   distinction is upstream's to defend, not this port's.
/// * **The destination exists and is a regular file.** A directory or a device
///   has no business being copied onto its own backup name.
///
/// The suffix must be worked out *before* backups are switched off, because it
/// is the backup type that decides the name — upstream flags that ordering with
/// a comment of its own.
///
/// Returns `None` when the command is left alone, which includes the case where
/// the backup name could not be worked out at all. That degrades to the
/// ordinary path and its `'foo' and 'foo' are the same file`, which is a true
/// statement about a command that is not going to run either way.
fn same_name_backup_rewrite(
    sources: &[OsString],
    dest: &OsString,
    dest_is_dir: bool,
    flags: &CpFlags,
) -> Option<(OsString, CpFlags)> {
    if dest_is_dir || !flags.force || !flags.backup.enabled() {
        return None;
    }
    // Exactly one source. Upstream reaches this code only in its `n_files == 2`
    // branch; here the same thing is said by asking, because the count was
    // settled further up.
    let [source] = sources else {
        return None;
    };
    if source != dest {
        return None;
    }
    let target = Path::new(dest);
    if !fs::metadata(target).is_ok_and(|m| m.is_file()) {
        return None;
    }
    let name = flags.backup.find_name(target).ok()?;
    let mut without_backups = flags.clone();
    without_backups.backup = backup::Backup::disabled();
    Some((name.into_os_string(), without_backups))
}

/// `None` if `dest` is a directory, otherwise the failure that says why not.
///
/// GNU asks this by *opening* the operand with `O_DIRECTORY` and keeping the
/// errno. Asking `stat` gives the same two answers — `ENOENT` for a name that
/// is not there, `ENOTDIR` for one that is something else — without needing
/// `O_PATH`, which is a Linux extension the target does not have. The case the
/// two could part company on is a directory that can be stat'd but not
/// searched; `O_PATH` opens that successfully and so does `stat`, so they
/// agree there too.
fn dest_directory_error(dest: &Path) -> Option<io::Error> {
    match fs::metadata(dest) {
        Ok(m) if m.is_dir() => None,
        Ok(_) => Some(io::Error::from(io::ErrorKind::NotADirectory)),
        Err(e) => Some(e),
    }
}

/// What this command has already copied, and where it put it.
///
/// Two of GNU's refusals need it, and both exist to stop one operand
/// destroying the result of an earlier one in the same command — `cp a
/// other/a d` would otherwise leave `d/a` holding `other/a`, and the copy of
/// `a` the user asked for would be gone with nothing said. GNU keeps two hash
/// tables for this (`copy.c`'s `src_info` and `dest_info`); the two fields
/// below are the same information.
///
/// Only *command-line* sources go in. A file reached by recursing into a
/// directory cannot be named twice on one command line, so recording it would
/// be work spent on a question that cannot arise. GNU's third table,
/// `src_to_dest`, is not like that and is not here: see [`Copied`].
#[derive(Default)]
struct Seen {
    /// Non-directory sources already copied. Keyed on the file's identity
    /// *and* the entry that named it, which is GNU's `triple_compare`: `cp a
    /// ./a d` is one file named twice, while `cp a hard-link-to-a d` is two
    /// entries that happen to share an inode and is a legitimate request.
    sources: HashSet<(FileId, EntryId)>,
    /// Destinations this command created, by path *and* identity. Both halves
    /// are needed: the path is what a later operand would collide with, and
    /// the identity is what says the thing at that path is still the one we
    /// made.
    dests: HashSet<(PathBuf, FileId)>,
}

impl Seen {
    /// Records a non-directory source and reports whether it was already
    /// there. Recorded even when the copy goes on to fail, which is GNU's
    /// behaviour and the reason `cp f f d` with `d/f` a directory reports the
    /// refusal once and the repeat once rather than the refusal twice.
    fn saw_source(&mut self, id: FileId, entry: EntryId) -> bool {
        !self.sources.insert((id, entry))
    }

    /// Whether `target` is a destination this command created and which is
    /// still the same file.
    fn made(&self, target: &Path, id: FileId) -> bool {
        self.dests.contains(&(target.to_path_buf(), id))
    }

    /// Remember a destination just written. `lstat`, as GNU does: what is
    /// wanted is the identity of the *entry*, so that a symbolic link just
    /// created is recognised as itself rather than as whatever it points at.
    fn record_dest(&mut self, target: &Path) {
        if let Ok(m) = fs::symlink_metadata(target)
            && let Some(id) = file_id(target, &m)
        {
            self.dests.insert((target.to_path_buf(), id));
        }
    }
}

/// "Have I copied this inode already, and where to?" — GNU's one `src_to_dest`
/// table (`copy.c:1997`), with its `remember_copied` and `src_to_dest_lookup`.
///
/// Two rules read it, and they look unrelated until you notice that both are
/// asking what a *second* appearance of one inode should become:
///
/// * **`--preserve=links`** answers "a hard link to where the first appearance
///   landed", which is why the value has to be a nameable destination.
/// * **The directory rule** answers "nothing — a directory cannot appear twice
///   except by being hard-linked, and hard-linked directories are what GNU
///   refuses", with its comment naming Netapp snapshot trees as where they turn
///   up. That refusal quotes the earlier destination too.
///
/// One table for both, as GNU has, and one for the whole command rather than
/// one per operand: `cp --preserve=links a b d` has to notice at `b` that it
/// already wrote `a`'s inode to `d/a`, so `hash_init` is called once in `main`
/// (`cp.c:1284`). Its comment "in this command line argument" is about which
/// *arguments can share* an inode, not about the table's lifetime.
///
/// The two rules were split here once — the directory half lived on [`Seen`],
/// which only [`copy_one`] can reach — and the split was a bug, not a
/// simplification: a directory found by *walking* was then checked against
/// nothing at all. Measured before the merge, with `parent/child` a directory:
/// `cp -r parent/child parent d` copied that subtree twice and exited 0, where
/// GNU refuses the repeat with `will not create hard link 'd/parent/child' to
/// directory 'd/child'` and exits 1. See design-decisions.md 736.
#[derive(Default)]
struct Copied(HashMap<FileId, PathBuf>);

impl Copied {
    /// GNU's `remember_copied`: note that `id` is being copied to `target`, and
    /// answer with where it went *last* time if there was a last time.
    ///
    /// Recording and looking up in one call is GNU's shape and not a
    /// convenience: the two must not be separable, because a source that was
    /// looked up and then not recorded would let a third operand link to a
    /// destination the second one never made.
    ///
    /// By reference, and so are the two below, because a caller that has to
    /// [`Copied::forget`] a failed copy needs the same id afterwards. Taking it
    /// by value costs nothing on a host with inode numbers — [`FileId`] is two
    /// words there and `Copy` — but the portable stand-in is a `PathBuf`, which
    /// is not, so the caller could not use the id again and the whole
    /// `cfg(not(unix))` build stopped compiling. Nobody noticed for a while:
    /// this crate's gate is the `x86_64-slateos` target, whose `target-family`
    /// is `unix`.
    fn remember(&mut self, id: &FileId, target: &Path) -> Option<PathBuf> {
        if let Some(earlier) = self.0.get(id) {
            return Some(earlier.clone());
        }
        self.0.insert(id.to_owned(), target.to_path_buf());
        None
    }

    /// GNU's `src_to_dest_lookup` (`copy.c:2670`): where did this inode go —
    /// *without* claiming it is going here.
    ///
    /// The difference from [`Copied::remember`] is load-bearing and is GNU's.
    /// A directory reached by walking is looked up and never recorded, because
    /// only a directory *named on the command line* can be named twice;
    /// recording walked ones instead would make the second half of a `cp -r p d
    /// p` accuse the first half's entries of repeating themselves.
    fn lookup(&self, id: &FileId) -> Option<&Path> {
        self.0.get(id).map(PathBuf::as_path)
    }

    /// GNU's `forget_created`, called from its `un_backup` label
    /// (`copy.c:3362`) when a copy that had just been recorded failed.
    ///
    /// Without it a later hard link would be made to a destination that does
    /// not exist, and the second operand would report `cannot create hard link`
    /// instead of the failure the first one actually had. Measured: GNU's
    /// `cp --preserve=links a b d` with `a` unreadable reports the *same*
    /// `cannot open … for reading` twice.
    fn forget(&mut self, id: &FileId) {
        self.0.remove(id);
    }
}

/// What is at the destination path, as far as `cp` needs to know.
///
/// GNU carries the same three states in two variables — `new_dst` and whether
/// `dst_sb` was filled in — and the third state is the one that makes an enum
/// worth having: a destination that is *there* and cannot be stat'd. See
/// [`Dest::Opaque`].
enum Dest {
    /// Nothing is there. GNU's `new_dst = true`.
    New,
    /// Something is there, and this is it.
    Exists(fs::Metadata),
    /// Something is there whose `stat` failed with `ELOOP`, under `-f`, which
    /// asked for it to be unlinked rather than given up on. `copy.c:2326`
    /// leaves `new_dst = false` here with the comment "leave new_dst=false so
    /// we unlink later", and that is the whole of the variant: a
    /// self-referential symlink is replaced by `cp -f a loop` and reported as
    /// `cannot stat 'loop': Too many levels of symbolic links` without it.
    ///
    /// Deliberately *not* folded into [`Dest::New`]: the difference decides
    /// which `open` [`create_destination`] tries first, and that in turn
    /// decides whether the destination is unlinked or the copy is refused as a
    /// dangling symlink.
    Opaque,
}

impl Dest {
    /// The `stat`, when there is one. `None` covers both "nothing is there"
    /// and "something is there that could not be stat'd", which is right for
    /// every caller: all of them are asking a question about the destination's
    /// *kind*, and neither state has one.
    fn metadata(&self) -> Option<&fs::Metadata> {
        match self {
            Dest::Exists(m) => Some(m),
            Dest::New | Dest::Opaque => None,
        }
    }

    /// Whether something is there — GNU's `! new_dst`.
    fn exists(&self) -> bool {
        !matches!(self, Dest::New)
    }
}

/// `stat` the destination, the way GNU's `copy_internal` does (`copy.c:2302`).
///
/// A regular file can be written *through* a symbolic link and nothing else
/// can, so a regular source follows the destination name and a directory or a
/// symlink does not. `--remove-destination` joins the second group even for a
/// regular source, because it is going to unlink that name rather than write
/// through it — which is what makes `cp --remove-destination a dangling-link`
/// replace the link instead of refusing.
///
/// `--backup` joins that group for the same reason (`copy.c:2313`) and with the
/// same kind of consequence. The destination is about to be *renamed*, and a
/// rename moves the link rather than what it points at. Without this line, `cp
/// -b a link-to-b` would follow the link, conclude it was looking at `b`, and
/// then rename the link anyway — leaving `b` neither backed up nor overwritten
/// while a fresh regular `link-to-b` appeared beside it. Measured against 9.4,
/// which backs up the link itself.
///
/// # Errors
///
/// Any `stat` failure other than "it isn't there", which is [`Dest::New`], and
/// other than `ELOOP` under `-f`, which is [`Dest::Opaque`].
fn stat_destination(src_meta: &fs::Metadata, target: &Path, flags: &CpFlags) -> io::Result<Dest> {
    let use_lstat = src_meta.is_dir()
        || src_meta.file_type().is_symlink()
        || flags.remove_destination
        || flags.backup.enabled();
    let stat = if use_lstat {
        fs::symlink_metadata(target)
    } else {
        fs::metadata(target)
    };
    match stat {
        Ok(m) => Ok(Dest::Exists(m)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Dest::New),
        Err(e) if flags.force && is_eloop(&e) => Ok(Dest::Opaque),
        Err(e) => Err(e),
    }
}

/// Whether an `io::Error` is `ELOOP`.
///
/// By raw code rather than `ErrorKind`: `ErrorKind::FilesystemLoop` is still
/// unstable, and the one place this is asked has to be right on the target
/// rather than merely compile.
#[cfg(unix)]
fn is_eloop(e: &io::Error) -> bool {
    e.raw_os_error() == Some(libc_eloop())
}

/// `ELOOP` is 40 on Linux, which is the only ABI this ships on; there is no
/// `libc` dependency in this crate to read it from. Named rather than written
/// inline so that a port to another target has one place to look.
#[cfg(unix)]
const fn libc_eloop() -> i32 {
    40
}

/// Windows has no `ELOOP`, and `-f` on the development host therefore never
/// reaches [`Dest::Opaque`]. See [`open_new`]'s non-unix arm for the same
/// split.
#[cfg(not(unix))]
fn is_eloop(_e: &io::Error) -> bool {
    false
}

/// `-n`'s refusal: `cp: not replacing 'dst'`, on **stderr**, and the operand
/// counts as a failure.
///
/// Both halves are measured against 9.4 and both are surprising. It is a
/// diagnostic rather than a `--verbose`-style note, so `cp -n a b 2>/dev/null`
/// is silent; and `cp -n` over an existing file *exits 1*, so `-n` is not
/// "skip quietly" — `copy.h`'s comment on `I_ALWAYS_NO` reads "Skip and fail".
/// The quiet spelling is `--update=none`, which this `cp` does not have yet.
///
/// Ubuntu's `cp` does neither: `debian/patches/cp-n.diff` makes `-n` a silent
/// success. That patch is why the differential harness builds its own
/// reference — see `scripts/diff-wsl.sh` and `design-decisions.md` §726.
fn refuse_no_clobber<O: Write, E: Write>(target: &Path, job: &mut Job<'_, O, E>) {
    let _ = writeln!(job.err, "cp: not replacing {}", quoteaf_os(target));
}

/// The three helpers this used to define privately — `can_write_any_file`,
/// `writable_destination` and `dest_mode`, plus the `euidaccess`/`geteuid`
/// binding under them — now live in [`coreutils::overwrite`], because `mv`
/// needs the identical three and a second copy of a decision about whether to
/// destroy a file is the kind of duplicate that is only noticed after the data
/// is gone. Upstream shares them by construction: `cp` and `mv` are two front
/// ends over one `copy.c`.
/// `-i`'s question — [`coreutils::overwrite::overwrite_ok`] with this program's
/// name and this program's answer to `clears_destination`.
///
/// That last is the whole of `cp`'s share of the decision. Upstream computes it
/// as `x->move_mode || x->unlink_dest_before_opening ||
/// x->unlink_dest_after_failed_open`; the first disjunct is `mv` and the other
/// two are exactly `--remove-destination` and `-f`. It picks between
///
/// ```text
/// cp: unwritable 'b' (mode 0444, r--r--r--); try anyway?
/// cp: replace 'b', overriding mode 0444 (r--r--r--)?
/// ```
///
/// which is the difference between a warning that the copy will probably fail
/// and a warning that it will probably succeed by destroying something the mode
/// was protecting.
fn overwrite_ok<O: Write, E: Write>(
    target: &Path,
    dest_meta: Option<&fs::Metadata>,
    job: &mut Job<'_, O, E>,
) -> bool {
    let clears_destination = job.flags.force || job.flags.remove_destination;
    overwrite::overwrite_ok(
        job.err,
        "cp",
        target,
        dest_meta,
        clears_destination,
        job.answers,
    )
}

/// The whole of the "is this destination to be left alone" decision — GNU's
/// one block at `copy.c:2421`, which handles `-n` and `-i` together because
/// they are two values of one field.
///
/// Returns `true` when the copy should go ahead. The two refusals differ in
/// what they print — `-n` says `not replacing 'b'`, `-i` says nothing at all
/// beyond the question it already asked — but not in the status: both make the
/// operand a failure, which is `copy.h`'s "Skip and fail" for `I_ALWAYS_NO` and
/// `return_val = x->interactive == I_ALWAYS_SKIP` (false here) for
/// `I_ASK_USER`.
///
/// A **directory** source is exempt from both, as GNU's `! S_ISDIR (src_mode)`
/// makes it: `cp -rn tree dest` descends and refuses the files inside one at a
/// time, and `cp -ri tree dest` asks about them one at a time, rather than
/// either putting a single question about the tree.
fn overwrite_allowed<O: Write, E: Write>(
    src_meta: &fs::Metadata,
    target: &Path,
    dest: &Dest,
    job: &mut Job<'_, O, E>,
) -> bool {
    if src_meta.is_dir() || !dest.exists() {
        return true;
    }
    match job.flags.interactive {
        // `AlwaysYes` is `mv -f`, and this program's parser never produces it —
        // `cp -f` is `unlink_dest_after_failed_open`, a different field. It is
        // in the shared enum because it is in `copy.h`'s, and the arm is here
        // rather than under a catch-all so that adding an option which *does*
        // set it is a compile error rather than a silent fall-through.
        Interactive::Unspecified | Interactive::AlwaysYes => true,
        Interactive::AlwaysNo => {
            refuse_no_clobber(target, job);
            false
        }
        // `AlwaysSkip` is `--update=none`, which `mv` has and this `cp` does
        // not, so its parser cannot produce this either. The arm is spelled out
        // rather than folded into a catch-all so that adding `cp --update` is a
        // compile error here — and it has to be, because the `bool` this
        // function returns cannot say what `AlwaysSkip` means. Upstream's
        // `return_val = x->interactive == I_ALWAYS_SKIP` (`copy.c:2429`) is a
        // *skip that succeeds*, so implementing `cp -u` means widening this
        // return type, exactly as `mv`'s [`Verdict`] was widened. Answering
        // `false` in the meantime would be the same wrong exit status `mv` had
        // before that change. Silent, at least, because that half of
        // `AlwaysSkip` this signature *can* express.
        Interactive::AlwaysSkip => false,
        Interactive::AskUser => overwrite_ok(target, dest.metadata(), job),
    }
}

/// `--remove-destination`: unlink an existing destination before the copy is
/// attempted at all.
///
/// Returns `false` when it could not, having said so. A directory destination
/// is left alone — GNU guards this with `! S_ISDIR (dst_sb.st_mode)`
/// (`copy.c:2570`), so `cp -T --remove-destination a dir` still reports
/// `cannot overwrite directory 'dir' with non-directory` and `dir` survives.
///
/// `dest` is updated to [`Dest::New`] on success, which is GNU's `new_dst =
/// true` and matters twice over: [`create_destination`] must then create
/// rather than truncate, and [`place_source`]'s symlink arm must not announce
/// a second `removed` for a name that is already gone.
///
/// **A destination that `--backup` is about to move aside is left alone.**
/// Upstream this unlink is not a separate step but the `else if` of the backup
/// block (`copy.c:2568`), and reading the two as independent removes the very
/// file the backup exists to keep: `cp --remove-destination -b a b` would
/// delete `b`, find nothing to rename, and report a plain copy — which is
/// `--backup` silently doing nothing at all. See [`backup_takes_destination`],
/// which is that `if`'s condition and is asked here for its `else`.
fn remove_destination_first<O: Write, E: Write>(
    src: &Path,
    target: &Path,
    dest: &mut Dest,
    job: &mut Job<'_, O, E>,
) -> bool {
    if !job.flags.remove_destination || backup_takes_destination(src, dest, job.flags) {
        return true;
    }
    match dest {
        Dest::Exists(m) if !m.is_dir() => {}
        _ => return true,
    }
    if let Err(e) = fs::remove_file(target)
        && e.kind() != io::ErrorKind::NotFound
    {
        let why = strerror(&e);
        let _ = writeln!(job.err, "cp: cannot remove {}: {why}", quoteaf_os(target));
        return false;
    }
    *dest = Dest::New;
    // On stdout and before the arrow line, unlike `-f`'s removal, which comes
    // after it. The two are not printed from the same place in GNU either:
    // this one is in `copy_internal` ahead of `emit_verbose`, `-f`'s is inside
    // `copy_reg` behind it. Measured — `cp --remove-destination -v a ro` says
    // `removed 'ro'` then `'a' -> 'ro'`, and `cp -fv a ro` says the reverse.
    if job.flags.verbose {
        let _ = writeln!(job.out, "removed {}", quoteaf_os(target));
    }
    true
}

/// Copy one source. Returns `false` if it should count against the exit status.
fn copy_one<O: Write, E: Write>(
    src: &OsString,
    dest: &Path,
    dest_is_dir: bool,
    mut seen: Option<&mut Seen>,
    job: &mut Job<'_, O, E>,
) -> bool {
    let flags = job.flags;
    let src_path = Path::new(src);

    // Whether a symlink *operand* is followed is the whole of `-P`/`-H`/`-L`,
    // and with none of them given it depends on `-r`: plain `cp link dst`
    // copies what the link points at, while `cp -r link dst` copies the link
    // itself. [`CpFlags::follow_operand`] holds that rule and its citation.
    //
    // Not following is what keeps `cp -r` finite: a followed link to an
    // ancestor is an endless descent (module docs, bug 1). `cp -Lr` therefore
    // *is* endless on such a tree — measured, and GNU is too, which is why
    // there is no guard against it here.
    let metadata = if flags.follow_operand() {
        fs::metadata(src_path)
    } else {
        fs::symlink_metadata(src_path)
    };
    let metadata = match metadata {
        Ok(m) => m,
        Err(e) => {
            // `strerror`, not `{e}`: why it failed has to read the same wherever
            // it is printed. See [`coreutils::errmsg`] — on a Windows *host*
            // `{e}` says `The system cannot find the file specified. (os error
            // 2)`, which is neither POSIX's wording nor what this utility prints
            // on the target it ships on.
            let why = strerror(&e);
            let _ = writeln!(job.err, "cp: cannot stat {}: {why}", quoteaf_os(src));
            return false;
        }
    };

    // Before anything is worked out about the destination, as in GNU: the
    // refusal is a fact about the source alone, and asking it here is what
    // makes `cp tree/.. dst` say which of its two problems came first.
    if metadata.is_dir() && !flags.recursive {
        let _ = writeln!(
            job.err,
            "cp: -r not specified; omitting directory {}",
            quoteaf_os(src)
        );
        return false;
    }

    // A non-directory named twice is asked about here, before the destination
    // is worked out at all, and GNU asks it in the same place: `cp f f d` where
    // `d/f` is a directory prints the refusal for the first `f` and this
    // warning for the second, which only happens if the source is recorded
    // even when its copy failed. The repeat is *not* an error — the user asked
    // for a file that is already there, and it is.
    //
    // Identity, not spelling: `cp a ./a d` is the same request twice. But two
    // hard links to one inode are two entries, and copying both is a
    // legitimate thing to ask for, so [`same_entry`] separates them.
    //
    // `--backup` turns the warning off (`copy.c:2283`), because with it the
    // repeat is no longer pointless: `cp -b a a d` copies `a` to `d/a`, then
    // moves that `d/a` to `d/a~` and copies `a` again. Silly, but it is what
    // was asked for and every file survives it.
    //
    // The backup test comes *last* in the chain rather than first, and that is
    // deliberate: upstream skips the lookup but still runs `record_file`
    // (`copy.c:2291`), and [`Seen::saw_source`] is the lookup and the record in
    // one call. Testing earlier would short-circuit past the recording.
    if !metadata.is_dir()
        && let Some(seen) = seen.as_deref_mut()
        && let Some(id) = file_id(src_path, &metadata)
        && let Some(entry) = entry_id(src_path)
        && seen.saw_source(id, entry)
        && !flags.backup.enabled()
    {
        let _ = writeln!(
            job.err,
            "cp: warning: source file {} specified more than once",
            quoteaf_os(src)
        );
        return true;
    }

    let target = compute_target(src_path, dest, dest_is_dir);

    // GNU stats the destination here, and a failure that is *not* "it isn't
    // there" ends this operand rather than being rediscovered later while
    // opening it: `cp a b/c` where `b` is a file says `cannot stat 'b/c'`, not
    // `cannot create regular file 'b/c'`.
    let mut dest_state = match stat_destination(&metadata, &target, flags) {
        Ok(d) => d,
        Err(e) => {
            let why = strerror(&e);
            let _ = writeln!(job.err, "cp: cannot stat {}: {why}", quoteaf_os(&target));
            return false;
        }
    };

    if let Some(dest_meta) = dest_state.metadata() {
        // Module docs, bug 7. `stat` results rather than strings, which is the
        // only comparison that catches every spelling; GNU's `same_file_ok`
        // makes the same one, at the same point in the same order. Asked only
        // when the destination exists, again as GNU asks it.
        //
        // **`-n` skips this question entirely**, and that is upstream's guard
        // rather than an optimisation: `copy.c:2344` calls `same_file_ok` only
        // when `x->interactive` is neither `I_ALWAYS_NO` nor `I_ALWAYS_SKIP`.
        // So `cp -n a link-to-a` says `not replacing 'a'` and not `are the same
        // file` — measured. `-i` is *not* in that guard, so `cp -i a
        // link-to-a` does say `are the same file`, and never asks: there is
        // nothing to ask about a copy that would destroy its own source.
        //
        // `--remove-destination` excuses a destination that is a *symlink*,
        // however it resolves: the link is unlinked below rather than written
        // through, so replacing a link to the source with a copy of the source
        // does not truncate anything. GNU makes exactly this exception, in
        // `same_file_ok`'s `x->move_mode || x->unlink_dest_before_opening` arm
        // (`copy.c:1877`), and it is why `cp --remove-destination a self`
        // succeeds where plain `cp a self` says "are the same file".
        let link_replaced = flags.remove_destination && dest_meta.file_type().is_symlink();
        if flags.interactive != Interactive::AlwaysNo
            && !link_replaced
            && is_same_file(src_path, &target, !flags.follow_operand())
        {
            let _ = writeln!(
                job.err,
                "cp: {} and {} are the same file",
                quoteaf_os(src),
                quoteaf_os(&target)
            );
            return false;
        }

        // `-n`'s refusal and `-i`'s question, which are one block upstream
        // because they are two settings of one field. Before every refusal
        // below, which is GNU's order and is visible in two of them at once:
        // `cp -Tn a d` with `d` a directory says `not replacing 'd'` rather
        // than `cannot overwrite directory`, and `cp -n a other/a d` says it
        // rather than `will not overwrite just-created`. Both measured, and
        // both go the same way for `-i`, which asks first and then refuses.
        if !overwrite_allowed(&metadata, &target, &dest_state, job) {
            return false;
        }

        // Neither kind can be put where the other is. Without these two the
        // walk would go on and fail somewhere less informative — a directory
        // source would report `cannot create directory … File exists` about a
        // name that is not a directory at all.
        if metadata.is_dir() && !dest_meta.is_dir() {
            let _ = writeln!(
                job.err,
                "cp: cannot overwrite non-directory {} with directory {}",
                quoteaf_os(&target),
                quoteaf_os(src)
            );
            return false;
        }
        // After the refusal above and before the one below, which is where GNU
        // asks it — inside the "destination is not a directory" arm.
        //
        // This is the one that stops `cp a other/a d` silently throwing away
        // the copy of `a` it made a moment ago. Nothing about the two operands
        // is wrong on its own; what is wrong is the pair, and only a record of
        // what this command already wrote can see it.
        //
        // `--backup=numbered` — and *only* numbered — lifts it (`copy.c:2474`),
        // because numbered backups are the one shape under which the second
        // operand cannot destroy the first: `d/a` becomes `d/a.~1~` and stays.
        // Simple and existing backups both reuse one name, so `cp -b a other/a
        // d` would write `d/a~` twice and lose the first copy after all; GNU
        // keeps the refusal for them and upstream's own comment says so —
        // "Note that it works fine if you use --backup=numbered."
        if !dest_meta.is_dir()
            && flags.backup.kind() != BackupType::Numbered
            && let Some(seen) = seen.as_deref_mut()
            && let Some(id) = file_id(&target, dest_meta)
            && seen.made(&target, id)
        {
            let _ = writeln!(
                job.err,
                "cp: will not overwrite just-created {} with {}",
                quoteaf_os(&target),
                quoteaf_os(src)
            );
            return false;
        }

        if !metadata.is_dir() && dest_meta.is_dir() {
            let _ = writeln!(
                job.err,
                "cp: cannot overwrite directory {} with non-directory",
                quoteaf_os(&target)
            );
            return false;
        }
    }

    // `--remove-destination`, at GNU's point in the order: after the
    // just-created *file* guard above (`copy.c:2470`) and before the
    // just-created *symlink* one below (`copy.c:2591`). Which is observable —
    // the symlink guard re-`lstat`s, so a link this command created and then
    // unlinked here is not there to be complained about.
    if !remove_destination_first(src_path, &target, &mut dest_state, job) {
        return false;
    }

    // The same guard again, for the case the one above cannot see. A regular
    // source stats its destination *followed*, so when that destination is a
    // symlink this command just created, the identity compared above is the
    // link's target rather than the link — and writing through it would
    // clobber whatever the link points at. GNU asks this separately for the
    // same reason, with its own `lstat`.
    //
    // Any `--backup` lifts it (`copy.c:2594`), and here the reason is that the
    // premise has gone: with backups on the destination was `lstat`ed above,
    // so a symlink destination has already been renamed out of the way and
    // there is nothing left to copy *through*. Unlike the guard above, this one
    // is off for every backup shape, not only numbered.
    if !flags.backup.enabled()
        && let Some(seen) = seen.as_deref_mut()
        && let Ok(link_meta) = fs::symlink_metadata(&target)
        && link_meta.file_type().is_symlink()
        && let Some(id) = file_id(&target, &link_meta)
        && seen.made(&target, id)
    {
        let _ = writeln!(
            job.err,
            "cp: will not copy {} through just-created symlink {}",
            quoteaf_os(src),
            quoteaf_os(&target)
        );
        return false;
    }

    // A directory named twice is asked about here, not up with the file case:
    // GNU reaches it only after the two refusals above, so `cp -r t t d` with
    // `d/t` a plain file reports "cannot overwrite non-directory" for *both*
    // operands rather than warning about the second.
    //
    // And the question asked is a different one. Two operands naming one
    // directory are a repeat only if they were also going to the *same place*;
    // where they are not, the user has asked for one inode to appear twice in
    // the destination tree, which for a directory can only be done by hard
    // linking it — and hard-linked directories are what GNU refuses here, with
    // its comment naming Netapp snapshot trees as where they turn up.
    //
    // `remember`, not `lookup`: an operand *is* recorded, because a later
    // operand — or a walk that reaches this same directory from elsewhere —
    // has to be able to find it. [`copy_entry`] takes the other half.
    // Unconditional, where the rest of [`Seen`] is built only for two or more
    // operands, because the walk reads this table and a single operand's walk
    // can reach a directory it was itself given (`cp -r parent d` cannot, but
    // `cp -r parent/child parent d` is two operands only by accident of how
    // the repeat is reached). GNU's `hash_init` is likewise unconditional.
    //
    // `-r` is not tested for: without it a directory operand is refused above,
    // at "omitting directory", and never reaches this line.
    //
    // Nothing follows the two refusals: a miss has already been recorded by
    // `remember` itself, which is why it is one call and not a lookup and an
    // insert.
    if metadata.is_dir()
        && let Some(id) = file_id(src_path, &metadata)
        && let Some(earlier) = job.copied.remember(&id, &target)
    {
        if same_entry(&earlier, &target) {
            let _ = writeln!(
                job.err,
                "cp: warning: source directory {} specified more than once",
                quoteaf_os(src)
            );
            return true;
        }
        // Two *destinations* for one source directory is not an error when
        // following symlinks was asked for: `cp -RL a b d`, where `a` and `b`
        // are links to one directory, is a request for two independent copies
        // of it and GNU makes them silently (`copy.c:2723`). `-H` says the same
        // for operands, which is all [`copy_one`] handles.
        if !flags.follow_operand() {
            let _ = writeln!(
                job.err,
                "cp: will not create hard link {} to directory {}",
                quoteaf_os(&target),
                quoteaf_os(&earlier)
            );
            return false;
        }
    }

    let placed = place_source(src, src_path, &metadata, &target, &dest_state, job);

    // One recording site, reached however the copy was done, and only on
    // success — a destination that was never written is not one a later
    // operand can be accused of overwriting. GNU records in the same single
    // place and under the same condition.
    //
    // [`Placed::Linked`] is the exception, and it is GNU's: the hard-link path
    // returns before `record_file` ever runs, so the `will not overwrite
    // just-created` refusal does not protect a destination that was linked
    // rather than copied. See [`Placed`] for the measurement.
    if placed == Placed::Copied
        && let Some(seen) = seen
    {
        seen.record_dest(&target);
    }
    placed.is_ok()
}

/// Make the copy, now that the destination path is settled and every refusal
/// has been made. Split out of [`copy_one`] so that it has one place to record
/// what it created rather than one per kind of source.
///
/// The only thing this adds to [`place_entity`] is the one refusal that can
/// arise for an operand and cannot arise inside a walk.
fn place_source<O: Write, E: Write>(
    src: &OsString,
    src_path: &Path,
    metadata: &fs::Metadata,
    target: &Path,
    dest: &Dest,
    job: &mut Job<'_, O, E>,
) -> Placed {
    // Module docs, bug 2: without this, `cp -r a a` and `cp -r a .` copy what
    // they have just written, for ever. Not in [`place_entity`], because a
    // directory reached by walking is by construction not an ancestor of the
    // destination — the walk that reached it would not have terminated.
    if metadata.is_dir() && is_inside(target, src_path) {
        let _ = writeln!(
            job.err,
            "cp: cannot copy a directory, {}, into itself, {}",
            quoteaf_os(src),
            quoteaf_os(target)
        );
        return Placed::Failed;
    }

    // `true`: this is the operand path, which is the whole of what
    // `command_line_arg` means to `-H` and to `--preserve=links`.
    place_entity(src_path, metadata, target, dest, true, job)
}

/// The part of a destination's permission bits that is deliberately not on it
/// yet, and what has to be put back once it is safe.
///
/// GNU's three locals `omitted_permissions`, `restore_dst_mode` and `dst_mode`
/// (`copy.c:2211`), carried together because they are one fact in three pieces:
/// a destination is created **narrower** than its source on purpose, and
/// something has to remember by how much.
///
/// Two different reasons put bits in here, and they are the two branches of
/// GNU's expression at `copy.c:2900`:
///
/// * **The ownership is about to change.** Under `--preserve=ownership` the
///   destination is created with no group or other permissions at all
///   (`S_IRWXG | S_IRWXO`, i.e. `0o077`), because between the creation and the
///   `chown` it belongs to the *copying* user. A source that is group-readable
///   by its own owner's group would, for that window, be readable by the
///   copier's group instead — a different set of people.
/// * **It is a directory whose contents are not written yet.** Group- and
///   other-*write* (`0o022`) are withheld so that nobody can slip a file into a
///   directory that is about to look like a faithful copy.
///
/// The first subsumes the second, which is why GNU's expression is an
/// `if`/`else` rather than a union — and why a *regular file* has a debt at all
/// under `-p`, where before `-p` existed only directories did.
#[derive(Clone, Copy, Default)]
struct ModeDebt {
    /// GNU's `omitted_permissions`: the bits withheld at creation.
    omitted: u32,
    /// GNU's `restore_dst_mode` and `dst_mode` in one value. `Some(mode)` means
    /// the destination's mode has already been read and must be written back —
    /// either because a directory was forced owner-rwx so it could be filled,
    /// or because the settle-up stat showed the withheld bits genuinely absent.
    forced: Option<u32>,
}

impl ModeDebt {
    /// GNU's `omitted_permissions = dst_mode_bits & (…)` (`copy.c:2899`).
    fn new(flags: &CpFlags, src_mode: u32, is_dir: bool) -> Self {
        let withhold = if flags.preserve.ownership {
            0o077
        } else if is_dir {
            0o022
        } else {
            0
        };
        ModeDebt {
            omitted: src_mode & withhold,
            forced: None,
        }
    }
}

/// What kind of destination [`preserve_attributes`] is stamping.
///
/// Three kinds and not a `bool`, because all three answer the two questions
/// differently: a symlink takes neither an ownership step (it was chowned where
/// it was made) nor a mode at all, and a directory settles its withheld
/// permissions by a different formula from a regular file's. See
/// [`settle_mode`].
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
enum Made {
    Regular,
    Directory,
    Symlink,
}

/// What [`copy_tree`] achieved.
///
/// The distinction the caller needs is not "did everything work" but "is there
/// a directory there to stamp": GNU leaves the reason in a comment at its
/// `copy_dir` call (`copy.c:3017`) — "Don't just return if this fails --
/// otherwise, the failure to read a single file in a source directory would
/// cause the containing destination directory not to have owner/perms set
/// properly." A `bool` return could not tell the two apart, and folding them
/// together is how a `cp -rp` whose tree contains one unreadable file would
/// leave the whole destination directory at the forced owner-rwx.
enum TreeResult {
    /// The directory is there. `new` is whether this run created it — GNU's
    /// `new_dst`, which decides whether the ownership is worth setting and
    /// whether `--no-preserve=mode` applies. `ok` is whether everything inside
    /// it arrived.
    Made { new: bool, ok: bool },
    /// The directory itself could not be created or made usable, so there is
    /// nothing to stamp and the failure has already been reported. GNU's
    /// `goto un_backup`.
    Unmade,
}

/// What [`place_entity`] did with one source.
///
/// `Linked` is not a variety of `Copied` that the caller may round off, and the
/// reason is a measured GNU behaviour rather than a tidiness argument: a
/// destination reached by hard-linking is **not** recorded in GNU's `dest_info`,
/// because its `earlier_file` branch returns at `copy.c:2748`, well before the
/// `record_file` at `copy.c:3217`. The consequence is visible —
///
/// ```text
/// $ cp --preserve=links a b o/b d      # a and b hard-linked, o/b unrelated
/// $ echo $?
/// 0                                    # d/b is now o/b; the link is gone
/// $ cp a b o/b d                       # the same command without the option
/// cp: will not overwrite just-created 'd/b' with 'o/b'
/// ```
///
/// — and it is faithfully reproduced by [`copy_one`] declining to
/// [`Seen::record_dest`] a `Linked` destination. A `bool` return could not
/// express that, which is the whole reason this enum exists.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
enum Placed {
    /// Nothing usable is at the destination, and the failure has been reported.
    Failed,
    /// The destination was written.
    Copied,
    /// The destination was made a hard link to an earlier one, under
    /// `--preserve=links`. Nothing was copied and nothing was recorded.
    Linked,
}

impl Placed {
    /// Whether the operand succeeded, for the exit status. Both kinds of
    /// success count; only [`Seen`] cares which.
    fn is_ok(self) -> bool {
        self != Placed::Failed
    }
}

/// How many names the file has. GNU's `st_nlink`.
#[cfg(unix)]
fn hard_links(meta: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.nlink()
}

/// A host without hard links answers 1 to everything, which switches
/// `--preserve=links` off by exactly the amount that host cannot honour it: the
/// `st_nlink > 1` half of the condition never fires, and the dereference half
/// still does, so `cp --preserve=links -L la lb d` is the only spelling that
/// reaches [`create_hard_link`] — and there it fails with whatever the platform
/// says about [`fs::hard_link`], which is the honest answer.
#[cfg(not(unix))]
fn hard_links(_meta: &fs::Metadata) -> u64 {
    1
}

/// Link `earlier` to `target`, replacing whatever is at `target`. GNU's
/// `create_hard_link` (`copy.c:2122`) over gnulib's `force_linkat`.
///
/// "Replacing" is why this is not one `fs::hard_link` call. `link(2)` fails with
/// `EEXIST` and has no force flag, so gnulib links to a fresh name in the
/// destination's own directory and `rename`s that over the destination — which
/// is atomic, and is what makes `cp --preserve=links a b d` work when `d/b` was
/// already something else. The temporary must be in the *same* directory or the
/// rename would cross a filesystem and fail.
///
/// The unlink of the temporary is unconditional in gnulib, and its comment says
/// why: if `dsttmp` and `target` were already the same link, `renameat` is a
/// no-op that leaves both names, so the cleanup cannot be skipped on success.
///
/// `-v` prints `removed 'target'` *here*, after the arrow line rather than
/// before it, because this replacement happens after `emit_verbose` rather than
/// in the pre-copy unlink. Measured, with `d/b` a dangling symlink:
///
/// ```text
/// 'a' -> 'd/a'
/// 'b' -> 'd/b'
/// removed 'd/b'
/// ```
///
/// # Following
///
/// GNU passes `AT_SYMLINK_FOLLOW` when `should_dereference`; [`fs::hard_link`]
/// is `linkat` with no flags and cannot. The difference is unreachable rather
/// than unimplemented: the thing being linked *from* is a destination this same
/// command created, and a command that dereferences creates no symlinks — under
/// `-L`, and under `-H` for an operand, every source is stat'd through, so every
/// destination is a regular file. The reachable case is the opposite one and
/// needs the flag *off*: `cp -P --preserve=links l1 l2 d`, where `l1` and `l2`
/// are two hard links to one symlink, must give `d/l1` and `d/l2` one inode that
/// is still a symlink — measured, and what this produces.
fn create_hard_link<O: Write, E: Write>(
    earlier: &Path,
    target: &Path,
    job: &mut Job<'_, O, E>,
) -> bool {
    let existed = match fs::hard_link(earlier, target) {
        Ok(()) => false,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => match link_over(earlier, target) {
            Ok(()) => true,
            Err(e) => return report_link_failure(&e, earlier, target, job),
        },
        Err(e) => return report_link_failure(&e, earlier, target, job),
    };
    if existed && job.flags.verbose {
        let _ = writeln!(job.out, "removed {}", quoteaf_os(target));
    }
    true
}

/// gnulib's `cannot create hard link %s to %s`, destination first.
fn report_link_failure<O: Write, E: Write>(
    e: &io::Error,
    earlier: &Path,
    target: &Path,
    job: &mut Job<'_, O, E>,
) -> bool {
    let why = strerror(e);
    let _ = writeln!(
        job.err,
        "cp: cannot create hard link {} to {}: {why}",
        quoteaf_os(target),
        quoteaf_os(earlier)
    );
    false
}

/// The replace half of `force_linkat`: link into a temporary name beside
/// `target`, rename it over, and remove the temporary either way.
///
/// The name is gnulib's `CuXXXXXX` pattern with the random part supplied by the
/// only two things available without a dependency — the process id and a
/// counter — and retried, because a collision must not be reported as the
/// caller's failure. `O_EXCL` semantics come free: `link(2)` fails with
/// `EEXIST` rather than clobbering, so a name that loses the race is simply
/// tried again.
fn link_over(earlier: &Path, target: &Path) -> io::Result<()> {
    let (dir, base) = split_entry(target);
    for attempt in 0..PLACE_TEMP_TRIES {
        let mut name = OsString::from("Cu");
        name.push(format!("{:x}{attempt:x}", std::process::id()));
        // Beside the destination and not in `/tmp`: `rename` cannot cross a
        // filesystem, and the destination's directory is the only place
        // guaranteed to be on the same one.
        let tmp = dir.join(&name);
        match fs::hard_link(earlier, &tmp) {
            Ok(()) => {
                let result = fs::rename(&tmp, target);
                // Even when the rename worked: if `tmp` and `target` were
                // already one link, the rename was a no-op and left both names
                // (gnulib's own comment at `force-link.c:117`).
                let _ = fs::remove_file(&tmp);
                return result;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    // Every candidate name was taken, which needs `PLACE_TEMP_TRIES`
    // simultaneous `cp`s in one directory. Reported as the errno the last
    // attempt earned rather than as a panic.
    let _ = base;
    Err(io::Error::from(io::ErrorKind::AlreadyExists))
}

/// How many temporary names [`link_over`] tries before giving up. gnulib's
/// `try_tempname_len` uses six random characters and the whole space; this
/// walks a counter instead, and the bound is what stops an unlucky directory
/// from spinning.
const PLACE_TEMP_TRIES: u32 = 64;

/// Copy one source of a known kind onto a settled destination path: the symlink,
/// the directory and the regular file, and nothing else.
///
/// The single place all three kinds are dispatched, reached identically by an
/// operand (through [`place_source`]) and by an entry found inside a tree
/// (through [`copy_entry`]). GNU funnels both through one `copy_internal`, and
/// the reason to match that is not tidiness: everything that happens *after* the
/// bytes are written — `-p` and each `--preserve=` attribute — happens for all
/// three kinds, so a second copy of this dispatch is a second place to forget
/// one of them. The two callers had already drifted once, when the symlink arm's
/// unlink of an existing destination reached an operand and not a walked entry,
/// and `cp -r` over a tree it had copied before answered `cannot create symbolic
/// link …: File exists`.
fn place_entity<O: Write, E: Write>(
    src_path: &Path,
    metadata: &fs::Metadata,
    target: &Path,
    dest: &Dest,
    command_line_arg: bool,
    job: &mut Job<'_, O, E>,
) -> Placed {
    let src_mode = permission_bits(metadata);
    // Computed here, before the kind is dispatched, because GNU computes it
    // here — one expression covering all three kinds (`copy.c:2899`), read by
    // whichever of them creates the destination and settled by the tail they
    // share. See [`ModeDebt`].
    let debt = ModeDebt::new(job.flags, src_mode, metadata.is_dir());
    let mut dest_exists = dest.exists();

    // Clearing the way, before anything is said or written. GNU's one unlink
    // (`copy.c:2570`) covers every reason a destination has to *go* rather than
    // be written through, and reaching it before `emit_verbose` (`copy.c:2630`)
    // is what makes a `cp -v` that cannot clear the way announce nothing.
    //
    // Two of GNU's reasons apply to this `cp`, and they are `||`-ed there too:
    //
    // * **The source is a symlink that is not being followed.** `symlinkat` has
    //   no "replace", and refusing instead would leave `cp -r` unable to update
    //   a tree it had already copied once. GNU writes this as `dereference ==
    //   DEREF_NEVER && ! S_ISREG (src_mode)`, which for this `cp` is exactly a
    //   symlink source that survived the stat as one — reachable for an operand
    //   under `-P`, or under `-r` with none of `-P`/`-H`/`-L`, never under `-H`.
    // * **`--preserve=links`, and the destination has more than one link.**
    //   Writing *through* it would change every other name for that inode, and
    //   `--preserve=links` is the one option whose user is demonstrably paying
    //   attention to link counts. Measured: `cp -v --preserve=links a b o/b d`
    //   with `a` and `b` hard-linked prints `removed 'd/b'` before the third
    //   operand's arrow line, and `d/a` keeps the bytes of `a`.
    //
    // Both reasons sit inside GNU's `else if (! S_ISDIR (dst_sb.st_mode) && …)`
    // (`copy.c:2539`), and so do these. A directory is never unlinked to clear
    // the way: `unlink` cannot remove one in the first place, so trying would
    // turn `cp -T --preserve=links a existing_dir` into `cannot remove
    // 'existing_dir': Is a directory` — an errno-shaped complaint about the
    // wrong thing entirely, where GNU reaches its own `cannot overwrite
    // directory %s with non-directory` and leaves the directory standing. Note
    // that on ext4 *every* directory trips the link-count half of the test:
    // `.` and the entry in its parent are two links before anything else
    // points at it.
    let dest_is_dir = dest.metadata().is_some_and(fs::Metadata::is_dir);
    let dest_multiply_linked =
        job.flags.preserve.links && dest.metadata().is_some_and(|m| hard_links(m) > 1);

    // `-b`: the destination is moved aside rather than written over, and this is
    // GNU's block at `copy.c:2517`. It is the `if` whose `else if` is the unlink
    // below, in upstream too — the two are alternatives, and reading them as
    // independent would unlink the very destination that had just been renamed
    // out of harm's way, which is the backup made and then thrown away. The
    // condition itself is [`backup_takes_destination`], which documents its three
    // clauses and is asked for its `else` by [`remove_destination_first`].
    let mut moved_aside: Option<PathBuf> = None;
    if backup_takes_destination(src_path, dest, job.flags) {
        // The one refusal, and it is the reason `cp` needs the *suffix* even
        // when the type is numbered: `cd /tmp; rm -f a a~; : > a; echo A > a~;
        // cp --backup=simple a~ a` would name the backup of `a` exactly `a~`,
        // rename the source on top of itself, and leave two empty files where
        // there had been one empty and one full. Upstream's own comment carries
        // that recipe verbatim. Numbered backups are exempt because the name
        // they choose is never one the user typed.
        if job.flags.backup.kind() != BackupType::Numbered
            && source_is_dst_backup(src_path, metadata, target, job.flags.backup.simple_suffix())
        {
            let _ = writeln!(
                job.err,
                "cp: backing up {} might destroy source;  {} not copied",
                quoteaf_os(target),
                quoteaf_os(src_path)
            );
            return Placed::Failed;
        }
        match job.flags.backup.rename(target) {
            Ok(name) => moved_aside = Some(name),
            // "Nothing was there" is not a failure: upstream's `else if (errno
            // != ENOENT)`. It can happen even though the `stat` above found
            // something, because the two are separate syscalls — and it is the
            // ordinary answer for a *dangling* symlink destination under a
            // simple rename that has already moved it.
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => {
                let why = strerror(&e);
                let _ = writeln!(job.err, "cp: cannot backup {}: {why}", quoteaf_os(target));
                return Placed::Failed;
            }
        }
        // Upstream's `new_dst = true`, set on both of the paths above: whether
        // the destination was renamed away or was never there, the name is free
        // now and the copy must create rather than open.
        dest_exists = false;
    } else if dest_exists
        && !dest_is_dir
        && (metadata.file_type().is_symlink() || dest_multiply_linked)
    {
        if !remove_before_writing(target, job) {
            return Placed::Failed;
        }
        dest_exists = false;
    }

    // *After* the removal above and before anything is created, so that a
    // failure to make the copy is still announced — `-v` reports what was
    // attempted, not what worked. Directories are the exception and announce
    // themselves, from inside [`copy_tree`], because GNU will not say it made
    // one until the `mkdir` has actually happened (`copy.c:2625`).
    //
    // The backup name goes into the line, which is why the block above is
    // *before* this one rather than after: `cp -vb a b` prints
    // `'a' -> 'b' (backup: 'b~')`, one line naming both the copy and the move
    // that made room for it.
    if !metadata.is_dir() {
        announce(job, src_path, target, moved_aside.as_deref());
    }

    // `--preserve=links`: the second name for an inode is a hard link to where
    // the first one landed, not a second copy of its bytes. GNU's `earlier_file`
    // block (`copy.c:2683`), whose condition this is:
    //
    // * **`1 < st_nlink`** — the source is *already* multiply linked, so a
    //   second operand naming it is possible. This is the ordinary case.
    // * **or the source is being dereferenced** — under `-L`, or `-H` for an
    //   operand, two *different* symlinks resolve to one inode whose link count
    //   is 1, and `cp --preserve=links -L la lb d` is measurably expected to
    //   link `d/la` and `d/lb` even so.
    //
    // Directories are excluded: their branch of `earlier_file` is the
    // hard-linked-directory refusal, which lives in [`copy_one`] because only
    // an operand can reach it.
    let mut recorded = None;
    if !metadata.is_dir()
        && job.flags.preserve.links
        && (hard_links(metadata) > 1 || job.flags.should_dereference(command_line_arg))
        && let Some(id) = file_id(src_path, metadata)
    {
        if let Some(earlier) = job.copied.remember(&id, target) {
            return if create_hard_link(&earlier, target, job) {
                Placed::Linked
            } else {
                // GNU reaches its `un_backup` label from here too (`copy.c:2705`
                // is one of eleven `goto`s to it), and does *not* run the
                // `forget_created` half — `earlier_file` is non-null on this
                // path, which is exactly the `recorded == None` this branch
                // leaves behind. See the tail below.
                backup::un_backup(
                    "cp",
                    moved_aside.as_deref(),
                    target,
                    job.flags.verbose,
                    &mut *job.out,
                    &mut *job.err,
                );
                Placed::Failed
            };
        }
        recorded = Some(id);
    }

    let ok = place_bytes(src_path, metadata, src_mode, target, dest_exists, debt, job);

    // GNU's `un_backup` label: a source recorded a moment ago whose copy then
    // failed must be un-recorded, or a later operand naming the same inode
    // would try to hard-link to a destination that does not exist and would
    // report `cannot create hard link` in place of the failure that actually
    // happened. The guard there is `earlier_file == nullptr`, which is this
    // `recorded.is_some()` — the linking path above never reaches here.
    if !ok {
        if let Some(id) = &recorded {
            job.copied.forget(id);
        }
        // And the half the label is named for. In upstream's order: forget
        // first, then put the backup back.
        backup::un_backup(
            "cp",
            moved_aside.as_deref(),
            target,
            job.flags.verbose,
            &mut *job.out,
            &mut *job.err,
        );
    }

    if ok { Placed::Copied } else { Placed::Failed }
}

/// Whether [`place_entity`]'s backup block will move this destination aside —
/// the `if` at `copy.c:2517`, written once because two places need it.
///
/// [`place_entity`] asks it to decide whether to make a backup;
/// [`remove_destination_first`] asks it to decide whether *not* to unlink,
/// because upstream that unlink is this block's `else if` rather than a step of
/// its own. Keeping the condition in one function is what stops the two
/// drifting into a state where both fire, which is a destination deleted and
/// then "backed up" from nothing.
///
/// The three conditions are upstream's:
///
/// * **The destination is there.** Nothing to move aside otherwise, and the
///   whole of `copy.c`'s surrounding block is inside `rename_errno == EEXIST`,
///   which is set only when the destination's `stat` succeeded.
/// * **The destination is not a directory.** Upstream writes this as
///   `x->move_mode || ! S_ISDIR (…)`, with a `FIXME` saying `mv` backs up a
///   destination directory and `cp` deliberately does not — so that `cp -rb`
///   can merge into an existing hierarchy instead of renaming it away.
/// * **The source's last component is not `.` or `..`.** `cp -rb a/. d` copies
///   `a`'s *contents* into an existing `d`, so backing up `d` would move the
///   directory the copy is about to fill.
///
/// [`Dest::Opaque`] answers `false` to the second, which reads like a
/// difference from upstream and cannot be reached: a destination is only
/// `Opaque` when its `stat` failed with `ELOOP`, and with backups on
/// [`stat_destination`] uses `lstat`, which a symlink loop does not trouble.
fn backup_takes_destination(src: &Path, dest: &Dest, flags: &CpFlags) -> bool {
    flags.backup.enabled()
        && dest.exists()
        && !dest.metadata().is_some_and(fs::Metadata::is_dir)
        && !src_base_is_dot_or_dotdot(src)
}

/// The three kinds, dispatched. Split from [`place_entity`] only so that the
/// preamble it shares — the unlink, the announcement and the link bookkeeping —
/// has one exit to attach the `un_backup` step to rather than one per arm.
fn place_bytes<O: Write, E: Write>(
    src_path: &Path,
    metadata: &fs::Metadata,
    src_mode: u32,
    target: &Path,
    dest_exists: bool,
    mut debt: ModeDebt,
    job: &mut Job<'_, O, E>,
) -> bool {
    if metadata.file_type().is_symlink() {
        if let Err(e) = clone_symlink(src_path, target) {
            let why = strerror(&e);
            let _ = writeln!(
                job.err,
                "cp: cannot create symbolic link {}: {why}",
                quoteaf_os(target)
            );
            return false;
        }
        // GNU chowns a just-created link *here*, inside the symlink arm
        // (`copy.c:3175`), and not in the shared tail — whose ownership step
        // skips a symlink destination outright (`!dest_is_symlink &&
        // x->preserve_ownership`). Two calls that look like one: dropping this
        // in favour of the tail's would leave `cp -P -p` unable to preserve a
        // link's owner at all, because the tail would never reach it.
        //
        // Unconditional where the tail's is guarded by "the owner differs":
        // the link was made a line ago, so it is new by construction, and GNU's
        // guard is `new_dst || …` in the first place.
        let src = Source::new(On::Path(src_path, Link::NoFollow), src_path, metadata);
        if job.flags.preserve.ownership
            && chown_to_source(
                src,
                On::Path(target, Link::NoFollow),
                target,
                Made::Symlink,
                true,
                job,
            ) == Chowned::Failed
        {
            return false;
        }
        return preserve_attributes(
            src,
            On::Path(target, Link::NoFollow),
            target,
            Made::Symlink,
            true,
            &mut debt,
            job,
        );
    }

    if metadata.is_dir() {
        let (new, contents_ok) = match copy_tree(src_path, src_mode, target, &mut debt, job) {
            TreeResult::Unmade => return false,
            TreeResult::Made { new, ok } => (new, ok),
        };
        // Run whether or not the walk succeeded — see [`TreeResult`] — and
        // *after* it, which is the other half of GNU's order: writing entries
        // into a directory moves its modification time, so a `-p` that stamped
        // the times before filling it would stamp them with a value the next
        // `mkdir` inside overwrites.
        let stamped = preserve_attributes(
            Source::new(On::Path(src_path, Link::Follow), src_path, metadata),
            On::Path(target, Link::Follow),
            target,
            Made::Directory,
            new,
            &mut debt,
            job,
        );
        return contents_ok && stamped;
    }

    copy_regular_file(src_path, metadata, target, dest_exists, debt, job)
}

/// Unlink a destination that has to go before the copy can be made, and say so
/// under `-v`. GNU's `unlinkat` at `copy.c:2580` with the `removed %s` that
/// follows it.
///
/// The announcement is reached on "it was already gone" as well as on success,
/// which is GNU's control flow rather than an oversight — its condition is
/// `unlinkat (…) != 0 && errno != ENOENT`, so a destination that vanished
/// between the stat and the unlink is still announced as removed. Only a race
/// can produce that, and agreeing about it costs nothing.
fn remove_before_writing<O: Write, E: Write>(target: &Path, job: &mut Job<'_, O, E>) -> bool {
    if let Err(e) = fs::remove_file(target)
        && e.kind() != io::ErrorKind::NotFound
    {
        let why = strerror(&e);
        let _ = writeln!(job.err, "cp: cannot remove {}: {why}", quoteaf_os(target));
        return false;
    }
    // On stdout, in its own sentence and before the arrow line
    // (`copy.c:2586`).
    if job.flags.verbose {
        let _ = writeln!(job.out, "removed {}", quoteaf_os(target));
    }
    true
}

/// Where one source lands: GNU's `do_copy` (`cp.c:734`), whose four lines are
/// the entire rule.
///
/// ```c
/// ASSIGN_STRDUPA (arg_base, last_component (arg));
/// strip_trailing_slashes (arg_base);
/// /* For 'cp -R source/.. dest', don't copy into 'dest/..'. */
/// arg_base += STREQ (arg_base, "..");
/// dst_name = file_name_concat (target_directory, arg_base, &arg_in_concat);
/// ```
///
/// Three things in it are not guessable, and module docs bug 5 is what happened
/// when they were guessed at:
///
/// * **The component is bytes, not a normalised path component.**
///   [`split_entry`] is `last_component` followed by `strip_trailing_slashes`
///   already, and it keeps what it finds: `a/.` ends in the component `.`,
///   where `Path::file_name` reports `a`.
/// * **`.` is a perfectly good component to append.** `cp -r a/. dst` targets
///   `dst/.`, which *is* `dst` — which is exactly why that idiom copies `a`'s
///   contents into `dst` instead of creating `dst/a`.
/// * **A last component of `..`, and only `..`, becomes `.`.** The `+= STREQ`
///   is a pointer bump past the first dot of `".."`, and the comment above it
///   says what it is for: without it `cp -r a/.. dst` would write into the
///   destination's *parent*, which is nobody's request. Note it is the
///   *component* that is compared, so `a/..x` and `a/...` are untouched.
///
/// A source whose last component is empty — `/`, or `//` — appends nothing, and
/// `dest.join("")` yields `dest/` exactly as `file_name_concat` does.
///
/// Infallible, unlike the version this replaced: every source names somewhere,
/// because `.` and the empty string both name the destination itself.
fn compute_target(src: &Path, dest: &Path, dest_is_dir: bool) -> PathBuf {
    if !dest_is_dir {
        return dest.to_path_buf();
    }
    let (_, base) = split_entry(src);
    if base == ".." {
        return dest.join(".");
    }
    dest.join(base)
}

/// Would writing at `target` write inside `root`?
///
/// Both are resolved as far as they exist — `target` usually does not exist yet,
/// so its nearest existing ancestor is canonicalised and the rest appended. That
/// makes `cp -r a .` (target `./a`) and `cp -r a a` (target `a/a`) both
/// recognisable as the same directory reached by a different spelling, which a
/// textual comparison would miss.
fn is_inside(target: &Path, root: &Path) -> bool {
    match (
        resolve_as_far_as_exists(root),
        resolve_as_far_as_exists(target),
    ) {
        (Some(root), Some(target)) => target.starts_with(&root),
        // If neither can be resolved at all there is nothing useful to say, and
        // refusing a copy on the strength of a failed lookup would be worse than
        // the loop this guards against is likely. The `read_dir` walk still
        // terminates on any real tree; only a self-copy loops.
        _ => false,
    }
}

/// `canonicalize`, but tolerating a path that does not exist yet: the longest
/// existing prefix is canonicalised and the remaining components appended.
fn resolve_as_far_as_exists(path: &Path) -> Option<PathBuf> {
    if let Ok(real) = fs::canonicalize(path) {
        return Some(real);
    }
    let mut tail: Vec<&std::ffi::OsStr> = Vec::new();
    let mut here = path;
    loop {
        let name = here.file_name()?;
        tail.push(name);
        here = here.parent()?;
        // An empty parent means a bare relative name: resolve against the
        // current directory, which is what the kernel would do with it.
        let base = if here.as_os_str().is_empty() {
            fs::canonicalize(Path::new(".")).ok()?
        } else if let Ok(real) = fs::canonicalize(here) {
            real
        } else {
            continue;
        };
        let mut out = base;
        for name in tail.iter().rev() {
            out.push(name);
        }
        return Some(out);
    }
}

/// Copy the tree at `src`, whose permission bits are `src_mode`, to `dest`,
/// reporting every failure to `err`.
///
/// A failure on one entry does not abandon the others — module docs, bug 6 —
/// and does not stop the caller stamping the directory either; see
/// [`TreeResult`].
///
/// The mode is taken as an argument rather than re-stat'd because the caller
/// has already stat'd `src` and a second look could see a different directory.
///
/// `debt` arrives with the bits to withhold at `mkdir` already in it and leaves
/// with what the caller has to put back. The settle-up itself is deliberately
/// *not* here: with `-p` it is a different chmod, and it has to happen after
/// the `chown` that the caller — not this function — performs. Doing it here
/// would put a `chmod` before a `chown` and drop the set-user-ID bit off every
/// directory copied by a non-root user. See [`preserve_attributes`].
fn copy_tree<O: Write, E: Write>(
    src: &Path,
    src_mode: u32,
    dest: &Path,
    debt: &mut ModeDebt,
    job: &mut Job<'_, O, E>,
) -> TreeResult {
    let mut ok = true;

    let new = match make_dir(dest, src_mode & !debt.omitted) {
        Ok(true) => match permission_bits_of(dest) {
            Ok(made) => {
                // A directory is announced *here* and nowhere else, and GNU
                // says why in a comment of its own (`copy.c`, above the
                // `emit_verbose` at 2991): "we don't always create the
                // destination directory, so --verbose should not announce
                // anything until we're sure we'll create a directory." So
                // `cp -rv a b` where `b/a` already exists announces the files
                // it refreshes and says nothing about the directory holding
                // them — the directory was not copied, it was reused.
                announce(job, src, dest, None);
                // The adjustment in the opposite direction from the debt: a
                // source that is not owner-rwx — 0500 is perfectly ordinary —
                // would leave this process unable to fill the directory it has
                // just made. So owner-rwx goes on now, and what the directory
                // really got is remembered as the mode to go back to.
                if made & 0o700 != 0o700 {
                    debt.forced = Some(made);
                    if let Err(e) = set_mode(dest, made | 0o700) {
                        let why = strerror(&e);
                        let _ = writeln!(
                            job.err,
                            "cp: setting permissions for {}: {why}",
                            quoteaf_os(dest)
                        );
                        return TreeResult::Unmade;
                    }
                }
                true
            }
            Err(e) => {
                let why = strerror(&e);
                let _ = writeln!(job.err, "cp: cannot stat {}: {why}", quoteaf_os(dest));
                return TreeResult::Unmade;
            }
        },
        Ok(false) => {
            // The destination directory was already there. GNU leaves its mode
            // alone — exactly as it leaves an existing *file*'s mode alone — so
            // there is nothing to withhold and nothing to put back
            // (`copy.c:2996`).
            debt.omitted = 0;
            false
        }
        Err(e) => {
            let why = strerror(&e);
            let _ = writeln!(
                job.err,
                "cp: cannot create directory {}: {why}",
                quoteaf_os(dest)
            );
            return TreeResult::Unmade;
        }
    };

    // An unreadable source directory is *not* a reason to leave the copy
    // early: the directory has already been created, and it must still end up
    // with the mode it is supposed to have. GNU carries the mode over in this
    // case too, and a `dst` left at the forced owner-rwx would be a copy of a
    // 0500 directory that anyone could write into.
    match read_dir_fastread(src) {
        Ok(entries) => {
            for entry in entries {
                if !copy_entry(&entry, dest, job) {
                    ok = false;
                }
            }
        }
        Err(e) => {
            // GNU's wording, and it is the only one it has for this: `copy_dir`
            // slurps the whole directory with `savedir` and reports every way
            // that can fail as `cannot access`. "cannot read directory" would
            // be the more precise sentence and is what `rm` prints, but a
            // utility that differs from GNU only in the words of a diagnostic
            // is still a utility whose output a script cannot match on.
            let why = strerror(&e);
            let _ = writeln!(job.err, "cp: cannot access {}: {why}", quoteaf_os(src));
            ok = false;
        }
    }

    TreeResult::Made { new, ok }
}

/// Every entry of `src`, read in one go and put in the order GNU walks them.
///
/// This is gnulib's `savedir (dir, SAVEDIR_SORT_FASTREAD)`, which is what
/// `copy.c`'s `copy_dir` calls, reproduced for two reasons that are the same
/// reason twice.
///
/// **The order is observable now.** Until `--verbose` there was no way to tell
/// what order a tree was walked in — the copy it leaves is the same either way
/// — and `fs::read_dir`'s raw `readdir` order was as good as any. `cp -rv` puts
/// that order on stdout, and on ext4 the two disagree: a directory holding
/// `a.txt`, `sub` and `link` created in the order `sub`, `a.txt`, `link` is
/// named by GNU in creation order and by an unsorted `readdir` in hash order.
/// Neither is more correct, but only one of them is GNU's, and this program's
/// job is to be indistinguishable from GNU.
///
/// **And the order GNU picked is the fast one**, which is why gnulib calls it
/// `FASTREAD` rather than `SORTED`. Inode number is roughly on-disk position on
/// every filesystem that allocates inodes in tables, so walking a directory in
/// inode order turns the scattered reads of a `stat` per entry into a forward
/// scan. That is a real win on a cold cache and costs one sort of a list that
/// had to be materialised anyway.
///
/// The eager read is gnulib's too, and it changes one thing besides order: a
/// `readdir` that fails part-way through now abandons the whole directory
/// rather than copying the entries it had already seen. `savedir` returns
/// `NULL` in exactly that case, and `copy_dir` reports it as the one
/// `cannot access` diagnostic — so this is not a new behaviour so much as the
/// one GNU always had.
///
/// # Errors
///
/// Opening the directory, or any `readdir` within it.
fn read_dir_fastread(src: &Path) -> io::Result<Vec<fs::DirEntry>> {
    // `mut` is written only by the `#[cfg(unix)]` arm below. Off Unix there is
    // no inode to sort by, so the binding is never assigned to and the compiler
    // would rightly say so.
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut entries: Vec<fs::DirEntry> = fs::read_dir(src)?.collect::<io::Result<Vec<_>>>()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirEntryExt as _;
        // `d_ino` straight out of the `dirent`, not a `stat` — the sort must
        // not cost what it is there to save. Unstable sort because gnulib's
        // `qsort_r` is unstable too, and the only way to get a tie is two hard
        // links to one inode in one directory, where the two orders differ in
        // which of two names is copied first and in nothing else.
        entries.sort_unstable_by_key(fs::DirEntry::ino);
    }
    // Off Unix there is nothing to sort by, which is also gnulib's answer:
    // `SAVEDIR_SORT_FASTREAD` degrades to `SAVEDIR_SORT_NONE` where
    // `D_INO_IN_DIRENT` is not defined. See the `#[cfg(unix)]` arm above.
    Ok(entries)
}

/// One entry of a directory being walked. Split out of [`copy_tree`] only to
/// keep the mode bookkeeping either side of the walk readable in one screen.
///
/// The containing directory is no longer a parameter: the `readdir` that could
/// fail now happens in [`read_dir_fastread`], so the only caller that ever had
/// to name the *source directory* in a diagnostic is the one that reads it.
fn copy_entry<O: Write, E: Write>(
    entry: &fs::DirEntry,
    dest: &Path,
    job: &mut Job<'_, O, E>,
) -> bool {
    let from = entry.path();
    let to = dest.join(entry.file_name());

    // `DirEntry::metadata` does **not** follow symlinks, unlike `Path::is_dir`.
    // That is the whole of the fix for bug 1, and it also hands over the mode
    // the copy is to be created with, which a second `stat` might not.
    //
    // `-L` is the one policy that wants the other answer *here*, and asking for
    // it costs the extra `stat` that `entry.metadata()` was avoiding — there is
    // no following variant of it. That is the right way round: the option that
    // is not given pays nothing. See [`CpFlags::follow_walked`] for why `-H`
    // takes this branch and not the other one.
    let meta = if job.flags.follow_walked() {
        fs::metadata(&from)
    } else {
        entry.metadata()
    };
    let meta = match meta {
        Ok(m) => m,
        Err(e) => {
            let why = strerror(&e);
            let _ = writeln!(job.err, "cp: cannot stat {}: {why}", quoteaf_os(&from));
            return false;
        }
    };

    // The destination is stat'd for an entry found by walking, exactly as it is
    // for an operand: GNU reaches both through one `copy_internal`, so `-n`,
    // `--remove-destination` and the replace-a-symlink unlink all apply inside
    // a tree. Measured — `cp -rn s d` over an existing `d/x/f` says
    // `not replacing 'd/x/f'` and exits 1, and `cp -r --remove-destination`
    // announces `removed 'd/x/f'` for the same file.
    //
    // Of the refusals [`copy_one`] makes either side of this, the ones about a
    // *file* named twice are not here and cannot arise — a file found by
    // walking was not named on the command line, and nothing this command
    // created can be reached inside the tree it is filling. The one about a
    // *directory* seen twice is a different matter and is below: a walk can
    // reach a directory an operand already copied.
    let mut dest_state = match stat_destination(&meta, &to, job.flags) {
        Ok(d) => d,
        Err(e) => {
            let why = strerror(&e);
            let _ = writeln!(job.err, "cp: cannot stat {}: {why}", quoteaf_os(&to));
            return false;
        }
    };
    // `-n`'s refusal and `-i`'s question, for an entry found by walking. The
    // same-file check [`copy_one`] makes just before this one is not here and
    // cannot fire: a tree is not being copied into itself, and if it were the
    // walk would not terminate to reach this point.
    if !overwrite_allowed(&meta, &to, &dest_state, job) {
        return false;
    }
    // The two kind mismatches, in [`copy_one`]'s order and with its wording,
    // because they are the same `copy_internal` lines. Reaching them here is
    // what stops a directory landing on a file as `cannot create directory …:
    // File exists`, which named the right path and the wrong problem.
    if let Some(dest_meta) = dest_state.metadata() {
        if meta.is_dir() && !dest_meta.is_dir() {
            let _ = writeln!(
                job.err,
                "cp: cannot overwrite non-directory {} with directory {}",
                quoteaf_os(&to),
                quoteaf_os(&from)
            );
            return false;
        }
        if !meta.is_dir() && dest_meta.is_dir() {
            let _ = writeln!(
                job.err,
                "cp: cannot overwrite directory {} with non-directory",
                quoteaf_os(&to)
            );
            return false;
        }
    }
    if !remove_destination_first(&from, &to, &mut dest_state, job) {
        return false;
    }

    // A directory the walk has arrived at whose inode was already copied is
    // that directory a second time. `cp -r parent/child parent d` is the plain
    // way to get here: `parent/child` is copied to `d/child`, and then the walk
    // into `parent` finds the very same directory again. Writing it out a
    // second time would put one inode in two places, which for a directory
    // means hard-linking it, which is what GNU refuses (`copy.c:2690`).
    //
    // A *lookup*, never a `remember`: GNU records only command-line directories
    // (`copy.c:2667`) because only those can be named twice, and recording
    // walked ones would make the ordinary second visit to a shared subtree —
    // there is none, but the table would not know that — into an accusation.
    //
    // Two of GNU's four arms for this are deliberately absent, and neither can
    // be reached from a walk. Both compare the *earlier destination* with
    // something: `same_nameat (AT_FDCWD, src_name, …, earlier_file)` with this
    // source, `same_nameat (dst_dirfd, dst_relname, …, earlier_file)` with this
    // target. The first needs the place an operand was copied *to* to be the
    // directory the walk is now standing on, which is a copy into itself and is
    // refused at the operand before any walk starts (see [`place_source`]); GNU
    // can reach it only because it additionally records the inode of the first
    // destination directory it creates (`copy.c:2982`), which this `cp` does
    // not do — see design-decisions.md 724 for why it refuses up front instead.
    // The second needs two operands to have been copied to one path, which
    // [`copy_one`]'s own arm answers first, with the warning that names the
    // operand.
    if meta.is_dir()
        && let Some(id) = file_id(&from, &meta)
        && let Some(earlier) = job.copied.lookup(&id).map(Path::to_path_buf)
    {
        // GNU's third arm, with `command_line_arg` false so that only `-L`
        // satisfies it: following symlinks was asked for, so two paths reaching
        // one directory are a request for two independent copies of it and are
        // made silently. `cp -RL a b d` with `a/l` and `b/l` both links to `c`
        // is the case in its comment.
        if !job.flags.follow_walked() {
            let _ = writeln!(
                job.err,
                "cp: will not create hard link {} to directory {}",
                quoteaf_os(&to),
                quoteaf_os(&earlier)
            );
            return false;
        }
    }

    // The same dispatch an operand goes through, and literally the same code:
    // GNU reaches both through one `copy_internal`, so a link found inside a
    // tree is named exactly as a link named on the command line is, and every
    // attribute `-p` restores is restored in both. See [`place_entity`].
    // `false`: an entry found by walking is not a command-line argument, which
    // is what makes `-H` follow operands only and what keeps
    // `--preserve=links` from consulting its table for a singly-linked file
    // inside a tree.
    place_entity(&from, &meta, &to, &dest_state, false, job).is_ok()
}

/// Create `dest` as a directory with mode `mode`, before the umask is applied.
///
/// `Ok(true)` if it was created, `Ok(false)` if a directory was already there —
/// a distinction the caller needs, because an existing directory's mode is left
/// alone. Plain `create_dir` and not `create_dir_all`: GNU's single `mkdirat`
/// does not invent missing parents either, and `cp -r a no/such/dir` must fail
/// rather than quietly build the path.
fn make_dir(dest: &Path, mode: u32) -> io::Result<bool> {
    match create_dir_with_mode(dest, mode) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            // Only "already there" if it is a *directory*. A regular file under
            // that name is a failure, and reporting it as one is what stops the
            // walk from writing a directory's contents into whatever it found.
            if fs::metadata(dest).is_ok_and(|m| m.is_dir()) {
                Ok(false)
            } else {
                Err(e)
            }
        }
        Err(e) => Err(e),
    }
}

/// Copy the regular file `src` to `dst`.
///
/// This does by hand what `fs::copy` does in one call, for two reasons that are
/// not about speed:
///
/// * **`fs::copy` reports four different failures as one error.** The source
///   not opening, the destination not being creatable, a read fault and a write
///   fault all arrive as a single `io::Error` with nothing to say which
///   happened. GNU has a different sentence for each, and which sentence is
///   printed is the difference between knowing that a disk is full and knowing
///   that a file is unreadable.
/// * **`fs::copy` ends by giving the destination the source's exact mode.**
///   That is wrong twice: it ignores the umask on a file it has just created,
///   so a 0777 source lands as 0777 where GNU lands it as 0755; and it
///   overwrites the mode of a destination that *already existed*, so copying a
///   0777 file over somebody's 0600 one published it. See module docs, bug 8.
fn copy_regular_file<O: Write, E: Write>(
    src: &Path,
    src_meta: &fs::Metadata,
    dst: &Path,
    dest_exists: bool,
    mut debt: ModeDebt,
    job: &mut Job<'_, O, E>,
) -> bool {
    // The announcement is [`place_entity`]'s and has already happened, which is
    // GNU's order: `emit_verbose` (`copy.c:2630`) runs before `copy_reg`, so an
    // unreadable source is announced and *then* complained about.
    let mut input = match fs::File::open(src) {
        Ok(f) => f,
        Err(e) => {
            let why = strerror(&e);
            let _ = writeln!(
                job.err,
                "cp: cannot open {} for reading: {why}",
                quoteaf_os(src)
            );
            return false;
        }
    };

    let (mut output, new_dst) = match create_destination(src_meta, dst, dest_exists, &mut debt, job)
    {
        Ok(pair) => pair,
        Err(DestError::Dangling) => {
            let _ = writeln!(
                job.err,
                "cp: not writing through dangling symlink {}",
                quoteaf_os(dst)
            );
            return false;
        }
        Err(DestError::Remove(e)) => {
            let why = strerror(&e);
            let _ = writeln!(job.err, "cp: cannot remove {}: {why}", quoteaf_os(dst));
            return false;
        }
        Err(DestError::Io(e)) => {
            let why = strerror(&e);
            let _ = writeln!(
                job.err,
                "cp: cannot create regular file {}: {why}",
                quoteaf_os(dst)
            );
            return false;
        }
    };

    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = match input.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            // A signal arriving mid-read is not a read failure, and reporting
            // it as one would make `cp` unreliable under any job control.
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => {
                let why = strerror(&e);
                let _ = writeln!(job.err, "cp: error reading {}: {why}", quoteaf_os(src));
                return false;
            }
        };
        let Some(chunk) = buf.get(..n) else {
            // Unreachable: `read` returns at most the buffer's length. Handled
            // rather than indexed so the crate's `indexing_slicing` lint has
            // nothing to complain about and a broken `Read` cannot panic here.
            break;
        };
        if let Err(e) = output.write_all(chunk) {
            let why = strerror(&e);
            let _ = writeln!(job.err, "cp: error writing {}: {why}", quoteaf_os(dst));
            return false;
        }
    }

    // Through the descriptor the bytes were just written through, and not
    // through `dst`. GNU does the same (`copy_reg` passes `dest_desc` to
    // `fdutimensat`, `set_owner` and `copy_acl`), and the reason is the
    // set-user-ID bit: granting it *by name*, after the write, leaves a window
    // in which the name can be made to mean a different file. See
    // [`fsattr::On`]. This is also why the tail lives here rather than in
    // [`place_entity`] — that is exactly GNU's split between `copy_reg` and
    // `copy_internal`, whose tail is skipped for a regular file
    // (`copy.c:3233`, `if (copied_as_regular) return delayed_ok;`).
    preserve_attributes(
        Source::new(On::File(&input), src, src_meta),
        On::File(&output),
        dst,
        Made::Regular,
        new_dst,
        &mut debt,
        job,
    )
}

// ------------------------------------------------------------ preserving ---

/// The source of a copy, as the tail that puts its attributes back needs it.
///
/// Four things that always travel together and always describe the same file:
/// what to read its attributes *through*, what to call it in a diagnostic that
/// blames it, the `stat` the copy has already taken of it, and the mode the
/// destination is meant to end with.
///
/// The last is here rather than as a parameter beside it because it starts as
/// the source's mode and is then narrowed in one place — a `chown` that could
/// not be done takes the set-user-ID, set-group-ID and sticky bits off it, see
/// [`Chowned`] — and every step after that must see the narrowed value. GNU
/// keeps it in one `src_mode` local through the same run of steps, for the same
/// reason.
#[derive(Clone, Copy)]
struct Source<'a> {
    /// A **descriptor** for a regular file — the one its bytes were read
    /// through — and a *path* for a directory or a symlink, which have none
    /// here. See [`fsattr::On`].
    on: On<'a>,
    /// What to call it in a diagnostic. Only the extended-attribute steps blame
    /// the source by name; every other sentence in the tail names the
    /// destination, because every other step writes to it.
    name: &'a Path,
    /// The `stat` the copy already took. Its timestamps and owner are what the
    /// tail writes; its mode seeded [`Self::mode`].
    meta: &'a fs::Metadata,
    /// The permission bits the destination is to end with — the source's, less
    /// whatever an impossible `chown` has since taken off them.
    mode: u32,
}

impl<'a> Source<'a> {
    /// A source about to have its attributes copied, before anything has
    /// narrowed the mode.
    fn new(on: On<'a>, name: &'a Path, meta: &'a fs::Metadata) -> Self {
        Source {
            on,
            name,
            meta,
            mode: permission_bits(meta),
        }
    }
}

/// Put back onto the finished destination whatever `-p` asked to keep: the
/// timestamps, then the ownership, then the extended attributes, then the mode.
///
/// **The order is correctness, not arrangement**, and GNU leaves the reason for
/// each step in a line above it. Two reasons, and they point the same way:
///
/// * `copy.c:3211` — "chown turns off set[ug]id bits for non-root, so do the
///   chmod last". A `chmod` written before the `chown` compiles, runs, and
///   quietly drops the set-user-ID bit off every copy a non-root user makes.
/// * `copy.c:3244` — "Set xattrs after ownership as changing owners will clear
///   capabilities". A `setxattr` written before the `chown` loses
///   `security.capability`, which the kernel strips when a file changes hands.
///
/// `on` is the destination in the matching form: a descriptor for a regular
/// file, a path for a directory or a symlink. That is GNU's own split, and it
/// is a security property rather than a saved syscall; see [`fsattr::On`].
///
/// Returns `false` only for a failure that is fatal, which is what
/// [`CpFlags::require_preserve`] and [`CpFlags::require_preserve_xattr`] decide:
/// the diagnostic is printed either way, but only an attribute the user asked
/// for *by name* turns a copy that happened into an exit status of 1.
fn preserve_attributes<O: Write, E: Write>(
    mut src: Source<'_>,
    on: On<'_>,
    dst: &Path,
    made: Made,
    new_dst: bool,
    debt: &mut ModeDebt,
    job: &mut Job<'_, O, E>,
) -> bool {
    if job.flags.preserve.timestamps {
        // `and_then` because a source whose timestamps cannot even be read is
        // the same failure to the user as one whose copy cannot be stamped:
        // the destination has the wrong times either way, and `preserving
        // times for` is the sentence for that.
        if let Err(e) = times_of(src.meta).and_then(|times| fsattr::set_times(on, times)) {
            let why = strerror(&e);
            let _ = writeln!(
                job.err,
                "cp: preserving times for {}: {why}",
                quoteaf_os(dst)
            );
            if job.flags.require_preserve {
                return false;
            }
        }
    }

    // A symlink's owner was set where the link was made, so GNU's ownership
    // step is guarded by `!dest_is_symlink` and this one is guarded the same
    // way. Note that the guard is *here* rather than an early return above:
    // the extended-attribute step below applies to a symlink destination and
    // GNU runs it for one.
    if made != Made::Symlink
        && job.flags.preserve.ownership
        && (new_dst || owner_differs(on, src.meta))
    {
        match chown_to_source(src, on, dst, made, new_dst, job) {
            Chowned::Done => {}
            // GNU's `case 0`: the copy continues, but *narrower* than its
            // source. A user who could not be given the file cannot be handed
            // its set-user-ID bit either — that would be a privilege nobody
            // granted, on a file that is now theirs.
            Chowned::Disowned => src.mode &= !0o7000,
            Chowned::Failed => return false,
        }
    }

    // A failure here is only *fatal* if the user named `xattr` — GNU's two call
    // sites both write `! copy_attr (…) && x->require_preserve_xattr`
    // (`copy.c:1657` and `copy.c:3246`). Under `--preserve=all` the diagnostic
    // is printed and the copy still succeeds, which is the whole difference
    // between asking for everything and asking for this.
    let fatal = job.flags.preserve.xattr
        && !copy_xattrs(src, on, dst, fsattr::Xattrs::Ordinary, job)
        && job.flags.require_preserve_xattr;

    // Where the two call sites *do* differ is what a fatal one does next, and
    // this reproduces the difference rather than tidying it. `copy_internal`
    // returns out of the function, so a directory whose attributes could not be
    // carried does not get its mode preserved either; `copy_reg` sets
    // `return_val = false` and carries on to the mode step, so a regular file
    // does. That is observable in `ls -l`, and matching only one of the two
    // would change what one of the kinds comes out as.
    if fatal && made != Made::Regular {
        return false;
    }

    // "The operations beyond this point may dereference a symlink"
    // (`copy.c:3251`), and nothing portable can set a symlink's mode in any
    // case — Linux has no working `lchmod` at all.
    if made == Made::Symlink {
        return true;
    }

    settle_mode(src, on, dst, made, new_dst, debt, job) && !fatal
}

/// Carry the extended attributes of one class from the source to the copy, and
/// say as much about the ones that would not go as the options asked for.
///
/// gnulib decides how loud to be by picking one of three error callbacks
/// (`copy.c:3700`), which reads as two booleans and is three behaviours:
///
/// | Asked for | Printed | Exit status |
/// |---|---|---|
/// | `--preserve=xattr` | every failure | 1 |
/// | `--preserve=all` | all but "this filesystem has none" | 0 |
/// | `-a` | nothing at all | 0 |
///
/// The middle row's exception is gnulib's `errno_unsupported`, `ENOTSUP ||
/// ENODATA` — not the same test as [`fsattr`]'s, which decides whether there is
/// a failure at all rather than whether to mention one.
///
/// Returns `false` if anything at all failed, printed or not, which is what
/// gnulib's `copy_attr` returns; the caller turns that into an exit status only
/// under `--preserve=xattr`.
fn copy_xattrs<O: Write, E: Write>(
    src: Source<'_>,
    on: On<'_>,
    dst: &Path,
    which: fsattr::Xattrs,
    job: &mut Job<'_, O, E>,
) -> bool {
    // libattr's path form is `l*` throughout, so the source and the destination
    // are both named without following a link. Handed to [`fsattr`] explicitly
    // rather than left to the caller: for a symlink destination the difference
    // is the whole meaning of the call, and for the two other kinds the two
    // forms name the same file, so nothing else in the tail has had to care.
    let failures = fsattr::copy_xattrs(nofollow(src.on), nofollow(on), which);
    if failures.is_empty() {
        return true;
    }

    let all_errors = job.flags.require_preserve_xattr;
    let some_errors = !all_errors && !job.flags.reduce_diagnostics;
    for failure in &failures {
        if all_errors || (some_errors && !errno_unsupported(&failure.err)) {
            let why = strerror(&failure.err);
            let what = xattr_sentence(&failure.at, src.name, dst);
            let _ = writeln!(job.err, "cp: {what}: {why}");
        }
    }
    false
}

/// Name a file without following it, whatever form the rest of the tail is
/// using. A descriptor already names one file and cannot be redirected.
fn nofollow(on: On<'_>) -> On<'_> {
    match on {
        On::Path(path, _) => On::Path(path, Link::NoFollow),
        On::File(file) => On::File(file),
    }
}

/// libattr's four sentences, filled in. They are not interchangeable: two name
/// the attribute and two do not, and two blame the source while two blame the
/// destination.
fn xattr_sentence(at: &fsattr::XattrStep, src: &Path, dst: &Path) -> String {
    // The attribute name goes through `quoteaf` for the same reason the file
    // names do — coreutils gives libattr `copy_attr_quote`, which is `quoteaf`,
    // and libattr quotes the name with it as well as the path.
    match at {
        fsattr::XattrStep::List => format!("listing attributes of {}", quoteaf_os(src)),
        fsattr::XattrStep::Get(name) => {
            format!("getting attribute {} of {}", quoteaf(name), quoteaf_os(src))
        }
        fsattr::XattrStep::Set(name) => format!(
            "setting attribute {} for {}",
            quoteaf(name),
            quoteaf_os(dst)
        ),
        fsattr::XattrStep::SetAll => format!("setting attributes for {}", quoteaf_os(dst)),
    }
}

/// gnulib's `errno_unsupported` (`copy.c:700`): the two errors that mean the
/// filesystem has nothing to say rather than that something went wrong.
///
/// `ENODATA` is in it and is not in [`fsattr`]'s equivalent, which is the
/// difference between the two tests. It cannot come from the initial
/// `listxattr` — a filesystem does not answer "no such attribute" to a request
/// for the list — but it can come from a `getxattr` for a name that was removed
/// between the listing and the read, and a copy losing a race with `setfattr -x`
/// is not a failure worth a diagnostic.
fn errno_unsupported(e: &io::Error) -> bool {
    e.raw_os_error()
        .is_some_and(|n| n == libc_enotsup() || n == libc_enodata())
}

/// `ENOTSUP` (== `EOPNOTSUPP`) on Linux. Named here rather than pulled from a
/// `libc` crate for the same reason [`libc_eloop`] is.
const fn libc_enotsup() -> i32 {
    95
}

/// `ENODATA` on Linux — "the attribute you named is not there".
const fn libc_enodata() -> i32 {
    61
}

/// GNU's `set_owner` (`copy.c:889`), whose three outcomes are three different
/// things rather than a success and a failure.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
enum Chowned {
    /// The owner and group are the source's now.
    Done,
    /// They are not, and the caller must drop the set-user-ID, set-group-ID and
    /// sticky bits from the mode it is about to restore.
    ///
    /// **Not a failure.** An ordinary user copying a root-owned file cannot
    /// give the copy away, and this is the overwhelmingly common outcome of
    /// `cp -p`; a set-user-ID bit on a file that is now *theirs* would be a
    /// privilege nobody granted. That is why GNU makes it a third return value
    /// rather than an error — the copy succeeds, narrower than its source, and
    /// says nothing.
    Disowned,
    /// It failed for a reason worth reporting, it has been reported, and
    /// [`CpFlags::require_preserve`] says that ends the copy.
    Failed,
}

/// Give `on` the source's owner and group. See [`Chowned`] for the outcomes.
fn chown_to_source<O: Write, E: Write>(
    src: Source<'_>,
    on: On<'_>,
    dst: &Path,
    made: Made,
    new_dst: bool,
    job: &mut Job<'_, O, E>,
) -> Chowned {
    let fatal = if job.flags.require_preserve {
        Chowned::Failed
    } else {
        Chowned::Disowned
    };

    // Narrowing an *existing* destination first, because changing its owner
    // while it still wears its old mode is a window in which the new owner
    // holds permissions the copy will never have. GNU calls it exactly that —
    // "a window of vulnerability" — and closes it here (`copy.c:900`).
    if !new_dst && job.flags.preserve.mode && !narrow_before_chown(src, on, dst, job) {
        return fatal;
    }

    // The retry after a refusal, the `EPERM`-or-`EINVAL` test and the root check
    // are [`fsattr::take_ownership`]; what stays here is the sentence and
    // whether it ends the copy, which is the half `mv` does differently.
    //
    // A symlink gets no retry, which is GNU's asymmetry rather than ours: its
    // symlink arm (`copy.c:3178`) is a bare `lchownat`, while `copy_reg` and the
    // shared tail both retry. Matching it matters because the difference is
    // visible in `ls -l` on the copied link.
    let retry = if made == Made::Symlink {
        GroupRetry::No
    } else {
        GroupRetry::Yes
    };
    match fsattr::take_ownership(on, owner_of(src.meta), retry) {
        Ownership::Taken => Chowned::Done,
        Ownership::Denied => Chowned::Disowned,
        Ownership::Failed(e) => {
            let why = strerror(&e);
            let _ = writeln!(
                job.err,
                "cp: failed to preserve ownership for {}: {why}",
                quoteaf_os(dst)
            );
            fatal
        }
    }
}

/// Narrow an existing destination's mode to something its incoming owner cannot
/// be harmed by, before the `chown` that hands it over.
///
/// The window this closes is not subtle. `cp -p src dst` over an existing `dst`
/// of mode 0666 sets `dst`'s owner to `src`'s and *then* narrows it to `src`'s
/// 0600 — so between the two calls the file belongs to somebody who did not ask
/// for it and is writable by everybody. A `dst` carrying a set-user-ID bit is
/// worse: for that instant it is a setuid program, owned by the new owner,
/// containing whatever `dst` held.
///
/// The temporary mode is `old & new & S_IRWXU`: only bits that *both* the old
/// and the new mode already grant, and only to the owner. So it can take away
/// nothing the final `chmod` will not put back, and it cannot fail for asking
/// for a permission the caller did not already have.
///
/// Returns `false` when the narrowing failed, in which case the caller must not
/// chown at all.
fn narrow_before_chown<O: Write, E: Write>(
    src: Source<'_>,
    on: On<'_>,
    dst: &Path,
    job: &mut Job<'_, O, E>,
) -> bool {
    let old = match current_mode(on) {
        Ok(mode) => mode,
        Err(e) => {
            let why = strerror(&e);
            let _ = writeln!(job.err, "cp: cannot stat {}: {why}", quoteaf_os(dst));
            return false;
        }
    };
    let new = src.mode;
    // GNU's condition is `USE_ACL || (old & CHMOD_MODE_BITS & (~new | special))`
    // (`copy.c:917`), and this kernel has access-control lists, so the first
    // half is true and the second is never consulted. Narrowing unconditionally
    // is not belt-and-braces: the mode-bit test asks "does the old mode grant
    // anything the new one does not?", and an ACL can grant what no mode bit
    // shows. A destination at 0600 that also carries `user:mallory:rw` passes
    // the test against a 0600 source and would be handed to the new owner with
    // mallory's entry intact.
    //
    // The narrowing is `qset_acl`, not a chmod, for the same reason — see
    // `fsattr::set_mode_exactly`, which is that call: a chmod leaves named
    // entries standing, so a chmod-only narrowing closes the mode-bit half of
    // the window and leaves the ACL half open.
    let Err(e) = fsattr::set_mode_exactly(on, old & new & 0o700) else {
        return true;
    };
    // GNU's `owner_failure_ok`, which is `chown_failure_ok` for this step: a
    // non-root user who may not chmod the file was never going to manage the
    // chown either, and saying so twice helps nobody.
    if !is_denied_ownership(&e) || chown_privileges() {
        let why = strerror(&e);
        let _ = writeln!(
            job.err,
            "cp: clearing permissions for {}: {why}",
            quoteaf_os(dst)
        );
    }
    false
}

/// The last step: give the destination the mode it is meant to end with.
///
/// Three branches, and they are GNU's three — at `copy.c:3289` for a directory
/// and at `copy.c:1669` for a regular file, which is the same decision written
/// twice because the two live in different functions:
///
/// * **`--preserve=mode`** copies the source's whole `07777`, special bits
///   included, and does *not* apply the umask. That is the point of the option:
///   a preserved mode is the source's mode, not a fresh file's.
/// * **`--no-preserve=mode` on a destination this run created** gives it the
///   mode it would have had if nobody had asked — 0666 for a file, 0777 for a
///   directory, each less the umask. See [`CpFlags::explicit_no_preserve_mode`]
///   for why this is not the same as "no `-p` was given".
/// * **Otherwise**, settle the [`ModeDebt`]: put back what was withheld at
///   creation, less the umask.
///
/// The third branch is spelled differently for the two kinds, and that is GNU's
/// doing rather than an accident of this port. A directory's mode after `mkdir`
/// is not predictable — POSIX leaves the special bits implementation-defined —
/// so GNU reads it back and ORs the withheld bits into what it finds. A regular
/// file's is predictable, and `copy_reg`, holding a descriptor, simply writes
/// `src_mode & 0o777 & ~umask` without a stat. The two agree on the answer.
fn settle_mode<O: Write, E: Write>(
    src: Source<'_>,
    on: On<'_>,
    dst: &Path,
    made: Made,
    new_dst: bool,
    debt: &mut ModeDebt,
    job: &mut Job<'_, O, E>,
) -> bool {
    if job.flags.preserve.mode {
        // GNU's `copy_acl (…, src_mode)` — the mode *and* the access-control
        // lists, because on this kernel the two are one thing: an ACL entry
        // grants access no mode bit shows, so a `--preserve=mode` that copied
        // only the bits would produce a copy the kernel treats differently from
        // its source. See `fsattr::copy_permissions`, which is that call.
        //
        // Its diagnostic is the one place in `cp` that uses the *unquoted*
        // style, `quotef`, for a name. Matched rather than tidied: a utility
        // that differs from GNU only in the punctuation of a diagnostic is
        // still one whose output a script cannot match on.
        if let Err(e) = fsattr::copy_permissions(src.on, on, src.mode) {
            let why = strerror(&e);
            let _ = writeln!(
                job.err,
                "cp: preserving permissions for {}: {why}",
                quotef_os(dst)
            );
            return !job.flags.require_preserve;
        }
        return true;
    }

    if job.flags.explicit_no_preserve_mode && new_dst {
        // GNU's `MODE_RW_UGO` for a file and `S_IRWXUGO` for a directory
        // (`copy.c:3302`). A socket gets the directory's answer there too; this
        // `cp` copies neither sockets nor devices, so the two kinds below are
        // all of them.
        let default = if made == Made::Directory {
            0o777
        } else {
            0o666
        };
        // `set_acl`, not a chmod: GNU's line here is `set_acl (dst_name,
        // dest_desc, MODE_RW_UGO & ~cached_umask ())` (`copy.c:1685`). The
        // destination is one this run created, so it has no access ACL of its
        // own — but it may have *inherited* one from a parent directory's
        // default ACL, and `--no-preserve=mode` asking for 0666 & ~umask means
        // 0666 & ~umask and not "plus whatever the parent grants". The
        // inherited *default* ACL on a new directory is left alone, which is
        // also GNU's behaviour: it is the parent's policy for what comes next,
        // and this option says nothing about it.
        if let Err(e) = fsattr::set_mode_exactly(on, default & !cached_umask()) {
            let why = strerror(&e);
            let _ = writeln!(
                job.err,
                "cp: setting permissions for {}: {why}",
                quotef_os(dst)
            );
            // GNU returns `false` outright here, without consulting
            // `require_preserve`: `--no-preserve=mode` never sets it, so a
            // check would read as if the flag mattered when it cannot.
            return false;
        }
        return true;
    }

    // What was withheld goes back on, less the umask — the subtraction the
    // kernel would have made had the mode gone to `mkdir`/`open` outright, and
    // why a 1777 source directory produces a 1755 copy under the ordinary 022.
    // Skipping it would publish group-write on every copy of a 0775 directory
    // made by a process whose umask says otherwise.
    debt.omitted &= !cached_umask();

    if made == Made::Regular {
        // `copy_reg`'s form. Reached only under `--preserve=ownership` without
        // `--preserve=mode`, which is the only way a regular file acquires a
        // debt at all — and a regular file never carries a forced mode, because
        // nothing has to be opened through it.
        if debt.omitted == 0 {
            return true;
        }
        return chmod_settling(on, src.mode & 0o777 & !cached_umask(), dst, job);
    }

    // The stat is what a *debt* needs; the chmod below is what a *forced* mode
    // needs, and the two are separate conditions. GNU's `if (restore_dst_mode)`
    // sits outside its `if (omitted_permissions)` (`copy.c:3327`) for exactly
    // this case: a 0500 source directory owes nothing — 0500 withholds no
    // group or other bit — but was still forced to 0700 so it could be filled,
    // and returning early on "no debt" would leave every copy of a read-only
    // directory writable by its owner.
    if debt.omitted != 0 && debt.forced.is_none() {
        // Deducing the mode the directory actually got is not worth attempting
        // — `mkdir` applies implementation-defined rules to the special bits —
        // so it is read back. GNU says the same in the same place.
        match current_mode(on) {
            Ok(now) => {
                if debt.omitted & !now != 0 {
                    debt.forced = Some(now);
                }
            }
            Err(e) => {
                let why = strerror(&e);
                let _ = writeln!(job.err, "cp: cannot stat {}: {why}", quoteaf_os(dst));
                return false;
            }
        }
    }
    match debt.forced {
        Some(mode) => chmod_settling(on, mode | debt.omitted, dst, job),
        None => true,
    }
}

/// The settle-up chmod and its diagnostic, which is `quoteaf`'s where
/// [`settle_mode`]'s preserve branch is `quotef`'s. See there.
fn chmod_settling<O: Write, E: Write>(
    on: On<'_>,
    mode: u32,
    dst: &Path,
    job: &mut Job<'_, O, E>,
) -> bool {
    let Err(e) = fsattr::set_mode(on, mode) else {
        return true;
    };
    let why = strerror(&e);
    let _ = writeln!(
        job.err,
        "cp: preserving permissions for {}: {why}",
        quoteaf_os(dst)
    );
    !job.flags.require_preserve
}

/// The permission bits currently on whatever `on` names.
///
/// # Errors
///
/// Whatever the `stat` said.
fn current_mode(on: On<'_>) -> io::Result<u32> {
    let meta = match on {
        On::File(f) => f.metadata()?,
        On::Path(path, Link::Follow) => fs::metadata(path)?,
        On::Path(path, Link::NoFollow) => fs::symlink_metadata(path)?,
    };
    Ok(permission_bits(&meta))
}

/// Why a destination could not be opened for writing.
enum DestError {
    Io(io::Error),
    /// The name is a symlink that points at nothing. Resolving it to a
    /// (directory, name) pair to write through is racy by construction, so GNU
    /// refuses and says so rather than creating the link's target.
    Dangling,
    /// `-f` had to unlink a destination that would not open, and could not.
    /// A different sentence from [`DestError::Io`]'s — GNU's `cannot remove
    /// %s` against `cannot create regular file %s` — which is why it is a
    /// variant rather than an `io::Error` the caller has to guess about.
    Remove(io::Error),
}

/// Open `dst` for writing, creating it with the source's mode if it is new and
/// leaving its mode entirely alone if it is not.
///
/// This is GNU's `copy_reg` (`copy.c:1280`–`1348`) and the shape is load-bearing
/// in three places, all of which are `-f`:
///
/// * **Which open is tried first is decided by `dest_exists`, not by what the
///   first open answers.** GNU branches on `new_dst`: an existing destination
///   gets `O_WRONLY|O_TRUNC` and a new one gets `O_WRONLY|O_CREAT|O_EXCL` with
///   the mode. Deriving that from a failed `O_EXCL` would work for a plain
///   file, and would get [`Dest::Opaque`] wrong — the whole point of that state
///   is that a `stat` failed but the name is occupied.
/// * **`-f` unlinks on the `O_TRUNC` failure only.** That is why `cp -f a
///   dangling-link` still refuses: the open that fails there is the `O_EXCL`
///   one, which reports `EEXIST` and is the dangling-symlink case below, not
///   this one. Measured against 9.4 — the destination survives.
/// * **A new file's mode goes to the kernel with the `O_CREAT`**, which is the
///   only place the umask can narrow it without a window in which the file
///   exists at the wider mode. That is true of the file `-f` recreates too, so
///   `cp -f` over a 0400 destination leaves the *source's* mode behind rather
///   than the one it removed.
///
/// The `bool` in the success case is GNU's `*new_dst` **as it stands after the
/// open**, which is not `!dest_exists`: a destination that vanished between the
/// stat and the open, and one that `-f` unlinked, both end up newly created.
/// The distinction is what decides whether `-p` bothers to `chown`, whether
/// `--no-preserve=mode` applies, and — through [`ModeDebt`] — whether any
/// permissions were withheld at all.
///
/// # Errors
///
/// [`DestError::Dangling`] for a destination symlink that points at nothing,
/// [`DestError::Remove`] for an unlink `-f` could not do, and
/// [`DestError::Io`] for every other failure to open.
fn create_destination<O: Write, E: Write>(
    src_meta: &fs::Metadata,
    dst: &Path,
    dest_exists: bool,
    debt: &mut ModeDebt,
    job: &mut Job<'_, O, E>,
) -> Result<(fs::File, bool), DestError> {
    if dest_exists {
        match open_truncating(dst) {
            // Nothing was withheld, because nothing was created: an existing
            // file's mode is not `cp`'s to narrow even for an instant. GNU
            // zeroes the same two locals on this arm (`copy.c:1499`).
            Ok(f) => {
                debt.omitted = 0;
                return Ok((f, false));
            }
            // It went away between the stat and the open. GNU reaches its
            // `O_CREAT` arm in exactly this case too (`dest_errno == ENOENT`),
            // so a race loses nothing.
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => {
                if !job.flags.force {
                    return Err(DestError::Io(e));
                }
                if let Err(e) = fs::remove_file(dst)
                    && e.kind() != io::ErrorKind::NotFound
                {
                    return Err(DestError::Remove(e));
                }
                // *After* the removal and *after* [`copy_regular_file`]'s
                // `announce`, which is what puts `removed 'ro'` below
                // `'a' -> 'ro'` where `--remove-destination` puts it above.
                // GNU prints it from this same point inside `copy_reg`.
                if job.flags.verbose {
                    let _ = writeln!(job.out, "removed {}", quoteaf_os(dst));
                }
            }
        }
    }

    match open_new(dst, permission_bits(src_meta) & !debt.omitted) {
        Ok(f) => Ok((f, true)),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            // `symlink_metadata` sees the link itself; `metadata` follows it,
            // so failing there is exactly "points at nothing".
            if fs::symlink_metadata(dst).is_ok_and(|m| m.file_type().is_symlink())
                && fs::metadata(dst).is_err()
            {
                Err(DestError::Dangling)
            } else {
                // Occupied by something that is not a dangling link — a race
                // against another process, since the caller stat'd it as
                // absent a moment ago. Reported as the open failure it is.
                Err(DestError::Io(e))
            }
        }
        Err(e) => Err(DestError::Io(e)),
    }
}

/// `O_WRONLY|O_TRUNC`, with no `O_CREAT` and no mode: the destination is known
/// to be there and its permissions are not `cp`'s to change. See module docs,
/// bug 8.
fn open_truncating(dst: &Path) -> io::Result<fs::File> {
    fs::OpenOptions::new().write(true).truncate(true).open(dst)
}

/// `O_WRONLY|O_CREAT|O_EXCL` with `mode`, which the kernel narrows by the umask.
#[cfg(unix)]
fn open_new(dst: &Path, mode: u32) -> io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(dst)
}

/// The development host has no mode to give, so the file is created with
/// whatever Windows would have given it. The target OS is the `#[cfg(unix)]`
/// arm above; see [`permission_bits`].
#[cfg(not(unix))]
fn open_new(dst: &Path, _mode: u32) -> io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dst)
}

/// [`permission_bits`] of the name `path`, without following a final symlink.
fn permission_bits_of(path: &Path) -> io::Result<u32> {
    fs::symlink_metadata(path).map(|m| permission_bits(&m))
}

/// `mkdir(path, mode)`; the kernel narrows `mode` by the umask.
#[cfg(unix)]
fn create_dir_with_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new().mode(mode).create(path)
}

/// See [`open_new`]'s non-unix arm.
#[cfg(not(unix))]
fn create_dir_with_mode(path: &Path, _mode: u32) -> io::Result<()> {
    fs::create_dir(path)
}

/// `chmod(path, mode)`.
#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

/// `set_permissions` on Windows only toggles the read-only flag, which is not
/// what POSIX is asking for; doing nothing is the honest answer. The target OS
/// is the `#[cfg(unix)]` arm above.
#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> io::Result<()> {
    Ok(())
}

/// The process's file-mode creation mask, remembered, as GNU remembers it: a
/// deep `cp -r` should not go and ask the kernel once per directory.
///
/// A real `cp` is one process with one umask for its whole life, so caching
/// changes no answer. The **test build does not cache** — `cargo test` runs
/// dozens of copies inside one process, and the mode tests set the umask around
/// each one, so a value remembered from the first would make every later row
/// assert against the wrong mask. That is the cache being wrong about the test
/// harness, not the tests being wrong about `cp`.
///
/// Not asking for it inline is the point of [`coreutils::umask`]: GNU's own
/// `cached_umask` reads the value by *setting* it, and repeating that here —
/// uncached, on every copy, with an all-denying probe value — is what made two
/// of the tests below fail intermittently. See that module's docs.
#[cfg(all(unix, not(test)))]
fn cached_umask() -> u32 {
    use std::sync::OnceLock;
    static CACHE: OnceLock<u32> = OnceLock::new();
    *CACHE.get_or_init(coreutils::umask::current)
}

/// See the caching note above.
#[cfg(all(unix, test))]
fn cached_umask() -> u32 {
    coreutils::umask::current()
}

/// Windows has no umask. Zero makes [`copy_tree`]'s subtraction a no-op.
#[cfg(not(unix))]
fn cached_umask() -> u32 {
    coreutils::umask::current()
}

/// Reproduce the symlink at `src` as a symlink at `at`.
///
/// The link's *text* is copied verbatim, so a relative link keeps meaning
/// whatever it means relative to its new directory — which is what makes copying
/// a self-consistent tree of relative links produce another self-consistent
/// tree.
#[cfg(unix)]
fn clone_symlink(src: &Path, at: &Path) -> io::Result<()> {
    let points_at = fs::read_link(src)?;
    std::os::unix::fs::symlink(points_at, at)
}

/// Recreating a symlink needs a distinction between file and directory links on
/// Windows, and a privilege the test host does not necessarily have. Refusing is
/// the only answer that does not silently produce something other than a
/// symlink — and silently producing something else is precisely bug 1.
#[cfg(not(unix))]
fn clone_symlink(_src: &Path, _at: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "recreating a symlink is not supported on this host",
    ))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::expect_used
)]
mod tests {
    use super::*;
    /// The canned answer queue is shared with `rm`'s prompt tests; see
    /// [`coreutils::yesno`].
    use coreutils::yesno::Canned;
    use scratchdir::ScratchDir;

    fn args(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    /// `(flags, operands)` from a successful parse, or a panic naming the error.
    fn run_parse(items: &[&str]) -> (CpFlags, Vec<String>) {
        match parse_args(&args(items)).unwrap() {
            Request::Run(f, p) => (
                f,
                p.iter().map(|o| o.to_string_lossy().into_owned()).collect(),
            ),
            other => panic!("expected Run, got {other:?}"),
        }
    }

    fn fail(items: &[&str]) -> getopt::Error {
        parse_args(&args(items)).unwrap_err()
    }

    // ------------------------------------------------------------ parsing --

    #[test]
    fn no_args() {
        let (f, p) = run_parse(&[]);
        assert!(!f.recursive);
        assert!(p.is_empty());
    }

    #[test]
    fn simple_copy() {
        let (f, p) = run_parse(&["a", "b"]);
        assert!(!f.recursive);
        assert_eq!(p, vec!["a", "b"]);
    }

    #[test]
    fn dash_r_sets_recursive() {
        let (f, p) = run_parse(&["-r", "src", "dst"]);
        assert!(f.recursive);
        assert_eq!(p, vec!["src", "dst"]);
    }

    #[test]
    fn capital_r_also_recursive() {
        assert!(run_parse(&["-R", "src", "dst"]).0.recursive);
        assert!(run_parse(&["-rR", "a", "b"]).0.recursive);
        assert!(run_parse(&["--recursive", "a", "b"]).0.recursive);
    }

    #[test]
    fn flag_may_follow_operands() {
        let (f, p) = run_parse(&["a", "b", "-r"]);
        assert!(f.recursive);
        assert_eq!(p, vec!["a", "b"]);
    }

    #[test]
    fn multiple_sources() {
        assert_eq!(run_parse(&["a", "b", "c", "d"]).1, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn bare_dash_is_an_operand() {
        assert_eq!(run_parse(&["-", "dest"]).1, vec!["-", "dest"]);
    }

    /// Bug 4 in the module docs: this used to answer `unknown option: --`.
    #[test]
    fn double_dash_ends_options() {
        assert_eq!(run_parse(&["--", "-foo", "bar"]).1, vec!["-foo", "bar"]);
        let (f, p) = run_parse(&["--", "-r"]);
        assert!(!f.recursive, "-r after -- is a filename, not a flag");
        assert_eq!(p, vec!["-r"]);
    }

    #[test]
    fn double_dash_alone_leaves_no_operands() {
        assert!(run_parse(&["--"]).1.is_empty());
    }

    #[test]
    fn long_options_abbreviate() {
        assert!(run_parse(&["--recur", "a", "b"]).0.recursive);
    }

    /// `--r` must stay ambiguous — `--recursive`, `--reflink` and
    /// `--remove-destination` all begin with it. This is the test that fails if
    /// someone prunes the table to what is actually handled.
    #[test]
    fn ambiguous_abbreviation_is_refused() {
        let e = fail(&["--r"]);
        assert!(e.sentence.contains("ambiguous"), "{:?}", e.sentence);
        assert!(e.sentence.contains("--recursive"), "{:?}", e.sentence);
        assert!(e.sentence.contains("--reflink"), "{:?}", e.sentence);
    }

    /// `--pa` is **not** ambiguous, because `--path` and `--parents` are one
    /// option under two spellings. An earlier revision of this test asserted the
    /// opposite from recall; the measurement says otherwise:
    ///
    /// ```text
    /// $ cp --pa=1 a b
    /// cp: option '--parents' doesn't allow an argument
    /// ```
    ///
    /// which is `getopt_long` having resolved it, then complaining about the
    /// value. So the alias resolves, and the name it resolves to is the first
    /// *table* entry that matched — `--parents`, which precedes `--path` in
    /// `cp`'s table (it is the other way round in `rmdir`'s).
    ///
    /// Only `--pa` actually reaches both spellings; `--pat` is already past
    /// `--parents` and `--paren` already past `--path`. Each is listed with the
    /// name GNU answers with, measured the same way, because the interesting
    /// claim is not "it resolves" but *which* of the two it names:
    ///
    /// ```text
    /// --pa     cp: option '--parents' doesn't allow an argument
    /// --pat    cp: option '--path'    doesn't allow an argument
    /// --paren  cp: option '--parents' doesn't allow an argument
    /// ```
    #[test]
    fn the_deprecated_alias_does_not_make_its_own_option_ambiguous() {
        for (typed, named) in [
            ("--pa", "--parents"),
            ("--pat", "--path"),
            ("--paren", "--parents"),
        ] {
            let e = fail(&[typed, "a", "b"]);
            assert!(
                !e.sentence.contains("ambiguous"),
                "{typed}: {:?}",
                e.sentence
            );
            // It resolves, and is then refused for the separate reason that
            // this `cp` implements neither spelling.
            assert!(
                e.sentence
                    .contains(&format!("'{named}' is not implemented")),
                "{typed}: {:?}",
                e.sentence
            );
        }
    }

    /// The other half of the rule, and the half that a naive "hide the aliases"
    /// implementation gets wrong: `--p` matches `--parents`, `--path` **and**
    /// `--preserve`, and is ambiguous — but the message lists two, not three.
    /// Measured:
    ///
    /// ```text
    /// cp: option '--p' is ambiguous; possibilities: '--parents' '--preserve'
    /// ```
    #[test]
    fn a_prefix_that_reaches_past_the_alias_is_still_ambiguous() {
        assert_eq!(
            fail(&["--p", "a", "b"]).sentence,
            "option '--p' is ambiguous; possibilities: '--parents' '--preserve'"
        );
    }

    /// An exact alias resolves to itself, not to what it aliases — `getopt_long`
    /// returns the entry it matched.
    #[test]
    fn the_exact_alias_spelling_is_reported_as_typed() {
        assert!(
            fail(&["--path", "a", "b"])
                .sentence
                .contains("'--path' is not implemented"),
            "{:?}",
            fail(&["--path", "a", "b"]).sentence
        );
    }

    #[test]
    fn help_and_version_are_requests() {
        assert_eq!(parse_args(&args(&["--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&args(&["--version"])).unwrap(), Request::Version);
    }

    #[test]
    fn unknown_short_is_invalid_option() {
        let e = fail(&["-q", "a", "b"]);
        assert!(e.sentence.contains("invalid option"), "{:?}", e.sentence);
        assert!(e.sentence.contains('q'), "{:?}", e.sentence);
    }

    #[test]
    fn unrecognized_long_echoes_what_was_typed() {
        let e = fail(&["--zzz=1", "a", "b"]);
        assert!(
            e.sentence.contains("unrecognized option"),
            "{:?}",
            e.sentence
        );
        assert!(e.sentence.contains("--zzz=1"), "{:?}", e.sentence);
    }

    /// Ignoring any of these would produce a destination that looks right and is
    /// not — `-b` silently overwrites the backup it promised, `-l` and `-s`
    /// silently copy instead of linking, `-u` silently copies a file it was
    /// told to leave alone.
    #[test]
    fn unimplemented_short_options_are_rejected_by_name() {
        for flag in ["-l", "-s", "-u", "-x", "-Z"] {
            let e = fail(&[flag, "a", "b"]);
            assert!(
                e.sentence.contains("not implemented"),
                "{flag}: {:?}",
                e.sentence
            );
        }
    }

    #[test]
    fn unimplemented_long_options_are_rejected_by_name() {
        for name in [
            "--attributes-only",
            "--copy-contents",
            "--link",
            "--one-file-system",
            "--parents",
            "--strip-trailing-slashes",
            "--symbolic-link",
            "--update",
            // Given values inline so the option cannot swallow an operand and
            // turn a rejection test into an arity test.
            "--sparse=always",
            "--reflink=always",
        ] {
            let e = fail(&[name, "a", "b"]);
            assert!(
                e.sentence.contains("not implemented"),
                "{name}: {:?}",
                e.sentence
            );
        }
    }

    /// The option spellings [`help_text`] documents, read out of the text
    /// itself: every line indented under the option list whose first word is an
    /// option, up to the two-space gap before the description, split on `", "`
    /// and cut at the `=` or `[` that introduces a value.
    fn documented_options() -> HashSet<String> {
        let mut set = HashSet::new();
        for line in help_text().lines() {
            let Some(rest) = line.strip_prefix("  ") else {
                continue;
            };
            let rest = rest.trim_start();
            if !rest.starts_with('-') {
                continue;
            }
            for token in rest.split("  ").next().unwrap_or(rest).split(", ") {
                let name = token.split(['=', '[']).next().unwrap_or(token).trim();
                if name.starts_with('-') {
                    set.insert(name.to_string());
                }
            }
        }
        set
    }

    /// Whether this argv gets past "recognised, but not implemented here".
    fn is_implemented(argv: &[&str]) -> bool {
        match parse_args(&args(argv)) {
            Err(e) => !e.sentence.contains("not implemented"),
            Ok(_) => true,
        }
    }

    /// `--help` names every option this `cp` acts on, and names nothing else.
    ///
    /// Derived from [`SHORT_OPTIONS`] and [`LONG_OPTIONS`] rather than written
    /// out, because a hand-written list would be the same document as the help
    /// text and would go wrong in the same way at the same moment. It already
    /// had: `-a` was implemented, tested and shipped with no help line at all,
    /// and nothing noticed, because no test read the help.
    #[test]
    fn help_documents_exactly_the_options_this_cp_has() {
        let mut implemented: HashSet<String> = HashSet::new();

        for (name, takes) in LONG_OPTIONS {
            // A value is attached rather than given as the next word, so that a
            // value-taking option cannot swallow `a` and turn this into a test
            // about operand arity.
            let spelled = match takes {
                Takes::Nothing => format!("--{name}"),
                Takes::Optional | Takes::Required => format!("--{name}=x"),
            };
            if is_implemented(&[&spelled, "a", "b"]) {
                implemented.insert(format!("--{name}"));
            }
        }

        let letters = SHORT_OPTIONS.as_bytes();
        let mut i = 0;
        while i < letters.len() {
            let c = char::from(letters[i]);
            let takes = letters.get(i + 1) == Some(&b':');
            i += usize::from(takes) + 1;
            let spelled = if takes {
                format!("-{c}x")
            } else {
                format!("-{c}")
            };
            if is_implemented(&[&spelled, "a", "b"]) {
                implemented.insert(format!("-{c}"));
            }
        }

        // `--path` is `--parents` under another name and would never get a line
        // of its own, so it must not reach this comparison as a *separate*
        // option. It does not, because `--parents` is not implemented -- if it
        // ever is, this assertion is the reminder to decide which spelling the
        // help names.
        let documented = documented_options();
        let mut missing: Vec<&String> = implemented.difference(&documented).collect();
        let mut extra: Vec<&String> = documented.difference(&implemented).collect();
        missing.sort();
        extra.sort();
        assert!(
            missing.is_empty() && extra.is_empty(),
            "implemented but undocumented: {missing:?}; documented but not implemented: {extra:?}"
        );
    }

    // --------------------------------------------- -p and the preserve list --

    /// `-p` is the three POSIX attributes and `require_preserve`, and a bare
    /// `--preserve` is the same thing — GNU's `case 'p'` is *fallen into* by
    /// the no-argument `--preserve` (`cp.c:1088`) rather than duplicated.
    #[test]
    fn dash_p_and_bare_preserve_are_the_same_option() {
        for spelling in ["-p", "--preserve", "--pres"] {
            let (f, p) = run_parse(&[spelling, "a", "b"]);
            assert_eq!(f.preserve, Preserve::posix(), "{spelling}");
            assert!(f.require_preserve, "{spelling}");
            assert!(!f.explicit_no_preserve_mode, "{spelling}");
            assert_eq!(p, vec!["a", "b"]);
        }
    }

    /// `-p` names three attributes; it does not un-name a fourth.
    ///
    /// GNU's `case 'p'` is three assignments (`cp.c:1104`) and never mentions
    /// `preserve_links`, so the two halves of `cp -d -p` do not fight. An
    /// assignment of the whole [`Preserve`] passes every test above — each
    /// gives one option — and fails only here, where the observable difference
    /// is whether two hard-linked sources reach the destination as one inode or
    /// as two.
    #[test]
    fn dash_p_leaves_the_attributes_it_does_not_name_alone() {
        let (f, _) = run_parse(&["-d", "-p", "a", "b"]);
        assert!(f.preserve.links, "-d's half survives the -p after it");
        assert!(f.preserve.mode && f.preserve.timestamps && f.preserve.ownership);

        let (g, _) = run_parse(&["-p", "-d", "a", "b"]);
        assert_eq!(
            f.preserve, g.preserve,
            "neither option can undo the other, so their order cannot matter"
        );

        let (h, _) = run_parse(&["--preserve=links", "-p", "a", "b"]);
        assert!(h.preserve.links, "and the spelled-out half survives too");
    }

    /// One word turns on one attribute and no others. A `--preserve=mode` that
    /// quietly restored the timestamps too would pass every test that only
    /// checked `-p`.
    #[test]
    fn each_preserve_word_sets_only_its_own_attribute() {
        let rows: &[(&str, Preserve)] = &[
            (
                "mode",
                Preserve {
                    mode: true,
                    ..Preserve::NONE
                },
            ),
            (
                "timestamps",
                Preserve {
                    timestamps: true,
                    ..Preserve::NONE
                },
            ),
            (
                "ownership",
                Preserve {
                    ownership: true,
                    ..Preserve::NONE
                },
            ),
        ];
        for &(word, want) in rows {
            let (f, _) = run_parse(&[&format!("--preserve={word}"), "a", "b"]);
            assert_eq!(f.preserve, want, "--preserve={word}");
            assert!(f.require_preserve, "--preserve={word}");
        }
    }

    /// A comma-separated list is applied left to right, and each of the seven
    /// words has a distinct first letter — so a single character reaches one.
    /// That is `argmatch`'s prefix rule, not a table lookup.
    #[test]
    fn preserve_words_are_a_list_and_abbreviate() {
        let (f, _) = run_parse(&["--preserve=mode,timestamps", "a", "b"]);
        assert_eq!(
            f.preserve,
            Preserve {
                mode: true,
                timestamps: true,
                ..Preserve::NONE
            }
        );
        let (g, _) = run_parse(&["--preserve=m,t,o", "a", "b"]);
        assert_eq!(g.preserve, Preserve::posix());
        let (h, _) = run_parse(&["--preserve=timestamp", "a", "b"]);
        assert!(h.preserve.timestamps && !h.preserve.mode);
    }

    /// The last mention of an attribute wins, because each word is applied as
    /// it is read rather than collected and resolved at the end.
    #[test]
    fn a_later_word_overrides_an_earlier_one() {
        let (f, _) = run_parse(&["-p", "--no-preserve=mode", "a", "b"]);
        assert!(!f.preserve.mode, "--no-preserve=mode came second");
        assert!(f.preserve.timestamps && f.preserve.ownership, "and only it");
        assert!(f.explicit_no_preserve_mode);

        let (g, _) = run_parse(&["--no-preserve=mode", "-p", "a", "b"]);
        assert!(g.preserve.mode, "-p came second");
        assert!(
            g.explicit_no_preserve_mode,
            "but `-p` does not clear the flag: GNU's `case 'p'` sets \
             preserve_mode and leaves explicit_no_preserve_mode alone"
        );
    }

    /// `--no-preserve=mode` is not the same as never having said `-p`: it
    /// records that the default mode was asked for *by name*, which is what
    /// makes a new destination come out 0666 rather than the source's mode.
    #[test]
    fn no_preserve_mode_is_explicit() {
        let (f, _) = run_parse(&["--no-preserve=mode", "a", "b"]);
        assert!(!f.preserve.mode);
        assert!(f.explicit_no_preserve_mode);
        assert!(
            !f.require_preserve,
            "only --preserve sets it; a failure to *not* preserve is not a thing"
        );
    }

    /// `--no-preserve=all` turns off all five and sets the mode flag, which is
    /// `decode_preserve_arg`'s `PRESERVE_ALL` arm in its `off` direction.
    #[test]
    fn no_preserve_all_turns_everything_off() {
        let (f, _) = run_parse(&["-a", "--no-preserve=all", "a", "b"]);
        assert_eq!(f.preserve, Preserve::NONE);
        assert!(f.explicit_no_preserve_mode);
    }

    /// `--preserve=all` is every attribute at once, and `-a` is that plus
    /// `-dR`. The one thing `all` is *not* is `context`: GNU's `PRESERVE_ALL`
    /// guards the security-context line with `if (selinux_enabled)`, so on a
    /// machine without SELinux it does not ask for one either.
    #[test]
    fn preserve_all_is_every_attribute_this_cp_has() {
        let (f, _) = run_parse(&["--preserve=all", "a", "b"]);
        assert_eq!(f.preserve, Preserve::ALL);
        assert!(f.require_preserve);
        assert!(!f.explicit_no_preserve_mode);
    }

    /// `-a` is "like `-dR --preserve=all`", and the difference is the word
    /// *like*: it sets `reduce_diagnostics`, which nothing else in `cp` sets.
    #[test]
    fn archive_is_dash_d_dash_r_preserve_all_and_one_thing_more() {
        for spelling in ["-a", "--archive"] {
            let (f, _) = run_parse(&[spelling, "a", "b"]);
            assert_eq!(f.preserve, Preserve::ALL, "{spelling}");
            assert!(f.recursive, "{spelling}");
            assert_eq!(f.dereference, Deref::Never, "{spelling}");
            assert!(f.require_preserve, "{spelling}");
            assert!(f.reduce_diagnostics, "{spelling}");
        }

        let (g, _) = run_parse(&["-dR", "--preserve=all", "a", "b"]);
        assert!(
            !g.reduce_diagnostics,
            "the spelled-out form complains about an attribute it could not \
             carry; `-a` says nothing"
        );
    }

    /// Naming `xattr` promises to *fail* if the extended attributes cannot be
    /// carried; getting it through `all` or `-a` does not. That is GNU's
    /// `require_preserve_xattr`, which `PRESERVE_XATTR` sets and `PRESERVE_ALL`
    /// deliberately does not.
    #[test]
    fn only_the_xattr_word_by_name_makes_a_failure_fatal() {
        let (f, _) = run_parse(&["--preserve=xattr", "a", "b"]);
        assert!(f.preserve.xattr && f.require_preserve_xattr);
        assert!(!f.reduce_diagnostics, "and it is the loudest of the three");

        for asked in ["--preserve=all", "-a"] {
            let (g, _) = run_parse(&[asked, "a", "b"]);
            assert!(g.preserve.xattr, "{asked} asks for them");
            assert!(!g.require_preserve_xattr, "{asked} does not insist on them");
        }

        let (h, _) = run_parse(&["--preserve=xattr", "--no-preserve=xattr", "a", "b"]);
        assert!(
            !h.preserve.xattr && !h.require_preserve_xattr,
            "the off direction clears both, as GNU's `!on` does"
        );
    }

    /// The one attribute this `cp` cannot write is refused **on `--preserve`
    /// and accepted on `--no-preserve`**, which is not an inconsistency:
    /// `--no-preserve=context` asks for something already true.
    #[test]
    fn the_unwritable_attribute_is_refused_one_way_only() {
        let e = fail(&["--preserve=context", "a", "b"]);
        assert!(e.sentence.contains("not implemented"), "{:?}", e.sentence);
        assert!(
            e.sentence.contains("context"),
            "the diagnostic names the word, not the option: {:?}",
            e.sentence
        );
        let (f, _) = run_parse(&["--no-preserve=context", "a", "b"]);
        assert!(!f.require_preserve, "--no-preserve=context");
    }

    /// `links` is spelled like the other `--preserve` words and abbreviates like
    /// them, but it is not one of `-p`'s three: asking for it by name is the
    /// only way to get it.
    #[test]
    fn links_is_a_preserve_word_of_its_own() {
        let (f, _) = run_parse(&["--preserve=links", "a", "b"]);
        assert!(f.preserve.links);
        assert_eq!(
            Preserve {
                links: false,
                ..f.preserve
            },
            Preserve::NONE,
            "--preserve=links turns on that and nothing else"
        );
        assert!(f.require_preserve);

        let (g, _) = run_parse(&["-p", "a", "b"]);
        assert!(
            !g.preserve.links,
            "GNU's `-p` is mode,ownership,timestamps -- links is not in it"
        );

        let (h, _) = run_parse(&["--preserve=li", "a", "b"]);
        assert!(h.preserve.links, "argmatch accepts any unambiguous prefix");

        let (i, _) = run_parse(&["--preserve=links", "--no-preserve=links", "a", "b"]);
        assert!(!i.preserve.links, "the last word wins, as for every other");
    }

    /// `-d` is two options in one letter, and neither half is optional.
    ///
    /// The `require_preserve` half of the assertion is the one worth having:
    /// GNU's `case 'd'` sets the two fields and nothing else, so `cp -d` and
    /// `cp -P --preserve=links` are the same command but for the promise to
    /// fail when an attribute cannot be carried — which only the spelled-out
    /// one makes.
    #[test]
    fn d_is_no_dereference_and_preserve_links() {
        let (f, _) = run_parse(&["-d", "a", "b"]);
        assert_eq!(f.dereference, Deref::Never);
        assert!(f.preserve.links);
        assert_eq!(
            Preserve {
                links: false,
                ..f.preserve
            },
            Preserve::NONE,
            "-d carries no other attribute"
        );
        assert!(!f.require_preserve, "GNU's `case 'd'` does not set it");
        assert!(!f.recursive, "-d is not -dR");

        // Both halves are ordinary assignments, so a later option overrides
        // either one independently — `cp -dL` follows links and still links,
        // `cp -d --no-preserve=links` keeps the links themselves and does not.
        let (g, _) = run_parse(&["-d", "-L", "a", "b"]);
        assert_eq!(g.dereference, Deref::Always);
        assert!(g.preserve.links);
        let (h, _) = run_parse(&["-d", "--no-preserve=links", "a", "b"]);
        assert_eq!(h.dereference, Deref::Never);
        assert!(!h.preserve.links);
        let (i, _) = run_parse(&["-L", "-d", "a", "b"]);
        assert_eq!(i.dereference, Deref::Never, "the last one wins");
    }

    /// A word the table does not have, and the empty word, which is a prefix of
    /// every entry and so ambiguous. Both measured from GNU 9.4.
    #[test]
    fn a_bad_preserve_word_is_refused_in_argmatch_words() {
        let e = fail(&["--preserve=bogus", "a", "b"]);
        assert!(
            e.sentence.starts_with(
                "invalid argument \u{2018}bogus\u{2019} for \u{2018}--preserve\u{2019}"
            ),
            "{:?}",
            e.sentence
        );
        assert!(
            e.sentence.contains("Valid arguments are:"),
            "{:?}",
            e.sentence
        );
        assert!(
            e.sentence.contains("\u{2018}timestamps\u{2019}"),
            "{:?}",
            e.sentence
        );

        let empty = fail(&["--preserve=", "a", "b"]);
        assert!(
            empty
                .sentence
                .starts_with("ambiguous argument \u{2018}\u{2019} for \u{2018}--preserve\u{2019}"),
            "{:?}",
            empty.sentence
        );

        // The option named in the diagnostic is the one that was typed.
        let off = fail(&["--no-preserve=bogus", "a", "b"]);
        assert!(
            off.sentence.contains("\u{2018}--no-preserve\u{2019}"),
            "{:?}",
            off.sentence
        );
    }

    /// A bad word part-way through a list is still refused, and the words
    /// before it having been applied does not save it.
    #[test]
    fn a_bad_word_after_a_good_one_is_still_refused() {
        let e = fail(&["--preserve=mode,bogus", "a", "b"]);
        assert!(e.sentence.contains("invalid argument"), "{:?}", e.sentence);
    }

    /// `--no-preserve` takes a *required* argument, so it cannot be given bare
    /// — GNU's table says `required_argument` and so does ours.
    #[test]
    fn no_preserve_requires_a_list() {
        let e = fail(&["a", "b", "--no-preserve"]);
        assert!(
            e.sentence.contains("requires an argument"),
            "{:?}",
            e.sentence
        );
    }

    /// `--preserve`'s argument is *optional*, which is the whole reason a bare
    /// `--preserve` means `-p`. `--preserve=` is therefore an empty list rather
    /// than a missing one, and is refused by `argmatch` — see above.
    #[test]
    fn preserve_takes_an_optional_argument() {
        assert_eq!(run_parse(&["a", "b", "--preserve"]).1, vec!["a", "b"]);
    }

    // ------------------------------------------- the three overwrite flags --

    /// Three fields, not one: a test that passed with all three sharing a single
    /// `overwrite` field would not notice that `-f` and `--remove-destination`
    /// unlink at different times.
    #[test]
    fn each_overwrite_option_sets_only_its_own_field() {
        for (spelling, force, remove, interactive) in [
            ("-f", true, false, Interactive::Unspecified),
            ("--force", true, false, Interactive::Unspecified),
            (
                "--remove-destination",
                false,
                true,
                Interactive::Unspecified,
            ),
            ("-n", false, false, Interactive::AlwaysNo),
            ("--no-clobber", false, false, Interactive::AlwaysNo),
            ("-i", false, false, Interactive::AskUser),
            ("--interactive", false, false, Interactive::AskUser),
        ] {
            let (f, p) = run_parse(&[spelling, "a", "b"]);
            assert_eq!(f.force, force, "{spelling}");
            assert_eq!(f.remove_destination, remove, "{spelling}");
            assert_eq!(f.interactive, interactive, "{spelling}");
            assert_eq!(p, vec!["a", "b"], "{spelling}");
        }
    }

    /// None of the three excludes another, and GNU rejects no pairing of them —
    /// so neither may this. Measured against 9.4: `cp -fn a b` refuses like `-n`
    /// alone, so the parse must keep both fields rather than have one clear the
    /// other.
    #[test]
    fn the_overwrite_options_combine() {
        let (f, _) = run_parse(&["-fn", "--remove-destination", "a", "b"]);
        assert!(f.force);
        assert!(f.remove_destination);
        assert_eq!(f.interactive, Interactive::AlwaysNo);
    }

    /// `--rem` is the shortest unambiguous prefix: `--recursive` and `--reflink`
    /// share `--re`. This is the abbreviation the manual's own examples use.
    #[test]
    fn remove_destination_abbreviates_to_rem() {
        assert!(run_parse(&["--rem", "a", "b"]).0.remove_destination);
        assert!(fail(&["--re", "a", "b"]).sentence.contains("ambiguous"));
    }

    /// `--no-c` reaches `--no-clobber` alone; `--no-` reaches four options.
    #[test]
    fn no_clobber_abbreviates_past_the_other_no_options() {
        assert_eq!(
            run_parse(&["--no-c", "a", "b"]).0.interactive,
            Interactive::AlwaysNo
        );
        let e = fail(&["--no-", "a", "b"]);
        assert!(e.sentence.contains("ambiguous"), "{:?}", e.sentence);
        assert!(e.sentence.contains("--no-clobber"), "{:?}", e.sentence);
        assert!(e.sentence.contains("--no-dereference"), "{:?}", e.sentence);
    }

    /// `Interactive` is an enum rather than a bool because `-i` and `-n` are two
    /// settings of one field and the last one wins. Repeating either is not an
    /// error and does not change the answer.
    #[test]
    fn repeating_an_overwrite_policy_is_not_an_error() {
        assert_eq!(
            run_parse(&["-n", "-n", "--no-clobber", "a", "b"])
                .0
                .interactive,
            Interactive::AlwaysNo
        );
        assert_eq!(
            run_parse(&["-i", "-i", "--interactive", "a", "b"])
                .0
                .interactive,
            Interactive::AskUser
        );
    }

    /// The last of `-i` and `-n` wins, in both directions — GNU keeps one
    /// `x.interactive` and each option assigns to it. Measured against 9.4:
    /// `cp -in` refuses without asking and `cp -ni` asks.
    #[test]
    fn the_last_of_i_and_n_wins() {
        for (argv, want) in [
            (["-i", "-n"], Interactive::AlwaysNo),
            (["-n", "-i"], Interactive::AskUser),
            (["--interactive", "--no-clobber"], Interactive::AlwaysNo),
            (["--no-clobber", "--interactive"], Interactive::AskUser),
        ] {
            let (f, _) = run_parse(&[argv[0], argv[1], "a", "b"]);
            assert_eq!(f.interactive, want, "{argv:?}");
        }
        // And in one clustered argument, where the bytes are read left to right.
        assert_eq!(
            run_parse(&["-in", "a", "b"]).0.interactive,
            Interactive::AlwaysNo
        );
        assert_eq!(
            run_parse(&["-ni", "a", "b"]).0.interactive,
            Interactive::AskUser
        );
    }

    #[test]
    fn the_overwrite_options_are_off_by_default() {
        let (f, _) = run_parse(&["a", "b"]);
        assert!(!f.force);
        assert!(!f.remove_destination);
        assert_eq!(f.interactive, Interactive::Unspecified);
    }

    #[test]
    fn value_on_an_option_that_takes_none() {
        let e = fail(&["--recursive=yes", "a", "b"]);
        assert!(e.sentence.contains("doesn't allow"), "{:?}", e.sentence);
    }

    // ------------------------------------------- where the destination is --

    /// All three spellings of `-t`, and the fact that its value never lands in
    /// the operand list. `-td` is the one that could only work through a table
    /// that says the letter takes a value.
    #[test]
    fn a_target_directory_is_taken_out_of_the_operands() {
        for spelling in [
            &["-t", "d", "a", "b"][..],
            &["-td", "a", "b"][..],
            &["--target-directory=d", "a", "b"][..],
            &["a", "b", "-t", "d"][..],
        ] {
            let (f, p) = run_parse(spelling);
            assert_eq!(f.target_directory, Some(OsString::from("d")));
            assert_eq!(p, ["a", "b"], "{spelling:?}");
        }
    }

    /// GNU compares nothing here — it asks only whether one was already given —
    /// so naming the same directory twice fails just as two different ones do.
    #[test]
    fn a_second_target_directory_is_refused() {
        for spelling in [
            &["-t", "d", "-t", "d", "a"][..],
            &["-t", "d", "-t", "e", "a"][..],
        ] {
            let e = fail(spelling);
            assert_eq!(e.sentence, "multiple target directories specified");
            // `error (EXIT_FAILURE, …)` upstream, not `usage`, so there is no
            // "Try 'cp --help'" after it.
            assert_eq!(e.referral, None, "{spelling:?}");
        }
    }

    #[test]
    fn a_missing_target_directory_value_is_a_getopt_error() {
        let e = fail(&["-t"]);
        assert!(
            e.sentence.contains("option requires an argument"),
            "{:?}",
            e.sentence
        );
    }

    // --------------------------------------------------- non-UTF-8 argv --

    /// The regression test for the reason this file was rewritten. On this OS a
    /// filename may hold any byte but `/` and NUL, and byte `0x80` alone is not
    /// valid UTF-8, so an operand containing it cannot be a `String` at all.
    #[test]
    #[cfg(unix)]
    fn a_non_utf8_operand_survives_parsing() {
        use std::os::unix::ffi::OsStringExt;
        let bad = OsString::from_vec(vec![b'a', 0x80, b'b']);
        assert!(
            bad.to_str().is_none(),
            "the fixture must be un-representable as String, or it tests nothing"
        );
        match parse_args(&[OsString::from("-r"), bad.clone(), OsString::from("d")]).unwrap() {
            Request::Run(f, p) => {
                assert!(f.recursive);
                assert_eq!(p, vec![bad, OsString::from("d")]);
            }
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[test]
    #[cfg(unix)]
    fn a_non_utf8_long_option_is_unrecognised_not_a_panic() {
        use std::os::unix::ffi::OsStringExt;
        let bad = OsString::from_vec(vec![b'-', b'-', 0x80]);
        let e = parse_args(&[bad]).unwrap_err();
        assert!(
            e.sentence.starts_with("unrecognized option"),
            "{:?}",
            e.sentence
        );
    }

    /// The two tests above are `#[cfg(unix)]`, so on the development host —
    /// Windows — the regression tests for the bug this file was rewritten to fix
    /// **do not run at all**. That is the same blind spot that let the bug
    /// survive, so it is closed rather than noted. Windows has its own argument
    /// that no `String` can hold: an unpaired surrogate (a UTF-16 code unit in
    /// `0xD800..=0xDFFF` with no partner), which reaches the same `unwrap` in
    /// `env::args()` by a different route.
    #[test]
    #[cfg(windows)]
    fn a_non_utf8_operand_survives_parsing() {
        use std::os::windows::ffi::OsStringExt;
        let bad = OsString::from_wide(&[0x0061, 0xD800, 0x0062]);
        assert!(
            bad.to_str().is_none(),
            "the fixture must be un-representable as String, or it tests nothing"
        );
        match parse_args(&[OsString::from("-r"), bad.clone(), OsString::from("d")]).unwrap() {
            Request::Run(f, p) => {
                assert!(f.recursive);
                assert_eq!(p, vec![bad, OsString::from("d")]);
            }
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[test]
    #[cfg(windows)]
    fn a_non_utf8_long_option_is_unrecognised_not_a_panic() {
        use std::os::windows::ffi::OsStringExt;
        let bad = OsString::from_wide(&[0x002D, 0x002D, 0xD800]);
        let e = parse_args(&[bad]).unwrap_err();
        assert!(
            e.sentence.starts_with("unrecognized option"),
            "{:?}",
            e.sentence
        );
    }

    // ----------------------------------------------------- compute_target --

    #[test]
    fn target_file_to_file() {
        let t = compute_target(Path::new("a.txt"), Path::new("b.txt"), false);
        assert_eq!(t, PathBuf::from("b.txt"));
    }

    #[test]
    fn target_file_into_dir() {
        let t = compute_target(Path::new("src/a.txt"), Path::new("dst"), true);
        assert_eq!(t, PathBuf::from("dst").join("a.txt"));
    }

    #[test]
    fn target_dir_into_dir_appends_basename() {
        let t = compute_target(Path::new("src/sub"), Path::new("dst"), true);
        assert_eq!(t, PathBuf::from("dst").join("sub"));
    }

    /// Trailing slashes are decoration on the component, not part of it —
    /// `strip_trailing_slashes (arg_base)`.
    #[test]
    fn trailing_slashes_do_not_change_the_name() {
        for src in ["src/sub/", "src/sub///"] {
            let t = compute_target(Path::new(src), Path::new("dst"), true);
            assert_eq!(t, PathBuf::from("dst").join("sub"), "{src}");
        }
    }

    /// Module docs, bug 5, and the case the first fix broke. `.` is a component
    /// like any other, and appending it names the destination itself — which is
    /// what makes `cp -r a/. dst` the idiom for filling `dst` with `a`'s
    /// contents. `Path::file_name` answers `a` here, which is how the target
    /// came to be `dst/a`.
    #[test]
    fn a_dot_component_names_the_destination_itself() {
        for src in ["a/.", "a/./", ".", "./"] {
            let t = compute_target(Path::new(src), Path::new("dst"), true);
            assert_eq!(t, PathBuf::from("dst").join("."), "{src}");
        }
        assert_eq!(
            Path::new("a/.").file_name(),
            Some(std::ffi::OsStr::new("a")),
            "the normalising answer this rule must not use"
        );
    }

    /// `arg_base += STREQ (arg_base, "..")`: a last component of exactly `..`
    /// becomes `.`, so the copy never reaches the destination's parent.
    #[test]
    fn a_dotdot_component_becomes_a_dot() {
        for src in ["a/..", "..", "a/../"] {
            let t = compute_target(Path::new(src), Path::new("dst"), true);
            assert_eq!(t, PathBuf::from("dst").join("."), "{src}");
        }
    }

    /// The comparison is against the whole component, so a name that merely
    /// *begins* with two dots is an ordinary name.
    #[test]
    fn only_dotdot_itself_is_special() {
        for (src, base) in [("a/..x", "..x"), ("a/...", "..."), ("a/..", ".")] {
            let t = compute_target(Path::new(src), Path::new("dst"), true);
            assert_eq!(t, PathBuf::from("dst").join(base), "{src}");
        }
    }

    /// A root has no last component at all. `file_name_concat` appends the
    /// empty string, which is a separator and nothing else.
    #[test]
    fn a_root_source_appends_nothing() {
        let t = compute_target(Path::new("/"), Path::new("dst"), true);
        assert_eq!(t, PathBuf::from("dst").join(""));
    }

    #[test]
    fn a_source_with_no_file_name_is_fine_when_dest_is_not_a_dir() {
        let t = compute_target(Path::new("a/.."), Path::new("dst"), false);
        assert_eq!(t, PathBuf::from("dst"));
    }

    // ------------------------------------------------------------ copying --

    /// A private directory for one test, removed when the binding drops.
    ///
    /// This used to be a hand-rolled pid-and-counter helper — another copy of
    /// the thing `scratchdir` exists to replace, and the copy in which a second
    /// bug was still live. Its child paths came from `Path::join`, which uses
    /// the *host's* separator: `\` on this development box. Every path handed to
    /// the subject was therefore one component containing backslashes, which is
    /// not an input the target can produce — `/` is this OS's only separator and
    /// `\` is an ordinary byte in a filename.
    ///
    /// `cp -b`'s numbered-backup scan is what noticed. It derives the directory
    /// to read from the last separator in the name it is given, found none, and
    /// so scanned the process's current directory instead of the scratch one:
    /// it saw no `c.~1~` there and answered `c~` where `c.~2~` was correct. Two
    /// tests red on Windows and green on Linux, for a disagreement that was
    /// entirely the fixture's. See [`ScratchDir::path`], which appends `/`.
    fn scratch(stem: &str) -> ScratchDir {
        ScratchDir::new(&format!("cp_test_{stem}"))
    }

    /// Every option off — `cp a b` with nothing else given.
    ///
    /// The named sets below are each this with one or two fields changed, built
    /// with `..off()` rather than spelled out in full. That is not brevity for
    /// its own sake: written out, each set repeats every field, so adding an
    /// option to [`CpFlags`] means editing two dozen of them, and a reader
    /// cannot see at a glance which field a given set is *about*. With
    /// `..off()` the difference is the whole body.
    ///
    /// A function and not a `const`, which every one of these was until `-b`
    /// arrived. [`CpFlags::backup`] owns its suffix (a `Vec<u8>`, because the
    /// bytes come from `-S`'s argument), so `CpFlags` has a destructor — and
    /// `..off()` *drops* the fields it did not take, which a constant may not do
    /// at compile time. Calling a function per set costs an allocation nobody
    /// times and keeps the `..` shorthand that makes the sets readable.
    fn off() -> CpFlags {
        CpFlags {
            recursive: false,
            target_directory: None,
            no_target_directory: false,
            verbose: false,
            dereference: Deref::Undefined,
            interactive: Interactive::Unspecified,
            force: false,
            remove_destination: false,
            preserve: Preserve::NONE,
            explicit_no_preserve_mode: false,
            require_preserve: false,
            require_preserve_xattr: false,
            reduce_diagnostics: false,
            backup: backup::Backup::disabled(),
        }
    }
    fn plain() -> CpFlags {
        off()
    }
    fn recursive() -> CpFlags {
        CpFlags {
            recursive: true,
            ..off()
        }
    }
    /// `-T`, which the two above never set. Named for what it does rather than
    /// for the letter: the destination is a name, not a directory to fill.
    fn as_name() -> CpFlags {
        CpFlags {
            no_target_directory: true,
            ..off()
        }
    }
    fn verbose() -> CpFlags {
        CpFlags {
            verbose: true,
            ..off()
        }
    }
    fn verbose_recursive() -> CpFlags {
        CpFlags {
            recursive: true,
            verbose: true,
            ..off()
        }
    }
    // The three below are `#[cfg(unix)]` because every test that uses one has
    // to create a symlink to mean anything, and the development host cannot.
    // Without the gate they are dead code there and the build is not warning-
    // free. [`the_dereference_table`] needs no filesystem and so runs on both.
    /// `-P`: the link, not its target, with no `-r` to make that the default.
    #[cfg(unix)]
    fn no_deref() -> CpFlags {
        CpFlags {
            dereference: Deref::Never,
            ..off()
        }
    }
    /// `-Lr`: follow every link, including ones found inside the tree.
    #[cfg(unix)]
    fn deref_all_r() -> CpFlags {
        CpFlags {
            recursive: true,
            dereference: Deref::Always,
            ..off()
        }
    }
    /// `-Hr`: follow the operand, keep the links found underneath it. The one
    /// combination in which the two questions have different answers.
    #[cfg(unix)]
    fn deref_cmd_r() -> CpFlags {
        CpFlags {
            recursive: true,
            dereference: Deref::CommandLine,
            ..off()
        }
    }
    /// `-fv`. The overwrite sets are verbose because the whole difference
    /// between `-f` and `--remove-destination` is which line comes out first.
    /// `#[cfg(unix)]` for the same reason as the three above: every test that
    /// uses one needs either a symlink or a mode that denies.
    #[cfg(unix)]
    fn force_v() -> CpFlags {
        CpFlags {
            force: true,
            verbose: true,
            ..off()
        }
    }
    /// `--remove-destination -v`.
    #[cfg(unix)]
    fn remove_dest_v() -> CpFlags {
        CpFlags {
            remove_destination: true,
            verbose: true,
            ..off()
        }
    }
    /// `-nv`.
    fn no_clobber_v() -> CpFlags {
        CpFlags {
            interactive: Interactive::AlwaysNo,
            verbose: true,
            ..off()
        }
    }
    /// `-i`. Not verbose, unlike the three above: `-i`'s interesting output is
    /// the question, which goes to stderr, and a `-v` line on stdout would only
    /// be noise in the tests that assert stderr is *exactly* the question.
    fn ask() -> CpFlags {
        CpFlags {
            interactive: Interactive::AskUser,
            ..off()
        }
    }

    /// `copy_all` plus whatever it wrote to its error sink.
    ///
    /// The stdout half is dropped here rather than returned, because all but a
    /// handful of these tests do not set `-v` and so could only ever assert
    /// that it was empty. [`cp_out`] is the same call for the ones that care.
    fn cp(flags: &CpFlags, paths: &[&Path]) -> (bool, String) {
        let (ok, _out, err) = cp_out(flags, paths);
        (ok, err)
    }

    /// `copy_all` plus *both* of the things it wrote: `(ok, stdout, stderr)`.
    ///
    /// The two sinks are separate `Vec`s and not one, which is the point: a
    /// test that asserted on their concatenation could not tell a `--verbose`
    /// line on stdout from the same text misdirected to stderr, and getting
    /// that wrong is exactly the bug worth catching — GNU's is a `printf`.
    fn cp_out(flags: &CpFlags, paths: &[&Path]) -> (bool, String, String) {
        let (ok, out, err, _) = cp_answering(flags, paths, &[]);
        (ok, out, err)
    }

    /// The same call with a queue of canned answers for `-i`'s prompts, and the
    /// number of them consumed back: `(ok, stdout, stderr, prompts)`.
    ///
    /// The count is the fourth value because "did not ask" and "asked and was
    /// declined" are two different behaviours that leave the same file on disk
    /// and, for a declining answer, the same exit status. Only the count tells
    /// them apart. An empty queue is end of input, which is a decline — so a
    /// test that passes no answers is testing `cp -i </dev/null`.
    fn cp_answering(
        flags: &CpFlags,
        paths: &[&Path],
        answers: &[&str],
    ) -> (bool, String, String, usize) {
        let owned: Vec<OsString> = paths.iter().map(|p| p.as_os_str().to_owned()).collect();
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let mut canned = Canned::new(answers);
        let mut copied = Copied::default();
        let ok = {
            let mut job = Job {
                flags,
                copied: &mut copied,
                out: &mut out,
                err: &mut err,
                answers: &mut canned,
            };
            copy_all(&mut job, &owned)
        };
        (
            ok,
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
            canned.consumed(),
        )
    }

    #[test]
    fn copies_a_file() {
        let dir = scratch("file");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"hello").unwrap();
        let (ok, err) = cp(&plain(), &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert_eq!(fs::read(&a).unwrap(), b"hello", "the source stays");
        assert_eq!(fs::read(&b).unwrap(), b"hello");
    }

    #[test]
    fn copies_a_file_into_a_directory() {
        let dir = scratch("into_dir");
        let a = dir.path("a");
        let sub = dir.path("sub");
        fs::write(&a, b"x").unwrap();
        fs::create_dir(&sub).unwrap();
        let (ok, err) = cp(&plain(), &[&a, &sub]);
        assert!(ok, "{err}");
        assert!(sub.join("a").is_file());
    }

    // --------------------------------------------- which failure it reports --

    /// GNU names the errno rather than restating the option's requirement, and
    /// the two errnos read differently enough to matter: one says the name is
    /// missing, the other that it is the wrong kind of thing.
    #[test]
    fn the_target_diagnostic_names_the_reason() {
        let dir = scratch("target_why");
        let a = dir.path("a");
        let b = dir.path("b");
        let file = dir.path("plain");
        fs::write(&a, b"1").unwrap();
        fs::write(&b, b"2").unwrap();
        fs::write(&file, b"3").unwrap();

        let missing = dir.path("nosuch");
        let (ok, e) = cp(&plain(), &[&a, &b, &missing]);
        assert!(!ok);
        assert!(
            e.ends_with(": No such file or directory\n"),
            "a name that is not there: {e}"
        );

        let (ok, e) = cp(&plain(), &[&a, &b, &file]);
        assert!(!ok);
        assert!(
            e.ends_with(": Not a directory\n"),
            "a name that is something else: {e}"
        );
    }

    /// A destination whose *parent* is not a directory is a failed `stat`, not
    /// a failed create, and GNU says which. Reporting it at the create would
    /// name the wrong operation and, once `cp` grows `-i`, would do so after
    /// having already asked to overwrite something.
    #[cfg(unix)]
    #[test]
    fn a_destination_under_a_plain_file_fails_at_the_stat() {
        let dir = scratch("dst_stat");
        let a = dir.path("a");
        let blocking = dir.path("blocking");
        fs::write(&a, b"1").unwrap();
        fs::write(&blocking, b"2").unwrap();

        let under = blocking.join("under");
        let (ok, e) = cp(&plain(), &[&a, &under]);
        assert!(!ok);
        assert!(e.starts_with("cp: cannot stat "), "{e}");
        assert!(e.ends_with(": Not a directory\n"), "{e}");
    }

    /// Under `-r` a symlink operand is copied as a link, so its own inode is
    /// what a naive comparison sees — and that inode is never the destination's.
    /// GNU resolves both sides unless *both* are links, which is what makes
    /// `cp -r link file` a refusal where `link` points at `file`.
    #[cfg(unix)]
    #[test]
    fn a_symlink_operand_resolving_to_the_destination_is_refused() {
        let dir = scratch("link_same");
        let file = dir.path("file");
        let link = dir.path("link");
        fs::write(&file, b"kept").unwrap();
        std::os::unix::fs::symlink("file", &link).unwrap();

        let (ok, e) = cp(&recursive(), &[&link, &file]);
        assert!(!ok);
        assert!(e.contains("are the same file"), "{e}");
        assert_eq!(fs::read(&file).unwrap(), b"kept");
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    /// The other half of that rule: two *distinct* links to one file are not
    /// the same file, because replacing one with a copy of the other leaves
    /// what they point at alone.
    #[cfg(unix)]
    #[test]
    fn two_symlinks_to_one_file_are_not_the_same_file() {
        let dir = scratch("two_links");
        let file = dir.path("file");
        let one = dir.path("one");
        let two = dir.path("two");
        fs::write(&file, b"kept").unwrap();
        std::os::unix::fs::symlink("file", &one).unwrap();
        std::os::unix::fs::symlink("file", &two).unwrap();

        let (ok, e) = cp(&recursive(), &[&one, &two]);
        assert!(ok, "{e}");
        assert_eq!(fs::read(&file).unwrap(), b"kept", "the target is untouched");
    }

    // ------------------------------------------- one operand against another --
    //
    // Three refusals that no single operand can be judged by: what is wrong is
    // the *pair*, and only a record of what the command already wrote can see
    // it. The data-loss one is `cp a other/a d` — without the guard, `d/a`
    // ends up holding `other/a` and the copy of `a` the user asked for is gone
    // with nothing printed.

    #[test]
    fn a_second_source_will_not_overwrite_the_copy_the_first_just_made() {
        let dir = scratch("just_created");
        let other = dir.path("other");
        let dest = dir.path("dest");
        fs::create_dir(&other).unwrap();
        fs::create_dir(&dest).unwrap();
        let first = dir.path("f");
        let second = other.join("f");
        fs::write(&first, b"first").unwrap();
        fs::write(&second, b"second").unwrap();

        let (ok, e) = cp(&plain(), &[&first, &second, &dest]);
        assert!(!ok, "the pair must count against the exit status");
        // Not asserted against a whole quoted path: the scratch directory is
        // absolute and its spelling differs by host.
        assert!(e.contains("will not overwrite just-created"), "{e}");
        assert_eq!(
            fs::read(dest.join("f")).unwrap(),
            b"first",
            "the copy that was asked for first survives"
        );
    }

    /// The same guard, for the case the first cannot see. A regular source
    /// stats its destination *followed*, so a destination that is a symlink
    /// this command just made compares as whatever it points at; without a
    /// second look at the link itself the copy goes through it.
    #[cfg(unix)]
    #[test]
    fn a_second_source_will_not_be_written_through_a_just_created_symlink() {
        let dir = scratch("through_link");
        let other = dir.path("other");
        let dest = dir.path("dest");
        fs::create_dir(&other).unwrap();
        fs::create_dir(&dest).unwrap();
        let pointee = dir.path("pointee");
        fs::write(&pointee, b"untouched").unwrap();
        let link = dir.path("l");
        std::os::unix::fs::symlink(&pointee, &link).unwrap();
        let plain = other.join("l");
        fs::write(&plain, b"second").unwrap();

        let (ok, e) = cp(&recursive(), &[&link, &plain, &dest]);
        assert!(!ok, "{e}");
        assert!(e.contains("through just-created symlink"), "{e}");
        assert_eq!(
            fs::read(&pointee).unwrap(),
            b"untouched",
            "what the link points at is not written to"
        );
    }

    #[test]
    fn one_source_named_twice_is_a_warning_and_not_a_failure() {
        let dir = scratch("named_twice");
        let dest = dir.path("dest");
        fs::create_dir(&dest).unwrap();
        let f = dir.path("f");
        fs::write(&f, b"body").unwrap();
        let dotted = dir.path(".").join("f");

        let (ok, e) = cp(&plain(), &[&f, &dotted, &dest]);
        assert!(ok, "a repeat is not an error: {e}");
        assert!(e.contains("specified more than once"), "{e}");
        assert_eq!(fs::read(dest.join("f")).unwrap(), b"body");
    }

    /// One directory named twice but landing in *two* places. The user has
    /// asked for one inode to appear twice in the destination tree, which for a
    /// directory could only be done by hard-linking it, and GNU refuses rather
    /// than making a second copy. `src/.` and `src` are the same directory
    /// reached by two spellings whose targets differ — `dest/.` and `dest/src`
    /// — which is what makes it reachable at all without hard-linked
    /// directories to hand.
    #[test]
    fn one_directory_going_to_two_places_will_not_be_hard_linked() {
        let dir = scratch("two_places");
        let dest = dir.path("dest");
        let src = dir.path("src");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::create_dir(&dest).unwrap();

        let (ok, e) = cp(&recursive(), &[&src.join("."), &src, &dest]);
        assert!(!ok, "{e}");
        assert!(e.contains("will not create hard link"), "{e}");
        // The first spelling was copied; only the second is refused.
        assert!(dest.join("sub").is_dir(), "{e}");
        assert!(!dest.join("src").exists(), "{e}");
    }

    /// The same two destinations are *not* refused when a dereference option
    /// asked for them: `cp -RL a b d`, with `a` and `b` links to one directory,
    /// is a request for two independent copies (`copy.c:2723`).
    #[test]
    #[cfg(unix)]
    fn following_links_makes_two_copies_of_one_directory_instead() {
        let dir = scratch("two_copies");
        let dest = dir.path("dest");
        let real = dir.path("real");
        fs::create_dir(&dest).unwrap();
        fs::create_dir(&real).unwrap();
        fs::write(real.join("f"), b"body").unwrap();
        std::os::unix::fs::symlink(&real, dir.path("a")).unwrap();
        std::os::unix::fs::symlink(&real, dir.path("b")).unwrap();

        let flags = CpFlags {
            recursive: true,
            dereference: Deref::Always,
            ..CpFlags::default()
        };
        let (ok, e) = cp(&flags, &[&dir.path("a"), &dir.path("b"), &dest]);
        assert!(ok, "{e}");
        assert_eq!(fs::read(dest.join("a").join("f")).unwrap(), b"body");
        assert_eq!(fs::read(dest.join("b").join("f")).unwrap(), b"body");
    }

    /// Builds `parent/{child/{f},top}` under a fresh scratch directory with an
    /// empty `dest`, and hands back the three paths the walked-repeat tests
    /// name. The shape is the smallest one where an operand and a *walk* reach
    /// one directory: `parent/child` is copied by name, and then `parent` is
    /// walked into and offers the same directory a second time.
    /// The scratch directory comes back first because it is a *guard*: dropping
    /// it removes the tree, so a caller that discarded it would be left holding
    /// three paths into a directory that no longer exists.
    fn nested_pair(tag: &str) -> (ScratchDir, PathBuf, PathBuf) {
        let dir = scratch(tag);
        let parent = dir.path("parent");
        let child = dir.path("parent/child");
        fs::create_dir_all(&child).unwrap();
        fs::write(dir.path("parent/child/f"), b"body").unwrap();
        fs::write(dir.path("parent/top"), b"top").unwrap();
        let dest = dir.path("dest");
        fs::create_dir(&dest).unwrap();
        (dir, parent, dest)
    }

    /// The bug this table's merge was for. `cp -r parent/child parent dest`
    /// copies `parent/child` to `dest/child`, then walks `parent` and finds
    /// that same directory again. Before the merge the walk consulted no
    /// record at all and copied the subtree a second time, exiting 0; GNU
    /// refuses the repeat and exits 1. See [`Copied`].
    #[test]
    fn a_directory_reached_by_walking_is_refused_a_second_time() {
        let (_dir, parent, dest) = nested_pair("walked_repeat");

        let (ok, e) = cp(&recursive(), &[&parent.join("child"), &parent, &dest]);
        assert!(!ok, "{e}");
        assert!(e.contains("will not create hard link"), "{e}");
        assert!(
            e.contains("to directory"),
            "the refusal names where the inode landed first: {e}"
        );
        assert_eq!(fs::read(dest.join("child").join("f")).unwrap(), b"body");
        assert!(
            !dest.join("parent").join("child").exists(),
            "the repeat is refused, not copied: {e}"
        );
        // GNU's `copy_dir` does not stop at a failed entry, so the sibling
        // after the refused one is still copied. Measured: its `-v` prints
        // `'parent/top' -> 'dest/parent/top'` after the diagnostic.
        assert_eq!(
            fs::read(dest.join("parent").join("top")).unwrap(),
            b"top",
            "the walk carries on past the refusal: {e}"
        );
    }

    /// `-L` is the one answer that makes the same two paths legitimate: it asks
    /// for every name to be followed, so one directory reached twice is two
    /// independent copies of it and is made silently. GNU's third arm
    /// (`copy.c:2723`) with `command_line_arg` false.
    #[test]
    fn following_links_lets_the_walk_copy_a_directory_twice() {
        let (_dir, parent, dest) = nested_pair("walked_repeat_L");

        let flags = CpFlags {
            dereference: Deref::Always,
            ..recursive()
        };
        let (ok, e) = cp(&flags, &[&parent.join("child"), &parent, &dest]);
        assert!(ok, "{e}");
        assert_eq!(fs::read(dest.join("child").join("f")).unwrap(), b"body");
        assert_eq!(
            fs::read(dest.join("parent").join("child").join("f")).unwrap(),
            b"body"
        );
    }

    /// The same two operands the other way round, which is *not* a repeat: the
    /// walk reaches `parent/child` first, and a directory found by walking is
    /// looked up and never recorded, so the operand that names it afterwards
    /// finds nothing in the table. Recording walked directories — the obvious
    /// simplification of [`Copied::lookup`] into [`Copied::remember`] — turns
    /// this into a spurious refusal. Measured against GNU: both trees land and
    /// the status is 0.
    #[test]
    fn a_walk_that_arrives_first_does_not_refuse_the_operand() {
        let (_dir, parent, dest) = nested_pair("walk_then_operand");

        let (ok, e) = cp(&recursive(), &[&parent, &parent.join("child"), &dest]);
        assert!(ok, "{e}");
        assert_eq!(
            fs::read(dest.join("parent").join("child").join("f")).unwrap(),
            b"body"
        );
        assert_eq!(fs::read(dest.join("child").join("f")).unwrap(), b"body");
    }

    // ------------------------------------------- where the destination is --

    /// `-t` with the destination named by the option, and — the part that has
    /// no equivalent without it — a *single* operand still going inside the
    /// directory rather than being taken for the destination.
    #[test]
    fn a_target_directory_takes_every_operand_as_a_source() {
        let dir = scratch("t_dest");
        let dest = dir.path("dest");
        fs::create_dir(&dest).unwrap();
        let a = dir.path("a");
        fs::write(&a, b"A").unwrap();

        let flags = CpFlags {
            target_directory: Some(dest.clone().into_os_string()),
            ..plain()
        };
        let (ok, e) = cp(&flags, &[&a]);
        assert!(ok, "{e}");
        assert_eq!(e, "");
        assert_eq!(fs::read(dest.join("a")).unwrap(), b"A");
    }

    /// The wording is `target directory`, not the bare `target` the last
    /// operand gets: the user named this one as a directory, so the diagnostic
    /// says which claim failed.
    #[test]
    fn a_target_directory_that_is_not_one_says_so() {
        let dir = scratch("t_notdir");
        let not_a_dir = dir.path("plain");
        fs::write(&not_a_dir, b"x").unwrap();
        let a = dir.path("a");
        fs::write(&a, b"A").unwrap();

        let flags = CpFlags {
            target_directory: Some(not_a_dir.clone().into_os_string()),
            ..off()
        };
        let (ok, e) = cp(&flags, &[&a]);
        assert!(!ok);
        assert!(e.starts_with("cp: target directory "), "{e}");
        assert!(e.contains("Not a directory"), "{e}");
    }

    /// Checked before `-t`'s directory is looked at, which is GNU's order and
    /// is visible: the directory here does not exist, and the combination is
    /// still what gets reported.
    #[test]
    fn the_two_destination_options_cannot_be_combined() {
        let flags = CpFlags {
            target_directory: Some(OsString::from("nosuch")),
            no_target_directory: true,
            ..off()
        };
        let (ok, e) = cp(&flags, &[Path::new("a"), Path::new("b")]);
        assert!(!ok);
        assert_eq!(
            e,
            "cp: cannot combine --target-directory (-t) and --no-target-directory (-T)\n"
        );
    }

    /// Without `-T` this would be `cp a b dir` and put both inside `dir`. With
    /// it the destination is one name, so the third operand has nowhere to go.
    #[test]
    fn a_third_operand_has_nowhere_to_go_under_no_target_directory() {
        let (ok, e) = cp(
            &as_name(),
            &[Path::new("a"), Path::new("b"), Path::new("c")],
        );
        assert!(!ok);
        assert!(e.starts_with("cp: extra operand "), "{e}");
        assert!(e.contains("'c'"), "{e}");
    }

    /// The whole point of `-T`: a destination that *is* a directory is not
    /// somewhere to copy into, so the copy is refused rather than silently
    /// landing one level down.
    #[test]
    fn no_target_directory_will_not_copy_into_a_directory() {
        let dir = scratch("cap_T");
        let a = dir.path("a");
        fs::write(&a, b"A").unwrap();
        let d = dir.path("d");
        fs::create_dir(&d).unwrap();

        let (ok, e) = cp(&as_name(), &[&a, &d]);
        assert!(!ok);
        assert!(e.contains("cannot overwrite directory"), "{e}");
        assert!(!d.join("a").exists(), "nothing went inside it");
    }

    /// And the same destination without `-T`, so the test above is pinning the
    /// flag rather than a refusal that was there anyway.
    #[test]
    fn without_it_the_same_destination_is_copied_into() {
        let dir = scratch("no_cap_T");
        let a = dir.path("a");
        fs::write(&a, b"A").unwrap();
        let d = dir.path("d");
        fs::create_dir(&d).unwrap();

        let (ok, e) = cp(&plain(), &[&a, &d]);
        assert!(ok, "{e}");
        assert_eq!(fs::read(d.join("a")).unwrap(), b"A");
    }

    /// `cp -rT src dst` is how a tree is copied *onto* another rather than
    /// inside it — the one thing plain `cp -r` cannot express once `dst`
    /// exists.
    #[test]
    fn recursive_no_target_directory_copies_a_tree_onto_the_destination() {
        let dir = scratch("rT");
        let src = dir.path("src");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("x"), b"X").unwrap();
        let dst = dir.path("dst");
        fs::create_dir(&dst).unwrap();
        fs::write(dst.join("keep"), b"K").unwrap();

        let flags = CpFlags {
            recursive: true,
            ..as_name()
        };
        let (ok, e) = cp(&flags, &[&src, &dst]);
        assert!(ok, "{e}");
        assert_eq!(fs::read(dst.join("x")).unwrap(), b"X");
        assert!(!dst.join("src").exists(), "not one level down");
        assert_eq!(
            fs::read(dst.join("keep")).unwrap(),
            b"K",
            "a merge, not a replacement"
        );
    }

    /// The repeat tables count *sources*, and under `-t` every operand is one —
    /// so two operands is two sources even though there are only two words.
    #[test]
    fn a_target_directory_still_notices_a_source_named_twice() {
        let dir = scratch("t_twice");
        let dest = dir.path("dest");
        fs::create_dir(&dest).unwrap();
        let f = dir.path("f");
        fs::write(&f, b"body").unwrap();
        let dotted = dir.path(".").join("f");

        let flags = CpFlags {
            target_directory: Some(dest.clone().into_os_string()),
            ..plain()
        };
        let (ok, e) = cp(&flags, &[&f, &dotted]);
        assert!(ok, "{e}");
        assert!(e.contains("specified more than once"), "{e}");
    }

    // ----------------------------------------------------- --verbose says --

    /// The whole of `-v` on one file: the arrow line, on **stdout**, and
    /// nothing on stderr. Both halves are asserted, because the sink is the
    /// half that is easy to get wrong and impossible to see once the two are
    /// merged into a terminal.
    #[test]
    fn verbose_names_the_copy_on_stdout() {
        let dir = scratch("v_one");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"hello").unwrap();

        let (ok, out, err) = cp_out(&verbose(), &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(err, "", "a report of work done is not a diagnostic");
        assert_eq!(out, format!("{} -> {}\n", quoteaf_os(&a), quoteaf_os(&b)));
    }

    /// And without it, silence — so the test above is pinning the option and
    /// not merely observing that `cp` talks.
    #[test]
    fn without_verbose_a_copy_says_nothing() {
        let dir = scratch("v_off");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"hello").unwrap();

        let (ok, out, err) = cp_out(&plain(), &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(out, "");
        assert_eq!(err, "");
    }

    /// GNU announces *before* it opens the source, so a copy that fails is
    /// still announced. This is the case that decides whether `-v` reports
    /// attempts or successes, and upstream's answer is attempts.
    #[test]
    fn verbose_announces_a_copy_that_then_fails() {
        let dir = scratch("v_fail");
        let missing = dir.path("nosuch").join("a");
        let b = dir.path("b");

        // The failure is `cannot stat`, from before the announcement — so this
        // one is *not* announced, which is the other half of the rule.
        let (ok, out, err) = cp_out(&verbose(), &[&missing, &b]);
        assert!(!ok);
        assert!(err.contains("cannot stat"), "{err}");
        assert_eq!(out, "", "a source that could not be stat'd is not a copy");

        // Whereas a source that stats and then cannot be *written* is: the
        // destination here is a directory that `cp` without `-r` will not
        // overwrite, and the refusal comes from `copy_one`, still before the
        // announcement.
        let a = dir.path("a");
        fs::write(&a, b"x").unwrap();
        let d = dir.path("d");
        fs::create_dir(&d).unwrap();
        let onto = d.join("a");
        fs::create_dir(&onto).unwrap();
        let (ok, out, err) = cp_out(&verbose(), &[&a, &d]);
        assert!(!ok);
        assert!(err.contains("cannot overwrite directory"), "{err}");
        assert_eq!(out, "");
    }

    /// A tree names the directory and then everything in it, and the directory
    /// line comes first because the `mkdir` does.
    #[test]
    fn verbose_names_a_created_directory_and_then_its_contents() {
        let dir = scratch("v_tree");
        let src = dir.path("src");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("f"), b"F").unwrap();
        let dst = dir.path("dst");

        let (ok, out, err) = cp_out(&verbose_recursive(), &[&src, &dst]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(
            lines,
            vec![
                format!("{} -> {}", quoteaf_os(&src), quoteaf_os(&dst)),
                format!(
                    "{} -> {}",
                    quoteaf_os(src.join("f")),
                    quoteaf_os(dst.join("f"))
                ),
            ],
            "{out}"
        );
    }

    /// The rule GNU wrote a comment to explain: a directory is announced only
    /// when it is *created*. Copying the same tree a second time refreshes the
    /// files and reuses the directory, so the second run names the files alone.
    #[test]
    fn verbose_is_silent_about_a_directory_that_was_already_there() {
        let dir = scratch("v_again");
        let src = dir.path("src");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("f"), b"F").unwrap();
        let dst = dir.path("dst");
        fs::create_dir(&dst).unwrap();
        fs::create_dir(dst.join("src")).unwrap();

        let (ok, out, err) = cp_out(&verbose_recursive(), &[&src, &dst]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert_eq!(
            out,
            format!(
                "{} -> {}\n",
                quoteaf_os(src.join("f")),
                quoteaf_os(dst.join("src").join("f"))
            ),
            "the directory was reused, so only the file was copied"
        );
    }

    /// A name with a space in it is quoted, which is the reason the line uses
    /// the diagnostic quoting style at all: without it, `a b -> c` could be one
    /// copy or the tail of a different one.
    ///
    /// Asserted by splitting the line on its arrow rather than by rebuilding it
    /// with [`quoteaf_os`], which is what the two tests above do: rebuilding it
    /// would agree with any style at all, including one that never quotes. The
    /// question here is whether the space in the source's name reached the
    /// reader as a quoted name or as two words, and only reading the halves
    /// apart can answer it.
    #[test]
    fn verbose_quotes_a_name_that_needs_it() {
        let dir = scratch("v_quote");
        let a = dir.path("a b");
        let c = dir.path("c");
        fs::write(&a, b"x").unwrap();

        let (ok, out, err) = cp_out(&verbose(), &[&a, &c]);
        assert!(ok, "{err}");
        let line = out.strip_suffix('\n').unwrap_or(&out);
        let (rendered_src, _) = line.rsplit_once(" -> ").unwrap_or((line, ""));
        assert!(rendered_src.starts_with('\''), "{rendered_src}");
        assert!(rendered_src.ends_with("a b'"), "{rendered_src}");
    }

    // -------------------------------------- -P / -H / -L: links or targets --

    /// The whole of `cp.c:1239` and `copy.c:845` as a table, with no
    /// filesystem in the way. Every row is a command line; the two columns are
    /// the only two questions the rest of the program ever asks.
    ///
    /// The two rows worth staring at are the `-H` ones — they are the only
    /// place the columns disagree, and a single `follow: bool` could not
    /// express them at all.
    #[test]
    fn the_dereference_table() {
        let rows: &[(bool, Deref, bool, bool)] = &[
            // recursive, given,               operand, walked
            (false, Deref::Undefined, true, true),
            (true, Deref::Undefined, false, false),
            (false, Deref::Never, false, false),
            (true, Deref::Never, false, false),
            (false, Deref::Always, true, true),
            (true, Deref::Always, true, true),
            (false, Deref::CommandLine, true, false),
            (true, Deref::CommandLine, true, false),
        ];
        for &(recursive, dereference, operand, walked) in rows {
            let flags = CpFlags {
                recursive,
                dereference,
                ..plain()
            };
            assert_eq!(
                (flags.follow_operand(), flags.follow_walked()),
                (operand, walked),
                "{recursive} {dereference:?}"
            );
        }
    }

    /// `-P` alone. Without it, `cp link dst` writes a *file*; the point of the
    /// option is to get `-r`'s behaviour without `-r`.
    #[cfg(unix)]
    #[test]
    fn no_dereference_copies_the_link_without_r() {
        let dir = scratch("P_link");
        fs::write(dir.path("file"), b"BODY").unwrap();
        let link = dir.path("link");
        std::os::unix::fs::symlink("file", &link).unwrap();
        let dst = dir.path("dst");

        let (ok, e) = cp(&no_deref(), &[&link, &dst]);
        assert!(ok, "{e}");
        let meta = fs::symlink_metadata(&dst).unwrap();
        assert!(meta.file_type().is_symlink(), "a link, not its target");
        assert_eq!(fs::read_link(&dst).unwrap(), PathBuf::from("file"));
    }

    /// The same-file guard keys on the policy and not on `-r`, which is what
    /// changed when `-P` arrived: two distinct links to one file are two
    /// distinct things to copy, so this is allowed. Under [`PLAIN`] — where
    /// both are followed — the identical command is refused, and the test
    /// above this one in the file pins that half.
    #[cfg(unix)]
    #[test]
    fn no_dereference_lets_one_link_replace_another_without_r() {
        let dir = scratch("P_two_links");
        fs::write(dir.path("file"), b"BODY").unwrap();
        let (one, two) = (dir.path("one"), dir.path("two"));
        std::os::unix::fs::symlink("file", &one).unwrap();
        std::os::unix::fs::symlink("file", &two).unwrap();

        let (ok, e) = cp(&no_deref(), &[&one, &two]);
        assert!(ok, "{e}");
        assert_eq!(e, "");
        let (ok, e) = cp(&plain(), &[&one, &two]);
        assert!(!ok, "followed, they are one file");
        assert!(e.contains("are the same file"), "{e}");
    }

    /// `-L` reaches *inside* the tree, which is the half no option could
    /// express before [`Job`] carried the flags into the recursion: the link
    /// to a file becomes a file, and the link to a directory becomes a
    /// directory with the contents copied again.
    #[cfg(unix)]
    #[test]
    fn dereference_follows_links_found_by_the_walk() {
        let dir = scratch("L_walk");
        let src = dir.path("t");
        fs::create_dir(&src).unwrap();
        fs::create_dir(src.join("sub")).unwrap();
        fs::write(src.join("a.txt"), b"A").unwrap();
        fs::write(src.join("sub/s.txt"), b"S").unwrap();
        std::os::unix::fs::symlink("a.txt", src.join("flink")).unwrap();
        std::os::unix::fs::symlink("sub", src.join("dlink")).unwrap();
        let dst = dir.path("d");

        let (ok, e) = cp(&deref_all_r(), &[&src, &dst]);
        assert!(ok, "{e}");
        assert!(
            !fs::symlink_metadata(dst.join("flink"))
                .unwrap()
                .file_type()
                .is_symlink(),
            "the link to a file became a file"
        );
        assert_eq!(fs::read(dst.join("flink")).unwrap(), b"A");
        assert!(
            dst.join("dlink").is_dir()
                && !fs::symlink_metadata(dst.join("dlink"))
                    .unwrap()
                    .file_type()
                    .is_symlink(),
            "the link to a directory became a directory"
        );
        assert_eq!(fs::read(dst.join("dlink/s.txt")).unwrap(), b"S");
    }

    /// A link that points at nothing has no target to copy, so `-L` fails on
    /// it where the default would have copied the link. GNU's wording, from
    /// the same `stat` this one comes from: `cannot stat 't/dangle'`.
    #[cfg(unix)]
    #[test]
    fn dereference_fails_on_a_dangling_link_in_the_tree() {
        let dir = scratch("L_dangle");
        let src = dir.path("t");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("a.txt"), b"A").unwrap();
        std::os::unix::fs::symlink("nowhere", src.join("dangle")).unwrap();
        let dst = dir.path("d");

        let (ok, e) = cp(&deref_all_r(), &[&src, &dst]);
        assert!(!ok, "one entry failed, so the copy failed");
        assert!(e.contains("cannot stat "), "{e}");
        assert!(e.contains("dangle"), "{e}");
        // The rest of the directory is still copied: one bad entry ends that
        // entry, not the walk.
        assert_eq!(fs::read(dst.join("a.txt")).unwrap(), b"A");
    }

    /// `-H` is the split one: the operand is followed, so a link to a
    /// directory is descended into, and every link *found* down there is
    /// copied as a link — including a dangling one, which `-L` could not have
    /// copied at all.
    #[cfg(unix)]
    #[test]
    fn command_line_dereference_follows_only_the_operand() {
        let dir = scratch("H_split");
        let src = dir.path("t");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("a.txt"), b"A").unwrap();
        std::os::unix::fs::symlink("a.txt", src.join("flink")).unwrap();
        std::os::unix::fs::symlink("nowhere", src.join("dangle")).unwrap();
        let dlink = dir.path("dlink");
        std::os::unix::fs::symlink("t", &dlink).unwrap();
        let dst = dir.path("d");

        let (ok, e) = cp(&deref_cmd_r(), &[&dlink, &dst]);
        assert!(ok, "{e}");
        assert!(
            dst.is_dir() && !fs::symlink_metadata(&dst).unwrap().file_type().is_symlink(),
            "the operand was followed"
        );
        assert_eq!(fs::read(dst.join("a.txt")).unwrap(), b"A");
        for name in ["flink", "dangle"] {
            assert!(
                fs::symlink_metadata(dst.join(name))
                    .unwrap()
                    .file_type()
                    .is_symlink(),
                "{name} was found by the walk, so it stays a link"
            );
        }
    }

    /// Replacing a link says two things, in this order: the removal and then
    /// the copy. The removal line exists because there is no atomic "replace"
    /// for a symlink — and it is the only case where `cp` unlinks anything, so
    /// a regular file overwritten in place says nothing extra.
    #[cfg(unix)]
    #[test]
    fn verbose_names_the_link_it_removed_first() {
        let dir = scratch("P_removed");
        fs::write(dir.path("file"), b"BODY").unwrap();
        let (one, two) = (dir.path("one"), dir.path("two"));
        std::os::unix::fs::symlink("file", &one).unwrap();
        std::os::unix::fs::symlink("file", &two).unwrap();

        let flags = CpFlags {
            verbose: true,
            ..no_deref()
        };
        let (ok, out, err) = cp_out(&flags, &[&one, &two]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2, "{out}");
        assert!(lines[0].starts_with("removed "), "{out}");
        assert!(lines[0].contains("two"), "{out}");
        assert!(lines[1].contains(" -> "), "{out}");

        // Nothing to remove, nothing said.
        let three = dir.path("three");
        let (ok, out, err) = cp_out(&flags, &[&one, &three]);
        assert!(ok, "{err}");
        assert_eq!(out.lines().count(), 1, "{out}");
    }

    /// Three options that set one field, so the last one given wins and there
    /// is no diagnostic for giving two. Measured against GNU: `cp -LP link d`
    /// writes a link and `cp -PL link d` writes a file.
    #[test]
    fn the_last_dereference_option_wins() {
        for (argv, want) in [
            (["-P", "-L"], Deref::Always),
            (["-L", "-P"], Deref::Never),
            (["-H", "-P"], Deref::Never),
            (["-P", "-H"], Deref::CommandLine),
        ] {
            let (flags, _) = run_parse(&[argv[0], argv[1], "a", "b"]);
            assert_eq!(flags.dereference, want, "{argv:?}");
        }
    }

    /// The long spellings, and the fact that `-H` has none — GNU gives it no
    /// entry in `long_opts[]`, so `--H` is not an option at all.
    #[test]
    fn the_dereference_long_spellings() {
        let (flags, _) = run_parse(&["--dereference", "a", "b"]);
        assert_eq!(flags.dereference, Deref::Always);
        let (flags, _) = run_parse(&["--no-dereference", "a", "b"]);
        assert_eq!(flags.dereference, Deref::Never);
    }

    // -------------------------------------------- the four overwrite policies --

    /// Read the whole file, as bytes, for a test that cares what landed there.
    fn body(p: &Path) -> Vec<u8> {
        fs::read(p).unwrap_or_else(|e| panic!("reading {}: {e}", p.display()))
    }

    /// True when a mode of 0400 would deny nothing, which is the condition the
    /// `-f` tests need and cannot create. `cp-diff.sh`'s section 16 guards its
    /// own 0400 cases with the same question, spelled `[ "$(id -u)" -ne 0 ]`.
    #[cfg(unix)]
    fn root() -> bool {
        unsafe extern "C" {
            fn geteuid() -> u32;
        }
        // SAFETY: `geteuid` takes no arguments, dereferences nothing, and
        // cannot fail — POSIX gives it no error return.
        unsafe { geteuid() == 0 }
    }

    /// A destination with a second name, for telling "truncated in place" from
    /// "unlinked and recreated" — which is the whole of the difference between
    /// `-f` and `--remove-destination`, and is invisible in the destination's
    /// own contents because both end up holding the source's bytes.
    ///
    /// Returns `(destination, witness)`, two names for one file holding
    /// `BBBB`. Afterwards the witness answers the question: it still holds
    /// `BBBB` if the file was replaced, and holds the source's bytes if it was
    /// written through.
    ///
    /// The obvious test — compare the inode number before and after — is
    /// **wrong**, and was written that way first and failed roughly half the
    /// time on tmpfs: a filesystem is free to hand the just-freed inode number
    /// straight back to the file created a microsecond later. It fails as a
    /// proof of replacement and, in the `-f` direction, as a proof of
    /// non-replacement. A second link cannot be faked either way.
    #[cfg(unix)]
    fn linked_destination(dir: &ScratchDir) -> (PathBuf, PathBuf) {
        let dst = dir.path("b");
        let witness = dir.path("witness");
        fs::write(&dst, b"BBBB").unwrap();
        fs::hard_link(&dst, &witness).unwrap();
        (dst, witness)
    }

    /// `-f` on a destination that opens for writing is a no-op: the same
    /// truncate-in-place that plain `cp` does, with no `removed` line.
    #[cfg(unix)]
    #[test]
    fn force_does_not_unlink_a_destination_that_opens() {
        let dir = scratch("force_noop");
        let a = dir.path("a");
        fs::write(&a, b"A").unwrap();
        let (b, witness) = linked_destination(&dir);

        let (ok, out, err) = cp_out(&force_v(), &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert_eq!(out, format!("{} -> {}\n", quoteaf_os(&a), quoteaf_os(&b)));
        assert_eq!(body(&b), b"A", "truncated, not appended to");
        assert_eq!(
            body(&witness),
            b"A",
            "-f must not replace a destination it could simply write to"
        );
    }

    /// `--remove-destination` on that same destination *does* unlink it, which
    /// is the entire difference between the two options.
    #[cfg(unix)]
    #[test]
    fn remove_destination_unlinks_a_destination_that_opens() {
        let dir = scratch("rmdest_noop");
        let a = dir.path("a");
        fs::write(&a, b"A").unwrap();
        let (b, witness) = linked_destination(&dir);

        let (ok, out, err) = cp_out(&remove_dest_v(), &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert_eq!(body(&b), b"A");
        assert_eq!(body(&witness), b"BBBB", "the old file is still there");
        // And the order of the two lines: `removed` first, because the removal
        // is the first thing the option does rather than a recovery from a
        // failure already under way. `-f` prints them the other way round.
        assert_eq!(
            out,
            format!(
                "removed {}\n{} -> {}\n",
                quoteaf_os(&b),
                quoteaf_os(&a),
                quoteaf_os(&b)
            )
        );
    }

    /// The two verbose lines in the order `-f` puts them, on the one
    /// destination that actually makes `-f` do something: mode 0400, so the
    /// `O_WRONLY` open fails with `EACCES` and the unlink is the retry.
    ///
    /// Skipped for a root copier, for whom 0400 denies nothing — the same
    /// guard `cp-diff.sh`'s section 16 uses.
    #[cfg(unix)]
    #[test]
    fn force_unlinks_only_after_the_open_has_failed() {
        use std::os::unix::fs::PermissionsExt as _;

        if root() {
            return;
        }

        let dir = scratch("force_ro");
        let a = dir.path("a");
        fs::write(&a, b"A").unwrap();
        let (ro, witness) = linked_destination(&dir);
        fs::set_permissions(&ro, fs::Permissions::from_mode(0o400)).unwrap();

        // Without `-f` it is a plain failure and the destination is untouched.
        let (ok, err) = cp(&verbose(), &[&a, &ro]);
        assert!(!ok);
        assert!(err.contains("cannot create regular file"), "{err}");
        assert_eq!(body(&ro), b"BBBB");

        let (ok, out, err) = cp_out(&force_v(), &[&a, &ro]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert_eq!(
            out,
            format!(
                "{} -> {}\nremoved {}\n",
                quoteaf_os(&a),
                quoteaf_os(&ro),
                quoteaf_os(&ro)
            ),
            "the arrow is printed before the removal, not after"
        );
        assert_eq!(body(&ro), b"A");
        assert_eq!(
            body(&witness),
            b"BBBB",
            "the 0400 file was unlinked, not somehow written to"
        );
    }

    /// "Force" does not mean force: a dangling symlink is refused with `-f`
    /// exactly as without it. The refusal comes from the *create* arm, and `-f`
    /// retries by creating, so it retries into the same wall.
    #[cfg(unix)]
    #[test]
    fn force_does_not_write_through_a_dangling_symlink() {
        let dir = scratch("force_dangling");
        let a = dir.path("a");
        let dang = dir.path("dang");
        fs::write(&a, b"A").unwrap();
        std::os::unix::fs::symlink("nowhere", &dang).unwrap();

        for flags in [&verbose(), &force_v()] {
            let (ok, out, err) = cp_out(flags, &[&a, &dang]);
            assert!(!ok, "{out}");
            assert!(
                err.contains("not writing through dangling symlink"),
                "{err}"
            );
            // Announced and then refused: `-v` reports attempts, and the
            // refusal comes from below the announcement. See
            // [`verbose_announces_a_copy_that_then_fails`].
            assert_eq!(
                out,
                format!("{} -> {}\n", quoteaf_os(&a), quoteaf_os(&dang))
            );
            assert!(
                fs::symlink_metadata(&dang)
                    .unwrap()
                    .file_type()
                    .is_symlink(),
                "the link is still a link"
            );
            assert!(!dir.path("nowhere").exists(), "and still points at nothing");
        }

        // `--remove-destination` is the option that gets past it, because it
        // never asks whether the link resolves.
        let (ok, out, err) = cp_out(&remove_dest_v(), &[&a, &dang]);
        assert!(ok, "{err}");
        assert!(out.starts_with("removed "), "{out}");
        assert_eq!(body(&dang), b"A");
        assert!(
            !fs::symlink_metadata(&dang)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    /// A link that *does* resolve is written through, not replaced — and
    /// `--remove-destination` replaces it. The target's contents are the
    /// assertion: after `-f` the target changed, after
    /// `--remove-destination` it did not.
    #[cfg(unix)]
    #[test]
    fn only_remove_destination_replaces_a_working_symlink() {
        let dir = scratch("through_link");
        let a = dir.path("a");
        let t = dir.path("target");
        let lnk = dir.path("lnk");

        fs::write(&a, b"A").unwrap();
        fs::write(&t, b"OLD").unwrap();
        std::os::unix::fs::symlink("target", &lnk).unwrap();
        let (ok, err) = cp(&force_v(), &[&a, &lnk]);
        assert!(ok, "{err}");
        assert_eq!(body(&t), b"A", "written through the link");
        assert!(fs::symlink_metadata(&lnk).unwrap().file_type().is_symlink());

        fs::write(&t, b"OLD").unwrap();
        let (ok, err) = cp(&remove_dest_v(), &[&a, &lnk]);
        assert!(ok, "{err}");
        assert_eq!(body(&t), b"OLD", "the target was not touched");
        assert_eq!(body(&lnk), b"A");
        assert!(
            !fs::symlink_metadata(&lnk).unwrap().file_type().is_symlink(),
            "the link itself was replaced by a file"
        );
    }

    /// `cp a self` where `self` is a link to `a` is "the same file" — except
    /// under `--remove-destination`, which is excused from that check because
    /// after the unlink it would not be the same file.
    #[cfg(unix)]
    #[test]
    fn remove_destination_is_excused_from_the_same_file_check() {
        let dir = scratch("same_link");
        let a = dir.path("a");
        let me = dir.path("self");
        fs::write(&a, b"A").unwrap();
        std::os::unix::fs::symlink("a", &me).unwrap();

        let (ok, err) = cp(&verbose(), &[&a, &me]);
        assert!(!ok);
        assert!(err.contains("are the same file"), "{err}");

        let (ok, err) = cp(&remove_dest_v(), &[&a, &me]);
        assert!(ok, "{err}");
        assert_eq!(body(&me), b"A");
        assert_eq!(body(&a), b"A", "the source survived");
    }

    /// `-n` refuses on stderr and reports failure — it is not a quiet skip.
    /// This is the exact behaviour Ubuntu patches out of its own build, which
    /// is why `cp-diff.sh` compares against a from-source 9.4.
    #[test]
    fn no_clobber_refuses_and_fails() {
        let dir = scratch("noclobber");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"A").unwrap();
        fs::write(&b, b"BBBB").unwrap();

        let (ok, out, err) = cp_out(&no_clobber_v(), &[&a, &b]);
        assert!(!ok, "the status is 1, not 0");
        assert_eq!(err, format!("cp: not replacing {}\n", quoteaf_os(&b)));
        assert_eq!(out, "", "and no verbose line, because nothing was copied");
        assert_eq!(body(&b), b"BBBB");
    }

    /// With nothing in the way it copies and succeeds, so the refusal is about
    /// the destination existing and not about the option being given.
    #[test]
    fn no_clobber_copies_when_there_is_nothing_to_clobber() {
        let dir = scratch("noclobber_new");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"A").unwrap();

        let (ok, out, err) = cp_out(&no_clobber_v(), &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(out, format!("{} -> {}\n", quoteaf_os(&a), quoteaf_os(&b)));
        assert_eq!(body(&b), b"A");
    }

    /// One refusal must not abandon the sources after it, and must still be
    /// visible in the exit status. Module docs, bug 6, restated for `-n`.
    #[test]
    fn a_refusal_does_not_stop_the_other_sources() {
        let dir = scratch("noclobber_many");
        let d = dir.path("d");
        fs::create_dir(&d).unwrap();
        for name in ["1", "2", "3"] {
            fs::write(dir.path(name), name.as_bytes()).unwrap();
        }
        fs::write(d.join("2"), b"kept").unwrap();

        let (ok, out, err) = cp_out(
            &no_clobber_v(),
            &[&dir.path("1"), &dir.path("2"), &dir.path("3"), &d],
        );
        assert!(!ok);
        assert_eq!(
            err,
            format!("cp: not replacing {}\n", quoteaf_os(d.join("2")))
        );
        assert_eq!(body(&d.join("1")), b"1");
        assert_eq!(body(&d.join("2")), b"kept");
        assert_eq!(body(&d.join("3")), b"3", "the source after the refusal");
        assert_eq!(out.lines().count(), 2, "two copies announced, not three");
    }

    /// `-n` is checked per *entry*, not once per operand, so a recursive copy
    /// keeps every name the destination already has and adds the rest. The
    /// directory itself is exempt — otherwise `-rn` would refuse at the top and
    /// never descend at all.
    #[test]
    fn no_clobber_applies_inside_a_recursive_copy() {
        let dir = scratch("noclobber_r");
        let src = dir.path("src");
        let dst = dir.path("dst");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("keep"), b"new").unwrap();
        fs::write(src.join("add"), b"new").unwrap();
        fs::create_dir_all(dst.join("src")).unwrap();
        fs::write(dst.join("src").join("keep"), b"old").unwrap();

        let (ok, _out, err) = cp_out(
            &CpFlags {
                recursive: true,
                interactive: Interactive::AlwaysNo,
                verbose: true,
                ..off()
            },
            &[&src, &dst],
        );
        assert!(!ok, "the refusal is still a failure");
        assert!(err.contains("not replacing"), "{err}");
        assert_eq!(body(&dst.join("src").join("keep")), b"old");
        assert_eq!(
            body(&dst.join("src").join("add")),
            b"new",
            "descending happened despite the refusal"
        );
    }

    /// The question's exact text, and that a `y` gets on with it.
    ///
    /// `assert_eq!` on the whole of stderr rather than a `contains`, because the
    /// two things most easily got wrong here are both invisible to a
    /// `contains`: a trailing newline (GNU's `fprintf` has none — the cursor is
    /// meant to sit after the space, where the person is about to type) and a
    /// second line following it.
    #[test]
    fn a_yes_overwrites_and_the_question_is_the_whole_of_stderr() {
        let dir = scratch("ask_yes");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"A").unwrap();
        fs::write(&b, b"BBBB").unwrap();

        let (ok, out, err, asked) = cp_answering(&ask(), &[&a, &b], &["y\n"]);
        assert!(ok, "{err}");
        assert_eq!(err, format!("cp: overwrite {}? ", quoteaf_os(&b)));
        assert_eq!(out, "", "and nothing on stdout without -v");
        assert_eq!(asked, 1);
        assert_eq!(body(&b), b"A");
    }

    /// Declining is a **silent** exit 1: the question is the only thing
    /// written, and in particular `-i` does not borrow `-n`'s `not replacing`.
    /// End of input declines the same way, which is what a script piping a
    /// short file — or nothing at all — into `cp -i` gets.
    #[test]
    fn a_no_and_an_empty_queue_both_decline_silently() {
        let dir = scratch("ask_no");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"A").unwrap();
        fs::write(&b, b"BBBB").unwrap();

        for answers in [&["n\n"][..], &[][..]] {
            let (ok, out, err, asked) = cp_answering(&ask(), &[&a, &b], answers);
            assert!(!ok, "a decline is a failure, like -n's refusal");
            assert_eq!(err, format!("cp: overwrite {}? ", quoteaf_os(&b)));
            assert_eq!(out, "");
            assert_eq!(
                asked, 1,
                "end of input is still an answer that was asked for"
            );
            assert_eq!(body(&b), b"BBBB");
        }
    }

    /// With nothing in the way there is nothing to ask about, and the queue is
    /// untouched — the count is the only way to tell this from a `-i` that asked
    /// and was told yes, since both copy and both succeed.
    #[test]
    fn nothing_to_clobber_is_not_worth_asking_about() {
        let dir = scratch("ask_new");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"A").unwrap();

        let (ok, _out, err, asked) = cp_answering(&ask(), &[&a, &b], &["n"]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert_eq!(asked, 0);
        assert_eq!(body(&b), b"A");
    }

    /// One question per operand that needs one, answers taken in order, and a
    /// decline that does not abandon the sources after it. Module docs, bug 6,
    /// restated for `-i`.
    #[test]
    fn each_operand_gets_its_own_question_answered_in_order() {
        let dir = scratch("ask_many");
        let d = dir.path("d");
        fs::create_dir(&d).unwrap();
        for name in ["1", "2", "3"] {
            fs::write(dir.path(name), name.as_bytes()).unwrap();
            fs::write(d.join(name), b"old").unwrap();
        }

        let (ok, _out, err, asked) = cp_answering(
            &ask(),
            &[&dir.path("1"), &dir.path("2"), &dir.path("3"), &d],
            &["y", "n", "y"],
        );
        assert!(!ok, "the middle decline is still a failure");
        assert_eq!(asked, 3);
        assert_eq!(
            err,
            format!(
                "cp: overwrite {}? cp: overwrite {}? cp: overwrite {}? ",
                quoteaf_os(d.join("1")),
                quoteaf_os(d.join("2")),
                quoteaf_os(d.join("3")),
            ),
            "three questions run together on one line, none of them ending in \
             a newline — which is exactly what a person answering them sees"
        );
        assert_eq!(body(&d.join("1")), b"1");
        assert_eq!(body(&d.join("2")), b"old", "the declined one");
        assert_eq!(body(&d.join("3")), b"3", "the source after the decline");
    }

    /// `-i` is asked per *entry* found by the walk, not once per operand, and
    /// the directory itself is exempt for the same reason `-n` exempts it: a
    /// question at the top would decide the whole tree in one keystroke.
    #[test]
    fn interactive_asks_per_entry_inside_a_recursive_copy() {
        let dir = scratch("ask_r");
        let src = dir.path("src");
        let dst = dir.path("dst");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("keep"), b"new").unwrap();
        fs::write(src.join("add"), b"new").unwrap();
        fs::create_dir_all(dst.join("src")).unwrap();
        fs::write(dst.join("src").join("keep"), b"old").unwrap();

        let (ok, _out, err, asked) = cp_answering(
            &CpFlags {
                recursive: true,
                interactive: Interactive::AskUser,
                ..off()
            },
            &[&src, &dst],
            &["n"],
        );
        assert!(!ok);
        assert_eq!(
            asked, 1,
            "only `keep` exists at the destination; the directory is exempt"
        );
        assert_eq!(
            err,
            format!(
                "cp: overwrite {}? ",
                quoteaf_os(dst.join("src").join("keep"))
            )
        );
        assert_eq!(body(&dst.join("src").join("keep")), b"old");
        assert_eq!(
            body(&dst.join("src").join("add")),
            b"new",
            "descending happened despite the decline"
        );
    }

    /// The one place `-i` and `-n` see the world differently rather than merely
    /// answering differently: GNU guards the same-file check with `interactive
    /// != I_ALWAYS_NO` (`copy.c:2344`), so `cp -n a a` never gets as far as it
    /// and says `not replacing`, while `cp -i a a` reaches it, says `are the
    /// same file`, and asks nothing at all.
    #[test]
    fn interactive_reports_the_same_file_where_no_clobber_refuses() {
        let dir = scratch("ask_same");
        let a = dir.path("a");
        fs::write(&a, b"A").unwrap();

        let (ok, _out, err, asked) = cp_answering(&ask(), &[&a, &a], &["y"]);
        assert!(!ok);
        assert!(err.contains("are the same file"), "{err}");
        assert_eq!(asked, 0, "no question was put, so the `y` went unused");

        let (ok, err) = cp(&no_clobber_v(), &[&a, &a]);
        assert!(!ok);
        assert!(err.contains("not replacing"), "{err}");
        assert_eq!(body(&a), b"A");
    }

    /// The other two wordings. A destination the effective uid cannot write
    /// gets a question that quotes its mode, and `-f`/`--remove-destination`
    /// change that question from "try anyway" to "replace, overriding" —
    /// because with them `cp` means to unlink the file rather than write
    /// through its permission bits, so it is not really asking the same thing.
    #[cfg(unix)]
    #[test]
    fn an_unwritable_destination_gets_a_question_that_quotes_its_mode() {
        if root() {
            return;
        }
        let dir = scratch("ask_mode");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"A").unwrap();
        fs::write(&b, b"BBBB").unwrap();
        set_test_mode(&b, 0o444);

        let (ok, _out, err, asked) = cp_answering(&ask(), &[&a, &b], &["n"]);
        assert!(!ok);
        assert_eq!(
            err,
            format!(
                "cp: unwritable {} (mode 0444, r--r--r--); try anyway? ",
                quoteaf_os(&b)
            )
        );
        assert_eq!(asked, 1);

        for flags in [
            CpFlags {
                interactive: Interactive::AskUser,
                force: true,
                ..off()
            },
            CpFlags {
                interactive: Interactive::AskUser,
                remove_destination: true,
                ..off()
            },
        ] {
            let (ok, _out, err, asked) = cp_answering(&flags, &[&a, &b], &["n"]);
            assert!(!ok);
            assert_eq!(
                err,
                format!(
                    "cp: replace {}, overriding mode 0444 (r--r--r--)? ",
                    quoteaf_os(&b)
                )
            );
            assert_eq!(asked, 1);
        }
        assert_eq!(body(&b), b"BBBB", "every one of the three was declined");
    }

    /// `-iv` writes to both streams, and which text goes to which is the point:
    /// the question must reach a terminal even when stdout is a pipe, and the
    /// `->` line must reach the pipe rather than the terminal.
    #[test]
    fn the_question_is_on_stderr_and_the_verbose_line_on_stdout() {
        let dir = scratch("ask_verbose");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"A").unwrap();
        fs::write(&b, b"BBBB").unwrap();

        let (ok, out, err, asked) = cp_answering(
            &CpFlags {
                interactive: Interactive::AskUser,
                verbose: true,
                ..off()
            },
            &[&a, &b],
            &["y"],
        );
        assert!(ok, "{err}");
        assert_eq!(err, format!("cp: overwrite {}? ", quoteaf_os(&b)));
        assert_eq!(out, format!("{} -> {}\n", quoteaf_os(&a), quoteaf_os(&b)));
        assert_eq!(asked, 1);
    }

    /// The regression this change also fixed: `cp -r` over a tree that already
    /// contains a symlink used to report `File exists` and fail, because the
    /// symlink arm of the walk never unlinked what was in its way.
    /// `known-issues.md` -> `B-CP-R-COULD-NOT-REPLACE-AN-EXISTING-SYMLINK`.
    #[cfg(unix)]
    #[test]
    fn a_recursive_copy_replaces_an_existing_symlink() {
        let dir = scratch("r_relink");
        let src = dir.path("src");
        let dst = dir.path("dst");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("f"), b"F").unwrap();
        std::os::unix::fs::symlink("f", src.join("link")).unwrap();
        fs::create_dir_all(dst.join("src")).unwrap();
        std::os::unix::fs::symlink("elsewhere", dst.join("src").join("link")).unwrap();

        let (ok, err) = cp(&recursive(), &[&src, &dst]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert_eq!(
            fs::read_link(dst.join("src").join("link")).unwrap(),
            Path::new("f"),
            "the stale link was replaced, not left alone"
        );
    }

    /// None of the three removes a directory. `-T` is what aims them at one:
    /// without it the destination is a place to copy into and the question
    /// never arises.
    #[test]
    fn none_of_the_three_removes_a_directory() {
        let dir = scratch("vs_dir");
        let a = dir.path("a");
        let d = dir.path("d");
        fs::write(&a, b"A").unwrap();
        fs::create_dir(&d).unwrap();
        fs::write(d.join("witness"), b"w").unwrap();

        for over in [
            CpFlags {
                no_target_directory: true,
                force: true,
                ..off()
            },
            CpFlags {
                no_target_directory: true,
                remove_destination: true,
                ..off()
            },
            CpFlags {
                no_target_directory: true,
                interactive: Interactive::AlwaysNo,
                ..off()
            },
        ] {
            let (ok, err) = cp(&over, &[&a, &d]);
            assert!(!ok, "{err}");
            assert!(d.is_dir(), "still a directory");
            assert!(d.join("witness").is_file(), "and still has its contents");
        }
    }

    // ------------------------------------------------- -b, --backup and -S --

    /// `-b`, i.e. `--backup` with no word: [`BackupType::NumberedExisting`] and
    /// the default suffix.
    fn backup_b() -> CpFlags {
        CpFlags {
            backup: backup::Backup::new(BackupType::NumberedExisting, b"~".to_vec()),
            ..off()
        }
    }

    /// The same, verbose, because the `(backup: …)` clause is half of what `-b`
    /// is observable through.
    fn backup_bv() -> CpFlags {
        CpFlags {
            verbose: true,
            ..backup_b()
        }
    }

    #[test]
    fn backup_moves_the_destination_aside_and_keeps_its_bytes() {
        let dir = scratch("backup_simple");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"NEW").unwrap();
        fs::write(&b, b"OLD").unwrap();

        let (ok, out, err) = cp_out(&backup_bv(), &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert_eq!(
            out,
            format!(
                "{} -> {} (backup: {})\n",
                quoteaf_os(&a),
                quoteaf_os(&b),
                quoteaf_os(dir.path("b~"))
            )
        );
        assert_eq!(body(&b), b"NEW");
        assert_eq!(body(&dir.path("b~")), b"OLD");
    }

    /// Nothing to move aside is not an error, and prints no `(backup: …)`.
    #[test]
    fn a_destination_that_is_not_there_is_not_backed_up() {
        let dir = scratch("backup_absent");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"NEW").unwrap();

        let (ok, out, err) = cp_out(&backup_bv(), &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert_eq!(out, format!("{} -> {}\n", quoteaf_os(&a), quoteaf_os(&b)));
        assert!(!dir.path("b~").exists());
    }

    /// `-S` turns backups **on** by itself, and is not merely a name for the
    /// ones `-b` asked for: GNU's `case 'S'` is `make_backups = true;
    /// backup_suffix = optarg;` (`cp.c:1190`), the same first line as `case
    /// 'b'`. Asserted with and without the `-b` it does not need, because the
    /// tempting reading — a suffix that only takes effect alongside `-b` —
    /// leaves `cp -S .bak a b` silently overwriting.
    #[test]
    fn a_suffix_turns_backups_on_by_itself() {
        for spelling in [&["-S", ".bak"][..], &["-b", "-S", ".bak"][..]] {
            let (f, _) = run_parse(&[spelling, &["a", "b"]].concat());
            assert!(f.backup.enabled(), "{spelling:?}: -S must turn backups on");

            let dir = scratch("suffix");
            let a = dir.path("a");
            let b = dir.path("b");
            fs::write(&a, b"NEW").unwrap();
            fs::write(&b, b"OLD").unwrap();
            let (ok, err) = cp(&f, &[&a, &b]);
            assert!(ok, "{err}");
            assert_eq!(body(&b), b"NEW");
            assert_eq!(body(&dir.path("b.bak")), b"OLD", "{spelling:?}");
            assert!(!dir.path("b~").exists(), "{spelling:?}: the suffix is -S's");
        }
    }

    /// `--backup=numbered` names `b.~1~`, and again `b.~2~`.
    #[test]
    fn numbered_backups_count_up() {
        let (f, _) = run_parse(&["--backup=numbered", "a", "b"]);
        let dir = scratch("backup_numbered");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&b, b"OLD1").unwrap();
        fs::write(&a, b"NEW1").unwrap();
        assert!(cp(&f, &[&a, &b]).0);
        fs::write(&a, b"NEW2").unwrap();
        assert!(cp(&f, &[&a, &b]).0);

        assert_eq!(body(&dir.path("b.~1~")), b"OLD1");
        assert_eq!(body(&dir.path("b.~2~")), b"NEW1");
        assert_eq!(body(&b), b"NEW2");
    }

    /// `existing` — which bare `-b` selects — is numbered only where numbered
    /// backups are already there, and simple otherwise. Both halves in one
    /// test, because the difference between them *is* the option.
    #[test]
    fn existing_follows_what_the_directory_already_has() {
        let dir = scratch("backup_existing");
        let a = dir.path("a");
        fs::write(&a, b"NEW").unwrap();

        let plain_dst = dir.path("b");
        fs::write(&plain_dst, b"OLD").unwrap();
        assert!(cp(&backup_b(), &[&a, &plain_dst]).0);
        assert_eq!(body(&dir.path("b~")), b"OLD", "no numbers here, so simple");

        let numbered_dst = dir.path("c");
        fs::write(&numbered_dst, b"OLD").unwrap();
        fs::write(dir.path("c.~1~"), b"ANCIENT").unwrap();
        assert!(cp(&backup_b(), &[&a, &numbered_dst]).0);
        assert_eq!(
            body(&dir.path("c.~2~")),
            b"OLD",
            "numbers here, so numbered"
        );
        assert!(!dir.path("c~").exists());
    }

    /// `--backup` and `--no-clobber` are refused together, and — unlike every
    /// other diagnostic this program has — the sentence carries the referral,
    /// because upstream reaches it through `usage (EXIT_FAILURE)` rather than
    /// `die` (`cp.c:1223`).
    #[test]
    fn backup_and_no_clobber_are_refused_with_the_referral() {
        for spelling in [
            ["-n", "-b"],
            ["-b", "-n"],
            ["--backup", "--no-clobber"],
            ["-n", "-S.bak"],
        ] {
            let e = fail(&[spelling[0], spelling[1], "a", "b"]);
            assert_eq!(
                e.sentence, "options --backup and --no-clobber are mutually exclusive",
                "{spelling:?}"
            );
            assert!(e.referral.is_some(), "{spelling:?}: needs the Try line");
        }
    }

    /// `--no-clobber --backup=none` is refused too, which is not obvious: the
    /// two are asked about in the order GNU asks them, and the check is on
    /// *whether an option was given* (`make_backups`, `cp.c:1220`) rather than
    /// on the type it resolved to — which happens thirteen lines later
    /// (`cp.c:1233`). So a `--backup` that turned itself off still counts.
    #[test]
    fn backup_none_still_counts_as_having_asked() {
        let e = fail(&["--no-clobber", "--backup=none", "a", "b"]);
        assert_eq!(
            e.sentence,
            "options --backup and --no-clobber are mutually exclusive"
        );
    }

    /// On its own, though, `--backup=none` leaves backups off — the resolution
    /// the check above deliberately does not wait for.
    #[test]
    fn backup_none_makes_no_backup() {
        let (f, p) = run_parse(&["--backup=none", "a", "b"]);
        assert!(!f.backup.enabled());
        assert_eq!(p, vec!["a", "b"]);
    }

    /// An unknown word names itself and lists the ones that would have worked.
    #[test]
    fn an_unknown_backup_word_is_rejected_by_name() {
        let e = fail(&["--backup=zz", "a", "b"]);
        assert!(e.sentence.contains("invalid argument"), "{:?}", e.sentence);
        assert!(e.sentence.contains("zz"), "{:?}", e.sentence);
        assert!(e.sentence.contains("numbered"), "{:?}", e.sentence);
    }

    /// A simple backup whose name is the source is refused rather than made:
    /// `cp --backup=simple a~ a` would name `a`'s backup `a~`, overwrite the
    /// source with itself, and leave two copies of nothing. Upstream carries
    /// this recipe as a comment; this is it.
    #[test]
    fn a_backup_that_would_be_the_source_is_refused() {
        let dir = scratch("backup_eats_src");
        let a = dir.path("a");
        let a_tilde = dir.path("a~");
        fs::write(&a, b"EMPTYISH").unwrap();
        fs::write(&a_tilde, b"THE ONLY COPY").unwrap();

        let f = CpFlags {
            backup: backup::Backup::new(BackupType::Simple, b"~".to_vec()),
            ..off()
        };
        let (ok, err) = cp(&f, &[&a_tilde, &a]);
        assert!(!ok);
        assert!(err.contains("might destroy source"), "{err}");
        assert_eq!(body(&a_tilde), b"THE ONLY COPY", "left where it was");
        assert_eq!(body(&a), b"EMPTYISH", "and not written over");
    }

    /// The numbered type is exempt from that refusal, because the name it picks
    /// is never one the user typed.
    #[test]
    fn a_numbered_backup_of_the_sources_own_name_is_made() {
        let dir = scratch("backup_numbered_src");
        let a = dir.path("a");
        let a_tilde = dir.path("a~");
        fs::write(&a, b"OLD").unwrap();
        fs::write(&a_tilde, b"NEW").unwrap();

        let f = CpFlags {
            backup: backup::Backup::new(BackupType::Numbered, b"~".to_vec()),
            ..off()
        };
        let (ok, err) = cp(&f, &[&a_tilde, &a]);
        assert!(ok, "{err}");
        assert_eq!(body(&dir.path("a.~1~")), b"OLD");
        assert_eq!(body(&a), b"NEW");
    }

    /// `--remove-destination` and `--backup` are upstream's `if` and `else if`
    /// (`copy.c:2517`), not two steps: the backup happens and the unlink does
    /// **not**. Read as independent, this deletes the very file the backup
    /// exists to keep — the destination is removed, the rename finds nothing,
    /// and `--backup` silently does nothing at all.
    #[cfg(unix)]
    #[test]
    fn remove_destination_gives_way_to_a_backup() {
        let dir = scratch("backup_vs_rmdest");
        let a = dir.path("a");
        fs::write(&a, b"NEW").unwrap();
        let (b, witness) = linked_destination(&dir);

        let f = CpFlags {
            remove_destination: true,
            ..backup_bv()
        };
        let (ok, out, err) = cp_out(&f, &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert!(!out.contains("removed"), "must not unlink as well: {out}");
        assert!(out.contains("(backup: "), "{out}");
        assert_eq!(body(&b), b"NEW");
        assert_eq!(body(&dir.path("b~")), b"BBBB", "the backup, not a deletion");
        assert_eq!(body(&witness), b"BBBB", "renamed aside, so the link holds");
    }

    /// A destination directory is not moved aside, so `cp -rb` merges into an
    /// existing hierarchy and backs up the *files* it lands on. Upstream writes
    /// the condition as `x->move_mode || ! S_ISDIR (…)` with a `FIXME` saying
    /// `mv` does back up a directory and `cp` deliberately does not.
    #[test]
    fn a_destination_directory_is_merged_into_and_its_files_backed_up() {
        let dir = scratch("backup_tree");
        let src = dir.path("src");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("f"), b"NEW").unwrap();
        let dst = dir.path("dst");
        fs::create_dir_all(dst.join("src")).unwrap();
        fs::write(dst.join("src/f"), b"OLD").unwrap();

        let f = CpFlags {
            recursive: true,
            ..backup_b()
        };
        let (ok, err) = cp(&f, &[&src, &dst]);
        assert!(ok, "{err}");
        assert!(dst.join("src").is_dir(), "not renamed away");
        assert!(!dst.join("src~").exists());
        assert_eq!(body(&dst.join("src/f")), b"NEW");
        assert_eq!(body(&dst.join("src/f~")), b"OLD");
    }

    /// A source whose last component is `.` copies the *contents* into the
    /// destination, so backing the destination up would move the directory the
    /// copy is about to fill.
    #[test]
    fn a_dot_source_does_not_back_up_the_destination() {
        let dir = scratch("backup_dot");
        let src = dir.path("src");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("f"), b"NEW").unwrap();
        let dst = dir.path("dst");
        fs::create_dir(&dst).unwrap();

        let f = CpFlags {
            recursive: true,
            ..backup_b()
        };
        let (ok, err) = cp(&f, &[&src.join("."), &dst]);
        assert!(ok, "{err}");
        assert_eq!(body(&dst.join("f")), b"NEW");
        assert!(!dir.path("dst~").exists());
    }

    /// A backup made for a copy that then failed is put back, which is
    /// upstream's `un_backup` (`copy.c:3350`). Without it a failed `cp -b`
    /// leaves *no* file under the destination's own name — the worst of both.
    #[cfg(unix)]
    #[test]
    fn a_backup_is_restored_when_the_copy_fails() {
        if root() {
            return; // 0000 denies nobody here, so the copy would succeed.
        }
        let dir = scratch("backup_un");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"NEW").unwrap();
        fs::write(&b, b"OLD").unwrap();
        set_test_mode(&a, 0o000);

        let (ok, err) = cp(&backup_b(), &[&a, &b]);
        assert!(!ok, "an unreadable source cannot be copied");
        assert!(err.contains("cannot open"), "{err}");
        assert_eq!(body(&b), b"OLD", "put back under its own name");
        assert!(!dir.path("b~").exists(), "and not left as the backup");
        set_test_mode(&a, 0o600);
    }

    // ---------------------------------------- the order a directory is read --

    /// Whatever the order, every entry has to come out exactly once. Asserted
    /// on both platforms, because only one of them sorts and a sort that drops
    /// or duplicates an entry would be a silently incomplete copy.
    #[test]
    fn the_directory_read_returns_every_entry_once() {
        let dir = scratch("read_all");
        for name in ["a", "b", "c", "d"] {
            fs::write(dir.path(name), b"x").unwrap();
        }
        fs::create_dir(dir.path("sub")).unwrap();

        let mut got: Vec<String> = read_dir_fastread(dir.dir())
            .unwrap()
            .iter()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        got.sort();
        assert_eq!(got, ["a", "b", "c", "d", "sub"]);
    }

    /// And on Unix the order is inode-ascending, which is gnulib's
    /// `SAVEDIR_SORT_FASTREAD` and therefore GNU `cp`'s. Asserted against the
    /// inodes themselves rather than against a fixed list of names: what the
    /// filesystem allocates is its business, and the claim being made is only
    /// that whatever it allocated comes back in order.
    ///
    /// Five entries rather than two, because a two-element list is sorted by
    /// half the possible implementations of `sort` including several wrong
    /// ones.
    #[cfg(unix)]
    #[test]
    fn the_directory_read_is_in_inode_order() {
        use std::os::unix::fs::DirEntryExt as _;

        let dir = scratch("read_ino");
        // Names deliberately anti-correlated with creation order, so that a
        // sort by *name* would produce a different answer and be caught.
        for name in ["e", "d", "c", "b", "a"] {
            fs::write(dir.path(name), b"x").unwrap();
        }

        let inodes: Vec<u64> = read_dir_fastread(dir.dir())
            .unwrap()
            .iter()
            .map(fs::DirEntry::ino)
            .collect();
        assert_eq!(inodes.len(), 5);
        assert!(
            inodes.windows(2).all(|w| w[0] <= w[1]),
            "not ascending: {inodes:?}"
        );
    }

    /// The line the repeat rule draws. Two hard links share an inode but are
    /// two entries, and asking for a copy of each is a legitimate request —
    /// which is why the rule needs [`entry_id`] and not just [`file_id`].
    #[cfg(unix)]
    #[test]
    fn two_hard_links_to_one_file_are_not_one_source_named_twice() {
        let dir = scratch("two_hard");
        let dest = dir.path("dest");
        fs::create_dir(&dest).unwrap();
        let one = dir.path("one");
        let two = dir.path("two");
        fs::write(&one, b"body").unwrap();
        fs::hard_link(&one, &two).unwrap();

        let (ok, e) = cp(&plain(), &[&one, &two, &dest]);
        assert!(ok, "{e}");
        assert_eq!(e, "", "nothing to warn about");
        assert_eq!(fs::read(dest.join("one")).unwrap(), b"body");
        assert_eq!(fs::read(dest.join("two")).unwrap(), b"body");
    }

    /// With one source there is no pair, so the tables are never built. This
    /// asserts the case that would otherwise be caught by them wrongly: a
    /// single source copied onto a destination the *previous* run made.
    #[test]
    fn a_lone_source_is_never_a_repeat_of_itself() {
        let dir = scratch("lone");
        let dest = dir.path("dest");
        fs::create_dir(&dest).unwrap();
        let f = dir.path("f");
        fs::write(&f, b"body").unwrap();

        assert!(cp(&plain(), &[&f, &dest]).0);
        let (ok, e) = cp(&plain(), &[&f, &dest]);
        assert!(ok, "{e}");
        assert_eq!(e, "");
    }

    // ----------------------------------------------------- --preserve=links --
    //
    // Every one of these is `#[cfg(unix)]`. The option is about hard links, and
    // a host that has none has nothing here to assert — [`Copied`] is still
    // built there, but no source can ever have a second link and no
    // `should_dereference` case can fire without symlinks either, so the table
    // is unreachable rather than wrong. `scripts/cp-diff.sh` section 18
    // certifies the same behaviour against GNU itself; these exist so that
    // `cargo test` catches a regression without a GNU userland to compare
    // against.

    /// `--preserve=links` and nothing else. Not folded into the `..OFF` family
    /// near [`PLAIN`] because it and its two variants are `#[cfg(unix)]`, and an
    /// unused constant is a warning on the development host.
    #[cfg(unix)]
    fn links() -> CpFlags {
        CpFlags {
            preserve: Preserve {
                links: true,
                ..Preserve::NONE
            },
            ..off()
        }
    }
    /// `-v --preserve=links`. Most of these tests want it: the option's two
    /// orderings — whether `removed` comes before or after the arrow — are
    /// visible only in the verbose output, and getting them backwards is the
    /// mistake the obvious implementation makes.
    #[cfg(unix)]
    fn links_v() -> CpFlags {
        CpFlags {
            verbose: true,
            ..links()
        }
    }
    /// `-rv --preserve=links`.
    #[cfg(unix)]
    fn links_rv() -> CpFlags {
        CpFlags {
            recursive: true,
            ..links_v()
        }
    }

    /// `p`'s inode number, for asserting that two names are one file.
    ///
    /// Comparing inode numbers is sound here in a way it is not in
    /// [`linked_destination`]'s case: both files exist at the moment of the
    /// comparison, so neither number can be the other's recycled.
    ///
    /// `symlink_metadata`, so that a test about two *symlinks* being linked
    /// together compares the links and not what they point at.
    #[cfg(unix)]
    fn ino(p: &Path) -> u64 {
        use std::os::unix::fs::MetadataExt as _;
        fs::symlink_metadata(p)
            .unwrap_or_else(|e| panic!("stat {}: {e}", p.display()))
            .ino()
    }

    /// The whole option in one case: two operands that turn out to be one file
    /// land as one file, and are announced as two copies while doing it.
    #[cfg(unix)]
    #[test]
    fn preserve_links_makes_the_second_destination_a_link() {
        let dir = scratch("links_pair");
        let d = dir.path("d");
        fs::create_dir(&d).unwrap();
        let (a, b) = (dir.path("a"), dir.path("b"));
        fs::write(&a, b"body").unwrap();
        fs::hard_link(&a, &b).unwrap();

        let (ok, out, err) = cp_out(&links_v(), &[&a, &b, &d]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert_eq!(
            out,
            format!(
                "{} -> {}\n{} -> {}\n",
                quoteaf_os(&a),
                quoteaf_os(d.join("a")),
                quoteaf_os(&b),
                quoteaf_os(d.join("b"))
            ),
            "the second is announced as a copy, not as a link -- GNU's \
             `emit_verbose` runs before the `earlier_file` branch"
        );
        assert_eq!(ino(&d.join("a")), ino(&d.join("b")));
        assert_eq!(fs::read(d.join("b")).unwrap(), b"body");
    }

    /// And without the option, two files — so the test above pins the option
    /// rather than observing that a filesystem deduplicates, which none does.
    #[cfg(unix)]
    #[test]
    fn without_it_one_source_named_twice_lands_twice() {
        let dir = scratch("links_off");
        let d = dir.path("d");
        fs::create_dir(&d).unwrap();
        let (a, b) = (dir.path("a"), dir.path("b"));
        fs::write(&a, b"body").unwrap();
        fs::hard_link(&a, &b).unwrap();

        let (ok, err) = cp(&plain(), &[&a, &b, &d]);
        assert!(ok, "{err}");
        assert_ne!(ino(&d.join("a")), ino(&d.join("b")));
    }

    /// The table spans the whole invocation, not one operand: a pair found by
    /// *walking* a tree is linked the same way. This is the case a first
    /// implementation gets wrong by consulting the table only for command-line
    /// arguments, which is what GNU's `src_to_dest_lookup` looks like it does.
    #[cfg(unix)]
    #[test]
    fn preserve_links_spans_a_recursive_walk() {
        let dir = scratch("links_walk");
        let s = dir.path("s");
        fs::create_dir(&s).unwrap();
        fs::write(s.join("x"), b"body").unwrap();
        fs::hard_link(s.join("x"), s.join("y")).unwrap();
        let d = dir.path("d");
        fs::create_dir(&d).unwrap();

        let (ok, err) = cp(&links_rv(), &[&s, &d]);
        assert!(ok, "{err}");
        let out = d.join("s");
        assert_eq!(ino(&out.join("x")), ino(&out.join("y")));
    }

    /// A destination that already exists and has a second link of its own is
    /// unlinked *before* the copy is announced, so that the other link keeps
    /// the old bytes. This is GNU's pre-copy unlink at `copy.c:2570`, whose
    /// `preserve_links && 1 < dst_sb.st_nlink` clause exists for exactly this.
    ///
    /// The ordering is the assertion: `removed` first, then the arrow. The
    /// test below has the same two lines the other way round.
    #[cfg(unix)]
    #[test]
    fn a_multiply_linked_destination_is_removed_before_the_announce() {
        let dir = scratch("links_dst_nlink");
        let d = dir.path("d");
        fs::create_dir(&d).unwrap();
        let a = dir.path("a");
        fs::write(&a, b"new").unwrap();
        let (dst, witness) = (d.join("a"), dir.path("witness"));
        fs::write(&dst, b"OLD").unwrap();
        fs::hard_link(&dst, &witness).unwrap();

        let (ok, out, err) = cp_out(&links_v(), &[&a, &d]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2, "{out}");
        assert!(lines[0].starts_with("removed "), "{out}");
        assert!(lines[1].contains(" -> "), "{out}");
        assert_eq!(fs::read(&dst).unwrap(), b"new");
        assert_eq!(
            fs::read(&witness).unwrap(),
            b"OLD",
            "the other link was left alone, which is the point of unlinking"
        );
    }

    /// The other ordering. When the destination has only its own link, nothing
    /// is unlinked up front; the `removed` line comes from `force_linkat`
    /// renaming a fresh link over the destination, and so lands *after* the
    /// arrow line the copy already printed.
    #[cfg(unix)]
    #[test]
    fn a_replaced_destination_is_removed_after_the_announce() {
        let dir = scratch("links_replace");
        let d = dir.path("d");
        fs::create_dir(&d).unwrap();
        let (a, b) = (dir.path("a"), dir.path("b"));
        fs::write(&a, b"body").unwrap();
        fs::hard_link(&a, &b).unwrap();
        fs::write(d.join("b"), b"OLD").unwrap();

        let (ok, out, err) = cp_out(&links_v(), &[&a, &b, &d]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3, "{out}");
        assert!(lines[1].contains(" -> "), "{out}");
        assert!(lines[2].starts_with("removed "), "{out}");
        assert!(lines[2].contains("b'"), "{out}");
        assert_eq!(ino(&d.join("a")), ino(&d.join("b")));
    }

    /// Two *symlinks* to one file, followed. Neither the links nor the file
    /// they point at has a second hard link, so `st_nlink` is 1 throughout and
    /// the table is consulted only because the command dereferences —
    /// `should_dereference`'s other half. Both destinations are one file.
    #[cfg(unix)]
    #[test]
    fn dereferencing_links_two_symlinks_to_one_file() {
        for flags in [
            CpFlags {
                dereference: Deref::Always,
                ..links()
            },
            CpFlags {
                dereference: Deref::CommandLine,
                ..links()
            },
        ] {
            let dir = scratch("links_deref");
            let d = dir.path("d");
            fs::create_dir(&d).unwrap();
            fs::write(dir.path("real"), b"body").unwrap();
            let (la, lb) = (dir.path("la"), dir.path("lb"));
            std::os::unix::fs::symlink("real", &la).unwrap();
            std::os::unix::fs::symlink("real", &lb).unwrap();

            let (ok, err) = cp(&flags, &[&la, &lb, &d]);
            assert!(ok, "{err}");
            assert_eq!(ino(&d.join("la")), ino(&d.join("lb")), "{flags:?}");
            assert!(!d.join("lb").symlink_metadata().unwrap().is_symlink());
        }
    }

    /// And with `-P` the same two symlinks are two symlinks: nothing is
    /// dereferenced, so the operands are the links, and two links with one
    /// target are still two files.
    #[cfg(unix)]
    #[test]
    fn no_dereference_does_not_link_two_separate_symlinks() {
        let dir = scratch("links_P_two");
        let d = dir.path("d");
        fs::create_dir(&d).unwrap();
        fs::write(dir.path("real"), b"body").unwrap();
        let (la, lb) = (dir.path("la"), dir.path("lb"));
        std::os::unix::fs::symlink("real", &la).unwrap();
        std::os::unix::fs::symlink("real", &lb).unwrap();

        let flags = CpFlags {
            dereference: Deref::Never,
            ..links()
        };
        let (ok, err) = cp(&flags, &[&la, &lb, &d]);
        assert!(ok, "{err}");
        assert_ne!(ino(&d.join("la")), ino(&d.join("lb")));
    }

    /// Two hard links to one *symlink*, with `-P`. The operands are one file
    /// with `st_nlink == 2`, so the table fires — and what it must produce is a
    /// second hard link to the copied **symlink**, not to the symlink's target.
    /// That is `linkat` with flags `0`, which is what [`fs::hard_link`] gives;
    /// `AT_SYMLINK_FOLLOW` here would silently write a link to `real` instead.
    #[cfg(unix)]
    #[test]
    fn no_dereference_links_two_names_for_one_symlink() {
        let dir = scratch("links_P_one");
        let d = dir.path("d");
        fs::create_dir(&d).unwrap();
        fs::write(dir.path("real"), b"body").unwrap();
        let (la, lb) = (dir.path("la"), dir.path("lb"));
        std::os::unix::fs::symlink("real", &la).unwrap();
        fs::hard_link(&la, &lb).unwrap();

        let flags = CpFlags {
            dereference: Deref::Never,
            ..links()
        };
        let (ok, err) = cp(&flags, &[&la, &lb, &d]);
        assert!(ok, "{err}");
        assert_eq!(ino(&d.join("la")), ino(&d.join("lb")));
        assert!(
            d.join("lb").symlink_metadata().unwrap().is_symlink(),
            "the link is to the symlink, not through it"
        );
        assert_eq!(hard_links(&fs::symlink_metadata(d.join("la")).unwrap()), 2);
    }

    /// A source that fails to copy is *forgotten*, so the next name for it is
    /// tried on its own merits rather than linked to a destination that was
    /// never written. GNU does this at its `un_backup:` label, which calls
    /// `forget_created` when the file was not an `earlier_file`.
    ///
    /// The observable difference is the second diagnostic: forgotten, it is
    /// the same "cannot open for reading" again; remembered, it would be
    /// "cannot create hard link" naming a destination that does not exist.
    #[cfg(unix)]
    #[test]
    fn a_source_that_failed_to_copy_is_forgotten() {
        if root() {
            return;
        }
        use std::os::unix::fs::PermissionsExt as _;

        let dir = scratch("links_forget");
        let d = dir.path("d");
        fs::create_dir(&d).unwrap();
        let (a, b) = (dir.path("a"), dir.path("b"));
        fs::write(&a, b"body").unwrap();
        fs::hard_link(&a, &b).unwrap();
        fs::set_permissions(&a, fs::Permissions::from_mode(0o000)).unwrap();

        let (ok, out, err) = cp_out(&links_v(), &[&a, &b, &d]);
        assert!(!ok, "an unreadable source is a failure");
        assert_eq!(out.lines().count(), 2, "both were announced: {out}");
        assert_eq!(
            err.matches("for reading").count(),
            2,
            "the second failure is its own, not a link failure: {err}"
        );
        assert!(!d.join("a").exists() && !d.join("b").exists());
    }

    /// A destination reached by *linking* is not recorded as one this command
    /// created, so a later operand overwrites it silently where the same
    /// command without the option would refuse.
    ///
    /// This is a gap in GNU rather than a design: the non-directory
    /// `earlier_file` branch returns before `record_file` is reached, so the
    /// linked destination never enters `dest_info`. Measured, not inferred —
    /// `cp --preserve=links a b o/b d` exits 0 and `d/b` holds `o/b`'s bytes.
    #[cfg(unix)]
    #[test]
    fn a_linked_destination_is_not_recorded_as_created() {
        let dir = scratch("links_dest_info");
        let d = dir.path("d");
        let o = dir.path("o");
        fs::create_dir(&d).unwrap();
        fs::create_dir(&o).unwrap();
        let (a, b) = (dir.path("a"), dir.path("b"));
        fs::write(&a, b"body").unwrap();
        fs::hard_link(&a, &b).unwrap();
        fs::write(o.join("b"), b"other").unwrap();
        let other = o.join("b");

        let (ok, err) = cp(&links(), &[&a, &b, &other, &d]);
        assert!(ok, "no refusal, because `d/b` was never recorded: {err}");
        assert_eq!(err, "");
        assert_eq!(fs::read(d.join("b")).unwrap(), b"other");

        // Without the option, `d/b` *is* recorded, and the third operand is
        // refused. The two halves are the same command but for the option.
        let dir = scratch("links_dest_info_off");
        let d = dir.path("d");
        let o = dir.path("o");
        fs::create_dir(&d).unwrap();
        fs::create_dir(&o).unwrap();
        let (a, b) = (dir.path("a"), dir.path("b"));
        fs::write(&a, b"body").unwrap();
        fs::hard_link(&a, &b).unwrap();
        fs::write(o.join("b"), b"other").unwrap();
        let other = o.join("b");

        let (ok, err) = cp(&plain(), &[&a, &b, &other, &d]);
        assert!(!ok);
        assert!(err.contains("will not overwrite just-created"), "{err}");
        assert_eq!(fs::read(d.join("b")).unwrap(), b"body");
    }

    // ------------------------------------------------- modes, module docs 8 --
    //
    // These four run only on a POSIX host, because there is nothing on Windows
    // for them to assert about. `scripts/cp-diff.sh` is what certifies the same
    // behaviour against GNU itself; these exist so that a regression is caught
    // by `cargo test` on the target rather than only by a harness that needs a
    // GNU userland to run.

    // The mask itself, which only these tests set. Reading it is
    // `coreutils::umask::current`, which does not go through this call at all
    // — the point of that module.
    //
    // SAFETY (declaration): `umask` is POSIX, takes and returns `mode_t`, and
    // has no failure mode. `mode_t` is `u32` on Linux and on x86_64-slateos.
    #[cfg(unix)]
    unsafe extern "C" {
        fn umask(mask: u32) -> u32;
    }

    /// Set the umask, run `f`, put the umask back.
    ///
    /// The umask is process-wide and `cargo test` runs tests on threads of one
    /// process, so two tests doing this at once would each see the other's
    /// mask. The lock makes them take turns. It does *not* protect against an
    /// unrelated test creating a file while a mask is installed — nothing can,
    /// short of running single-threaded — which is why these are the only tests
    /// in this file that assert a mode at all.
    ///
    /// Every mask installed here leaves owner read and write set (`0022`,
    /// `0077`, `0002`, `0000`), so a file another test creates inside the
    /// window is still usable by the test that created it. That is deliberate,
    /// and is the difference between this and the all-denying `umask(0777)`
    /// probe that used to sit in `cached_umask`.
    #[cfg(unix)]
    fn with_umask<T>(mask: u32, f: impl FnOnce() -> T) -> T {
        use std::sync::Mutex;
        static TURN: Mutex<()> = Mutex::new(());
        // A poisoned lock means another umask test panicked; its `old` was
        // restored on unwind only if it got that far, so the mask may be
        // whatever it left. Proceeding is still the right call: the panic will
        // already be reported, and refusing here would hide this test's result
        // behind that one.
        let _guard = TURN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: `umask` cannot fail, takes and returns a plain integer, and
        // touches no memory.
        let old = unsafe { umask(mask) };
        let out = f();
        // SAFETY: as above.
        unsafe { umask(old) };
        out
    }

    #[cfg(unix)]
    fn mode_of(p: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(p).unwrap().permissions().mode() & 0o7777
    }

    #[cfg(unix)]
    fn set_test_mode(p: &Path, m: u32) {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(p, fs::Permissions::from_mode(m)).unwrap();
    }

    /// A new destination is created with the source's mode *narrowed by the
    /// umask* — which is what the kernel does when the mode is passed to
    /// `open`, and what `fs::copy`'s trailing `chmod` undid.
    ///
    /// The expectations are measured, not derived: each row was produced by
    /// GNU `cp` under WSL before it was written down here.
    #[cfg(unix)]
    #[test]
    fn mode_of_a_new_file_is_narrowed_by_umask() {
        // (umask, source mode, what GNU produces)
        let rows: &[(u32, u32, u32)] = &[
            (0o022, 0o777, 0o755),
            (0o022, 0o600, 0o600),
            (0o000, 0o777, 0o777),
            (0o077, 0o777, 0o700),
            (0o077, 0o600, 0o600),
        ];
        let dir = scratch("file_mode");
        for (i, &(mask, src_mode, want)) in rows.iter().enumerate() {
            let a = dir.path(&format!("a{i}"));
            let b = dir.path(&format!("b{i}"));
            fs::write(&a, b"x").unwrap();
            set_test_mode(&a, src_mode);
            let (ok, err) = with_umask(mask, || cp(&plain(), &[&a, &b]));
            assert!(ok, "{err}");
            assert_eq!(mode_of(&b), want, "umask {mask:04o}, source {src_mode:04o}");
        }
    }

    /// The security half of module docs, bug 8: copying a wide file over a
    /// narrow one must not widen the narrow one. GNU reopens an existing
    /// destination with no mode argument at all.
    #[cfg(unix)]
    #[test]
    fn an_existing_destination_keeps_its_own_mode() {
        let dir = scratch("keep_mode");
        let a = dir.path("public");
        let b = dir.path("private");
        fs::write(&a, b"wide").unwrap();
        set_test_mode(&a, 0o777);
        fs::write(&b, b"narrow").unwrap();
        set_test_mode(&b, 0o600);

        let (ok, err) = cp(&plain(), &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(fs::read(&b).unwrap(), b"wide", "contents are copied");
        assert_eq!(mode_of(&b), 0o600, "permissions are not");
    }

    /// A copied directory ends at `src & 07777 & ~umask`, sticky bit included
    /// — 1777 under 022 is 1755, which is what GNU produces and what the
    /// verbatim mode carry-over got wrong.
    #[cfg(unix)]
    #[test]
    fn mode_of_a_copied_directory_is_narrowed_by_umask() {
        let rows: &[(u32, u32, u32)] = &[
            (0o022, 0o777, 0o755),
            (0o022, 0o1777, 0o1755),
            (0o000, 0o1777, 0o1777),
            (0o077, 0o1777, 0o1700),
            (0o022, 0o700, 0o700),
        ];
        let dir = scratch("dir_mode");
        for (i, &(mask, src_mode, want)) in rows.iter().enumerate() {
            let a = dir.path(&format!("s{i}"));
            let b = dir.path(&format!("d{i}"));
            fs::create_dir(&a).unwrap();
            fs::write(a.join("inner"), b"x").unwrap();
            set_test_mode(&a, src_mode);
            let (ok, err) = with_umask(mask, || cp(&recursive(), &[&a, &b]));
            assert!(ok, "{err}");
            assert_eq!(mode_of(&b), want, "umask {mask:04o}, source {src_mode:04o}");
            assert!(b.join("inner").is_file(), "and it was actually filled");
        }
    }

    /// A source directory the copy cannot write into must still be filled: the
    /// mode goes on last, not first. 0500 is the case that mattered — it has no
    /// owner-write, so a copy that set the mode at `mkdir` time could not put
    /// anything inside what it had just made.
    #[cfg(unix)]
    #[test]
    fn a_read_only_source_directory_is_copied_whole_and_ends_read_only() {
        let dir = scratch("ro_dir");
        let a = dir.path("src");
        let b = dir.path("dst");
        fs::create_dir(&a).unwrap();
        fs::write(a.join("inner"), b"x").unwrap();
        set_test_mode(&a, 0o500);

        let (ok, err) = cp(&recursive(), &[&a, &b]);
        assert!(ok, "{err}");
        assert!(b.join("inner").is_file(), "contents got in");
        assert_eq!(mode_of(&b), 0o500, "and the mode went on afterwards");

        // Leave both writable again so the scratch directory can be removed.
        set_test_mode(&a, 0o700);
        set_test_mode(&b, 0o700);
    }

    // ----------------------------------------------------- -p on the disk --
    //
    // `scripts/cp-diff.sh` section 17 is what certifies these against GNU
    // itself, with the times in the comparison. These exist so that a
    // regression is caught by `cargo test` on the target — where there is no
    // GNU userland to compare with — and so that the *reason* each answer is
    // what it is stays next to the code.

    /// `-p` and nothing else, spelled as a value so a test can say what it
    /// means rather than which letters produce it.
    #[cfg(unix)]
    fn preserve() -> CpFlags {
        CpFlags {
            preserve: Preserve::posix(),
            require_preserve: true,
            ..off()
        }
    }
    /// `-rp`.
    #[cfg(unix)]
    fn preserve_r() -> CpFlags {
        CpFlags {
            recursive: true,
            preserve: Preserve::posix(),
            require_preserve: true,
            ..off()
        }
    }
    /// `--preserve=xattr`: the one spelling that insists.
    #[cfg(unix)]
    fn xattr_only() -> CpFlags {
        CpFlags {
            preserve: Preserve {
                xattr: true,
                ..Preserve::NONE
            },
            require_preserve: true,
            require_preserve_xattr: true,
            ..off()
        }
    }
    /// `--preserve=all`: everything, best-effort about the attributes.
    #[cfg(unix)]
    fn preserve_all() -> CpFlags {
        CpFlags {
            preserve: Preserve::ALL,
            require_preserve: true,
            ..off()
        }
    }
    /// `-a`: `PRESERVE_ALL` plus `-dR`, and silent about an attribute it could
    /// not carry.
    #[cfg(unix)]
    fn archive() -> CpFlags {
        CpFlags {
            recursive: true,
            dereference: Deref::Never,
            preserve: Preserve::ALL,
            require_preserve: true,
            reduce_diagnostics: true,
            ..off()
        }
    }
    /// The three above with `-RT`, for the tests whose destination has to be an
    /// existing directory *itself* rather than a directory to copy into.
    #[cfg(unix)]
    fn xattr_only_rt() -> CpFlags {
        CpFlags {
            recursive: true,
            no_target_directory: true,
            ..xattr_only()
        }
    }
    #[cfg(unix)]
    fn preserve_all_rt() -> CpFlags {
        CpFlags {
            recursive: true,
            no_target_directory: true,
            ..preserve_all()
        }
    }
    #[cfg(unix)]
    fn archive_t() -> CpFlags {
        CpFlags {
            no_target_directory: true,
            ..archive()
        }
    }

    #[cfg(unix)]
    fn mtime_of(p: &Path) -> std::time::SystemTime {
        fs::symlink_metadata(p).unwrap().modified().unwrap()
    }

    /// Give `p` a modification time far enough in the past that "the copy kept
    /// it" cannot be confused with "the copy happened to run at that instant".
    ///
    /// The nanoseconds are not round, which is the point: a `set_times` that
    /// carried the seconds and dropped the fraction would pass against a whole
    /// second and fails against this.
    #[cfg(unix)]
    fn stamp(p: &Path, secs: u64) -> std::time::SystemTime {
        let t = std::time::UNIX_EPOCH + std::time::Duration::new(secs, 123_456_789);
        fsattr::set_times(On::Path(p, Link::NoFollow), fsattr::Times::both(t)).unwrap();
        t
    }

    /// The three attributes together on a new regular destination. The mode is
    /// the source's *whole* mode and is not narrowed by the umask — which is
    /// the one thing separating a preserved mode from a fresh file's, and the
    /// reason a 0077 mask is installed here.
    #[cfg(unix)]
    #[test]
    fn preserve_keeps_the_mode_and_the_time_against_the_umask() {
        let dir = scratch("p_file");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"x").unwrap();
        set_test_mode(&a, 0o741);
        let want = stamp(&a, 1_000_000_000);

        let (ok, err) = with_umask(0o077, || cp(&preserve(), &[&a, &b]));
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert_eq!(mode_of(&b), 0o741, "the umask does not apply to -p");
        assert_eq!(mtime_of(&b), want);
    }

    /// The set-user-ID bit specifically, because it is the one attribute whose
    /// restoration is order-dependent: a `chmod` written before the `chown`
    /// loses it for every non-root user, silently and on every copy.
    #[cfg(unix)]
    #[test]
    fn preserve_keeps_the_setuid_bit() {
        let dir = scratch("p_suid");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"x").unwrap();
        set_test_mode(&a, 0o4755);

        let (ok, err) = cp(&preserve(), &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(mode_of(&b), 0o4755);
    }

    /// One attribute at a time. Each row asks for exactly one word and asserts
    /// that the *other* one did not happen — a `--preserve=mode` that quietly
    /// restored the times as well would pass every test that only ran `-p`.
    #[cfg(unix)]
    #[test]
    fn each_preserved_attribute_acts_alone() {
        let only_mode = CpFlags {
            preserve: Preserve {
                mode: true,
                ..Preserve::NONE
            },
            require_preserve: true,
            ..off()
        };
        let only_times = CpFlags {
            preserve: Preserve {
                timestamps: true,
                ..Preserve::NONE
            },
            require_preserve: true,
            ..off()
        };
        let dir = scratch("p_alone");

        let a = dir.path("a");
        fs::write(&a, b"x").unwrap();
        set_test_mode(&a, 0o741);
        let then = stamp(&a, 1_000_000_000);

        let m = dir.path("m");
        let (ok, err) = with_umask(0o022, || cp(&only_mode, &[&a, &m]));
        assert!(ok, "{err}");
        assert_eq!(mode_of(&m), 0o741, "the mode is the source's");
        assert_ne!(mtime_of(&m), then, "and the time is not");

        let t = dir.path("t");
        let (ok, err) = with_umask(0o022, || cp(&only_times, &[&a, &t]));
        assert!(ok, "{err}");
        assert_eq!(mtime_of(&t), then, "the time is the source's");
        assert_eq!(mode_of(&t), 0o741 & !0o022, "and the mode is a new file's");
    }

    /// A directory destination is never unlinked to clear the way, whichever
    /// of the two reasons asked for the way to be cleared. GNU puts both
    /// inside `else if (! S_ISDIR (dst_sb.st_mode) && …)` (`copy.c:2539`), and
    /// the link-count reason needs that guard on every system: a directory on
    /// ext4 has two links before anything else points at it, so
    /// `--preserve=links` would otherwise try to `unlink` every existing
    /// directory destination it was handed.
    #[cfg(unix)]
    #[test]
    fn a_directory_destination_is_not_unlinked_to_clear_the_way() {
        let links_t = CpFlags {
            no_target_directory: true,
            preserve: Preserve {
                links: true,
                ..Preserve::NONE
            },
            require_preserve: true,
            ..off()
        };
        let symlink_t = CpFlags {
            no_target_directory: true,
            dereference: Deref::Never,
            ..off()
        };
        let dir = scratch("d_keep");
        let a = dir.path("a");
        fs::write(&a, b"x").unwrap();
        let l = dir.path("l");
        std::os::unix::fs::symlink("a", &l).unwrap();

        for (what, flags) in [(&a, links_t), (&l, symlink_t)] {
            let d = dir.path("d");
            fs::create_dir(&d).unwrap();
            let (ok, err) = cp(&flags, &[what, &d]);
            assert!(!ok, "{}: {err}", what.display());
            assert!(
                err.contains("cannot overwrite directory"),
                "the refusal is about the kinds, not about `unlink`: {err:?}"
            );
            assert!(d.is_dir(), "and the directory is still there");
            fs::remove_dir(&d).unwrap();
        }
    }

    // ------------------------------------------------ extended attributes --

    /// Put a `user.` attribute on a file, or say the filesystem underneath the
    /// scratch directory has none. `/tmp` is usually ext4 and usually does; a
    /// tmpfs built without `CONFIG_TMPFS_XATTR` does not, and a test that
    /// failed there would be reporting the kernel's build options rather than
    /// this `cp`.
    #[cfg(unix)]
    fn seed_xattr(path: &Path, name: &[u8], value: &[u8]) -> bool {
        fsattr::set_xattr(On::Path(path, Link::NoFollow), name, value).is_ok()
    }

    /// What `path` has under `name`, or `None` if it has nothing.
    #[cfg(unix)]
    fn xattr_of(path: &Path, name: &[u8]) -> Option<Vec<u8>> {
        fsattr::get_xattr(On::Path(path, Link::NoFollow), name).ok()
    }

    /// The whole point of the option, on all three spellings that ask for it —
    /// and the byte string is deliberately not text: an attribute's value is
    /// arbitrary bytes, and a copy that round-tripped it through UTF-8 would
    /// corrupt exactly this.
    #[cfg(unix)]
    #[test]
    fn an_extended_attribute_crosses_under_every_option_that_asks() {
        const VALUE: &[u8] = b"\x00\xff\x80not text";
        let asked = [
            ("--preserve=xattr", xattr_only()),
            ("--preserve=all", preserve_all()),
            ("-a", archive()),
        ];
        for (spelling, flags) in asked {
            let dir = scratch("x_cross");
            let (a, b) = (dir.path("a"), dir.path("b"));
            fs::write(&a, b"body").unwrap();
            if !seed_xattr(&a, b"user.tag", VALUE) {
                return;
            }

            let (ok, err) = cp(&flags, &[&a, &b]);
            assert!(ok, "{spelling}: {err}");
            assert_eq!(err, "", "{spelling}");
            assert_eq!(
                xattr_of(&b, b"user.tag").as_deref(),
                Some(VALUE),
                "{spelling} did not carry the attribute"
            );
        }
    }

    /// And it does not cross otherwise. `-p` is three attributes and extended
    /// ones are not among them, so a `cp -p` that carried them would be doing
    /// something the user did not ask for — on every copy, at the cost of a
    /// `listxattr` per file.
    #[cfg(unix)]
    #[test]
    fn an_extended_attribute_stays_behind_when_it_was_not_asked_for() {
        let dir = scratch("x_nocross");
        let (a, b) = (dir.path("a"), dir.path("b"));
        fs::write(&a, b"body").unwrap();
        if !seed_xattr(&a, b"user.tag", b"v") {
            return;
        }

        let (ok, err) = cp(&preserve(), &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(
            xattr_of(&b, b"user.tag"),
            None,
            "-p is not --preserve=xattr"
        );
    }

    /// The three levels of loudness, on a failure that is neither hypothetical
    /// nor racy: an existing destination directory the user may read and search
    /// but not write. `setxattr` wants write permission on the file, so the
    /// attribute cannot be set — while the copy itself, which writes nothing
    /// into an empty tree, succeeds.
    ///
    /// | Asked for | Says | Exits |
    /// |---|---|---|
    /// | `--preserve=xattr` | the failure | 1 |
    /// | `--preserve=all` | the failure (`EACCES` is not "unsupported") | 0 |
    /// | `-a` | nothing | 0 |
    #[cfg(unix)]
    #[test]
    fn how_loudly_a_failed_attribute_is_reported_is_the_option_that_asked() {
        if chown_privileges() {
            return; // root may write to a directory whose mode forbids it.
        }
        let rows: [(&str, CpFlags, bool, bool); 3] = [
            ("--preserve=xattr", xattr_only_rt(), true, false),
            ("--preserve=all", preserve_all_rt(), true, true),
            ("-a", archive_t(), false, true),
        ];
        for (spelling, flags, speaks, succeeds) in rows {
            let dir = scratch("x_loud");
            let (a, b) = (dir.path("a"), dir.path("b"));
            fs::create_dir(&a).unwrap();
            fs::create_dir(&b).unwrap();
            if !seed_xattr(&a, b"user.tag", b"v") {
                return;
            }
            set_test_mode(&b, 0o555);

            let (ok, err) = cp(&flags, &[&a, &b]);
            // Restored before the assertions so a failure still cleans up.
            set_test_mode(&b, 0o755);
            assert_eq!(ok, succeeds, "{spelling}: {err}");
            if speaks {
                assert!(
                    err.contains("setting attribute 'user.tag' for"),
                    "{spelling}: {err:?}"
                );
            } else {
                assert_eq!(err, "", "{spelling} is the quiet one");
            }
        }
    }

    /// `--preserve=ownership` is the one option that makes a *regular* file
    /// carry a mode debt: GNU withholds `src & 0077` at creation so that the
    /// window between `open` and `chown` cannot be entered by the group or by
    /// anyone else, and puts it back afterwards. A copy that forgot the second
    /// half leaves 0700 where 0741 belongs.
    #[cfg(unix)]
    #[test]
    fn preserving_ownership_puts_the_withheld_bits_back() {
        let flags = CpFlags {
            preserve: Preserve {
                ownership: true,
                ..Preserve::NONE
            },
            require_preserve: true,
            ..off()
        };
        let dir = scratch("p_own");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"x").unwrap();
        set_test_mode(&a, 0o741);

        let (ok, err) = with_umask(0o000, || cp(&flags, &[&a, &b]));
        assert!(ok, "{err}");
        assert_eq!(mode_of(&b), 0o741);
    }

    /// `-p` over an *existing* destination replaces its mode, where a plain
    /// copy leaves it alone — [`an_existing_destination_keeps_its_own_mode`] is
    /// the other half of the same question.
    #[cfg(unix)]
    #[test]
    fn preserve_replaces_an_existing_destinations_mode_and_time() {
        let dir = scratch("p_over");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"new").unwrap();
        set_test_mode(&a, 0o741);
        let want = stamp(&a, 1_000_000_000);
        fs::write(&b, b"old").unwrap();
        set_test_mode(&b, 0o600);
        stamp(&b, 1_400_000_000);

        let (ok, err) = cp(&preserve(), &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(fs::read(&b).unwrap(), b"new");
        assert_eq!(mode_of(&b), 0o741);
        assert_eq!(mtime_of(&b), want);
    }

    /// A whole tree. The directory's own time is stamped *after* it is filled,
    /// because writing an entry into a directory moves its modification time —
    /// a `-p` that stamped it first would have the stamp overwritten by the
    /// next `mkdir` inside and leave the copy carrying the time of the copy.
    #[cfg(unix)]
    #[test]
    fn preserve_stamps_a_directory_after_filling_it() {
        let dir = scratch("p_tree");
        let src = dir.path("src");
        let dst = dir.path("dst");
        let sub = src.join("sub");
        fs::create_dir(&src).unwrap();
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("f"), b"x").unwrap();
        set_test_mode(&sub, 0o741);
        set_test_mode(&src, 0o755);
        // Innermost first: writing into a directory moves its own time, so
        // stamping `src` before `src/sub` existed would stamp it and then have
        // the `mkdir` undo it — the same ordering the code under test has to
        // get right, which is why it is worth stating here too.
        let inner = stamp(&sub, 1_000_000_000);
        let outer = stamp(&src, 1_100_000_000);

        let (ok, err) = with_umask(0o022, || cp(&preserve_r(), &[&src, &dst]));
        assert!(ok, "{err}");
        assert!(dst.join("sub").join("f").is_file(), "and it was filled");
        assert_eq!(mtime_of(&dst.join("sub")), inner);
        assert_eq!(mtime_of(&dst), outer, "the outer one too");
        assert_eq!(
            mode_of(&dst.join("sub")),
            0o741,
            "the umask does not apply to -p, at any depth"
        );
    }

    /// A 0500 source directory under `-p`: it has no owner-write, so the copy
    /// has to be forced open to be filled and then put back — *after* the
    /// ownership step, not instead of it. This is the case that failed when the
    /// settle-up chmod still lived in `copy_tree`.
    #[cfg(unix)]
    #[test]
    fn preserve_restores_a_read_only_directory_last() {
        let dir = scratch("p_ro");
        let src = dir.path("src");
        let dst = dir.path("dst");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("f"), b"x").unwrap();
        set_test_mode(&src, 0o500);

        let (ok, err) = cp(&preserve_r(), &[&src, &dst]);
        assert!(ok, "{err}");
        assert!(dst.join("f").is_file(), "contents got in");
        assert_eq!(mode_of(&dst), 0o500);

        set_test_mode(&src, 0o700);
        set_test_mode(&dst, 0o700);
    }

    /// `-P -p` on a symbolic link keeps the *link's* own times and does not try
    /// to chmod it — nothing portable can, and a `cp` that tried would fail on
    /// Linux, which has no working `lchmod` at all.
    #[cfg(unix)]
    #[test]
    fn preserve_stamps_a_symlink_without_chmodding_it() {
        let flags = CpFlags {
            dereference: Deref::Never,
            preserve: Preserve::posix(),
            require_preserve: true,
            ..off()
        };
        let dir = scratch("p_link");
        let target = dir.path("target");
        let link = dir.path("link");
        let copy = dir.path("copy");
        fs::write(&target, b"x").unwrap();
        std::os::unix::fs::symlink("target", &link).unwrap();
        let want = stamp(&link, 1_000_000_000);

        let (ok, err) = cp(&flags, &[&link, &copy]);
        assert!(ok, "{err}");
        assert_eq!(err, "", "and nothing was said about the mode");
        assert!(fs::symlink_metadata(&copy).unwrap().is_symlink());
        assert_eq!(mtime_of(&copy), want);
    }

    /// `--no-preserve=mode` on a destination this run created is not the same
    /// as never having asked: it gives 0666 for a file and 0777 for a
    /// directory, each less the umask, where a plain copy gives the source's
    /// mode less the umask. 0700 is the source that tells the two apart.
    #[cfg(unix)]
    #[test]
    fn no_preserve_mode_gives_a_new_destination_the_default() {
        let flags = CpFlags {
            explicit_no_preserve_mode: true,
            ..off()
        };
        let flags_r = CpFlags {
            recursive: true,
            explicit_no_preserve_mode: true,
            ..off()
        };
        let dir = scratch("no_p_mode");

        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"x").unwrap();
        set_test_mode(&a, 0o700);
        let (ok, err) = with_umask(0o022, || cp(&flags, &[&a, &b]));
        assert!(ok, "{err}");
        assert_eq!(mode_of(&b), 0o644, "0666 & ~022, not the source's 0700");

        let src = dir.path("src");
        let dst = dir.path("dst");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("f"), b"x").unwrap();
        set_test_mode(&src, 0o700);
        let (ok, err) = with_umask(0o022, || cp(&flags_r, &[&src, &dst]));
        assert!(ok, "{err}");
        assert_eq!(mode_of(&dst), 0o755, "0777 & ~022 for a directory");
    }

    /// …but only on a destination this run created. An existing one keeps its
    /// own mode, exactly as it does without the option: GNU's branch is
    /// `explicit_no_preserve_mode && new_dst`.
    #[cfg(unix)]
    #[test]
    fn no_preserve_mode_leaves_an_existing_destination_alone() {
        let flags = CpFlags {
            explicit_no_preserve_mode: true,
            ..off()
        };
        let dir = scratch("no_p_over");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"new").unwrap();
        set_test_mode(&a, 0o700);
        fs::write(&b, b"old").unwrap();
        set_test_mode(&b, 0o600);

        let (ok, err) = with_umask(0o022, || cp(&flags, &[&a, &b]));
        assert!(ok, "{err}");
        assert_eq!(fs::read(&b).unwrap(), b"new");
        assert_eq!(mode_of(&b), 0o600);
    }

    /// Module docs, bug 7. The assertion that matters is not the message but
    /// `fs::read`: the defect this pins reported success and said nothing, so a
    /// test that only checked the diagnostic would have passed against it.
    #[test]
    fn copying_a_file_onto_itself_is_refused_and_leaves_it_whole() {
        let dir = scratch("same_file");
        let a = dir.path("a");
        fs::write(&a, b"contents").unwrap();
        let (ok, err) = cp(&plain(), &[&a, &a]);
        assert!(!ok, "should have been refused");
        assert!(err.contains("are the same file"), "{err}");
        assert_eq!(fs::read(&a).unwrap(), b"contents", "the file must survive");
    }

    /// The same file reached by a second spelling. A string comparison of the
    /// two operands would let this one through, which is why there is not one.
    #[test]
    fn a_file_onto_itself_by_another_path_is_refused() {
        let dir = scratch("same_file_dotted");
        let a = dir.path("a");
        let sub = dir.path("sub");
        fs::write(&a, b"contents").unwrap();
        fs::create_dir(&sub).unwrap();
        let dotted = sub.join("..").join("a");
        let (ok, err) = cp(&plain(), &[&a, &dotted]);
        assert!(!ok, "should have been refused: {err}");
        assert!(err.contains("are the same file"), "{err}");
        assert_eq!(fs::read(&a).unwrap(), b"contents", "the file must survive");
    }

    /// A destination that merely *exists* is not the same file, and must still
    /// be overwritten. Without this the refusal above would be a way of
    /// breaking `cp` for every ordinary overwrite.
    #[test]
    fn an_existing_different_destination_is_still_overwritten() {
        let dir = scratch("same_file_neg");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"new").unwrap();
        fs::write(&b, b"old").unwrap();
        let (ok, err) = cp(&plain(), &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert_eq!(fs::read(&b).unwrap(), b"new");
    }

    #[test]
    fn no_operands_names_the_missing_thing() {
        let (ok, err) = cp(&plain(), &[]);
        assert!(!ok);
        assert!(err.contains("missing file operand"), "{err}");
    }

    #[test]
    fn one_operand_names_it() {
        let (ok, err) = cp(&plain(), &[Path::new("solo")]);
        assert!(!ok);
        assert!(err.contains("missing destination file operand"), "{err}");
        assert!(err.contains("solo"), "{err}");
    }

    #[test]
    fn a_directory_needs_recursive() {
        let dir = scratch("needs_r");
        let sub = dir.path("sub");
        fs::create_dir(&sub).unwrap();
        let (ok, err) = cp(&plain(), &[&sub, &dir.path("copy")]);
        assert!(!ok);
        assert!(err.contains("omitting directory"), "{err}");
        assert!(!dir.path("copy").exists());
    }

    #[test]
    fn copies_a_tree() {
        let dir = scratch("tree");
        let src = dir.path("src");
        fs::create_dir_all(src.join("deep/deeper")).unwrap();
        fs::write(src.join("top"), b"1").unwrap();
        fs::write(src.join("deep/mid"), b"2").unwrap();
        fs::write(src.join("deep/deeper/bottom"), b"3").unwrap();

        let dst = dir.path("dst");
        let (ok, err) = cp(&recursive(), &[&src, &dst]);
        assert!(ok, "{err}");
        assert_eq!(fs::read(dst.join("top")).unwrap(), b"1");
        assert_eq!(fs::read(dst.join("deep/mid")).unwrap(), b"2");
        assert_eq!(fs::read(dst.join("deep/deeper/bottom")).unwrap(), b"3");
    }

    #[test]
    fn a_failure_does_not_abort_the_rest() {
        let dir = scratch("partial");
        let sub = dir.path("sub");
        fs::create_dir(&sub).unwrap();
        let a = dir.path("a");
        let c = dir.path("c");
        fs::write(&a, b"a").unwrap();
        fs::write(&c, b"c").unwrap();
        let (ok, err) = cp(&plain(), &[&a, &dir.path("gone"), &c, &sub]);
        assert!(!ok, "the missing source must count against the status");
        assert!(err.contains("gone"), "{err}");
        assert!(sub.join("a").is_file(), "the first source must still copy");
        assert!(
            sub.join("c").is_file(),
            "and so must the one after the error"
        );
    }

    /// Bug 2 in the module docs. Before the fix this filled the disk.
    #[test]
    fn refuses_to_copy_a_directory_into_itself() {
        let dir = scratch("into_itself");
        let src = dir.path("src");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("f"), b"x").unwrap();

        // `cp -r src src` — the target resolves to `src/src`.
        let (ok, err) = cp(&recursive(), &[&src, &src]);
        assert!(!ok);
        assert!(err.contains("into itself"), "{err}");
        assert!(!src.join("src").exists());

        // `cp -r src src/nested` — the same thing spelled differently.
        let (ok, err) = cp(&recursive(), &[&src, &src.join("nested")]);
        assert!(!ok, "{err}");
        assert!(err.contains("into itself"), "{err}");
    }

    /// Bug 5, end to end. `inner/..` is the scratch directory itself and the
    /// destination is inside it, so the copy is refused — but by the
    /// *into-itself* rule, on a target of `dst/.`, and not by a refusal to name
    /// the source at all. Measured against GNU, which says
    /// `cannot copy a directory, '<dir>/inner/..', into itself, '<dir>/dst/.'`.
    #[test]
    fn a_dotdot_source_targets_the_destination_and_not_its_parent() {
        let dir = scratch("dotdot");
        let inner = dir.path("inner");
        let dst = dir.path("dst");
        fs::create_dir(&inner).unwrap();
        fs::create_dir(&dst).unwrap();
        fs::write(dir.path("sibling"), b"x").unwrap();

        let (ok, err) = cp(&recursive(), &[&inner.join(".."), &dst]);
        assert!(!ok);
        assert!(err.contains("into itself"), "{err}");
        assert!(
            err.contains(&format!("{}", dst.join(".").display())),
            "the target is the destination itself, not its parent: {err}"
        );
        assert!(
            !dir.dir().parent().unwrap().join("sibling").exists(),
            "nothing may be written beside the destination's parent"
        );
    }

    /// The idiom the first fix for bug 5 broke: `cp -r a/. dst` fills `dst`
    /// with `a`'s contents rather than creating `dst/a`.
    #[test]
    fn a_dot_source_copies_the_contents_into_the_destination() {
        let dir = scratch("dotsrc");
        let src = dir.path("src");
        let dst = dir.path("dst");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::create_dir(&dst).unwrap();
        fs::write(src.join("sub").join("f"), b"x").unwrap();

        let (ok, err) = cp(&recursive(), &[&src.join("."), &dst]);
        assert!(ok, "{err}");
        assert!(dst.join("sub").join("f").is_file(), "{err}");
        assert!(
            !dst.join("src").exists(),
            "the source's own name must not appear inside the destination"
        );
    }

    /// Bug 1 in the module docs, the non-terminating half. `loop` points at its
    /// own parent, so following it descends for ever. With `file_type()` the walk
    /// copies the link and stops.
    #[test]
    #[cfg(unix)]
    fn a_symlink_loop_in_the_tree_does_not_recurse_for_ever() {
        let dir = scratch("loop");
        let src = dir.path("src");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("sub/f"), b"x").unwrap();
        std::os::unix::fs::symlink("..", src.join("sub/loop")).unwrap();

        let dst = dir.path("dst");
        let (ok, err) = cp(&recursive(), &[&src, &dst]);
        assert!(ok, "{err}");
        assert_eq!(fs::read(dst.join("sub/f")).unwrap(), b"x");
        let link = fs::symlink_metadata(dst.join("sub/loop")).unwrap();
        assert!(
            link.file_type().is_symlink(),
            "the loop must arrive as a link, not as a copied subtree"
        );
        assert_eq!(
            fs::read_link(dst.join("sub/loop")).unwrap(),
            Path::new("..")
        );
    }

    /// Bug 1's other half: a link in the tree used to become a full copy of its
    /// target, so a tree of N links to one file produced N copies of that file.
    #[test]
    #[cfg(unix)]
    fn a_symlink_in_the_tree_is_copied_as_a_symlink() {
        let dir = scratch("tree_link");
        let src = dir.path("src");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("real"), b"contents").unwrap();
        std::os::unix::fs::symlink("real", src.join("link")).unwrap();

        let dst = dir.path("dst");
        let (ok, err) = cp(&recursive(), &[&src, &dst]);
        assert!(ok, "{err}");
        let meta = fs::symlink_metadata(dst.join("link")).unwrap();
        assert!(meta.file_type().is_symlink(), "a link must stay a link");
        assert_eq!(fs::read_link(dst.join("link")).unwrap(), Path::new("real"));
        // And the relative link still resolves, in its new directory.
        assert_eq!(fs::read(dst.join("link")).unwrap(), b"contents");
    }

    /// Without `-r`, a symlink operand is followed — that is GNU's behaviour and
    /// it is unchanged. Only the recursive case stopped following.
    #[test]
    #[cfg(unix)]
    fn without_recursive_a_symlink_operand_is_still_followed() {
        let dir = scratch("deref");
        let real = dir.path("real");
        fs::write(&real, b"contents").unwrap();
        let link = dir.path("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let out = dir.path("out");
        let (ok, err) = cp(&plain(), &[&link, &out]);
        assert!(ok, "{err}");
        let meta = fs::symlink_metadata(&out).unwrap();
        assert!(
            !meta.file_type().is_symlink(),
            "plain cp copies what the link points at"
        );
        assert_eq!(fs::read(&out).unwrap(), b"contents");
    }

    /// Bug 3 in the module docs: a 0700 source used to produce a 0755 copy,
    /// publishing everything inside it.
    #[test]
    #[cfg(unix)]
    fn a_copied_directory_keeps_the_source_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("modes");
        let src = dir.path("private");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("secret"), b"x").unwrap();
        fs::set_permissions(&src, fs::Permissions::from_mode(0o700)).unwrap();

        let dst = dir.path("copy");
        let (ok, err) = cp(&recursive(), &[&src, &dst]);
        assert!(ok, "{err}");
        let mode = fs::metadata(&dst).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "a private directory must stay private");
    }

    /// A file whose name is not valid UTF-8 — the case the whole rewrite is
    /// about — must copy like any other, including through a recursive walk.
    #[test]
    #[cfg(unix)]
    fn copies_a_tree_holding_a_name_that_is_not_utf8() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let dir = scratch("nonutf8");
        let src = dir.path("src");
        fs::create_dir(&src).unwrap();

        let mut name = src.clone().into_os_string().into_vec();
        name.extend_from_slice(b"/\x80bad");
        let odd = PathBuf::from(OsString::from_vec(name));
        fs::write(&odd, b"x").unwrap();

        let dst = dir.path("dst");
        let (ok, err) = cp(&recursive(), &[&src, &dst]);
        assert!(ok, "{err}");

        let mut want = dst.clone().into_os_string().into_vec();
        want.extend_from_slice(b"/\x80bad");
        let copied = PathBuf::from(OsString::from_vec(want));
        assert_eq!(fs::read(&copied).unwrap(), b"x");
        assert!(
            copied.as_os_str().as_bytes().ends_with(b"\x80bad"),
            "the name must survive byte for byte"
        );
    }

    // ---------------------------------------------------------- is_inside --

    #[test]
    fn is_inside_sees_through_a_different_spelling() {
        let dir = scratch("inside");
        let src = dir.path("src");
        fs::create_dir(&src).unwrap();

        assert!(is_inside(&src.join("child"), &src));
        assert!(is_inside(&src.join("a/b/c"), &src));
        // `src/./` and `src` are the same directory.
        assert!(is_inside(&src.join(".").join("child"), &src));
        assert!(!is_inside(&dir.path("sibling"), &src));
        // A sibling whose name merely starts with the source's is not inside it.
        assert!(!is_inside(&dir.path("srcery"), &src));
    }

    #[test]
    fn resolve_as_far_as_exists_handles_a_path_that_is_not_there_yet() {
        let dir = scratch("resolve");
        let deep = dir.path("nope/not/here");
        let resolved = resolve_as_far_as_exists(&deep).unwrap();
        assert!(resolved.ends_with("nope/not/here"), "{resolved:?}");
        assert!(resolved.is_absolute(), "{resolved:?}");
    }
}
