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
//! * **The first half of stage 2**, the *preserve tail* —
//!   [`preserve_attributes`] and everything it calls. This is
//!   `copy_internal`'s closing run of steps (`copy.c:3205` onwards) merged with
//!   `copy_reg`'s (`copy.c:1626` onwards), which upstream writes twice because
//!   they live in two functions and which this tree, until now, wrote *four*
//!   times — twice in `cp.rs` following upstream, and twice more in `mv.rs`
//!   because a move was written as its own program.
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
//! What is **not** here yet is everything that writes bytes: opening a
//! destination, the copy proper, and the walk. Those are still `cp`'s, and the
//! tail below is reached from `cp.rs` through call sites that build a [`Run`]
//! from its `Job`. `mv` still has its own copy of the tail as well; making it
//! call this one is the next step, and is the point at which the duplication
//! this module exists to end actually ends.

use crate::errmsg::strerror;
use crate::fsattr::{
    self, GroupRetry, Link, On, Ownership, chown_privileges, is_denied_ownership, owner_differs,
    owner_of, permission_bits, times_of,
};
use crate::quote::{escape_os, quoteaf_os, quotef_os};
use std::fs;
use std::io;
use std::io::Write;
use std::path::Path;

/// The engine's options: upstream's `struct cp_options`, restricted to the
/// fields the code in this module actually reads.
///
/// Restricted deliberately. `cp`'s own `CpFlags` has a dozen more — `-r`, `-v`,
/// `-T`, the backup type, the interactive mode — and none of them mean anything
/// to the steps here, which stamp a destination that already exists. Passing
/// the whole of `CpFlags` would compile and would quietly make this module
/// `cp`-shaped: `mv` would have to fabricate a value for every field it has no
/// concept of, and the next reader could not tell which of the dozen the engine
/// depends on. The list below *is* that answer.
///
/// Every field is one of GNU's, under GNU's name, and `mv` supplies for each
/// one the constant `mv.c`'s `cp_option_init` supplies — which is what makes
/// `mv` expressible as this engine rather than as a parallel implementation of
/// it. The three that look like they should be constants are the interesting
/// ones: `mv` sets [`Self::preserve_mode`] and friends *true* and all three of
/// the loudness flags *false*, so it preserves everything and fails at nothing.
#[derive(Clone, Copy)]
pub struct Opts {
    /// What to put in front of a diagnostic: `"cp"` or `"mv"`.
    ///
    /// The engine prints its own sentences rather than returning them, because
    /// GNU does — a copy reports each attribute it could not carry and then
    /// carries on to the next, so there is no single error to return and the
    /// caller has nothing to add. The prefix is the only part of the sentence
    /// that differs between the two programs, so it is the only part passed in.
    pub prog: &'static str,
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

/// One run of the engine: the options it was given, and the place it says
/// things.
///
/// The two are one value rather than two parameters because they travel
/// together through every step and always will — which is not an observation
/// about the current call graph but about what the steps *are*. A step of the
/// tail reads an option, does one syscall, and reports what it could not do;
/// there is no step that reads options without being able to complain and none
/// that complains without having read one. `cp.rs`'s `Job` is the same
/// discovery made once already, for the same reason, over a superset of these
/// fields.
///
/// It will grow. The walk (stage 3) needs the stdout `--verbose` announces on,
/// the table of which inode went where, and the stream `-i`'s prompts are
/// answered from — all three of which `Job` already carries, and all three of
/// which arrive here when the walk does. That is the direction of travel: `Job`
/// is being emptied into this, not copied alongside it.
pub struct Run<'a, E: Write> {
    /// What was asked for. By value, not by reference: it is nine `bool`s and a
    /// word, so a reference would cost as much as the copy and add a lifetime.
    pub opts: Opts,
    /// Where a failure to carry an attribute is reported.
    ///
    /// Generic rather than `Stderr`, so a test can assert on what a copy said —
    /// which is how this crate tests diagnostics at all. Not `dyn`, because
    /// unlike the prompt stream this one is written on a per-*file* path.
    pub err: &'a mut E,
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

/// What kind of destination [`preserve_attributes`] is stamping.
///
/// Three kinds and not a `bool`, because all three answer the two questions
/// differently: a symlink takes neither an ownership step (it was chowned where
/// it was made) nor a mode at all, and a directory settles its withheld
/// permissions by a different formula from a regular file's. See
/// [`settle_mode`].
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
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
/// * `copy.c:3245` — "chown turns off set[ug]id bits for non-root, so do the
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
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
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
