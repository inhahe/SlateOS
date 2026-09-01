//! What to do about a destination that is already there — `-i`, `-n` and `-f`.
//!
//! Upstream this is not a file of its own: it is four functions near the top of
//! `copy.c` (`can_write_any_file` via `lib/write-any-file.c`,
//! `writable_destination` at 1979, `overwrite_ok` at 1988, `abandon_move` at
//! 2062) plus one field of `struct cp_options`. But `copy.c` *is* the sharing —
//! `cp`, `mv` and `install` are three front ends over one copier, so upstream
//! gets the agreement for free and never had to decide where to put it. Here
//! `cp.rs` and `mv.rs` are separate programs, so the choice is between this
//! module and two copies.
//!
//! Two copies would be wrong for the same reason [`crate::yesno`] is a module:
//! the person typing the answer learned the rule once. But the stakes are
//! higher here than at the prompt, because three of the four things in this
//! file are decisions about *whether to destroy a file*, and a `cp` and an `mv`
//! that disagreed about them would disagree in a way nobody would notice until
//! the data was gone.
//!
//! # The four things
//!
//! * [`Interactive`] — `copy.h:75`'s enum. The reason `-i`/`-n`/`-f` are one
//!   field rather than three booleans, and therefore the reason `mv -in` is
//!   `-n` while `mv -ni` is `-i`.
//! * [`can_write_any_file`] and [`writable_destination`] — whether the
//!   permission bits are worth mentioning. These decide only the *wording* of
//!   the question, but they are also what `mv` consults to decide whether to
//!   ask it at all (see [`Interactive::Unspecified`]).
//! * [`overwrite_ok`] — the question, and the answer.
//!
//! What is deliberately *not* here is the surrounding decision — GNU's
//! `abandon_move` and the `else` branch beside it at `copy.c:2409-2431`. Those
//! two blocks are the one place where `cp` and `mv` genuinely differ, and the
//! comment upstream puts between them says so in as many words: "cp and mv
//! treat -i and -f differently." Folding them together would mean a parameter
//! for each difference, which is three parameters saying "am I mv", so each
//! program keeps its own and calls in here for the parts that are the same.

use std::fs;
use std::io::Write;
use std::path::Path;

use quoting::quoteaf_os;

use crate::yesno::{Answers, yesno};

/// What to do about a destination that is already there.
///
/// GNU's `enum Interactive` (`copy.h:75`). Four of its five members are here;
/// the fifth is `I_ALWAYS_SKIP`, reached only through `--update=none`, which
/// neither utility has yet and which differs from [`Interactive::AlwaysNo`] in
/// exactly the two ways `copy.h`'s own comments give: "Skip and fail" against
/// "Skip and ignore".
///
/// An enum and not three booleans because these are alternatives rather than
/// independent switches: GNU stores one value and lets the last option given
/// overwrite it, which is why `mv -in` is `-n` and `mv -ni` is `-i`. Booleans
/// could hold two at once, which is a state the command line cannot express.
///
/// The two programs do not populate it identically, and that is upstream's
/// doing rather than an omission here: `mv`'s `-f` sets
/// [`Interactive::AlwaysYes`] (`mv.c:373`), so it is part of this field and
/// last-wins applies to it, while `cp`'s `-f` is `unlink_dest_after_failed_open`
/// — a different field entirely, which is why `cp -f -i` still asks and
/// `mv -f -i` also asks but for the opposite reason. See [`overwrite_ok`]'s
/// `clears_destination`.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Interactive {
    /// None of `-n`, `-i`, `-f` given. GNU's `I_UNSPECIFIED`.
    ///
    /// Not simply "go ahead": for `mv` this is the value that consults
    /// [`writable_destination`] and `isatty(0)`, because GNU asks before
    /// clobbering a file the mode says you may not write **even with no `-i`**
    /// — as long as there is a human there to answer. See `abandon_move`
    /// (`copy.c:2069`).
    #[default]
    Unspecified,
    /// `-n` / `--no-clobber`: leave an existing destination alone, and *fail*.
    /// GNU's `I_ALWAYS_NO`, whose comment is "Skip and fail".
    AlwaysNo,
    /// `mv -f`: never ask, whatever the mode says. GNU's `I_ALWAYS_YES`.
    ///
    /// `cp` never sets this — its `-f` means something else — so a `cp` reading
    /// this field will not see it. It is in the shared enum rather than in
    /// `mv.rs` because it is in the shared enum upstream, and because the
    /// last-wins rule that gives `-f` its only observable effect is a property
    /// of the *field*, not of any one of its values.
    AlwaysYes,
    /// `-i` / `--interactive`: ask, and take silence for no. GNU's
    /// `I_ASK_USER`. See [`overwrite_ok`] for what is asked and
    /// [`writable_destination`] for why there are three wordings of it.
    AskUser,
}

#[cfg(unix)]
unsafe extern "C" {
    /// `euidaccess(path, mode)`, where mode 2 is `W_OK`. GNU asks this as
    /// `faccessat (dst_dirfd, dst_relname, W_OK, AT_EACCESS)`, and `AT_EACCESS`
    /// is the whole reason it is not plain `access(2)`: `access` answers about
    /// the *real* uid, which for a setuid `cp` is a question nobody asked.
    fn euidaccess(path: *const u8, mode: i32) -> i32;
    fn geteuid() -> u32;
}

/// GNU's `can_write_any_file` (`lib/write-any-file.c`): whether the permission
/// bits can be ignored, because in traditional Unix root's writes are never
/// refused for want of a `w`.
///
/// Not cached, where upstream caches it in a `static`. The call happens once
/// per prompt — that is, once per question put to a human — so a cache would be
/// saving a syscall per keystroke, and glibc already answers `geteuid` from a
/// cached value in any case.
#[cfg(unix)]
#[must_use]
pub fn can_write_any_file() -> bool {
    // SAFETY: `geteuid` takes no arguments, dereferences nothing, and cannot
    // fail — POSIX gives it no error return.
    unsafe { geteuid() == 0 }
}

/// Off unix there is no euid to ask about. The host build is a test vehicle,
/// not a shipping one.
#[cfg(not(unix))]
#[must_use]
pub fn can_write_any_file() -> bool {
    false
}

/// GNU's `writable_destination` (`copy.c:1979`): whether the plain question is
/// asked or one of the two that quote the mode.
///
/// Three parts, in upstream's order and short-circuiting as upstream does:
///
/// * **A symlink destination is always "writable".** Its own mode is `0777` on
///   every system that has modes at all, so testing it would say nothing; the
///   permission that matters belongs to whatever it points at, which the
///   `access` below is about to ask about anyway.
/// * **Root can write anything.** See [`can_write_any_file`].
/// * **Otherwise, `access(W_OK)` with the effective ids.** Not "does the mode
///   have a `w` for me" computed from the bits: that reading gets ACLs,
///   read-only mounts and immutable files all wrong, and each of those is a
///   case where the prompt would promise a write that then fails.
///
/// Note what this is *not* used for in `cp`: it changes only the wording of the
/// question. Answering `y` to `unwritable … try anyway?` goes on to attempt the
/// copy and — with no `-f` — to fail it with `Permission denied`, which is
/// measured and is upstream's behaviour too. The prompt is a warning, not a
/// gate. In `mv` it does one thing more, and that one is a gate: with no option
/// at all it is half of the condition that decides whether to ask.
#[cfg(unix)]
#[must_use]
pub fn writable_destination(target: &Path, dest_meta: &fs::Metadata) -> bool {
    if dest_meta.file_type().is_symlink() || can_write_any_file() {
        return true;
    }
    let Ok(c_path) = crate::pathname::c_path(target) else {
        // A path with an interior NUL cannot be handed to a C function. It also
        // cannot name a file, so the operation is about to fail anyway; say
        // "writable" so the question asked is the plain one rather than one
        // quoting a mode that was never read. This is the one caller that
        // *swallows* that error rather than reporting it, because the answer
        // here only chooses a wording.
        return true;
    };
    // SAFETY: `c_path` is NUL-terminated, has no interior NUL, and outlives the
    // call. `euidaccess` reads it and does not retain it.
    unsafe { euidaccess(c_path.as_ptr(), 2) == 0 }
}

/// Off unix there are no modes to quote and no `euidaccess` to ask, so `-i`
/// always puts the plain question. The host build is a test vehicle, not a
/// shipping one.
#[cfg(not(unix))]
#[must_use]
pub fn writable_destination(_target: &Path, _dest_meta: &fs::Metadata) -> bool {
    true
}

/// The destination's `st_mode`. Only the permission bits are ever printed, but
/// the whole word is what [`writable_destination`]'s symlink test needs and
/// what GNU passes around.
#[cfg(unix)]
#[must_use]
pub fn dest_mode(dest_meta: &fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    dest_meta.mode()
}

#[cfg(not(unix))]
#[must_use]
pub fn dest_mode(_dest_meta: &fs::Metadata) -> u32 {
    0
}

/// GNU's `overwrite_ok` (`copy.c:1988`): the question, and the answer.
///
/// Three wordings, all measured against 9.4 — here with `cp`'s name, but the
/// name is a parameter because upstream's is (`program_name`), and `mv` puts
/// the identical sentences under its own:
///
/// ```text
/// cp: overwrite 'b'?
/// cp: unwritable 'b' (mode 0444, r--r--r--); try anyway?
/// cp: replace 'b', overriding mode 0444 (r--r--r--)?
/// ```
///
/// The second and third are the same condition — [`writable_destination`] said
/// no — split by `clears_destination`: whether the destination is going to be
/// *removed* rather than written through. Upstream computes it as `x->move_mode
/// || x->unlink_dest_before_opening || x->unlink_dest_after_failed_open`, so it
/// is always true for `mv` and is `-f`/`--remove-destination` for `cp`. That
/// split is the difference between a warning that the operation will probably
/// fail and a warning that it will probably succeed by destroying something the
/// mode was protecting.
///
/// `dest_meta` is `None` when something is at the destination that could not be
/// `stat`'d, so there is no mode to quote and the plain question is the only one
/// that can be asked. GNU reaches that case with an *uninitialised* `dst_sb`
/// (`copy.c:2209` declares it and the `ELOOP` arm at 2326 leaves it alone), and
/// would print whatever was on its stack. That is not a behaviour worth
/// reproducing.
///
/// Four details that the obvious implementation gets wrong:
///
/// * **No trailing newline.** The cursor stays after the `? `, on the question's
///   line, as upstream. Which is why the flush below is not optional:
///   [`crate::stdfd::Stream`] buffers by line to a terminal, so an unflushed
///   prompt would leave the user looking at nothing while the utility waits for
///   a keypress.
/// * **It is on stderr**, not stdout — so `cp -i a b 2>/dev/null` asks
///   invisibly and `cp -iv a b > log` puts the question and the `'a' -> 'b'`
///   line in different places. Both measured.
/// * **The mode is four octal digits including the setuid bits** (`%04lo` of
///   `st_mode & CHMOD_MODE_BITS`, which is `07777`), and the `r--r--r--` beside
///   it is `strmode` with its type letter dropped — GNU writes `&perms[1]`.
/// * **Declining is failure, and is silent.** The caller returns `false`, which
///   is exit 1, and prints nothing: upstream's `skip:` label prints `not
///   replacing` only for [`Interactive::AlwaysNo`], and `skipped 'b'` only under
///   `--debug`.
pub fn overwrite_ok<W: Write>(
    err: &mut W,
    program: &str,
    target: &Path,
    dest_meta: Option<&fs::Metadata>,
    clears_destination: bool,
    answers: &mut dyn Answers,
) -> bool {
    let name = quoteaf_os(target);
    let sentence = match dest_meta {
        Some(m) if !writable_destination(target, m) => {
            let mode = dest_mode(m) & 0o7777;
            let perms = modechange::permission_string(mode);
            if clears_destination {
                format!("{program}: replace {name}, overriding mode {mode:04o} ({perms})? ")
            } else {
                format!("{program}: unwritable {name} (mode {mode:04o}, {perms}); try anyway? ")
            }
        }
        _ => format!("{program}: overwrite {name}? "),
    };
    let _ = err.write_all(sentence.as_bytes());
    let _ = err.flush();
    yesno(answers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yesno::Canned;

    /// The three sentences, in one place, so that a change to any of them is a
    /// change to a test rather than a silent change to two programs at once.
    #[test]
    fn the_three_wordings_are_selected_by_writability_and_removal() {
        let dir = tempdir();
        let plain = dir.join("plain");
        fs::write(&plain, b"x").expect("write");

        let mut err: Vec<u8> = Vec::new();
        let mut answers = Canned::new(&["y\n"]);
        assert!(overwrite_ok(
            &mut err,
            "cp",
            &plain,
            Some(&fs::symlink_metadata(&plain).expect("stat")),
            false,
            &mut answers
        ));
        assert_eq!(
            String::from_utf8_lossy(&err),
            format!("cp: overwrite {}? ", quoteaf_os(&plain))
        );

        // An unstattable destination gets the plain question too, because there
        // is no mode to put in the other two.
        let mut err: Vec<u8> = Vec::new();
        let mut answers = Canned::new(&["n\n"]);
        assert!(!overwrite_ok(
            &mut err,
            "mv",
            Path::new("opaque"),
            None,
            true,
            &mut answers
        ));
        assert_eq!(String::from_utf8_lossy(&err), "mv: overwrite 'opaque'? ");
    }

    #[cfg(unix)]
    #[test]
    fn an_unwritable_destination_picks_its_wording_from_clears_destination() {
        use std::os::unix::fs::PermissionsExt;

        // Running as root makes every file writable, so the two wordings this
        // test is about are unreachable. `can_write_any_file` is the thing
        // being relied on, so ask it rather than `id -u`.
        if can_write_any_file() {
            return;
        }

        let dir = tempdir();
        let ro = dir.join("ro");
        fs::write(&ro, b"x").expect("write");
        fs::set_permissions(&ro, fs::Permissions::from_mode(0o444)).expect("chmod");
        let meta = fs::symlink_metadata(&ro).expect("stat");

        let name = quoteaf_os(&ro).to_string();

        let mut err: Vec<u8> = Vec::new();
        let mut answers = Canned::new(&["n\n"]);
        assert!(!overwrite_ok(
            &mut err,
            "cp",
            &ro,
            Some(&meta),
            false,
            &mut answers
        ));
        assert_eq!(
            String::from_utf8_lossy(&err),
            format!("cp: unwritable {name} (mode 0444, r--r--r--); try anyway? ")
        );

        // `mv` always takes the other branch, because upstream's
        // `clears_destination` includes `x->move_mode` unconditionally.
        let mut err: Vec<u8> = Vec::new();
        let mut answers = Canned::new(&["y\n"]);
        assert!(overwrite_ok(
            &mut err,
            "mv",
            &ro,
            Some(&meta),
            true,
            &mut answers
        ));
        assert_eq!(
            String::from_utf8_lossy(&err),
            format!("mv: replace {name}, overriding mode 0444 (r--r--r--)? ")
        );
    }

    /// A symlink is "writable" whatever it points at, so `-i` over a dangling
    /// link asks the plain question rather than quoting the link's own `0777`.
    #[cfg(unix)]
    #[test]
    fn a_symlink_destination_is_always_writable() {
        let dir = tempdir();
        let link = dir.join("link");
        std::os::unix::fs::symlink("nowhere", &link).expect("symlink");
        let meta = fs::symlink_metadata(&link).expect("stat");
        assert!(writable_destination(&link, &meta));
    }

    /// End of input is "no", which is what makes a `-i` in a script safe rather
    /// than a hang. Pinned here because it is the property the two callers rely
    /// on and neither tests.
    #[test]
    fn silence_declines() {
        let dir = tempdir();
        let plain = dir.join("plain");
        fs::write(&plain, b"x").expect("write");
        let mut err: Vec<u8> = Vec::new();
        let mut answers = Canned::new(&[]);
        assert!(!overwrite_ok(
            &mut err,
            "cp",
            &plain,
            Some(&fs::symlink_metadata(&plain).expect("stat")),
            false,
            &mut answers
        ));
    }

    /// Nothing chooses [`Interactive::Unspecified`] — it is what a command line
    /// with none of the three options leaves behind, so it has to be the
    /// `Default`. Pinned because a reordering of the variants would silently
    /// change it, and the value it would change to is `AlwaysNo`.
    #[test]
    fn no_option_means_unspecified() {
        assert_eq!(Interactive::default(), Interactive::Unspecified);
    }

    fn tempdir() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let d = std::env::temp_dir().join(format!(
            "coreutils-overwrite-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).expect("mkdir");
        d
    }
}
