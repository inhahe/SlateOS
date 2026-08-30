#![deny(clippy::all)]

//! `chmod`'s mode string — compiled once, then applied to each file.
//!
//! A standalone crate for the same reason [`ere`] and `charwidth` are ones: the
//! shell and the utilities must agree, and the shell cannot depend on the
//! coreutils. `chmod u+X`, `install -m u+X` and `mkdir -m u+X` are the same
//! request written three times, and a script that sets a mode with one and
//! checks it with another is a script that will notice any disagreement.
//!
//! [`ere`]: https://example.invalid/see-userspace-ere
//!
//! # What was here instead
//!
//! Four independent parsers, none of them derived from the specification and
//! all of them written to the same wrong shape — a `who_mask` of `0o700` /
//! `0o070` / `0o007`, with setuid, setgid and sticky bolted on beside it as a
//! special case:
//!
//! | Copy | Where |
//! |---|---|
//! | `parse_symbolic` | `coreutils/src/bin/chmod.rs` |
//! | `parse_symbolic_mode` / `apply_symbolic_mode` | `userspace/chown/src/main.rs` |
//! | `parse_symbolic_mode` | `userspace/install/src/main.rs` |
//! | `parse_symbolic_umask` | `userspace/oils/src/interp.rs` |
//!
//! Between them they were wrong in thirteen ways, every one of which was
//! measured against GNU coreutils 9.4 rather than reasoned about. The list is
//! long enough to be the argument by itself:
//!
//! 1. **`X` was not implemented.** `chmod a+X` is the standard idiom for
//!    "executable if it is a directory or already executable", and the whole
//!    point of it is that it treats directories differently. `chmod.rs` broke
//!    out of its permission loop on the `X` and then silently discarded the
//!    rest of the clause; `install` mapped it to plain `x` with a comment
//!    reading "for simplicity", which sets the execute bit on every ordinary
//!    file it is asked about.
//! 2. **The umask was never consulted.** POSIX says a clause with no `who`
//!    applies the umask, which is why `chmod +w f` under the usual `umask 022`
//!    grants `u+w` alone. All four copies granted `a+w`. Measured: GNU answers
//!    `200`, they answered `222` — a file made group- and world-writable by a
//!    command that did not say so.
//! 3. **Copy sources were not implemented.** `chmod g=u` means "give the group
//!    what the owner has". Measured: `g=u` on a `640` file answers `660`.
//!    `chmod.rs` read the `u` as the start of a *who* clause it had already
//!    finished parsing, took no permission bits from it, and made the whole
//!    clause a no-op.
//! 4. **Operators could not be chained inside one clause.** `u+r-w` is one
//!    clause with two operations. Measured on a `600` file: GNU answers `400`,
//!    the copies answered `600`, having parsed the `+r` and dropped the `-w`.
//! 5. **A per-clause octal was not implemented** — `=644`, `+7`. The grammar
//!    admits an octal on the right of an operator, subject to rules of its own
//!    (below).
//! 6. **Trailing garbage was accepted.** `u+rZZZ` is an error in GNU and was a
//!    silent `u+r` here — an unnoticed typo quietly setting a different mode
//!    from the one written.
//! 7. **A bare `,` was accepted.** GNU rejects it; `chmod.rs` split on commas,
//!    skipped both empty halves, and reported success having changed nothing.
//! 8. **Setuid and setgid were not preserved on directories.** GNU keeps them
//!    unless the mode string *mentions* them, which is what stops
//!    `chmod -R 755 dir` from stripping the setgid bit off every directory it
//!    walks. None of the copies had the concept.
//! 9. **`t` ignored its `who`.** `chmod u+t` sets nothing in GNU, because the
//!    sticky bit belongs to `o`. `chmod.rs` set it for any `who` at all.
//! 10. **An octal above `07777` was accepted.** `chmod 77777 f` is an error
//!     upstream; `u32::from_str_radix` took it happily.
//! 11. **`invalid mode: '8'` came out as `invalid operator in mode: 8`** —
//!     a message about the wrong thing, pointing the reader at an operator
//!     they did not write.
//! 12. **`s` was scoped by hand** rather than by the `who` mask, so each copy
//!     needed its own reasoning about `o+s`, and they did not all agree.
//! 13. **The whole spec was re-parsed for every file** in the recursive case,
//!     which is the reason upstream compiles it once — and the reason a
//!     compiled form is what this crate returns.
//!
//! # The shape of the thing
//!
//! This is gnulib's `modechange.c` transcribed over bytes: [`compile`] turns a
//! mode string into a list of changes, and [`adjust`] applies that list to one
//! file's mode. Splitting it in two is not an optimisation — it is what makes
//! rule 8 expressible at all, because "did the string *mention* setgid" is a
//! property of the string that has to survive to the moment a directory is met.
//!
//! The grammar each clause matches is
//!
//! ```text
//! [ugoa]* ( [-+=] ( [rwxXst]* | [ugo] ) )+   |   [-+=][0-7]+
//! ```
//!
//! with clauses separated by `,`, or else the whole string is a single octal.
//! Three of its rules are worth stating because they are not visible in that
//! line:
//!
//! - **A per-clause octal may not have a `who`, and must end the clause.** So
//!   `=644` is valid, `u=644` is not, and neither is `=644x`.
//! - **An unrecognised permission letter does not fail where it is found.** The
//!   permission loop simply stops, the clause is recorded, and the failure
//!   happens at the end of the string, where the leftover text is found not to
//!   be a `,`. The distinction is invisible in the result — both are "invalid
//!   mode" — but transcribing it the other way makes `u+r-w` fail, since the
//!   `-` is also a letter the permission loop does not recognise.
//! - **An octal of fewer than five digits mentions setuid and setgid only if it
//!   sets them.** `chmod 755 dir` therefore leaves a directory's setgid bit
//!   alone, while `chmod 00755 dir` clears it. That is the entire difference
//!   between the two spellings, and it is invisible on a regular file.
//!
//! # Rendering the answer back
//!
//! [`permission_string`] is gnulib's `strmode`, and is here rather than in a
//! caller because the same twelve bits are involved and the same overload has
//! to be got right: setuid, setgid and sticky have no column of their own and
//! are shown in the execute slot. `chmod -v` needs it to say what it did, and
//! `ls -l` and `stat` need it for the same reason.
//!
//! [`file_type_letter`], [`file_type_name`] and the `S_IF*` constants are here
//! for the mirror-image reason: they are the *other* half of the same integer.
//! A mode word carries a file's type and its permissions in one number, and a
//! caller holding one half needs the mask that separates them. Five places had
//! written the seven type values out by hand — coreutils' `stat`,
//! `userspace/stat`, `cpio`, `mkinitramfs` and `ls` — and two of them had a
//! measurable bug for it: `userspace/stat` answered `unknown` where GNU
//! answers `weird file`, and `mkinitramfs` tested `mode & S_IFDIR` as though
//! the values were flags, which counts a block device and a socket as
//! directories because `S_IFCHR | S_IFDIR == S_IFBLK`.

/// Every bit a mode string can name: setuid, setgid, sticky, and `rwx` for all
/// three of user, group and other.
///
/// gnulib's `CHMOD_MODE_BITS`. A mode carries more than this — the file type
/// lives in the same word — so both the input and the output of [`adjust`] are
/// masked with it.
pub const CHMOD_MODE_BITS: u32 = 0o7777;

/// `S_ISUID`, `S_ISGID` and `S_ISVTX` — the three bits above the nine
/// permission bits, exported under their POSIX names because the callers that
/// *read* a mode need them as much as the parser that writes one. `ls --color`
/// picks `su`, `sg`, `st` and `tw` out of exactly these.
pub const S_ISUID: u32 = 0o4000;
pub const S_ISGID: u32 = 0o2000;
pub const S_ISVTX: u32 = 0o1000;

/// The world-writable bit, which `ls --color` colours `ow`/`tw` by.
pub const S_IWOTH: u32 = 0o0002;

/// The three execute bits together, gnulib's `S_IXUGO` — what `-F` stars a
/// file for, what `--color` picks `ex` by, and what a bare `+x` sets.
pub const S_IXUGO: u32 = 0o0111;

/// `rwx` for the owner, plus the setuid bit that `u` also selects.
const IRWXU: u32 = 0o0700;
const IRWXG: u32 = 0o0070;
const IRWXO: u32 = 0o0007;

/// The read bit of all three groups; likewise write. (Execute is
/// [`S_IXUGO`], which callers outside this crate need too.)
const R_ALL: u32 = 0o0444;
const W_ALL: u32 = 0o0222;

/// What a change does beyond adding or removing the bits it names.
///
/// gnulib's `MODE_ORDINARY_CHANGE` / `MODE_X_IF_ANY_X` / `MODE_COPY_EXISTING`.
/// Its `MODE_DONE` sentinel has no counterpart: a [`Vec`] knows its own length.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Special {
    /// Add, remove or set exactly the bits in `value`.
    Ordinary,
    /// `X`: additionally affect the execute bits, but only if the file is a
    /// directory or already has an execute bit set.
    XIfAnyX,
    /// `u`, `g` or `o` on the right of the operator: instead of using `value`
    /// as bits, read those bits off the file and copy them to the other two
    /// groups. Which group is read is which bits `value` holds.
    CopyExisting,
}

/// One `[-+=]…` operation, with the `who` that precedes it.
///
/// gnulib's `struct mode_change`. Private because the fields are a transcription
/// detail: upstream's are private to `modechange.c` for the same reason.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Change {
    /// `b'='`, `b'+'` or `b'-'`.
    op: u8,
    flag: Special,
    /// The bits the `who` selects, or `0` when no `who` was given — which is
    /// not the same as `a`, because it is the case the umask applies to.
    affected: u32,
    /// The bits to add, remove or set.
    value: u32,
    /// The bits the string named explicitly. Only setuid and setgid are ever
    /// read back out of this, and only on a directory; see [`adjust`].
    mentioned: u32,
}

/// A compiled mode string, ready to apply to any number of files.
///
/// The point of compiling once is not speed. `chmod -R` meets each directory
/// with a different question — whether *this* file's setgid bit was mentioned,
/// whether *this* file already has an execute bit — and those questions are
/// asked against the original string, which therefore has to outlive the parse.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Changes(Vec<Change>);

/// The result of applying a [`Changes`] to one file's mode.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Adjusted {
    /// The new mode, containing only [`CHMOD_MODE_BITS`].
    pub mode: u32,
    /// Which bits the change list had an opinion about.
    ///
    /// gnulib's `*pmode_bits`. `install` and `mkdir` use it to tell "the string
    /// asked for this bit to be off" apart from "the string said nothing about
    /// this bit", which they answer differently.
    pub mode_bits: u32,
}

/// This byte as an octal digit, or `None` if it is not one.
///
/// A named function rather than `c - b'0'` behind a range test, so that the
/// check admitting a byte and the conversion consuming it cannot drift apart —
/// and so that `8` and `9`, which *are* ASCII digits, are rejected by the same
/// expression that converts the rest.
fn octal_digit(c: u8) -> Option<u32> {
    char::from(c).to_digit(8)
}

/// Compile a mode string, or `None` if it is not one.
///
/// gnulib's `mode_compile`. There is exactly one failure, and upstream reports
/// it the same way — the caller's message is `invalid mode: ‘…’` whichever rule
/// was broken — so this returns an [`Option`] rather than inventing a taxonomy
/// of reasons that no user would ever see.
///
/// The argument is bytes because a mode string arrives in argv, which is bytes.
/// Every byte the grammar recognises is ASCII, so a non-ASCII one simply fails
/// to match, exactly as it would upstream.
#[must_use]
pub fn compile(spec: &[u8]) -> Option<Changes> {
    // The whole-string octal, which is a different grammar and not a clause:
    // no operator, no `who`, and it must be the entire string.
    if spec.first().copied().and_then(octal_digit).is_some() {
        let mut octal: u32 = 0;
        let mut i = 0;
        while let Some(digit) = spec.get(i).copied().and_then(octal_digit) {
            // The bound is checked every digit, so `octal` can never exceed
            // 0o77777 and the arithmetic cannot overflow. Saturating rather
            // than wrapping so that stays true if the bound is ever changed.
            octal = octal.saturating_mul(8).saturating_add(digit);
            if CHMOD_MODE_BITS < octal {
                return None;
            }
            i = i.saturating_add(1);
        }
        // Anything after the digits — `755x`, `755,u+w` — is not this grammar.
        if i != spec.len() {
            return None;
        }

        // Fewer than five digits mentions setuid and setgid only where it sets
        // them, which is what leaves a directory's setgid bit alone under
        // `chmod 755` and clears it under `chmod 00755`.
        let mentioned = if i < 5 {
            (octal & (S_ISUID | S_ISGID)) | S_ISVTX | 0o0777
        } else {
            CHMOD_MODE_BITS
        };
        return Some(Changes(vec![Change {
            op: b'=',
            flag: Special::Ordinary,
            affected: CHMOD_MODE_BITS,
            value: octal,
            mentioned,
        }]));
    }

    compile_symbolic(spec)
}

/// The `[ugoa]*([-+=]…)+ (,…)*` grammar.
///
/// Split out only for length; it is the second half of upstream's
/// `mode_compile` and shares its one failure.
fn compile_symbolic(spec: &[u8]) -> Option<Changes> {
    let mut changes: Vec<Change> = Vec::new();
    let mut p = 0usize;

    loop {
        // ---- the `who`, if any. `0` is not `a`: it is the umask case. ----
        let mut affected: u32 = 0;
        loop {
            match spec.get(p) {
                Some(b'u') => affected |= S_ISUID | IRWXU,
                Some(b'g') => affected |= S_ISGID | IRWXG,
                Some(b'o') => affected |= S_ISVTX | IRWXO,
                Some(b'a') => affected |= CHMOD_MODE_BITS,
                // The operator ends the `who` and starts the work below.
                Some(b'=' | b'+' | b'-') => break,
                // Including the end of the string, so `""` and `"u"` both fail
                // here rather than compiling to nothing.
                _ => return None,
            }
            p = p.saturating_add(1);
        }

        // ---- one or more `[-+=]…` operations sharing that `who` ----
        loop {
            let op = *spec.get(p)?;
            p = p.saturating_add(1);

            let mut value: u32;
            let mut mentioned: u32 = 0;
            // Overwritten by every branch but the copy-source ones, which is
            // upstream's way of saying "a bare u, g or o here is a copy".
            let mut flag = Special::CopyExisting;

            match spec.get(p) {
                Some(&c) if octal_digit(c).is_some() => {
                    let mut octal: u32 = 0;
                    while let Some(digit) = spec.get(p).copied().and_then(octal_digit) {
                        octal = octal.saturating_mul(8).saturating_add(digit);
                        if CHMOD_MODE_BITS < octal {
                            return None;
                        }
                        p = p.saturating_add(1);
                    }
                    // An octal takes the whole clause and cannot share it with
                    // a `who`: `u=644` and `=644x` are both refused here.
                    match spec.get(p) {
                        _ if affected != 0 => return None,
                        None | Some(b',') => {}
                        Some(_) => return None,
                    }
                    affected = CHMOD_MODE_BITS;
                    mentioned = CHMOD_MODE_BITS;
                    value = octal;
                    flag = Special::Ordinary;
                }
                // A copy source: the bits to use are read off the file later.
                Some(b'u') => {
                    value = IRWXU;
                    p = p.saturating_add(1);
                }
                Some(b'g') => {
                    value = IRWXG;
                    p = p.saturating_add(1);
                }
                Some(b'o') => {
                    value = IRWXO;
                    p = p.saturating_add(1);
                }
                _ => {
                    value = 0;
                    flag = Special::Ordinary;
                    loop {
                        match spec.get(p) {
                            Some(b'r') => value |= R_ALL,
                            Some(b'w') => value |= W_ALL,
                            Some(b'x') => value |= S_IXUGO,
                            Some(b'X') => flag = Special::XIfAnyX,
                            // Both, and let `affected` decide which survives.
                            Some(b's') => value |= S_ISUID | S_ISGID,
                            Some(b't') => value |= S_ISVTX,
                            // Not an error *here* — see the module docs. The
                            // clause ends, and whatever this byte is has to be
                            // a `,` or the end of the string to be accepted.
                            _ => break,
                        }
                        p = p.saturating_add(1);
                    }
                }
            }

            changes.push(Change {
                op,
                flag,
                affected,
                value,
                mentioned: if mentioned != 0 {
                    mentioned
                } else if affected != 0 {
                    affected & value
                } else {
                    value
                },
            });

            // `u+r-w`: another operator continues this clause with the same
            // `who`, rather than starting a new one.
            if !matches!(spec.get(p), Some(b'=' | b'+' | b'-')) {
                break;
            }
        }

        if spec.get(p) != Some(&b',') {
            break;
        }
        p = p.saturating_add(1);
    }

    // The one place trailing garbage is caught: `u+rZZZ` arrives here with `p`
    // on the first `Z`, which is neither a `,` nor the end.
    if p == spec.len() {
        Some(Changes(changes))
    } else {
        None
    }
}

/// The change list for `--reference=RFILE`: set the mode to exactly `ref_mode`.
///
/// gnulib's `mode_create_from_ref`, minus the `stat` — the caller has already
/// done that, and this crate touches no filesystem. `mentioned` is every bit,
/// so a reference file's setuid and setgid are copied onto a directory rather
/// than being preserved from it.
#[must_use]
pub fn from_reference(ref_mode: u32) -> Changes {
    Changes(vec![Change {
        op: b'=',
        flag: Special::Ordinary,
        affected: CHMOD_MODE_BITS,
        value: ref_mode & CHMOD_MODE_BITS,
        mentioned: CHMOD_MODE_BITS,
    }])
}

/// Apply a compiled mode string to one file's mode.
///
/// gnulib's `mode_adjust`. `dir` is doing two jobs at once, both of them
/// upstream's: it makes `X` fire even on a directory with no execute bit, and
/// it protects setuid and setgid from any change that did not name them.
///
/// `umask_value` is consulted only for clauses that gave no `who`. Passing `0`
/// therefore means "take every clause at its word", which is what `--reference`
/// and an octal string want and what `install -m` wants.
#[must_use]
pub fn adjust(oldmode: u32, dir: bool, umask_value: u32, changes: &Changes) -> Adjusted {
    let mut newmode = oldmode & CHMOD_MODE_BITS;
    let mut mode_bits: u32 = 0;

    for change in &changes.0 {
        let affected = change.affected;
        // On a directory, setuid and setgid survive anything that did not name
        // them. This is why `chmod -R 755 d` does not strip a setgid directory.
        let omit_change = if dir { S_ISUID | S_ISGID } else { 0 } & !change.mentioned;
        let mut value = change.value;

        match change.flag {
            Special::Ordinary => {}
            Special::CopyExisting => {
                // `value` names *which group to read*; replace it with what
                // that group actually has, spread across all three groups so
                // the `affected` mask below can pick out the destination.
                value &= newmode;
                value |= (if value & R_ALL != 0 { R_ALL } else { 0 })
                    | (if value & W_ALL != 0 { W_ALL } else { 0 })
                    | (if value & S_IXUGO != 0 { S_IXUGO } else { 0 });
            }
            Special::XIfAnyX => {
                if newmode & S_IXUGO != 0 || dir {
                    value |= S_IXUGO;
                }
            }
        }

        // A `who` limits the change to what it named; no `who` means the umask
        // decides instead. Either way the directory rule above wins.
        value &= (if affected != 0 {
            affected
        } else {
            !umask_value
        }) & !omit_change;

        match change.op {
            b'=' => {
                // With a `who`, `=` clears only what that `who` covers; without
                // one it clears everything. The bits held back for a directory
                // are preserved too, or the rule above would be undone here.
                let preserved = (if affected != 0 { !affected } else { 0 }) | omit_change;
                mode_bits |= CHMOD_MODE_BITS & !preserved;
                newmode = (newmode & preserved) | value;
            }
            b'+' => {
                mode_bits |= value;
                newmode |= value;
            }
            // Only `=`, `+` and `-` are ever recorded, so this is `-`.
            _ => {
                mode_bits |= value;
                newmode &= !value;
            }
        }
    }

    Adjusted {
        mode: newmode & CHMOD_MODE_BITS,
        mode_bits,
    }
}

/// The nine `rwxrwxrwx` characters of a mode, as `ls -l` and `chmod -v` show
/// them.
///
/// gnulib's `strmode` (`filemode.c`) less its first and last characters: the
/// leading file-type letter, which needs the part of the mode word this crate
/// deliberately masks off, and the trailing alternate-access marker, which every
/// caller in coreutils strips (`chmod.c` does it with `perms[10] = '\0'` before
/// printing `&perms[1]`).
///
/// The overload in the third column of each triple is the whole reason this is
/// not three lines of `if`: setuid, setgid and sticky have no column of their
/// own, so they are shown in the execute slot — lowercase when the execute bit
/// is *also* set, uppercase when it is not, so that one character carries two
/// bits and `chmod u+s` on a non-executable file is still legible as `S`.
#[must_use]
pub fn permission_string(mode: u32) -> String {
    let mut s = String::with_capacity(9);
    for (read, write, execute, special, letter) in [
        (0o400, 0o200, 0o100, S_ISUID, 's'),
        (0o040, 0o020, 0o010, S_ISGID, 's'),
        (0o004, 0o002, 0o001, S_ISVTX, 't'),
    ] {
        s.push(if mode & read != 0 { 'r' } else { '-' });
        s.push(if mode & write != 0 { 'w' } else { '-' });
        s.push(match (mode & execute != 0, mode & special != 0) {
            (true, false) => 'x',
            (false, false) => '-',
            (true, true) => letter,
            (false, true) => letter.to_ascii_uppercase(),
        });
    }
    s
}

// ----------------------------------------------------------- file types ---

/// The bits of a mode word that hold the file's *type* — POSIX's `S_IFMT`.
///
/// This is the part [`CHMOD_MODE_BITS`] masks off, and the two together are the
/// whole reason both live here: a mode word carries two unrelated things in one
/// integer, and a caller that forgets which half it is holding gets a
/// permission string with a directory bit in it.
pub const S_IFMT: u32 = 0o170000;

/// A named pipe (`p`).
pub const S_IFIFO: u32 = 0o010000;
/// A character device (`c`).
pub const S_IFCHR: u32 = 0o020000;
/// A directory (`d`).
pub const S_IFDIR: u32 = 0o040000;
/// A block device (`b`).
pub const S_IFBLK: u32 = 0o060000;
/// A regular file (`-`).
pub const S_IFREG: u32 = 0o100000;
/// A symbolic link (`l`).
pub const S_IFLNK: u32 = 0o120000;
/// A socket (`s`).
pub const S_IFSOCK: u32 = 0o140000;

/// The first character of `ls -l`'s mode string — gnulib's `ftypelet`.
///
/// `?` is the answer for a type this system has no letter for, which is not a
/// hypothetical: it is what a Solaris door or a filesystem the kernel knows and
/// we do not comes out as, and printing a wrong letter would be worse than
/// printing an honest question mark.
///
/// The `S_IF*` values are not bit flags despite looking like them — `S_IFBLK`
/// is `0o060000` and `S_IFCHR | S_IFDIR` is the same number — so this must
/// compare the masked field for equality and never test a bit.
#[must_use]
pub const fn file_type_letter(mode: u32) -> u8 {
    match mode & S_IFMT {
        S_IFIFO => b'p',
        S_IFCHR => b'c',
        S_IFDIR => b'd',
        S_IFBLK => b'b',
        S_IFREG => b'-',
        S_IFLNK => b'l',
        S_IFSOCK => b's',
        _ => b'?',
    }
}

/// The type as `stat`'s `%F` names it.
///
/// The wording is GNU's and is not free: "character special file" rather than
/// "character device", "fifo" rather than "named pipe". A script that greps
/// `stat -c %F` for one of these strings is matching the exact words.
#[must_use]
pub const fn file_type_name(mode: u32) -> &'static str {
    match mode & S_IFMT {
        S_IFIFO => "fifo",
        S_IFCHR => "character special file",
        S_IFDIR => "directory",
        S_IFBLK => "block special file",
        S_IFREG => "regular file",
        S_IFLNK => "symbolic link",
        S_IFSOCK => "socket",
        _ => "weird file",
    }
}

/// The whole ten-character mode string: the type letter and the nine
/// permission characters.
///
/// This is gnulib's `strmode` minus its eleventh character, the alternate-access
/// marker, which every caller in coreutils strips before printing.
#[must_use]
pub fn mode_string(mode: u32) -> String {
    let mut s = String::with_capacity(10);
    s.push(char::from(file_type_letter(mode)));
    s.push_str(&permission_string(mode));
    s
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// Compile and apply in one step, for the cases where the compiled form is
    /// not itself interesting. Panics if the spec does not compile.
    fn go(old: u32, spec: &str, umask: u32, dir: bool) -> u32 {
        let changes = compile(spec.as_bytes())
            .unwrap_or_else(|| panic!("{spec:?} should compile but did not"));
        adjust(old, dir, umask, &changes).mode
    }

    /// The common case: a regular file and no umask in play.
    fn f(old: u32, spec: &str) -> u32 {
        go(old, spec, 0, false)
    }

    // -------------------------------------------------------- file types ---

    /// Every row measured, GNU coreutils 9.4:
    ///
    /// ```text
    /// $ stat -c '%A %F' reg d sym pipe sock /dev/null /dev/loop0
    /// -rw-r--r-- regular file
    /// drwxr-xr-x directory
    /// lrwxrwxrwx symbolic link
    /// prw-r--r-- fifo
    /// srwxr-xr-x socket
    /// crw-rw-rw- character special file
    /// brw-rw---- block special file
    /// ```
    #[test]
    fn every_file_type_has_a_letter_and_a_name() {
        for (bits, letter, name) in [
            (S_IFREG, b'-', "regular file"),
            (S_IFDIR, b'd', "directory"),
            (S_IFLNK, b'l', "symbolic link"),
            (S_IFIFO, b'p', "fifo"),
            (S_IFSOCK, b's', "socket"),
            (S_IFCHR, b'c', "character special file"),
            (S_IFBLK, b'b', "block special file"),
        ] {
            assert_eq!(file_type_letter(bits | 0o644), letter);
            assert_eq!(file_type_name(bits | 0o644), name);
        }
        // A type this system has no letter for. gnulib prints `?` and
        // `weird file` rather than guessing, and so does this.
        assert_eq!(file_type_letter(0o160000), b'?');
        assert_eq!(file_type_name(0o160000), "weird file");
        // …including a mode with no type field at all, which is what a
        // caller that already masked with `CHMOD_MODE_BITS` would pass.
        assert_eq!(file_type_letter(0o644), b'?');
    }

    /// The `S_IF*` values look like flags and are not: `S_IFCHR | S_IFDIR`
    /// *is* `S_IFBLK`. Anything that tests a bit rather than comparing the
    /// masked field answers `b` for a directory, which is the mistake this
    /// pins against.
    #[test]
    fn the_type_field_is_a_number_and_not_a_set_of_flags() {
        assert_eq!(S_IFCHR | S_IFDIR, S_IFBLK);
        assert_eq!(file_type_letter(S_IFDIR), b'd');
        assert_eq!(file_type_letter(S_IFBLK), b'b');
    }

    /// The ten-character string is the letter and the nine permission
    /// characters, which is `stat -c %A` and `ls -l`'s first column.
    #[test]
    fn the_mode_string_is_the_type_letter_and_the_permissions() {
        assert_eq!(mode_string(S_IFREG | 0o644), "-rw-r--r--");
        assert_eq!(mode_string(S_IFDIR | 0o755), "drwxr-xr-x");
        assert_eq!(mode_string(S_IFLNK | 0o777), "lrwxrwxrwx");
        assert_eq!(mode_string(S_IFDIR | 0o1777), "drwxrwxrwt");
        assert_eq!(mode_string(S_IFREG | 0o4755), "-rwsr-xr-x");
        assert_eq!(mode_string(S_IFREG | 0o2644), "-rw-r-Sr--");
    }

    fn bad(spec: &str) -> bool {
        compile(spec.as_bytes()).is_none()
    }

    // ------------------------------------------------------------- octal ---

    #[test]
    fn a_whole_string_octal_sets_the_mode_exactly() {
        assert_eq!(f(0o777, "0"), 0o000);
        assert_eq!(f(0o000, "644"), 0o644);
        assert_eq!(f(0o000, "00700"), 0o700);
        assert_eq!(f(0o777, "4755"), 0o4755);
        assert_eq!(f(0o000, "7777"), 0o7777);
    }

    /// Measured: `chmod 8 f` is `chmod: invalid mode: ‘8’`. `8` is not an octal
    /// digit, so it is not the octal grammar; and it is not a `who` either, so
    /// the symbolic grammar refuses it too.
    #[test]
    fn a_non_octal_digit_is_not_a_mode_at_all() {
        assert!(bad("8"));
        assert!(bad("9"));
        assert!(bad("778"));
    }

    #[test]
    fn an_octal_above_all_the_mode_bits_is_refused() {
        assert!(bad("10000"));
        assert!(bad("77777"));
    }

    #[test]
    fn an_octal_must_be_the_whole_string() {
        assert!(bad("755x"));
        assert!(bad("755,u+w"));
        assert!(bad("755 "));
    }

    /// The one observable difference between `755` and `00755`, and it shows up
    /// only on a directory: a short octal does not mention setuid or setgid
    /// unless it sets them, and unmentioned means preserved.
    #[test]
    fn a_short_octal_leaves_a_directorys_setgid_alone() {
        assert_eq!(go(0o2755, "755", 0, true), 0o2755);
        assert_eq!(go(0o2755, "00755", 0, true), 0o0755);
        // A regular file has no such protection.
        assert_eq!(go(0o2755, "755", 0, false), 0o0755);
        // And an octal that names the bit sets it either way.
        assert_eq!(go(0o0755, "2755", 0, true), 0o2755);
    }

    // ---------------------------------------------------------- the umask ---

    /// Measured against GNU 9.4: `+w` on a `000` file answers `222` under
    /// `umask 000` and `200` under `umask 022`. Four hand-written parsers in
    /// this tree all answered `222` regardless.
    #[test]
    fn a_clause_with_no_who_applies_the_umask() {
        assert_eq!(go(0o000, "+w", 0o000, false), 0o222);
        assert_eq!(go(0o000, "+w", 0o022, false), 0o200);
        assert_eq!(go(0o000, "+w", 0o077, false), 0o200);
        assert_eq!(go(0o000, "+rwx", 0o022, false), 0o755);
        assert_eq!(go(0o000, "+rwx", 0o000, false), 0o777);
        // `=` too, and it is `=r` under `umask 044` that shows it.
        assert_eq!(go(0o000, "=r", 0o044, false), 0o400);
        assert_eq!(go(0o000, "=r", 0o000, false), 0o444);
    }

    /// A `who` is the caller saying what they meant, so the umask stands aside
    /// — which is why `chmod a+w` and `chmod +w` are different commands.
    #[test]
    fn a_who_overrides_the_umask() {
        assert_eq!(go(0o000, "a+w", 0o022, false), 0o222);
        assert_eq!(go(0o000, "g+w", 0o022, false), 0o020);
        assert_eq!(go(0o000, "u+w", 0o077, false), 0o200);
    }

    // ------------------------------------------------------------ the ops ---

    #[test]
    fn plus_adds_minus_removes_and_equals_replaces() {
        assert_eq!(f(0o644, "u+x"), 0o744);
        assert_eq!(f(0o666, "go-w"), 0o644);
        assert_eq!(f(0o777, "a=rx"), 0o555);
        assert_eq!(f(0o000, "u=rwx,g=rx,o=r"), 0o754);
    }

    /// `=` with a `who` preserves what that `who` does not cover.
    #[test]
    fn equals_with_a_who_is_not_a_whole_mode() {
        assert_eq!(f(0o777, "u=r"), 0o477);
        assert_eq!(f(0o000, "o=rwx"), 0o007);
    }

    /// Measured: `a=r` on a `7777` file answers `444`, so `a` clears the high
    /// bits as well. It does, because `a` selects them in `affected`.
    #[test]
    fn equals_all_clears_the_high_bits_too() {
        assert_eq!(f(0o7777, "a=r"), 0o444);
        assert_eq!(f(0o7777, "="), 0o000);
        // `u=` clears setuid but leaves setgid and sticky, since `u` selects
        // only the first of them.
        assert_eq!(f(0o7777, "u="), 0o3077);
    }

    /// Measured: `u+r-w` on a `600` file answers `400`. One clause, two
    /// operations, one `who` — the form every copy in this tree dropped.
    #[test]
    fn operators_chain_within_one_clause() {
        assert_eq!(f(0o600, "u+r-w"), 0o400);
        assert_eq!(f(0o000, "u=r+w-r"), 0o200);
        assert_eq!(f(0o000, "go+r+w"), 0o066);
    }

    // ------------------------------------------------------ copy sources ---

    /// Measured: `g=u` on `640` is `660`, `o+u` on `640` is `646`, and `u-o` on
    /// `641` is `641` — the last because `o`'s only bit is execute and `u` had
    /// none to remove.
    #[test]
    fn a_copy_source_reads_the_other_groups_bits() {
        assert_eq!(f(0o640, "g=u"), 0o660);
        assert_eq!(f(0o640, "o+u"), 0o646);
        assert_eq!(f(0o641, "u-o"), 0o641);
        assert_eq!(f(0o000, "u=rwx,g+u"), 0o770);
    }

    /// A copy source with no `who` is still subject to the umask. Measured:
    /// `=u` on a `700` file under `umask 022` answers `755`, not `777`.
    #[test]
    fn a_copy_source_with_no_who_takes_the_umask() {
        assert_eq!(go(0o700, "=u", 0o022, false), 0o755);
        assert_eq!(go(0o700, "=u", 0o000, false), 0o777);
    }

    /// The copy reads the mode as it stands *at that point in the list*, not as
    /// it was when the string was compiled.
    #[test]
    fn a_copy_source_sees_earlier_clauses() {
        assert_eq!(f(0o000, "u+x,g=u"), 0o110);
    }

    // ---------------------------------------------------------------- X ---

    /// `X` is the reason this is a compiled form rather than a number: the
    /// answer depends on the file. Measured: `a+X` is a no-op on a `000` file,
    /// gives `711` on a `700` file, `111` on a `010` file, and `711` on a `600`
    /// *directory*.
    #[test]
    fn x_fires_on_a_directory_or_on_an_existing_execute_bit() {
        assert_eq!(go(0o000, "a+X", 0, false), 0o000);
        assert_eq!(go(0o700, "a+X", 0, false), 0o711);
        assert_eq!(go(0o010, "a+X", 0, false), 0o111);
        assert_eq!(go(0o600, "a+X", 0, true), 0o711);
        assert_eq!(go(0o000, "a+X", 0, true), 0o111);
    }

    /// `X` may share a clause with ordinary letters, and only the execute bits
    /// are conditional.
    #[test]
    fn x_combines_with_the_other_letters() {
        assert_eq!(go(0o000, "a+rX", 0, false), 0o444);
        assert_eq!(go(0o100, "a+rX", 0, false), 0o555);
        assert_eq!(go(0o000, "a+rX", 0, true), 0o555);
    }

    // ------------------------------------------------------- high bits ---

    /// Measured: `u+s` is `4000`, `+s` is `6000`, `o+t` and `+t` are both
    /// `1000`. `s` names both setuid and setgid and lets the `who` choose;
    /// `t` names the sticky bit, which belongs to `o`.
    #[test]
    fn s_and_t_are_scoped_by_their_who() {
        assert_eq!(f(0o000, "u+s"), 0o4000);
        assert_eq!(f(0o000, "g+s"), 0o2000);
        assert_eq!(f(0o000, "+s"), 0o6000);
        assert_eq!(f(0o000, "o+t"), 0o1000);
        assert_eq!(f(0o000, "+t"), 0o1000);
        // The two that catch a hand-written scoping rule: `o+s` and `u+t` are
        // both no-ops, because `o` does not select setuid or setgid and `u`
        // does not select the sticky bit.
        assert_eq!(f(0o000, "o+s"), 0o000);
        assert_eq!(f(0o000, "u+t"), 0o000);
    }

    /// The umask never covers the high bits, so a `who`-less `+s` sets both
    /// however the umask is set.
    #[test]
    fn the_umask_does_not_reach_the_high_bits() {
        assert_eq!(go(0o000, "+s", 0o077, false), 0o6000);
    }

    // ------------------------------------------------- per-clause octal ---

    #[test]
    fn an_operator_may_take_an_octal() {
        assert_eq!(f(0o000, "=644"), 0o644);
        assert_eq!(f(0o000, "+7"), 0o007);
        assert_eq!(f(0o777, "-111"), 0o666);
        assert_eq!(f(0o000, "=644,+111"), 0o755);
    }

    /// A per-clause octal takes the whole clause: it may not have a `who`, and
    /// nothing may follow it but a comma.
    #[test]
    fn a_per_clause_octal_owns_its_clause() {
        assert!(bad("u=644"));
        assert!(bad("a+7"));
        assert!(bad("=644x"));
        assert!(bad("=644+1"));
        assert!(bad("=10000"));
    }

    /// A per-clause octal mentions everything, so it is *not* held back on a
    /// directory the way a short whole-string octal is.
    #[test]
    fn a_per_clause_octal_mentions_every_bit() {
        assert_eq!(go(0o2755, "=755", 0, true), 0o0755);
    }

    // -------------------------------------------------------- rejection ---

    /// Measured: `chmod u+rZZZ f` is `chmod: invalid mode: ‘u+rZZZ’`. It is
    /// refused at the end of the string rather than at the `Z`; see the module
    /// docs for why that distinction has to be transcribed.
    #[test]
    fn trailing_garbage_is_refused_rather_than_ignored() {
        assert!(bad("u+rZZZ"));
        assert!(bad("u+r "));
        assert!(bad("a+x!"));
        assert!(bad("u+r,g+wQ"));
    }

    #[test]
    fn a_string_that_is_not_a_clause_is_refused() {
        assert!(bad(""));
        assert!(bad(","));
        assert!(bad("u"));
        assert!(bad("ugo"));
        assert!(bad("u+r,"));
        assert!(bad("*"));
        assert!(bad("u*x"));
    }

    /// A clause may legally name no permissions at all, and then does nothing.
    /// Measured: `chmod u+ f`, and even a bare `chmod + f`, succeed and change
    /// nothing — the `who` is optional and so is the permission list, so an
    /// operator alone is a whole clause. Only `=` does anything in that state,
    /// because clearing is what `=` means.
    #[test]
    fn an_operator_with_no_permissions_is_a_valid_no_op() {
        assert_eq!(f(0o644, "u+"), 0o644);
        assert_eq!(f(0o644, "u-"), 0o644);
        assert_eq!(f(0o644, "+"), 0o644);
        assert_eq!(f(0o644, "-"), 0o644);
        assert_eq!(f(0o644, "u="), 0o044);
        assert_eq!(f(0o644, "="), 0o000);
    }

    // ---------------------------------------------------------- reference ---

    #[test]
    fn a_reference_sets_the_mode_exactly() {
        let changes = from_reference(0o4755);
        assert_eq!(adjust(0o000, false, 0o077, &changes).mode, 0o4755);
        // And it overwrites a directory's high bits rather than preserving
        // them, because it mentions every bit.
        assert_eq!(adjust(0o2755, true, 0, &changes).mode, 0o4755);
    }

    #[test]
    fn a_reference_keeps_only_the_mode_bits() {
        // A real `st_mode` carries the file type in the same word.
        let changes = from_reference(0o100_644);
        assert_eq!(adjust(0o000, false, 0, &changes).mode, 0o644);
    }

    // --------------------------------------------------------- mode_bits ---

    /// The bits the string had an opinion about, which is not the same as the
    /// bits it turned on.
    #[test]
    fn mode_bits_reports_what_was_asked_about() {
        let changes = compile(b"u+x").unwrap();
        assert_eq!(adjust(0o644, false, 0, &changes).mode_bits, 0o100);
        // `-` counts as an opinion.
        let changes = compile(b"go-w").unwrap();
        assert_eq!(adjust(0o666, false, 0, &changes).mode_bits, 0o022);
        // `=` with a `who` covers everything that `who` selects, set or not.
        let changes = compile(b"u=r").unwrap();
        assert_eq!(adjust(0o000, false, 0, &changes).mode_bits, S_ISUID | IRWXU);
        // `=` with no `who` covers every bit there is.
        let changes = compile(b"=r").unwrap();
        assert_eq!(adjust(0o000, false, 0, &changes).mode_bits, CHMOD_MODE_BITS);
    }

    // ------------------------------------------------------------- shape ---

    /// The input is bytes, so a mode string that is not UTF-8 is refused rather
    /// than being a decoding failure somewhere up the stack.
    #[test]
    fn a_non_ascii_byte_is_simply_not_in_the_grammar() {
        assert!(compile(b"u+\xff").is_none());
        assert!(compile(b"\xff").is_none());
        assert!(compile(b"u+r\x80").is_none());
    }

    /// Compiling is independent of any file, which is the property that lets
    /// `chmod -R` parse its argument once.
    #[test]
    fn one_compile_serves_many_files() {
        let changes = compile(b"a+X").unwrap();
        assert_eq!(adjust(0o644, false, 0, &changes).mode, 0o644);
        assert_eq!(adjust(0o755, false, 0, &changes).mode, 0o755);
        assert_eq!(adjust(0o644, true, 0, &changes).mode, 0o755);
    }

    /// Applying a change list twice must give the same answer as applying it
    /// once — every operator here is idempotent, and a `newmode` that drifted
    /// under repetition would mean state had leaked into the compiled form.
    #[test]
    fn applying_twice_changes_nothing_more() {
        for spec in [
            "u+x", "go-w", "a=rx", "u=r", "+w", "a+X", "g=u", "=644", "u+r-w", "755",
        ] {
            let changes = compile(spec.as_bytes()).unwrap();
            let once = adjust(0o642, false, 0o022, &changes).mode;
            let twice = adjust(once, false, 0o022, &changes).mode;
            assert_eq!(once, twice, "{spec} is not idempotent");
        }
    }

    /// Nothing outside the twelve mode bits ever comes back, whatever went in.
    #[test]
    fn the_answer_is_always_mode_bits_only() {
        for spec in ["u+x", "a=rwx", "+s", "=7777", "a+X", "g=u"] {
            let changes = compile(spec.as_bytes()).unwrap();
            let got = adjust(0o170_777, false, 0, &changes).mode;
            assert_eq!(got & !CHMOD_MODE_BITS, 0, "{spec} leaked a non-mode bit");
        }
    }

    // ------------------------------------------------------------ rendering ---

    #[test]
    fn the_ordinary_bits_render_in_three_triples() {
        assert_eq!(permission_string(0o777), "rwxrwxrwx");
        assert_eq!(permission_string(0o000), "---------");
        assert_eq!(permission_string(0o644), "rw-r--r--");
        assert_eq!(permission_string(0o755), "rwxr-xr-x");
        assert_eq!(permission_string(0o421), "r---w---x");
    }

    /// The overload: the special bit takes the execute column, and its case says
    /// whether the execute bit is there underneath it. Both halves matter —
    /// `4755` and `4644` differ only in that case, and reading `S` as "no setuid"
    /// is the mistake the case is there to prevent.
    #[test]
    fn a_special_bit_takes_the_execute_column_and_its_case_carries_the_x() {
        assert_eq!(permission_string(0o4755), "rwsr-xr-x");
        assert_eq!(permission_string(0o4644), "rwSr--r--");
        assert_eq!(permission_string(0o2755), "rwxr-sr-x");
        assert_eq!(permission_string(0o2644), "rw-r-Sr--");
        assert_eq!(permission_string(0o1777), "rwxrwxrwt");
        assert_eq!(permission_string(0o1666), "rw-rw-rwT");
        assert_eq!(permission_string(0o7777), "rwsrwsrwt");
        assert_eq!(permission_string(0o7000), "--S--S--T");
    }

    /// The file type lives in the same word and is not this function's business;
    /// a caller that passes a whole `st_mode` must get the same nine characters.
    #[test]
    fn bits_above_the_mode_are_ignored() {
        assert_eq!(permission_string(0o100_644), "rw-r--r--");
        assert_eq!(permission_string(0o040_755), "rwxr-xr-x");
    }
}
