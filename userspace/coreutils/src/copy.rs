//! The copy engine `cp` and `mv` are both supposed to be, named for the
//! upstream file it is being reassembled from.
//!
//! The two are one program upstream — there is a single `copy_internal` in
//! `copy.c`, parameterised by `struct cp_options`, and `mv` is that engine with
//! `move_mode` set (`mv.c:131`) — so anything written a second time for `mv` is
//! a second home for every bug the first one has. That is not a theoretical
//! risk. It has happened twice already in this tree, and both times the fix
//! landed on one copy and not the other: `mv` created its destinations with the
//! extra owner-write bit that lets a read-only file's extended attributes be
//! written and `cp` did not; then `cp` gained the repair for when the umask
//! eats that bit and `mv` did not. Two halves of one fix, one half in each
//! program, neither program wrong in a way its own tests could see.
//!
//! **The module is being assembled in stages and is not finished.** See
//! `known-issues.md` → `B-MVS-CROSS-DEVICE-DIRECTORY-MOVES-ARE-REFUSED` for the
//! plan, for why the stages are cut where they are, and for how each is
//! certified. What is here now is:
//!
//! * **Stage 1**, the leaf helpers that were already free of any option struct:
//!   [`ModeDebt`], [`read_dir_fastread`], [`make_dir`].
//! * **Stage 2's preserve tail** — [`preserve_attributes`] and everything it
//!   calls. This is `copy_internal`'s closing run of steps (`copy.c:3205`
//!   onwards) merged with `copy_reg`'s (`copy.c:1626` onwards), which upstream
//!   writes twice because they live in two functions and which this tree, until
//!   then, wrote *four* times — twice in `cp.rs` following upstream, and twice
//!   more in `mv.rs` because a move was written as its own program. Both
//!   programs now reach this one copy.
//! * **Stage 2's byte copy** — [`copy_bytes`], which is `sparse_copy`
//!   (`copy.c:307`) minus the hole detection nothing here has yet. This one had
//!   diverged rather than merely doubled: `cp` ran a read/write loop that could
//!   say which end failed but never offloaded, `mv` ran `io::copy` that
//!   offloaded but could not. Upstream's third sentence, `error copying SRC to
//!   DST`, is what let them become one without either giving anything up.
//!
//! That is what brought [`Opts`] and [`Run`] in, and their arrival is the point
//! at which this module stops being a bag of helpers and becomes an engine.
//! Stage 1 consulted no options at all — [`ModeDebt::new`] takes the single flag
//! it needs as a `bool` — which is what made that instalment moveable without
//! deciding anything, but it was never going to be a rule for the module. The
//! tail cannot be expressed without an options struct, and neither can the walk
//! that follows it. Upstream's is `struct cp_options`; ours is [`Opts`], the
//! fields the engine actually reads, with `mv` supplying for each one the
//! constant `mv.c`'s `cp_option_init` supplies. A module that refused one would
//! not be shareable; it would just be empty.
//!
//! * **Stage 3, opening the destination** — [`open_destination`] and the
//!   [`Dest`]/[`Clobber`]/[`Opened`]/[`DestError`] vocabulary around it. This
//!   is the one step where the two programs had genuinely *diverged* rather
//!   than merely been written twice, and all three differences ran the same
//!   way: `cp` knew something `mv` did not. See that function's table.
//!
//! What is **not** here yet is the walk that decides what to copy at all, which
//! is stage 4's, and which is still `cp`'s. Everything in this module is
//! reached from `cp.rs` and `mv.rs` alike through call sites that build a
//! [`Run`] from each program's own `Job`.

use crate::backup::{self, BackupType, source_is_dst_backup, src_base_is_dot_or_dotdot};
use crate::errmsg::strerror;
use crate::fileid::{Copied, file_id, nlink};
use crate::fsattr::{
    self, GroupRetry, Link, On, Ownership, chown_privileges, is_denied_ownership, owner_differs,
    owner_of, permission_bits, times_of,
};
use crate::hardlink;
use crate::overwrite::{self, Interactive};
use crate::quote::{escape_os, quoteaf_os, quotef_os};
use crate::yesno::Answers;
use std::fs;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Whether a symbolic link is copied, or whatever it points at is.
///
/// GNU's `enum Dereference_symlink` (`copy.h`), spelled the same way and with
/// the same four members, including the one that is not a policy: `Undefined`
/// means none of `-P`, `-H`, `-L` was given, and is resolved by
/// [`Deref::resolved`] rather than acted on. It is not an implementation
/// detail to be resolved away at parse time — whether "no symlink option was
/// given" means *follow* or *keep* depends on `-r`, which may be given after
/// it, so GNU resolves once after the option loop (`cp.c:1239`) and so does
/// this.
///
/// Two policies and not one, because "follow a link" is answered differently
/// depending on *where the link was found*. That distinction is the whole of
/// `-H`, and it is invisible in any single boolean.
// `Debug` is derived unconditionally here and on the other public enums in
// this module, rather than under `cfg_attr(test, …)`. The gate reads as a
// saving and is not one: `test` is *this crate's* cfg, and it is off when
// `cp`'s and `mv`'s own test binaries are compiled — so a gated `Debug` on a
// public type is one no caller can ever have. It cost an afternoon once.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Deref {
    /// None of the three options was given. Never observed outside
    /// [`Deref::resolved`].
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

impl Deref {
    /// This policy with `Undefined` replaced by what it means, given whether
    /// `-r` was asked for.
    ///
    /// GNU does this once, after the option loop (`cp.c:1239`), and calls the
    /// default "compatible with FreeBSD": recursive copies keep links, flat
    /// copies follow them. That is why plain `cp link dst` writes a *file* and
    /// plain `cp -r link dst` writes a *link* — one option that was never given
    /// changing meaning because another one was.
    ///
    /// Resolved on demand rather than written back into the value, so that
    /// `cp`'s parse tests can see `-r` and `-rP` as the different command lines
    /// they are, and so that there is no window in which an unresolved value
    /// could be read. GNU's `x.hard_link` also takes part in the rule
    /// (`x.recursive && ! x.hard_link`); `-l` is not implemented here, so its
    /// half of the condition is not yet expressible and is noted rather than
    /// guessed at.
    ///
    /// The rule lives on the *policy* rather than on [`Opts`] because `cp`
    /// needs it before it has an [`Opts`] to ask: three sites in `cp.rs` decide
    /// how to `stat` an operand, and its parse tests pin the whole table of
    /// answers. [`Opts`] and `cp`'s `CpFlags` each delegate here, so the rule is
    /// written once and neither can drift from it.
    #[must_use]
    pub fn resolved(self, recursive: bool) -> Deref {
        match self {
            Deref::Undefined if recursive => Deref::Never,
            Deref::Undefined => Deref::Always,
            given => given,
        }
    }

    /// Whether a source *named on the command line* is stat'd through.
    ///
    /// `copy.c:2250` picks `AT_SYMLINK_NOFOLLOW` exactly when the resolved
    /// policy is `DEREF_NEVER`, so `-H` follows here and `-P` does not.
    #[must_use]
    pub fn follow_operand(self, recursive: bool) -> bool {
        self.resolved(recursive) != Deref::Never
    }

    /// Whether a source *found by walking a directory* is stat'd through.
    ///
    /// GNU expresses this by handing the recursion a modified copy of the
    /// options: `copy.c:845` sets `non_command_line_options.dereference =
    /// DEREF_NEVER` when the policy is `DEREF_COMMAND_LINE_ARGUMENTS`. So only
    /// `-L` follows in here, which is what makes `cp -Hr` and `cp -Lr` differ
    /// at all — they agree about the operand and disagree about everything
    /// underneath it.
    #[must_use]
    pub fn follow_walked(self, recursive: bool) -> bool {
        self.resolved(recursive) == Deref::Always
    }

    /// GNU's `should_dereference` (`copy.c:2151`), which is the same question
    /// as the two above asked once with the *place* as a parameter rather than
    /// baked into the name.
    #[must_use]
    pub fn should_dereference(self, recursive: bool, command_line_arg: bool) -> bool {
        match self.resolved(recursive) {
            Deref::Always => true,
            Deref::CommandLine => command_line_arg,
            Deref::Never | Deref::Undefined => false,
        }
    }
}

/// The engine's options: upstream's `struct cp_options`, restricted to the
/// fields the code in this module actually reads.
///
/// Restricted deliberately, and the restriction is what the list is *for*.
/// Passing the whole of `cp`'s `CpFlags` would compile and would quietly make
/// this module `cp`-shaped: `mv` would have to fabricate a value for every
/// field it has no concept of, and the next reader could not tell which fields
/// the engine depends on. The list below is that answer, written down.
///
/// Every field is one of GNU's, under GNU's name, and `mv` supplies for each
/// one the constant `mv.c`'s `cp_option_init` supplies — which is what makes
/// `mv` expressible as this engine rather than as a parallel implementation of
/// it. The ones that look like they should be constants are the interesting
/// ones: `mv` sets [`Self::preserve_mode`] and friends *true* and all three of
/// the loudness flags *false*, so it preserves everything and fails at nothing.
///
/// **The list grew when the walk arrived (stage 4), and what stayed behind is
/// the useful part.** Before then it held only what the *preserve tail* and the
/// destination open read — nine scalars — and the docs here said `-r`, `-v`,
/// the backup type and the interactive mode "mean nothing to the steps here".
/// They meant nothing to a step that stamps a destination somebody else
/// created; they are most of what deciding *what to copy at all* consists of,
/// so they are here now. Exactly two of `CpFlags`' fields did not follow:
/// `target_directory` and `no_target_directory`, which shape the *operand
/// list* — they decide which argument is the destination, once, before any copy
/// starts, and no step inside the engine can see them. `mv` has its own pair,
/// in its own `Destination` type, for the same reason.
///
/// # Lifetime
///
/// One borrowed field, [`Self::backup`], and it is why this type has a lifetime
/// at all. Everything else is a scalar the struct owns; a [`backup::Backup`]
/// carries the suffix as a `Vec<u8>`, so holding it by value would cost an
/// allocation *per file copied* — `cp`'s [`Opts`] is rebuilt at every call, on
/// purpose (see `cp`'s `Job::run`) — and would take `Copy` away from a type
/// that is passed by value everywhere. Both programs own their policy for the
/// whole run, so a shared reference is free and cannot dangle.
#[derive(Clone, Copy)]
pub struct Opts<'a> {
    /// What to put in front of a diagnostic: `"cp"` or `"mv"`.
    ///
    /// The engine prints its own sentences rather than returning them, because
    /// GNU does — a copy reports each attribute it could not carry and then
    /// carries on to the next, so there is no single error to return and the
    /// caller has nothing to add. The prefix is the only part of the sentence
    /// that differs between the two programs, so it is the only part passed in.
    pub prog: &'static str,
    /// GNU's `recursive` (`copy.h:146`), `cp -r`. Whether a directory source is
    /// descended into or refused.
    ///
    /// Read by the walk, and — through [`Deref::resolved`] — by the rule
    /// that decides what a symlink operand means when no `-P`/`-H`/`-L` was
    /// given. `mv` sets it true (`mv.c:133`): a move is recursive by nature,
    /// because a rename moves a whole subtree in one call and the cross-device
    /// fallback has to reproduce that.
    pub recursive: bool,
    /// GNU's `verbose`, `-v`: name every copy as it is made, on **stdout**.
    ///
    /// The engine's, not the caller's, because the lines come from inside it —
    /// the arrow line for each entity, and the `removed %s` that
    /// `--remove-destination` and `-f` print either side of it. A caller that
    /// wanted to announce for itself could not: it does not know which
    /// directories were created rather than reused, which is the one case GNU
    /// declines to announce.
    pub verbose: bool,
    /// `-P` / `-H` / `-L`: what a symbolic link means. GNU's
    /// `enum Dereference_symlink`; ask [`Opts::follow_operand`],
    /// [`Opts::follow_walked`] or [`Opts::should_dereference`] rather than
    /// reading it, because [`Deref::Undefined`] is a real value with a rule
    /// behind it.
    ///
    /// `mv` sets `DEREF_NEVER` (`mv.c:131`), which is the only answer a move
    /// can give: renaming a symlink moves the link, so a cross-device fallback
    /// that followed it would turn a link into a copy of its target.
    pub dereference: Deref,
    /// GNU's `interactive` (`copy.h:73`): `-i`, `-n`, and the two values only
    /// `mv` and `--update` can produce. One field and not three booleans,
    /// because the options overwrite each other — the last one on the command
    /// line wins.
    pub interactive: Interactive,
    /// GNU's `unlink_dest_after_failed_open`, which is `cp -f`, and the field
    /// name is the whole of the semantics: `-f` does **not** mean "remove the
    /// destination", it means "if opening it for writing fails, remove it and
    /// create a new one". `mv` sets it false (`mv.c:128`).
    ///
    /// Under its GNU name rather than `cp`'s `force`, because `force` is what
    /// made it look like the other one. See [`Self::unlink_dest_before_opening`].
    pub unlink_dest_after_failed_open: bool,
    /// GNU's `unlink_dest_before_opening`, which is `cp --remove-destination`:
    /// unlink unconditionally, before the copy is attempted at all, so the name
    /// is replaced rather than written through.
    ///
    /// **`mv` sets it false** (`mv.c:127`), which is worth saying because a
    /// move *does* clear its destination and the obvious guess is that this is
    /// the field that does it. It is not, and upstream is explicit about the
    /// difference: `copy_internal`'s general unlink is guarded by `! x->
    /// move_mode` — "never unlink dst_name when in move mode" (`copy.c:2571`).
    /// A move's clearing happens in a different place for a different reason,
    /// on the *EXDEV fallback path only* (`copy.c:2869-2892`), so that a
    /// cross-device `mv` "acts as if it were really using the rename syscall".
    /// That unlink is `mv`'s own, ahead of the engine, and is why the engine's
    /// [`Dest`] is always `New` for a move.
    pub unlink_dest_before_opening: bool,
    /// `--preserve=links`: a second name for an inode is a hard link to where
    /// the first one landed, not a second copy of its bytes. Read by
    /// [`place_entity`], which consults [`Copied`] for the earlier
    /// destination. `mv` sets it true (`mv.c:135`).
    pub preserve_links: bool,
    /// `-b` / `--backup[=CONTROL]` / `-S SUFFIX`: what happens to a destination
    /// about to be replaced — the policy, not a `bool`, because "make backups"
    /// and "make *which* backups" arrive from four places that do not agree.
    ///
    /// Reaches further into the engine than an option that renames one file has
    /// any right to: it turns the destination `stat` into an `lstat`, and it is
    /// the `if` whose `else` is [`remove_destination_first`]'s unlink. Each of
    /// those sites names the `copy.c` line it comes from.
    ///
    /// The one borrowed field; see this type's *Lifetime* section.
    pub backup: &'a backup::Backup,
    /// `--preserve=mode`, and `mv`'s `move_mode`. The whole of `07777`, special
    /// bits included, and the umask is *not* applied — a preserved mode is the
    /// source's mode, not a fresh file's.
    pub preserve_mode: bool,
    /// `--preserve=timestamps`. The access and modification times; not the
    /// change time, which no interface can set.
    pub preserve_timestamps: bool,
    /// `--preserve=ownership`. Also what decides whether a destination is
    /// created with its group and other bits withheld — see [`ModeDebt::new`].
    pub preserve_ownership: bool,
    /// `--preserve=xattr`. The extended attributes *except* the two that are
    /// the file's permissions rather than data about it; those go with
    /// [`Self::preserve_mode`]. See [`fsattr::Xattrs`].
    pub preserve_xattr: bool,
    /// GNU's `require_preserve`: whether an attribute that could not be carried
    /// is an error rather than a warning. Set by `cp -p` and any
    /// `--preserve=`; `mv` leaves it false (`mv.c:143`), which is what makes
    /// every step of the tail non-fatal for a move without a single `mv`-shaped
    /// branch in this module.
    pub require_preserve: bool,
    /// GNU's `require_preserve_xattr`, a second flag beside
    /// [`Self::require_preserve`] because upstream sets them in different
    /// places: `--preserve=xattr` sets this one, while `--preserve=all` and
    /// `-a` turn attributes on without it and so carry them best-effort.
    pub require_preserve_xattr: bool,
    /// GNU's `reduce_diagnostics`: say nothing at all about an extended
    /// attribute that would not go. Set only by `cp -a`.
    pub reduce_diagnostics: bool,
    /// GNU's `explicit_no_preserve_mode`: `--no-preserve=mode` was given, whose
    /// effect is not "leave the mode alone" but "give a newly created
    /// destination the mode it would have had if nobody had asked".
    pub explicit_no_preserve_mode: bool,
    /// The process's file-mode creation mask, read once and carried.
    ///
    /// **The one field that is not one of GNU's**, and the deviation is
    /// deliberate. Upstream reads the mask through the global `cached_umask()`
    /// (`copy.c:3305`, `copy.c:1685`), which is a function-static cache because
    /// `copy.c` has nowhere better to put it. We do: the options struct that is
    /// already threaded through every step that needs it. A real `cp` is one
    /// process with one mask for its whole life, so building this once per run
    /// caches exactly as well as upstream's static does.
    ///
    /// And it caches *correctly* where a static would not. `cargo test` runs
    /// dozens of copies inside one process and several of the mode tests set
    /// the mask around each one, so a value remembered for the lifetime of the
    /// process would make every later row assert against the wrong mask — which
    /// is why the version of this that lived in `cp.rs` had to switch its cache
    /// off under `#[cfg(test)]`. That switch cannot follow the code here: this
    /// is a library module, and its `test` cfg is not set when the `cp` binary
    /// is built as a test. Carrying the value per run rather than per process
    /// removes the problem instead of relocating it, and is a thread-safety
    /// improvement besides — a process-wide cache one test poisons with its own
    /// mask is read by every *other* test running beside it. See
    /// [`crate::umask`], which exists because of that same class of failure.
    pub umask: u32,
}

impl Opts<'_> {
    /// Whether a source *named on the command line* is stat'd through.
    /// [`Deref::follow_operand`] holds the rule and its citation.
    #[must_use]
    pub fn follow_operand(self) -> bool {
        self.dereference.follow_operand(self.recursive)
    }

    /// Whether a source *found by walking a directory* is stat'd through.
    /// [`Deref::follow_walked`] holds the rule and its citation.
    #[must_use]
    pub fn follow_walked(self) -> bool {
        self.dereference.follow_walked(self.recursive)
    }

    /// [`Deref::should_dereference`] for this run's policy.
    ///
    /// [`Self::follow_operand`] is this with `true` and [`Self::follow_walked`]
    /// is this with `false`; they stay as they are because their call sites read
    /// better for it, and because each is asked where only one answer is
    /// possible. This one exists for [`place_entity`], which is reached from
    /// both places and so has to carry the distinction as data.
    #[must_use]
    pub fn should_dereference(self, command_line_arg: bool) -> bool {
        self.dereference
            .should_dereference(self.recursive, command_line_arg)
    }
}

/// One run of the engine: the options it was given, the places it says things,
/// and the two pieces of per-run state a walk cannot do without.
///
/// The first two are one value rather than two parameters because they travel
/// together through every step and always will — which is not an observation
/// about the current call graph but about what the steps *are*. A step of the
/// tail reads an option, does one syscall, and reports what it could not do;
/// there is no step that reads options without being able to complain and none
/// that complains without having read one. `cp.rs`'s `Job` is the same
/// discovery made once already, for the same reason, over a superset of these
/// fields.
///
/// **The other three arrived with the walk (stage 4), exactly as the note that
/// stood here predicted**: the stdout `--verbose` announces on, the table of
/// which inode went where, and the stream `-i`'s prompts are answered from. All
/// three were on `cp`'s `Job` before, and the direction of travel is that `Job`
/// is being emptied into this rather than copied alongside it. What is left on
/// `Job` now is the operand-shaping half — the flags that decide which argument
/// is the destination — and nothing a per-file step can see.
pub struct Run<'a, E: Write> {
    /// What was asked for. By value, not by reference: it is a dozen scalars
    /// and two words, so a reference would cost as much as the copy.
    pub opts: Opts<'a>,
    /// Where a failure to carry an attribute is reported.
    ///
    /// Generic rather than `Stderr`, so a test can assert on what a copy said —
    /// which is how this crate tests diagnostics at all. Not `dyn`, because
    /// unlike the prompt stream this one is written on a per-*file* path.
    pub err: &'a mut E,
    /// Where `--verbose` announces, and where the two `removed %s` sentences
    /// go. Measured: GNU's `emit_verbose` uses `printf`, so these are on stdout
    /// and are *not* diagnostics — `cp -v a b > log` captures the arrow line and
    /// `cp -v a b 2>/dev/null` does not silence it.
    ///
    /// `dyn` where [`Self::err`] is generic, and the asymmetry is deliberate
    /// rather than an oversight. A second type parameter would have to be named
    /// by all ten signatures that mention [`Run`], including the six steps of
    /// the preserve tail that never write to stdout at all — and `mv`'s tests
    /// build a [`Run`] for the cross-device path, which would have to invent a
    /// stdout type it has no use for. The cost is one indirect call per
    /// *announced line*, which happens only under `-v`; `err` is written on
    /// paths no option has to enable. [`crate::hardlink::force_link`] already
    /// takes its two streams this way, for the same reason.
    pub out: &'a mut dyn Write,
    /// The record of which inode went where: GNU's `src_to_dest`
    /// (`copy.c:255`), read by `--preserve=links` and by the
    /// directory-copied-twice refusal alike.
    ///
    /// On the run and not on the caller because the *walk* needs it, twice
    /// over: two hard-linked files inside one source directory must come out
    /// linked too, and a directory reached by walking has to be checked against
    /// the directories already copied. Before stage 4 this lived on `cp`'s
    /// `Job` and half the question was unreachable from the walk, which is
    /// `cp.rs`'s module-docs bug 10.
    pub copied: &'a mut Copied,
    /// Where `-i`'s prompts are answered.
    ///
    /// `dyn` for the reason it was `dyn` on `Job`: nothing but
    /// [`overwrite_allowed`] cares what the answers come from, and the one
    /// indirect call is per *prompt*, which is per human keypress.
    pub answers: &'a mut dyn Answers,
}

/// The ways a destination's permission bits are deliberately not the ones it is
/// to end with, and what has to be done about each once it is safe.
///
/// GNU's four locals `omitted_permissions`, `restore_dst_mode`, `dst_mode`
/// (`copy.c:2211`) and `extra_permissions` (`copy.c:1246`), carried together
/// because they are one fact in four pieces: a destination is created with a
/// mode that is not its final one on purpose, and something has to remember how
/// it differs.
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
///
/// [`Self::extra`] goes the other way — a bit that is on the destination and
/// must come *off* — and is here rather than beside it because the settle-up is
/// one step for both: whichever of the two is non-zero, the answer is the same
/// chmod to the mode the file was always meant to have.
#[derive(Clone, Copy, Default)]
pub struct ModeDebt {
    /// GNU's `omitted_permissions`: the bits withheld at creation.
    pub omitted: u32,
    /// GNU's `restore_dst_mode` and `dst_mode` in one value. `Some(mode)` means
    /// the destination's mode has already been read and must be written back —
    /// either because a directory was forced owner-rwx so it could be filled,
    /// or because the settle-up stat showed the withheld bits genuinely absent.
    pub forced: Option<u32>,
    /// GNU's `extra_permissions` (`copy.c:1453`): owner-write, granted to a
    /// destination that is not meant to have it, so that its extended
    /// attributes can be written.
    ///
    /// Linux's `xattr_permission` (`fs/xattr.c`) requires write access to the
    /// *inode* before it will set an attribute on it, so a copy of a read-only
    /// file — mode `0444` — is a file no `setxattr` can reach. The bit is added
    /// at creation and taken off by the settle-up, and it costs no exposure
    /// while it is on: the owner at that instant is the process doing the
    /// copying, which already holds a writable descriptor to the file it just
    /// created.
    ///
    /// Zero unless the destination is newly created, extended attributes are
    /// being carried, and the caller is not root — root's `setxattr` is not
    /// subject to the check, which is why GNU's condition is
    /// `preserve_xattr && !x->owner_privileges`.
    pub extra: u32,
}

impl ModeDebt {
    /// GNU's `omitted_permissions = dst_mode_bits & (…)` (`copy.c:2899`).
    ///
    /// Takes the one flag it reads rather than an options struct, so that a
    /// caller with no such struct — `mv`, whose `preserve_ownership` is
    /// unconditionally true (`mv.c:134`) — can reach it too.
    #[must_use]
    pub fn new(preserve_ownership: bool, src_mode: u32, is_dir: bool) -> Self {
        let withhold = if preserve_ownership {
            0o077
        } else if is_dir {
            0o022
        } else {
            0
        };
        ModeDebt {
            omitted: src_mode & withhold,
            forced: None,
            // Not decided here. Whether the extra owner-write bit is wanted
            // depends on whether the destination turns out to be newly created,
            // which is not known until the open; GNU sets it in the same
            // expression as the open mode (`copy.c:1451`) for that reason.
            extra: 0,
        }
    }
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
pub fn read_dir_fastread(src: &Path) -> io::Result<Vec<fs::DirEntry>> {
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

/// Create `dest` as a directory with mode `mode`, before the umask is applied.
///
/// `Ok(true)` if it was created, `Ok(false)` if a directory was already there —
/// a distinction the caller needs, because an existing directory's mode is left
/// alone. Plain `create_dir` and not `create_dir_all`: GNU's single `mkdirat`
/// does not invent missing parents either, and `cp -r a no/such/dir` must fail
/// rather than quietly build the path.
pub fn make_dir(dest: &Path, mode: u32) -> io::Result<bool> {
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

/// `mkdir(path, mode)`; the kernel narrows `mode` by the umask.
#[cfg(unix)]
pub fn create_dir_with_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new().mode(mode).create(path)
}

/// Windows has no mode to create a directory with, so the mode is dropped
/// rather than approximated — the same answer every other non-unix arm in
/// this crate gives. The target OS is the `#[cfg(unix)]` branch above.
#[cfg(not(unix))]
pub fn create_dir_with_mode(path: &Path, _mode: u32) -> io::Result<()> {
    fs::create_dir(path)
}

/// What the caller found at the destination's name, which is GNU's `new_dst`
/// (`copy.c:1456`) with the thing that may be done about it attached.
///
/// The two are one value because the second is only ever consulted inside the
/// first. `-f`'s unlink is reached from the arm where a destination exists and
/// would not open; there is no such thing as unlinking a destination that is
/// not there. Two independent parameters would spell a fourth combination —
/// "nothing is there, and you may remove it" — that the code would have to
/// ignore and the reader would have to work out was impossible.
///
/// **Which arm is taken is decided by the caller's `stat`, not by what the
/// first open answers.** GNU branches on `new_dst`, and deriving it instead
/// from a failed `O_EXCL` would work for a plain file and get an *opaque*
/// destination wrong — a name that is occupied by something a `stat` could not
/// describe is still occupied.
pub enum Dest<'a> {
    /// GNU's `new_dst == true`: nothing is at the name, or the caller has
    /// already unlinked what was. A move is always this, but *not* because of
    /// [`Opts::unlink_dest_before_opening`], which `mv` sets false
    /// (`mv.c:127`) — see that field. It is because the EXDEV fallback unlinks
    /// the destination itself before it copies anything, so that a
    /// cross-device `mv` "acts as if it were really using the rename syscall"
    /// (`copy.c:2870`), and sets `new_dst = true` on the spot (`copy.c:2892`).
    /// By the time the engine sees the name there is nothing there.
    New,
    /// Something is at the name, and it is to be truncated in place rather
    /// than recreated: an existing file's mode is not a copy's to narrow, even
    /// for an instant.
    Exists(Clobber<'a>),
}

/// What may be done with a destination that exists but will not open, and
/// where to say it was done.
///
/// The removal and the announcement are one thing rather than two flags,
/// because the announcement is meaningless without the removal: GNU prints
/// `removed %s` from inside the `unlink_dest_after_failed_open` branch, and
/// nowhere else.
pub enum Clobber<'a> {
    /// GNU's `unlink_dest_after_failed_open = false` (`mv.c:128`): a
    /// destination that will not open is an error to report, not something to
    /// remove. Every move is this, and so is every `cp` without `-f`.
    Never,
    /// `cp -f`: unlink it and try the create again.
    ///
    /// `verbose` is `cp -v`, and the sentence goes out *after* the removal and
    /// *after* the caller's own `'a' -> 'ro'` announcement — which is what puts
    /// `removed 'ro'` below that line where `--remove-destination` puts it
    /// above. GNU prints it from this same point inside `copy_reg`.
    Unlink {
        /// Whether to say so.
        verbose: bool,
        /// Standard output.
        ///
        /// `dyn` where [`Run::err`] is generic, and the reason is the opposite
        /// of that one rather than an inconsistency. Making this generic would
        /// put a writer type parameter on [`Dest`], which every [`Dest::New`]
        /// construction would then have to name — and a move has no stdout
        /// here to name, so it would have to invent one. A type parameter
        /// satisfied by a fiction is worse than a vtable dispatch taken at most
        /// once per operand and only under `-f`.
        out: &'a mut dyn Write,
    },
}

/// A destination that is open and ready to be written through.
pub struct Opened {
    /// The descriptor. Everything after this point works through it and not
    /// through the name — see [`fsattr::On`] for why that matters to the
    /// set-user-ID bit.
    pub file: fs::File,
    /// GNU's `*new_dst` **as it stands after the open**, which is not simply
    /// `matches!(dest, Dest::New)`: a destination that vanished between the
    /// caller's `stat` and the open, and one that `-f` unlinked, both end up
    /// newly created.
    ///
    /// The distinction is what decides whether `-p` bothers to `chown`,
    /// whether `--no-preserve=mode` applies, and — through [`ModeDebt`] —
    /// whether any permissions were withheld at all.
    pub new: bool,
}

/// Why a destination could not be opened for writing.
///
/// Three variants rather than one `io::Error` because GNU has three sentences,
/// and which one is printed is information the caller cannot reconstruct from
/// an `errno`.
pub enum DestError {
    /// Every other failure to open. `cp: cannot create regular file %s`.
    Io(io::Error),
    /// The name is a symlink that points at nothing. Resolving it to a
    /// (directory, name) pair to write through is racy by construction, so GNU
    /// refuses and says so rather than creating the link's target:
    /// `cp: not writing through dangling symlink %s`.
    ///
    /// It carries the `EEXIST` that revealed it, which no `cp` reads — its
    /// sentence names no error at all. `mv` does: a move reaches this only in a
    /// race, has no sentence of its own for it, and reporting the underlying
    /// `File exists` is what it did before this became shared code. A variant
    /// that threw the error away would have forced a move to *synthesise* one,
    /// and a synthesised `io::Error` has no `errno` for [`strerror`] to name.
    Dangling(io::Error),
    /// `-f` had to unlink a destination that would not open, and could not.
    /// GNU's `cannot remove %s`, which is a different sentence from
    /// [`DestError::Io`]'s — hence a variant rather than an `io::Error` the
    /// caller has to guess about.
    Remove(io::Error),
}

/// `S_IWUSR` — the bit [`open_destination`] adds so that extended attributes
/// can be written onto a copy of a read-only file. See [`ModeDebt::extra`].
const OWNER_WRITE: u32 = 0o200;

/// Open `dst` for writing, creating it with `src_mode` if it is new and leaving
/// its mode entirely alone if it is not.
///
/// This is GNU's `copy_reg` (`copy.c:1287`–`1349`), and it is the one step of
/// the engine where `cp` and `mv` had genuinely *diverged* rather than merely
/// been written twice. The differences were all in one direction — `cp` knew
/// things `mv` did not — and none of them were `mv` being right:
///
/// | | `cp` before | `mv` before |
/// |---|---|---|
/// | destination exists | truncate in place, `-f` unlinks on failure | not modelled; `mv` unlinks first |
/// | dangling symlink | refused with GNU's sentence | reported as `File exists` |
/// | umask ate the extra bit | repaired, and the debt cleared if it could not be | repaired, debt left claiming a bit that is not there |
///
/// The last of those is the one worth stating, because it is the shape of
/// defect this module exists to stop. `mv`'s [`ModeDebt::extra`] stayed set
/// after a failed repair, which is a lie about the file — harmless only
/// because a move takes [`settle_mode`]'s `preserve_mode` branch and returns
/// before anything reads it. It was a landmine for whoever changed that branch,
/// and it is gone rather than documented.
///
/// The shape is load-bearing in three places, all of which are `-f`:
///
/// * **Which open is tried first is decided by [`Dest`], not by what the first
///   open answers.** See that type.
/// * **`-f` unlinks on the `O_TRUNC` failure only.** That is why `cp -f a
///   dangling-link` still refuses: the open that fails there is the `O_EXCL`
///   one, which reports `EEXIST` and is [`DestError::Dangling`], not this.
///   Measured against 9.4 — the destination survives.
/// * **A new file's mode goes to the kernel with the `O_CREAT`**, which is the
///   only place the umask can narrow it without a window in which the file
///   exists at the wider mode. That is true of the file `-f` recreates too, so
///   `cp -f` over a `0400` destination leaves the *source's* mode behind rather
///   than the one it removed.
///
/// `preserve_xattr` is taken as a bare `bool` rather than as an [`Opts`], for
/// [`ModeDebt::new`]'s reason and one of its own: `cp`'s caller holds its
/// options and its stdout on the same `Job`, so asking for both an `Opts` and
/// the [`Clobber::Unlink`] writer would be two mutable borrows of one value.
///
/// # Errors
///
/// [`DestError::Dangling`] for a destination symlink that points at nothing,
/// [`DestError::Remove`] for an unlink `-f` could not do, and
/// [`DestError::Io`] for every other failure to open.
pub fn open_destination(
    dst: &Path,
    src_mode: u32,
    dest: Dest<'_>,
    preserve_xattr: bool,
    debt: &mut ModeDebt,
) -> Result<Opened, DestError> {
    if let Dest::Exists(clobber) = dest {
        match open_truncating(dst) {
            // Nothing was withheld, because nothing was created: an existing
            // file's mode is not a copy's to narrow even for an instant. GNU
            // zeroes the same two locals on this arm (`copy.c:1499`).
            Ok(file) => {
                debt.omitted = 0;
                debt.extra = 0;
                return Ok(Opened { file, new: false });
            }
            // It went away between the caller's `stat` and the open. GNU
            // reaches its `O_CREAT` arm in exactly this case too
            // (`dest_errno == ENOENT`), so a race loses nothing.
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => match clobber {
                Clobber::Never => return Err(DestError::Io(e)),
                Clobber::Unlink { verbose, out } => {
                    if let Err(e) = fs::remove_file(dst)
                        && e.kind() != io::ErrorKind::NotFound
                    {
                        return Err(DestError::Remove(e));
                    }
                    if verbose {
                        let _ = writeln!(out, "removed {}", quoteaf_os(dst));
                    }
                }
            },
        }
    }

    // GNU's `open_mode` (`copy.c:1451`), whose second half is the whole reason
    // `cp --preserve=xattr` of a read-only file works at all. See
    // [`ModeDebt::extra`].
    debt.extra = if preserve_xattr && !chown_privileges() {
        OWNER_WRITE
    } else {
        0
    };

    match open_new(dst, (src_mode & !debt.omitted) | debt.extra) {
        Ok(file) => {
            top_up_extra(&file, debt);
            Ok(Opened { file, new: true })
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            // `symlink_metadata` sees the link itself; `metadata` follows it,
            // so failing there is exactly "points at nothing".
            if fs::symlink_metadata(dst).is_ok_and(|m| m.file_type().is_symlink())
                && fs::metadata(dst).is_err()
            {
                Err(DestError::Dangling(e))
            } else {
                // Occupied by something that is not a dangling link — a race
                // against another process, since the caller stat'd it as absent
                // a moment ago. Reported as the open failure it is.
                Err(DestError::Io(e))
            }
        }
        Err(e) => Err(DestError::Io(e)),
    }
}

/// Put the extra owner-write bit on if the `open` did not manage it, and give
/// up on it if that cannot be done either.
///
/// The mode handed to `open` is narrowed by the umask, which can perfectly well
/// include `0o200` — `umask 0222` is unusual but legal, and under it the bit
/// asked for at creation simply does not arrive. Asking is therefore not the
/// same as having, and a copy of a read-only file under such a umask would
/// carry no extended attributes at all: every `setxattr` onto it is refused by
/// `xattr_permission`, and each refusal is reported, so what should be a silent
/// copy becomes a screenful of `Permission denied`.
///
/// GNU makes the same repair in the same place and with the same fallback
/// (`copy.c:1539`): *"if extra permissions needed for `copy_xattr` didn't
/// happen (e.g., due to umask) chmod to add them temporarily; if that fails
/// give up with extra permissions, letting `copy_attr` fail later."*
///
/// Giving up means clearing [`ModeDebt::extra`], which does two things at once
/// and both are wanted: the extended-attribute step goes on to fail and *say
/// so*, rather than the failure being hidden, and the settle-up does not chmod
/// a file whose mode is already the one it should end with.
///
/// A failure to read the mode back is folded into the same fallback rather than
/// given a diagnostic of its own. GNU has one — `cannot fstat %s` — but it
/// reaches that `fstat` for other reasons too (it sizes the copy buffer from
/// the result), so the call is free there and would be a stat-per-copy here,
/// added solely to have somewhere to fail. On a descriptor this function was
/// handed a moment ago there is no reachable failure to report.
fn top_up_extra(file: &fs::File, debt: &mut ModeDebt) {
    if debt.extra == 0 {
        return;
    }
    let on = On::File(file);
    let arrived = current_mode(on)
        .is_ok_and(|now| now | debt.extra == now || fsattr::set_mode(on, now | debt.extra).is_ok());
    if !arrived {
        debt.extra = 0;
    }
}

/// `O_WRONLY|O_TRUNC`, with no `O_CREAT` and no mode: the destination is known
/// to be there and its permissions are not a copy's to change. See `cp.rs`'s
/// module docs, bug 8.
fn open_truncating(dst: &Path) -> io::Result<fs::File> {
    fs::OpenOptions::new().write(true).truncate(true).open(dst)
}

/// `O_WRONLY|O_CREAT|O_EXCL` with `mode`, which the kernel narrows by the
/// umask.
///
/// `O_EXCL` is not an optimisation. Without it a name created between a
/// caller's unlink and this open would be opened and truncated, which is the
/// very thing the unlink was there to prevent.
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
/// arm above; see [`fsattr::permission_bits`].
#[cfg(not(unix))]
fn open_new(dst: &Path, _mode: u32) -> io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dst)
}

/// A byte copy that failed, carrying the sentence GNU prints for it.
///
/// **Returned rather than printed**, which is where this differs from
/// [`preserve_attributes`] and the rest of the tail. Those report an attribute
/// that would not go and carry on to the next one, so the caller has nothing to
/// decide and printing at the site is right. A failed byte copy *ends* the copy,
/// and the two callers then do different things: `cp` prints and moves to the
/// next operand; `mv` has a `-b` backup to put back first, which is its
/// `give_up_cross_device` and upstream's `un_backup` label. Returning the
/// sentence lets each do its own thing while keeping the wording — the part that
/// diverged and is the whole reason this function exists — in one place.
pub struct CopyError {
    /// The sentence, with no program prefix and no `: errno` tail. One of
    /// `error copying 'a' to 'b'`, `error reading 'a'`, `error writing 'b'`.
    pub what: String,
    /// The errno the caller renders after `: `, through `errmsg::strerror`.
    pub err: io::Error,
}

/// How far the in-kernel offload got.
///
/// `#[cfg(unix)]` along with [`offload`] itself, rather than defined always and
/// left half-unused off it: on a host with no `copy_file_range` there is no
/// offload to have got anywhere, so `Done` and `Failed` are not merely
/// unreachable but meaningless, and the compiler says so — a `dead_code`
/// warning for both variants on `x86_64-pc-windows-gnu`. Silencing that with an
/// `allow` would assert the type is fine there when the honest statement is
/// that it does not apply.
#[cfg(unix)]
enum Offload {
    /// The bytes are down; nothing further to do.
    Done,
    /// Nothing was copied and the reason says this pair of files cannot be
    /// offloaded. Fall back to the read/write loop, which starts from the top
    /// because nothing has been consumed.
    Unsupported,
    /// A real failure. Neither end can be blamed, so the caller gets the
    /// sentence that names both.
    Failed(io::Error),
}

/// Copy `input` to `output`, GNU's way: the in-kernel offload first, an explicit
/// read/write loop when that is not available.
///
/// This is `sparse_copy` (`copy.c:307`) minus its hole detection, which nothing
/// in this tree has yet — `hole_size` is always 0 for us, so the `if (!hole_size
/// && allow_reflink)` guard on the offload is always taken and the second loop is
/// reached only as a fallback.
///
/// **Three sentences, not two, and the third is the point.** Before this
/// function there were two copy bodies here, each half-right in the opposite
/// direction. `mv` used `io::copy` — which `std` specialises to
/// `copy_file_range`, so it got the offload — and reported every failure as
/// `error writing DST`, because one `io::Error` comes back for both ends and the
/// destination is the end that fails in practice. `cp` used a plain 64 KiB
/// read/write loop, which knows which end failed and says so, but never
/// offloaded at all and so was slower than GNU on any large copy. Each was
/// missing exactly what the other had.
///
/// GNU has neither problem because it has a **third** sentence. `copy_file_range`
/// does not report which side failed either, so upstream does not pretend to
/// know: it prints `error copying SRC to DST` (`copy.c:376`), naming both files
/// and letting the errno say the rest. `error reading %s` (`copy.c:402`) and
/// `error writing %s` (`copy.c:435`) belong to the fallback loop, which is its
/// own code and genuinely does know. So the distinction is not lost by
/// offloading; it is simply not claimed where it cannot be had.
///
/// **When the fallback is entered.** Only while *nothing has been copied yet*,
/// and only for an errno on upstream's list — `is_CLONENOTSUP` (`copy.c:298`),
/// plus a special case for the `ENOENT` "seen sometimes across CIFS shares"
/// (`copy.c:367`). The "nothing copied yet" half is not an optimisation but a
/// correctness condition: the fallback restarts from the current file offsets,
/// and after a partial offload those are no longer the beginning. Upstream's
/// comment gives the reason for the errno half — `EPERM` can mean
/// `copy_file_range` is filtered out by seccomp, in which case a plain copy
/// works, or it can mean the file is immutable, in which case the plain copy
/// fails too and reports the more accurate error.
///
/// `EINTR` is a retry at both layers. A signal arriving mid-copy is not a copy
/// failure, and reporting it as one would make both programs unreliable under
/// any job control.
///
/// See `known-issues.md` →
/// `B-MVS-CROSS-DEVICE-COPY-CANNOT-TELL-A-READ-FAILURE-FROM-A-WRITE-FAILURE`.
pub fn copy_bytes(
    input: &mut fs::File,
    output: &mut fs::File,
    src: &Path,
    dst: &Path,
) -> Result<(), CopyError> {
    // The offload is attempted only where one exists; everywhere else this
    // statement is not compiled at all and the function *is* the fallback loop,
    // which is the truthful shape rather than a stub that always declines. See
    // [`Offload`].
    #[cfg(unix)]
    match offload(input, output) {
        Offload::Done => return Ok(()),
        Offload::Failed(err) => {
            return Err(CopyError {
                what: format!("error copying {} to {}", quoteaf_os(src), quoteaf_os(dst)),
                err,
            });
        }
        // Nothing copied, and the reason was "not this pair of files". Falling
        // through to the loop below, which starts from the current file
        // offsets — unmoved, which is what `Unsupported` guarantees.
        Offload::Unsupported => {}
    }

    read_write_copy(input, output, src, dst)
}

/// Whether a `copy_file_range` failure means "not this pair of files" rather
/// than "the copy failed".
///
/// Upstream's `is_CLONENOTSUP` (`copy.c:298`) as one test, spelled the way the
/// rest of this crate spells an errno set — named constants matched against
/// `raw_os_error`, as `rename.rs` and `fsattr.rs` both do. `io::ErrorKind`
/// could not carry it in any case: it has no variant for `ENOSYS`, `ENOTTY`,
/// `EBADF`, `EXDEV` or `ETXTBSY`, and folds them into `Uncategorized`, which
/// cannot be named in a pattern on stable.
///
/// The numbers are Linux's, which is the only ABI this ships on — the same
/// statement `fsattr.rs` makes above its own `ENOTSUP`.
#[cfg(unix)]
fn is_clone_not_supported(e: &io::Error) -> bool {
    /// "Operation not permitted", which here may mean only that seccomp
    /// filters the call out — in which case a plain copy still works. It can
    /// also mean the file is immutable, in which case the fallback fails too
    /// and reports the more accurate error; that is upstream's stated reason
    /// (`copy.c:296`) for listing it despite the ambiguity.
    const EPERM: i32 = 1;
    /// "Permission denied", listed for the same reason as `EPERM`.
    const EACCES: i32 = 13;
    /// "Bad file descriptor" — a descriptor the call declines, such as one
    /// opened `O_APPEND`.
    const EBADF: i32 = 9;
    /// "Cross-device link": the two files are on different filesystems, which
    /// only the read/write fallback can bridge.
    const EXDEV: i32 = 18;
    /// "Not a typewriter", the kernel's answer for a file type it will not
    /// offload.
    const ENOTTY: i32 = 25;
    /// "Text file busy".
    const ETXTBSY: i32 = 26;
    /// "Invalid argument", including the answer for a source that is not a
    /// regular file.
    const EINVAL: i32 = 22;
    /// "Function not implemented" — the kernel predates the call.
    const ENOSYS: i32 = 38;
    /// `ENOTSUP`, which is `EOPNOTSUPP` on Linux: the filesystem declines.
    const ENOTSUP: i32 = 95;

    matches!(
        e.raw_os_error(),
        Some(EPERM | EACCES | EBADF | EXDEV | ENOTTY | ETXTBSY | EINVAL | ENOSYS | ENOTSUP)
    )
}

/// `copy_file_range` until EOF, or until something says it cannot be used.
///
/// The offset arguments are null, so both file positions advance and the next
/// call resumes where this one stopped — which is also what makes the fallback's
/// "nothing copied yet" precondition necessary.
///
/// `COPY_MAX` is upstream's `MIN (SSIZE_MAX, SIZE_MAX) >> 30 << 30`
/// (`copy.c:340`): the largest length that is safely representable, rounded down
/// to a gigabyte boundary so the kernel is never handed an awkward size.
#[cfg(unix)]
fn offload(input: &mut fs::File, output: &mut fs::File) -> Offload {
    use std::os::unix::io::AsRawFd;

    unsafe extern "C" {
        fn copy_file_range(
            fd_in: i32,
            off_in: *mut i64,
            fd_out: i32,
            off_out: *mut i64,
            len: usize,
            flags: u32,
        ) -> isize;
    }

    const COPY_MAX: usize = (isize::MAX as usize) >> 30 << 30;

    let (fd_in, fd_out) = (input.as_raw_fd(), output.as_raw_fd());
    let mut copied: u64 = 0;
    loop {
        // SAFETY: both descriptors are open for the duration of the call, held
        // by the `File`s the caller lends us. Both offset pointers are null,
        // which is the documented "use and advance the file position" form and
        // means nothing is written through them. `flags` is 0, the only value
        // Linux accepts.
        let n = unsafe {
            copy_file_range(
                fd_in,
                core::ptr::null_mut(),
                fd_out,
                core::ptr::null_mut(),
                COPY_MAX,
                0,
            )
        };
        if n == 0 {
            // Upstream falls back here rather than declaring success, because
            // `copy_file_range` wrongly returned 0 when reading from procfs on
            // Linux through at least 5.6.19 (`copy.c:345`). A zero on the first
            // call is therefore "empty, or lying"; a zero after real progress is
            // an honest EOF.
            return if copied == 0 {
                Offload::Unsupported
            } else {
                Offload::Done
            };
        }
        if n < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            if copied == 0 && (is_clone_not_supported(&e) || e.kind() == io::ErrorKind::NotFound) {
                return Offload::Unsupported;
            }
            return Offload::Failed(e);
        }
        copied = copied.saturating_add(n.unsigned_abs() as u64);
    }
}

/// GNU's second loop (`copy.c:392`): read a buffer, write it whole.
///
/// Not cfg-gated, and it is the *whole* of [`copy_bytes`] on a host with no
/// `copy_file_range`. That is not a degraded mode: `error reading`/`error
/// writing` are the sentences GNU itself emits on this path, so the development
/// host gets the same two diagnostics it would get from the reference. The
/// target OS takes the offload above and can reach the third.
///
/// 64 KiB because that is what `cp` used before this function existed and
/// nothing here is a reason to change it; upstream sizes its buffer from the
/// destination's `st_blksize`, which is a separate improvement and would move
/// throughput numbers this stage is not otherwise allowed to move.
fn read_write_copy(
    input: &mut fs::File,
    output: &mut fs::File,
    src: &Path,
    dst: &Path,
) -> Result<(), CopyError> {
    use io::Read;

    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = match input.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => {
                return Err(CopyError {
                    what: format!("error reading {}", quoteaf_os(src)),
                    err,
                });
            }
        };
        let Some(chunk) = buf.get(..n) else {
            // Unreachable: `read` returns at most the buffer's length. Handled
            // rather than indexed so the crate's `indexing_slicing` lint has
            // nothing to complain about and a broken `Read` cannot panic here.
            return Ok(());
        };
        if let Err(err) = output.write_all(chunk) {
            return Err(CopyError {
                what: format!("error writing {}", quoteaf_os(dst)),
                err,
            });
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Made {
    Regular,
    Directory,
    Symlink,
}

/// The source of a copy, as the steps that stamp its attributes onto the
/// destination need it: the handle they read from, the name a diagnostic
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
pub struct Source<'a> {
    /// A **descriptor** for a regular file — the one its bytes were read
    /// through — and a *path* for a directory or a symlink, which have none
    /// here. See [`fsattr::On`].
    pub on: On<'a>,
    /// What to call it in a diagnostic. Only the extended-attribute steps blame
    /// the source by name; every other sentence in the tail names the
    /// destination, because every other step writes to it.
    pub name: &'a Path,
    /// The `stat` the copy already took. Its timestamps and owner are what the
    /// tail writes; its mode seeded [`Self::mode`].
    pub meta: &'a fs::Metadata,
    /// The permission bits the destination is to end with — the source's, less
    /// whatever an impossible `chown` has since taken off them.
    pub mode: u32,
}

impl<'a> Source<'a> {
    /// A source about to have its attributes copied, before anything has
    /// narrowed the mode.
    #[must_use]
    pub fn new(on: On<'a>, name: &'a Path, meta: &'a fs::Metadata) -> Self {
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
/// * `copy.c:3245` — "chown turns off set\[ug\]id bits for non-root, so do the
///   chmod last". A `chmod` written before the `chown` compiles, runs, and
///   quietly drops the set-user-ID bit off every copy a non-root user makes.
/// * `copy.c:3279` — "Set xattrs after ownership as changing owners will clear
///   capabilities". A `setxattr` written before the `chown` loses
///   `security.capability`, which the kernel strips when a file changes hands.
///
/// `on` is the destination in the matching form: a descriptor for a regular
/// file, a path for a directory or a symlink. That is GNU's own split, and it
/// is a security property rather than a saved syscall; see [`fsattr::On`].
///
/// Returns `false` only for a failure that is fatal, which is what
/// [`Opts::require_preserve`] and [`Opts::require_preserve_xattr`] decide: the
/// diagnostic is printed either way, but only an attribute the user asked for
/// *by name* turns a copy that happened into an exit status of 1. A caller with
/// both of those false — `mv`, which sets neither (`mv.c:143` and `mv.c:146`)
/// — gets the every-step-is-a-warning behaviour a move wants, out of the same
/// code and with no branch that names it.
pub fn preserve_attributes<E: Write>(
    mut src: Source<'_>,
    on: On<'_>,
    dst: &Path,
    made: Made,
    new_dst: bool,
    debt: &mut ModeDebt,
    run: &mut Run<'_, E>,
) -> bool {
    let prog = run.opts.prog;
    if run.opts.preserve_timestamps {
        // `and_then` because a source whose timestamps cannot even be read is
        // the same failure to the user as one whose copy cannot be stamped:
        // the destination has the wrong times either way, and `preserving
        // times for` is the sentence for that.
        if let Err(e) = times_of(src.meta).and_then(|times| fsattr::set_times(on, times)) {
            let why = strerror(&e);
            let _ = writeln!(
                run.err,
                "{prog}: preserving times for {}: {why}",
                quoteaf_os(dst)
            );
            if run.opts.require_preserve {
                return false;
            }
        }
    }

    // A symlink's owner was set where the link was made, so GNU's ownership
    // step is guarded by `!dest_is_symlink` and this one is guarded the same
    // way. Note that the guard is *here* rather than an early return above:
    // the extended-attribute step below applies to a symlink destination and
    // GNU runs it for one.
    //
    // The `new_dst ||` is GNU's `copy_internal` guard (`copy.c:3265`) and *not*
    // its `copy_reg` one, which is `!SAME_OWNER_AND_GROUP` alone
    // (`copy.c:1645`) — so this one expression stands where upstream has two
    // that differ. They differ for a reason that does not survive the merge:
    // `copy_reg` has just `fstat`ed the destination descriptor, unconditionally
    // whenever ownership is being preserved (`copy.c:1529`), so its comparison
    // is against a fresh `stat` and is always meaningful; `copy_internal`'s
    // `dst_sb` was taken before the destination existed, so for a new one there
    // is nothing to compare against and the `new_dst ||` is what stops it
    // comparing against a stale reading. Ours takes the reading inside
    // [`owner_differs`], so neither problem applies and the wider guard is
    // safe — it costs at most a `chown` that changes nothing.
    //
    // That "at most" was measured rather than assumed, because the obvious
    // worry is that a no-op `chown` is still a write that can be refused, and a
    // refusal here is not free: it would take the set-user-ID bit off the copy
    // ([`Chowned::Disowned`] below). The sharpest case that can be built —
    // a source owned by you with a group you are *not* in, copied into a
    // set-group-ID directory carrying that same group, so the new destination
    // is born already owner-and-group-identical to the source and GNU skips the
    // `chown` we perform — was run against both binaries and produced `4755
    // inhahe:daemon` on each. It cannot fail: Linux checks group membership
    // only when the group is actually changing (`setattr_prepare`'s
    // `!vfsgid_eq_kgid(…) && !in_group_or_capable(…)`), so a `chown` to the
    // values already in place is permitted to anyone. GNU's own comment calls
    // its guard an optimisation — "Avoid calling chown if we know it's not
    // necessary" — which is exactly what it is.
    if made != Made::Symlink
        && run.opts.preserve_ownership
        && (new_dst || owner_differs(on, src.meta))
    {
        match chown_to_source(src, on, dst, made, new_dst, run) {
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
    // (`copy.c:1662` and `copy.c:3280`). Under `--preserve=all` the diagnostic
    // is printed and the copy still succeeds, which is the whole difference
    // between asking for everything and asking for this.
    let fatal = run.opts.preserve_xattr
        && !copy_xattrs(src, on, dst, fsattr::Xattrs::Ordinary, run)
        && run.opts.require_preserve_xattr;

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
    // (`copy.c:3284`), and nothing portable can set a symlink's mode in any
    // case — Linux has no working `lchmod` at all.
    if made == Made::Symlink {
        return true;
    }

    settle_mode(src, on, dst, made, new_dst, debt, run) && !fatal
}

/// Carry the extended attributes of one class from the source to the copy, and
/// say as much about the ones that would not go as the options asked for.
///
/// gnulib decides how loud to be by picking one of three error callbacks
/// (`copy.c:782`), which reads as two booleans and is three behaviours:
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
fn copy_xattrs<E: Write>(
    src: Source<'_>,
    on: On<'_>,
    dst: &Path,
    which: fsattr::Xattrs,
    run: &mut Run<'_, E>,
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

    let prog = run.opts.prog;
    let all_errors = run.opts.require_preserve_xattr;
    let some_errors = !all_errors && !run.opts.reduce_diagnostics;
    for failure in &failures {
        if all_errors || (some_errors && !fsattr::errno_unsupported(&failure.err)) {
            let why = strerror(&failure.err);
            let what = failure.at.sentence(src.name, dst);
            let _ = writeln!(run.err, "{prog}: {what}: {why}");
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

/// GNU's `set_owner` (`copy.c:897`), whose three outcomes are three different
/// things rather than a success and a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chowned {
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
    /// [`Opts::require_preserve`] says that ends the copy.
    ///
    /// Unreachable for a caller that leaves that flag false, which is the whole
    /// of why `mv` can share this function: `mv.c`'s `cp_option_init` never
    /// sets `require_preserve`, so a move's failed `chown` is always
    /// [`Self::Disowned`] and the variant that would end the copy simply never
    /// arises.
    Failed,
}

/// Give `on` the source's owner and group. See [`Chowned`] for the outcomes.
pub fn chown_to_source<E: Write>(
    src: Source<'_>,
    on: On<'_>,
    dst: &Path,
    made: Made,
    new_dst: bool,
    run: &mut Run<'_, E>,
) -> Chowned {
    let prog = run.opts.prog;
    let fatal = if run.opts.require_preserve {
        Chowned::Failed
    } else {
        Chowned::Disowned
    };

    // Narrowing an *existing* destination first, because changing its owner
    // while it still wears its old mode is a window in which the new owner
    // holds permissions the copy will never have. GNU calls it exactly that —
    // "a window of vulnerability" — and closes it here (`copy.c:905`, the
    // comment; the restrictive temporary mode it describes is `copy.c:911`).
    if !new_dst && run.opts.preserve_mode && !narrow_before_chown(src, on, dst, run) {
        return fatal;
    }

    // The retry after a refusal, the `EPERM`-or-`EINVAL` test and the root check
    // are [`fsattr::take_ownership`]; what stays here is the sentence and
    // whether it ends the copy, which is the half `mv` does differently.
    //
    // A symlink gets no retry, which is GNU's asymmetry rather than ours: its
    // symlink arm (`copy.c:3180`) is a bare `lchownat`, while `copy_reg` and the
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
            // The *name* is quoted for two of the three kinds and bare for the
            // third, and that is upstream's inconsistency rather than ours.
            // `set_owner` writes `quoteaf (dst_name)` (`copy.c:960`); the
            // symlink arm, which is a separate `lchownat` inline in
            // `copy_internal`, writes `dst_name` with no quoting function at
            // all (`copy.c:3186`). Reproduced rather than tidied, for the
            // reason every other matched inconsistency in this crate is: a
            // utility that differs from GNU only in the punctuation of a
            // diagnostic is still one whose output a script cannot match on.
            //
            // Encoded on `made` because `made` is already the thing that tells
            // the two apart — the same parameter that chose [`GroupRetry`]
            // above, for the same underlying fact that a link's ownership is
            // taken somewhere else in upstream's code.
            //
            // [`escape_os`] and not `Path::display`, which is the only rendering
            // of a bare name this crate allows: `display` replaces every byte
            // that is not valid UTF-8 with U+FFFD, so a link named in Latin-1
            // would be *reported under a name that is not its own* — and the
            // reader's obvious next move, pasting that name into a shell, would
            // find nothing. `escape_os` is gnulib's `escape` style: identical to
            // upstream's `%s` for every name that is text, and octal escapes
            // rather than corruption for one that is not.
            let name = if made == Made::Symlink {
                escape_os(dst)
            } else {
                quoteaf_os(dst)
            };
            let _ = writeln!(
                run.err,
                "{prog}: failed to preserve ownership for {name}: {why}"
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
fn narrow_before_chown<E: Write>(
    src: Source<'_>,
    on: On<'_>,
    dst: &Path,
    run: &mut Run<'_, E>,
) -> bool {
    let prog = run.opts.prog;
    let old = match current_mode(on) {
        Ok(mode) => mode,
        Err(e) => {
            let why = strerror(&e);
            let _ = writeln!(run.err, "{prog}: cannot stat {}: {why}", quoteaf_os(dst));
            return false;
        }
    };
    let new = src.mode;
    // GNU's condition is `USE_ACL || (old & CHMOD_MODE_BITS & (~new | special))`
    // (`copy.c:918`), and this kernel has access-control lists, so the first
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
            run.err,
            "{prog}: clearing permissions for {}: {why}",
            quoteaf_os(dst)
        );
    }
    false
}

/// The last step: give the destination the mode it is meant to end with.
///
/// Three branches, and they are GNU's three — at `copy.c:3290` for a directory
/// and at `copy.c:1672` for a regular file, which is the same decision written
/// twice because the two live in different functions:
///
/// * **`--preserve=mode`** copies the source's whole `07777`, special bits
///   included, and does *not* apply the umask. That is the point of the option:
///   a preserved mode is the source's mode, not a fresh file's.
/// * **`--no-preserve=mode` on a destination this run created** gives it the
///   mode it would have had if nobody had asked — 0666 for a file, 0777 for a
///   directory, each less the umask. See [`Opts::explicit_no_preserve_mode`]
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
fn settle_mode<E: Write>(
    src: Source<'_>,
    on: On<'_>,
    dst: &Path,
    made: Made,
    new_dst: bool,
    debt: &mut ModeDebt,
    run: &mut Run<'_, E>,
) -> bool {
    let prog = run.opts.prog;
    if run.opts.preserve_mode {
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
                run.err,
                "{prog}: preserving permissions for {}: {why}",
                quotef_os(dst)
            );
            return !run.opts.require_preserve;
        }
        return true;
    }

    if run.opts.explicit_no_preserve_mode && new_dst {
        // GNU's `MODE_RW_UGO` for a file and `S_IRWXUGO` for a directory
        // (`copy.c:3303`). A socket gets the directory's answer there too; this
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
        if let Err(e) = fsattr::set_mode_exactly(on, default & !run.opts.umask) {
            let why = strerror(&e);
            let _ = writeln!(
                run.err,
                "{prog}: setting permissions for {}: {why}",
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
    debt.omitted &= !run.opts.umask;

    if made == Made::Regular {
        // `copy_reg`'s form, and its condition is GNU's `omitted_permissions |
        // extra_permissions` (`copy.c:1688`) — the two are settled by one chmod
        // because the mode they are both measured against is the same one. A
        // regular file acquires a *debt* only under `--preserve=ownership`, and
        // an *extra* only under `--preserve=xattr` on a new destination, so
        // either alone is reason enough to write the mode. It never carries a
        // forced mode, because nothing has to be opened through it.
        if debt.omitted == 0 && debt.extra == 0 {
            return true;
        }
        return chmod_settling(on, src.mode & 0o777 & !run.opts.umask, dst, run);
    }

    // The stat is what a *debt* needs; the chmod below is what a *forced* mode
    // needs, and the two are separate conditions. GNU's `if (restore_dst_mode)`
    // sits outside its `if (omitted_permissions)` (`copy.c:3335`) for exactly
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
                let _ = writeln!(run.err, "{prog}: cannot stat {}: {why}", quoteaf_os(dst));
                return false;
            }
        }
    }
    match debt.forced {
        Some(mode) => chmod_settling(on, mode | debt.omitted, dst, run),
        None => true,
    }
}

/// The settle-up chmod and its diagnostic, which is `quoteaf`'s where
/// [`settle_mode`]'s preserve branch is `quotef`'s. See there.
fn chmod_settling<E: Write>(on: On<'_>, mode: u32, dst: &Path, run: &mut Run<'_, E>) -> bool {
    let Err(e) = fsattr::set_mode(on, mode) else {
        return true;
    };
    let prog = run.opts.prog;
    let why = strerror(&e);
    let _ = writeln!(
        run.err,
        "{prog}: preserving permissions for {}: {why}",
        quoteaf_os(dst)
    );
    !run.opts.require_preserve
}

/// The permission bits currently on whatever `on` names.
///
/// # Errors
///
/// Whatever the `stat` said.
pub fn current_mode(on: On<'_>) -> io::Result<u32> {
    let meta = match on {
        On::File(f) => f.metadata()?,
        On::Path(path, Link::Follow) => fs::metadata(path)?,
        On::Path(path, Link::NoFollow) => fs::symlink_metadata(path)?,
    };
    Ok(permission_bits(&meta))
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
///   [`crate::stdfd::close_stdout`]: with `-v` this utility finally *has* stdout
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
/// `backup` is `None` at the directory call site, and for `cp` always will be:
/// `cp` backs a destination up only when it is *not* a directory
/// (`copy.c:2526`), and a directory source onto a non-directory destination is
/// refused earlier with `cannot overwrite non-directory`. `mv` is the utility
/// for which that combination exists — upstream's `FIXME` at `copy.c:2521` is
/// about exactly it — so the parameter is a genuine `Option` rather than a
/// `None` waiting to be simplified away.
fn announce<E: Write>(run: &mut Run<'_, E>, src: &Path, dst: &Path, backup: Option<&Path>) {
    if !run.opts.verbose {
        return;
    }
    match backup {
        // One `writeln!` and not two, because the parenthesis is part of *this*
        // line rather than a note after it: GNU's `emit_verbose` prints the
        // arrow with `printf` and only then the suffix, with the newline last.
        // Two writes would let a `cp -v … | head` truncate between them.
        Some(name) => {
            let _ = writeln!(
                run.out,
                "{} -> {} (backup: {})",
                quoteaf_os(src),
                quoteaf_os(dst),
                quoteaf_os(name)
            );
        }
        None => {
            let _ = writeln!(run.out, "{} -> {}", quoteaf_os(src), quoteaf_os(dst));
        }
    }
}

/// What is at the destination path, as far as the engine needs to know.
///
/// GNU carries the same three states in two variables — `new_dst` and whether
/// `dst_sb` was filled in — and the third state is the one that makes an enum
/// worth having: a destination that is *there* and cannot be stat'd. See
/// [`DestState::Opaque`].
pub enum DestState {
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
    /// Deliberately *not* folded into [`DestState::New`]: the difference decides
    /// which `open` [`open_destination`] tries first, and that in turn
    /// decides whether the destination is unlinked or the copy is refused as a
    /// dangling symlink.
    Opaque,
}

impl DestState {
    /// The `stat`, when there is one. `None` covers both "nothing is there"
    /// and "something is there that could not be stat'd", which is right for
    /// every caller: all of them are asking a question about the destination's
    /// *kind*, and neither state has one.
    #[must_use]
    pub fn metadata(&self) -> Option<&fs::Metadata> {
        match self {
            DestState::Exists(m) => Some(m),
            DestState::New | DestState::Opaque => None,
        }
    }

    /// Whether something is there — GNU's `! new_dst`.
    #[must_use]
    pub fn exists(&self) -> bool {
        !matches!(self, DestState::New)
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
/// Any `stat` failure other than "it isn't there", which is [`DestState::New`], and
/// other than `ELOOP` under `-f`, which is [`DestState::Opaque`].
pub fn stat_destination(
    src_meta: &fs::Metadata,
    target: &Path,
    opts: Opts<'_>,
) -> io::Result<DestState> {
    let use_lstat = src_meta.is_dir()
        || src_meta.file_type().is_symlink()
        || opts.unlink_dest_before_opening
        || opts.backup.enabled();
    let stat = if use_lstat {
        fs::symlink_metadata(target)
    } else {
        fs::metadata(target)
    };
    match stat {
        Ok(m) => Ok(DestState::Exists(m)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(DestState::New),
        Err(e) if opts.unlink_dest_after_failed_open && is_eloop(&e) => Ok(DestState::Opaque),
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
/// reaches [`DestState::Opaque`]. See `open_new`'s non-unix arm for the same
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
fn refuse_no_clobber<E: Write>(target: &Path, run: &mut Run<'_, E>) {
    let prog = run.opts.prog;
    let _ = writeln!(run.err, "{prog}: not replacing {}", quoteaf_os(target));
}

/// The three helpers this used to define privately — `can_write_any_file`,
/// `writable_destination` and `dest_mode`, plus the `euidaccess`/`geteuid`
/// binding under them — now live in [`crate::overwrite`], because `mv`
/// needs the identical three and a second copy of a decision about whether to
/// destroy a file is the kind of duplicate that is only noticed after the data
/// is gone. Upstream shares them by construction: `cp` and `mv` are two front
/// ends over one `copy.c`.
/// `-i`'s question — [`overwrite::overwrite_ok`] with this program's
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
fn overwrite_ok<E: Write>(
    target: &Path,
    dest_meta: Option<&fs::Metadata>,
    run: &mut Run<'_, E>,
) -> bool {
    let prog = run.opts.prog;
    let clears_destination =
        run.opts.unlink_dest_after_failed_open || run.opts.unlink_dest_before_opening;
    overwrite::overwrite_ok(
        run.err,
        prog,
        target,
        dest_meta,
        clears_destination,
        run.answers,
    )
}

/// What a check decided about a destination that is already there.
///
/// Three outcomes and not a `bool`, because two of GNU's paths leave the
/// destination alone and they **disagree about the exit status**. Upstream
/// carries the distinction as two locals — `skipped` and `return_val`
/// (`copy.c:2341`) — and the second is written `return_val = x->interactive ==
/// I_ALWAYS_SKIP`, which is exactly what this enum names.
///
/// It lived in `mv.rs`, and was a `bool` there until `--update` arrived: every
/// refusal `mv` had until then was a *failure*, so one bit was enough.
/// `--update=none` and `--update=older` are the first two that are not, and the
/// bug that found this out was `mv --update=none` exiting 1 for a file it had
/// deliberately left alone. It is in the engine now because
/// [`overwrite_allowed`] needs the same three values for the same reason, and
/// a second copy of a distinction that has already been got wrong once is how
/// it gets got wrong twice.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// Nothing stands in the way; go on.
    Proceed,
    /// Reported, and this operand counts against the exit status.
    Refused,
    /// Left alone on purpose, silently, and the command still succeeds.
    Skipped,
}

/// The whole of the "is this destination to be left alone" decision — GNU's
/// one block at `copy.c:2422`, which handles `-n` and `-i` together because
/// they are two values of one field.
///
/// The two *refusals* differ in what they print — `-n` says `not replacing
/// 'b'`, `-i` says nothing at all beyond the question it already asked — but
/// not in the status: both make the operand a failure, which is `copy.h`'s
/// "Skip and fail" for `I_ALWAYS_NO` and `return_val = x->interactive ==
/// I_ALWAYS_SKIP` (false here) for `I_ASK_USER`. [`Verdict::Skipped`] is the
/// third answer and the reason a `bool` will not do: `--update=none` leaves the
/// destination alone *and succeeds*.
///
/// A **directory** source is exempt from all of it, as GNU's `! S_ISDIR
/// (src_mode)` makes it: `cp -rn tree dest` descends and refuses the files
/// inside one at a time, and `cp -ri tree dest` asks about them one at a time,
/// rather than either putting a single question about the tree.
///
/// # Every value of [`Interactive`] now has an answer here
///
/// It used to have three, because `cp`'s parser produces three and the `bool`
/// could say what all three needed. `mv`'s parser produces the other two — `mv
/// -f` is `AlwaysYes`, `mv --update=none` is `AlwaysSkip` — and `AlwaysSkip`
/// is the one that did not fit. Widening the return type is what makes routing
/// `mv` through here possible; it is deliberately done *before* that routing,
/// rather than discovering the wrong exit status afterwards, which is how `mv`
/// found the same defect the first time.
pub fn overwrite_allowed<E: Write>(
    src_meta: &fs::Metadata,
    target: &Path,
    dest: &DestState,
    run: &mut Run<'_, E>,
) -> Verdict {
    if src_meta.is_dir() || !dest.exists() {
        return Verdict::Proceed;
    }
    match run.opts.interactive {
        // `AlwaysYes` is `mv -f`, which no caller of this function sets yet —
        // `cp -f` is `unlink_dest_after_failed_open`, a different field. The arm
        // is here rather than under a catch-all so that a program which *does*
        // set it has to arrive at a compile error rather than a silent
        // fall-through, and `Proceed` is the answer waiting for it: `mv -f`
        // overwrites without asking.
        Interactive::Unspecified | Interactive::AlwaysYes => Verdict::Proceed,
        Interactive::AlwaysNo => {
            refuse_no_clobber(target, run);
            Verdict::Refused
        }
        // `AlwaysSkip` is `--update=none`, which `mv` has and `cp` does not, so
        // no caller reaches this arm yet — but unlike the one above it is
        // reachable *wrongly*, and was: while this function returned `bool` the
        // only thing it could say here was "do not copy, and fail", which is
        // upstream's `return_val = x->interactive == I_ALWAYS_SKIP`
        // (`copy.c:2430`) inverted. Silence was the half it could express and
        // the exit status was the half it could not. Spelled out rather than
        // folded into a catch-all so that a fifth [`Interactive`] value arrives
        // as a compile error at this line.
        Interactive::AlwaysSkip => Verdict::Skipped,
        Interactive::AskUser => {
            if overwrite_ok(target, dest.metadata(), run) {
                Verdict::Proceed
            } else {
                Verdict::Refused
            }
        }
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
/// `dest` is updated to [`DestState::New`] on success, which is GNU's `new_dst =
/// true` and matters twice over: [`open_destination`] must then create
/// rather than truncate, and `place_source`'s symlink arm must not announce
/// a second `removed` for a name that is already gone.
///
/// **A destination that `--backup` is about to move aside is left alone.**
/// Upstream this unlink is not a separate step but the `else if` of the backup
/// block (`copy.c:2570`), and reading the two as independent removes the very
/// file the backup exists to keep: `cp --remove-destination -b a b` would
/// delete `b`, find nothing to rename, and report a plain copy — which is
/// `--backup` silently doing nothing at all. See [`backup_takes_destination`],
/// which is that `if`'s condition and is asked here for its `else`.
pub fn remove_destination_first<E: Write>(
    src: &Path,
    target: &Path,
    dest: &mut DestState,
    run: &mut Run<'_, E>,
) -> bool {
    let prog = run.opts.prog;
    if !run.opts.unlink_dest_before_opening || backup_takes_destination(src, dest, run.opts) {
        return true;
    }
    match dest {
        DestState::Exists(m) if !m.is_dir() => {}
        _ => return true,
    }
    if let Err(e) = fs::remove_file(target)
        && e.kind() != io::ErrorKind::NotFound
    {
        let why = strerror(&e);
        let _ = writeln!(
            run.err,
            "{prog}: cannot remove {}: {why}",
            quoteaf_os(target)
        );
        return false;
    }
    *dest = DestState::New;
    // On stdout and before the arrow line, unlike `-f`'s removal, which comes
    // after it. The two are not printed from the same place in GNU either:
    // this one is in `copy_internal` ahead of `emit_verbose`, `-f`'s is inside
    // `copy_reg` behind it. Measured — `cp --remove-destination -v a ro` says
    // `removed 'ro'` then `'a' -> 'ro'`, and `cp -fv a ro` says the reverse.
    if run.opts.verbose {
        let _ = writeln!(run.out, "removed {}", quoteaf_os(target));
    }
    true
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
/// because its `earlier_file` branch returns at `copy.c:2751`, well before the
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
/// — and it is faithfully reproduced by `copy_one` declining to
/// `Seen::record_dest` a `Linked` destination. A `bool` return could not
/// express that, which is the whole reason this enum exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placed {
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
    /// success count; only `Seen` cares which.
    #[must_use]
    pub fn is_ok(self) -> bool {
        self != Placed::Failed
    }
}

/// Copy one source of a known kind onto a settled destination path: the symlink,
/// the directory and the regular file, and nothing else.
///
/// The single place all three kinds are dispatched, reached identically by an
/// operand (through `place_source`) and by an entry found inside a tree
/// (through [`copy_entry`]). GNU funnels both through one `copy_internal`, and
/// the reason to match that is not tidiness: everything that happens *after* the
/// bytes are written — `-p` and each `--preserve=` attribute — happens for all
/// three kinds, so a second copy of this dispatch is a second place to forget
/// one of them. The two callers had already drifted once, when the symlink arm's
/// unlink of an existing destination reached an operand and not a walked entry,
/// and `cp -r` over a tree it had copied before answered `cannot create symbolic
/// link …: File exists`.
pub fn place_entity<E: Write>(
    src_path: &Path,
    metadata: &fs::Metadata,
    target: &Path,
    dest: &DestState,
    command_line_arg: bool,
    run: &mut Run<'_, E>,
) -> Placed {
    let prog = run.opts.prog;
    let src_mode = permission_bits(metadata);
    // Computed here, before the kind is dispatched, because GNU computes it
    // here — one expression covering all three kinds (`copy.c:2899`), read by
    // whichever of them creates the destination and settled by the tail they
    // share. See [`ModeDebt`].
    let debt = ModeDebt::new(run.opts.preserve_ownership, src_mode, metadata.is_dir());
    let mut dest_exists = dest.exists();

    // Clearing the way, before anything is said or written. GNU's one unlink
    // (`copy.c:2570`) covers every reason a destination has to *go* rather than
    // be written through, and reaching it before `emit_verbose` (`copy.c:2630`)
    // is what makes a `cp -v` that cannot clear the way announce nothing.
    //
    // Two of GNU's reasons are expressible here, and they are `||`-ed there
    // too. The third is `x->move_mode`, which no caller sets yet:
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
        run.opts.preserve_links && dest.metadata().is_some_and(|m| nlink(m) > 1);

    // `-b`: the destination is moved aside rather than written over, and this is
    // GNU's block at `copy.c:2517`. It is the `if` whose `else if` is the unlink
    // below, in upstream too — the two are alternatives, and reading them as
    // independent would unlink the very destination that had just been renamed
    // out of harm's way, which is the backup made and then thrown away. The
    // condition itself is [`backup_takes_destination`], which documents its three
    // clauses and is asked for its `else` by [`remove_destination_first`].
    let mut moved_aside: Option<PathBuf> = None;
    if backup_takes_destination(src_path, dest, run.opts) {
        // The one refusal, and it is the reason `cp` needs the *suffix* even
        // when the type is numbered: `cd /tmp; rm -f a a~; : > a; echo A > a~;
        // cp --backup=simple a~ a` would name the backup of `a` exactly `a~`,
        // rename the source on top of itself, and leave two empty files where
        // there had been one empty and one full. Upstream's own comment carries
        // that recipe verbatim. Numbered backups are exempt because the name
        // they choose is never one the user typed.
        if run.opts.backup.kind() != BackupType::Numbered
            && source_is_dst_backup(src_path, metadata, target, run.opts.backup.simple_suffix())
        {
            let _ = writeln!(
                run.err,
                "{prog}: backing up {} might destroy source;  {} not copied",
                quoteaf_os(target),
                quoteaf_os(src_path)
            );
            return Placed::Failed;
        }
        match run.opts.backup.rename(target) {
            Ok(name) => moved_aside = Some(name),
            // "Nothing was there" is not a failure: upstream's `else if (errno
            // != ENOENT)`. It can happen even though the `stat` above found
            // something, because the two are separate syscalls — and it is the
            // ordinary answer for a *dangling* symlink destination under a
            // simple rename that has already moved it.
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => {
                let why = strerror(&e);
                let _ = writeln!(
                    run.err,
                    "{prog}: cannot backup {}: {why}",
                    quoteaf_os(target)
                );
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
        if !remove_before_writing(target, run) {
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
        announce(run, src_path, target, moved_aside.as_deref());
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
    // hard-linked-directory refusal, which lives in `copy_one` because only
    // an operand can reach it.
    //
    // On a host without hard links [`nlink`] answers 1 to everything, which
    // switches `--preserve=links` off by exactly the amount that host cannot
    // honour it: the first half of the condition never fires and the dereference
    // half still does, so `cp --preserve=links -L la lb d` is the only spelling
    // that reaches [`hardlink::force_link`] there — and it then fails with
    // whatever the platform says about [`fs::hard_link`], which is the honest
    // answer.
    let mut recorded = None;
    if !metadata.is_dir()
        && run.opts.preserve_links
        && (nlink(metadata) > 1 || run.opts.should_dereference(command_line_arg))
        && let Some(id) = file_id(src_path, metadata)
    {
        if let Some(earlier) = run.copied.remember(&id, target) {
            return if hardlink::force_link(
                prog,
                &earlier,
                target,
                run.opts.verbose,
                &mut *run.out,
                &mut *run.err,
            ) {
                Placed::Linked
            } else {
                // GNU reaches its `un_backup` label from here too (`copy.c:2705`
                // is one of eleven `goto`s to it), and does *not* run the
                // `forget_created` half — `earlier_file` is non-null on this
                // path, which is exactly the `recorded == None` this branch
                // leaves behind. See the tail below.
                backup::un_backup(
                    prog,
                    moved_aside.as_deref(),
                    target,
                    run.opts.verbose,
                    &mut *run.out,
                    &mut *run.err,
                );
                Placed::Failed
            };
        }
        recorded = Some(id);
    }

    let ok = place_bytes(src_path, metadata, src_mode, target, dest_exists, debt, run);

    // GNU's `un_backup` label: a source recorded a moment ago whose copy then
    // failed must be un-recorded, or a later operand naming the same inode
    // would try to hard-link to a destination that does not exist and would
    // report `cannot create hard link` in place of the failure that actually
    // happened. The guard there is `earlier_file == nullptr`, which is this
    // `recorded.is_some()` — the linking path above never reaches here.
    if !ok {
        if let Some(id) = &recorded {
            run.copied.forget(id);
        }
        // And the half the label is named for. In upstream's order: forget
        // first, then put the backup back.
        backup::un_backup(
            prog,
            moved_aside.as_deref(),
            target,
            run.opts.verbose,
            &mut *run.out,
            &mut *run.err,
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
/// [`DestState::Opaque`] answers `false` to the second, which reads like a
/// difference from upstream and cannot be reached: a destination is only
/// `Opaque` when its `stat` failed with `ELOOP`, and with backups on
/// [`stat_destination`] uses `lstat`, which a symlink loop does not trouble.
fn backup_takes_destination(src: &Path, dest: &DestState, opts: Opts<'_>) -> bool {
    opts.backup.enabled()
        && dest.exists()
        && !dest.metadata().is_some_and(fs::Metadata::is_dir)
        && !src_base_is_dot_or_dotdot(src)
}

/// The three kinds, dispatched. Split from [`place_entity`] only so that the
/// preamble it shares — the unlink, the announcement and the link bookkeeping —
/// has one exit to attach the `un_backup` step to rather than one per arm.
fn place_bytes<E: Write>(
    src_path: &Path,
    metadata: &fs::Metadata,
    src_mode: u32,
    target: &Path,
    dest_exists: bool,
    mut debt: ModeDebt,
    run: &mut Run<'_, E>,
) -> bool {
    let prog = run.opts.prog;
    if metadata.file_type().is_symlink() {
        if let Err(e) = clone_symlink(src_path, target) {
            let why = strerror(&e);
            let _ = writeln!(
                run.err,
                "{prog}: cannot create symbolic link {}: {why}",
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
        if run.opts.preserve_ownership
            && chown_to_source(
                src,
                On::Path(target, Link::NoFollow),
                target,
                Made::Symlink,
                true,
                run,
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
            run,
        );
    }

    if metadata.is_dir() {
        let (new, contents_ok) = match copy_tree(src_path, src_mode, target, &mut debt, run) {
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
            run,
        );
        return contents_ok && stamped;
    }

    copy_regular_file(src_path, metadata, target, dest_exists, debt, run)
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
fn remove_before_writing<E: Write>(target: &Path, run: &mut Run<'_, E>) -> bool {
    let prog = run.opts.prog;
    if let Err(e) = fs::remove_file(target)
        && e.kind() != io::ErrorKind::NotFound
    {
        let why = strerror(&e);
        let _ = writeln!(
            run.err,
            "{prog}: cannot remove {}: {why}",
            quoteaf_os(target)
        );
        return false;
    }
    // On stdout, in its own sentence and before the arrow line
    // (`copy.c:2586`).
    if run.opts.verbose {
        let _ = writeln!(run.out, "removed {}", quoteaf_os(target));
    }
    true
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
fn copy_tree<E: Write>(
    src: &Path,
    src_mode: u32,
    dest: &Path,
    debt: &mut ModeDebt,
    run: &mut Run<'_, E>,
) -> TreeResult {
    let prog = run.opts.prog;
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
                announce(run, src, dest, None);
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
                            run.err,
                            "{prog}: setting permissions for {}: {why}",
                            quoteaf_os(dest)
                        );
                        return TreeResult::Unmade;
                    }
                }
                true
            }
            Err(e) => {
                let why = strerror(&e);
                let _ = writeln!(run.err, "{prog}: cannot stat {}: {why}", quoteaf_os(dest));
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
                run.err,
                "{prog}: cannot create directory {}: {why}",
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
                if !copy_entry(&entry, dest, run) {
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
            let _ = writeln!(run.err, "{prog}: cannot access {}: {why}", quoteaf_os(src));
            ok = false;
        }
    }

    TreeResult::Made { new, ok }
}

/// One entry of a directory being walked. Split out of [`copy_tree`] only to
/// keep the mode bookkeeping either side of the walk readable in one screen.
///
/// The containing directory is no longer a parameter: the `readdir` that could
/// fail now happens in [`read_dir_fastread`], so the only caller that ever had
/// to name the *source directory* in a diagnostic is the one that reads it.
fn copy_entry<E: Write>(entry: &fs::DirEntry, dest: &Path, run: &mut Run<'_, E>) -> bool {
    let prog = run.opts.prog;
    let from = entry.path();
    let to = dest.join(entry.file_name());

    // `DirEntry::metadata` does **not** follow symlinks, unlike `Path::is_dir`.
    // That is the whole of the fix for bug 1, and it also hands over the mode
    // the copy is to be created with, which a second `stat` might not.
    //
    // `-L` is the one policy that wants the other answer *here*, and asking for
    // it costs the extra `stat` that `entry.metadata()` was avoiding — there is
    // no following variant of it. That is the right way round: the option that
    // is not given pays nothing. See [`Opts::follow_walked`] for why `-H`
    // takes this branch and not the other one.
    let meta = if run.opts.follow_walked() {
        fs::metadata(&from)
    } else {
        entry.metadata()
    };
    let meta = match meta {
        Ok(m) => m,
        Err(e) => {
            let why = strerror(&e);
            let _ = writeln!(run.err, "{prog}: cannot stat {}: {why}", quoteaf_os(&from));
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
    // Of the refusals `copy_one` makes either side of this, the ones about a
    // *file* named twice are not here and cannot arise — a file found by
    // walking was not named on the command line, and nothing this command
    // created can be reached inside the tree it is filling. The one about a
    // *directory* seen twice is a different matter and is below: a walk can
    // reach a directory an operand already copied.
    let mut dest_state = match stat_destination(&meta, &to, run.opts) {
        Ok(d) => d,
        Err(e) => {
            let why = strerror(&e);
            let _ = writeln!(run.err, "{prog}: cannot stat {}: {why}", quoteaf_os(&to));
            return false;
        }
    };
    // `-n`'s refusal and `-i`'s question, for an entry found by walking. The
    // same-file check `copy_one` makes just before this one is not here and
    // cannot fire: a tree is not being copied into itself, and if it were the
    // walk would not terminate to reach this point.
    match overwrite_allowed(&meta, &to, &dest_state, run) {
        Verdict::Proceed => {}
        Verdict::Refused => return false,
        // Unreachable from `cp`, whose parser has no `--update=none`, and
        // correct for whoever adds one: the entry is left where it is and the
        // walk goes on, without this operand counting against the status.
        Verdict::Skipped => return true,
    }
    // The two kind mismatches, in `copy_one`'s order and with its wording,
    // because they are the same `copy_internal` lines. Reaching them here is
    // what stops a directory landing on a file as `cannot create directory …:
    // File exists`, which named the right path and the wrong problem.
    if let Some(dest_meta) = dest_state.metadata() {
        if meta.is_dir() && !dest_meta.is_dir() {
            let _ = writeln!(
                run.err,
                "{prog}: cannot overwrite non-directory {} with directory {}",
                quoteaf_os(&to),
                quoteaf_os(&from)
            );
            return false;
        }
        if !meta.is_dir() && dest_meta.is_dir() {
            let _ = writeln!(
                run.err,
                "{prog}: cannot overwrite directory {} with non-directory",
                quoteaf_os(&to)
            );
            return false;
        }
    }
    if !remove_destination_first(&from, &to, &mut dest_state, run) {
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
    // refused at the operand before any walk starts (see `place_source`); GNU
    // can reach it only because it additionally records the inode of the first
    // destination directory it creates (`copy.c:2982`), which this `cp` does
    // not do — see design-decisions.md 724 for why it refuses up front instead.
    // The second needs two operands to have been copied to one path, which
    // `copy_one`'s own arm answers first, with the warning that names the
    // operand.
    if meta.is_dir()
        && let Some(id) = file_id(&from, &meta)
        && let Some(earlier) = run.copied.lookup(&id).map(Path::to_path_buf)
    {
        // GNU's third arm, with `command_line_arg` false so that only `-L`
        // satisfies it: following symlinks was asked for, so two paths reaching
        // one directory are a request for two independent copies of it and are
        // made silently. `cp -RL a b d` with `a/l` and `b/l` both links to `c`
        // is the case in its comment.
        if !run.opts.follow_walked() {
            let _ = writeln!(
                run.err,
                "{prog}: will not create hard link {} to directory {}",
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
    place_entity(&from, &meta, &to, &dest_state, false, run).is_ok()
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
fn copy_regular_file<E: Write>(
    src: &Path,
    src_meta: &fs::Metadata,
    dst: &Path,
    dest_exists: bool,
    mut debt: ModeDebt,
    run: &mut Run<'_, E>,
) -> bool {
    let prog = run.opts.prog;
    // The announcement is [`place_entity`]'s and has already happened, which is
    // GNU's order: `emit_verbose` (`copy.c:2630`) runs before `copy_reg`, so an
    // unreadable source is announced and *then* complained about.
    let mut input = match fs::File::open(src) {
        Ok(f) => f,
        Err(e) => {
            let why = strerror(&e);
            let _ = writeln!(
                run.err,
                "{prog}: cannot open {} for reading: {why}",
                quoteaf_os(src)
            );
            return false;
        }
    };

    // [`Clobber::Unlink`] borrows the stdout it would announce the removal on,
    // and the two options read beside it are *different fields* of the same
    // [`Run`] — which is why all three can be read in one expression. Before
    // stage 4 they could not be: the options lived on `cp`'s `Job` and this
    // code had to hoist each one into a local first, because the value it read
    // them through was the same one already borrowed for its stdout.
    let dest = if dest_exists {
        Dest::Exists(if run.opts.unlink_dest_after_failed_open {
            Clobber::Unlink {
                verbose: run.opts.verbose,
                out: &mut *run.out,
            }
        } else {
            Clobber::Never
        })
    } else {
        Dest::New
    };
    // On its own statement rather than as the `match` scrutinee: the borrow of
    // `run.out` inside `dest` ends when the call returns, and a scrutinee's
    // temporaries live to the end of the `match` — whose arms want `run.err`.
    let opened = open_destination(
        dst,
        permission_bits(src_meta),
        dest,
        run.opts.preserve_xattr,
        &mut debt,
    );
    let (mut output, new_dst) = match opened {
        Ok(Opened { file, new }) => (file, new),
        Err(DestError::Dangling(_)) => {
            // The `EEXIST` is dropped: GNU's sentence for this names no error
            // at all, because the failure is not the open's — the name resolved
            // to nothing and writing through it would be a race.
            let _ = writeln!(
                run.err,
                "{prog}: not writing through dangling symlink {}",
                quoteaf_os(dst)
            );
            return false;
        }
        Err(DestError::Remove(e)) => {
            let why = strerror(&e);
            let _ = writeln!(run.err, "{prog}: cannot remove {}: {why}", quoteaf_os(dst));
            return false;
        }
        Err(DestError::Io(e)) => {
            let why = strerror(&e);
            let _ = writeln!(
                run.err,
                "{prog}: cannot create regular file {}: {why}",
                quoteaf_os(dst)
            );
            return false;
        }
    };

    // The engine's body rather than a loop here, which is what makes this arm
    // and `mv`'s cross-device arm the same code. The gain is not only the
    // de-duplication: the loop that used to live here never offloaded, so it
    // pushed every byte of every copy through userspace where GNU hands the
    // work to the kernel. See [`copy_bytes`].
    //
    // The sentence comes back rather than being printed there because `mv` has
    // a backup to undo before it can print; `cp` has nothing to undo, so it
    // prints immediately and gives up on this operand — which is what the
    // `false` says, and it is the same `false` the loop returned.
    if let Err(CopyError { what, err }) = copy_bytes(&mut input, &mut output, src, dst) {
        let why = strerror(&err);
        let _ = writeln!(run.err, "{prog}: {what}: {why}");
        return false;
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
        run,
    )
}

/// [`permission_bits`] of the name `path`, without following a final symlink.
fn permission_bits_of(path: &Path) -> io::Result<u32> {
    fs::symlink_metadata(path).map(|m| permission_bits(&m))
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
