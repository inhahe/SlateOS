//! df — report file system disk space usage.
//!
//! A port of GNU `df` (coreutils 9.4), together with the Linux halves of the
//! three gnulib modules it cannot work without: `mountlist` (who is mounted
//! where), `fsusage` (how full they are) and `mbsalign` (how to pad a column
//! whose contents are not ASCII).
//!
//! # What it replaces
//!
//! The previous `df` accepted one option — `-h` — and printed a fixed
//! five-column table for the paths named on the command line, defaulting to
//! `/`. With no arguments it did **not** list the mounted file systems, which
//! is `df`'s entire purpose; it listed the root one. It parsed argv into
//! `Vec<String>`, so a non-UTF-8 path aborted the program before it printed
//! anything, and it had no notion of a mount list at all, so `-a`, `-l`, `-t`,
//! `-x`, `-T`, `--total`, `--output` and the block-size grammar were all
//! absent, and the `Filesystem` column was the *path the user typed* rather
//! than the device backing it.
//!
//! # The four parts that are not obvious
//!
//! **The mount list is filtered before it is printed, and the filter is
//! stateful.** Without `-a`, a mount point that two file systems claim is
//! resolved in favour of the one whose device the kernel agrees with, and a
//! "dummy" file system (`proc`, `sysfs`, an `autofs` trigger…) is dropped
//! outright — except that a `tmpfs`, alone among dummies, is kept if something
//! is actually stored on it. Getting this wrong does not produce a wrong
//! number; it produces the wrong *rows*. See [`Df::filter_mount_list`].
//!
//! **A percentage is not `used * 100 / total`.** Upstream computes it in
//! integers when it can and in `double` when it cannot, and rounds *up* in
//! both. `df` reporting 100% for a file system with a free block left is
//! deliberate — the ceiling is what makes "100%" mean "do not rely on this" —
//! and a naive rounding disagrees with GNU on almost every real file system.
//! See [`percent`].
//!
//! **A column is padded in display columns, not in bytes.** A mount point
//! containing a CJK character is two columns wide per character and a
//! combining mark is zero, so `format!("{:<width$}")` — which counts `char`s —
//! tears the table apart. Worse, a control character in a device name would
//! otherwise be written to the terminal raw; upstream replaces both those and
//! any undecodable byte with `?` first. See [`align_cell`].
//!
//! **The block size is a grammar, not a number** — the same one `du` reads,
//! through the same [`coreutils::human`] entry point, so `BLOCK_SIZE=K` in a
//! profile means one thing to both. `df` adds a wrinkle `du` has not: the
//! *header* is derived from the block size too, which is why `df -B K` says
//! `1K-blocks` and `df -B 1K` says `1024-blocks`. See [`get_header`].
//!
//! # Not implemented
//!
//! `-v` is accepted and ignored, which is what GNU does with it. There is no
//! `--direct`, because 9.4 has none either.
//!
//! Built only on unix-family targets — our `x86_64-slateos` presents as
//! `linux-musl`, so `cfg(unix)` matches. On a non-unix host everything except
//! [`RealSystem`] and `main` is still compiled and unit-tested against
//! `FakeSystem`; only the parts that need `statvfs` and `st_dev` are gated
//! out.

#![cfg_attr(not(unix), allow(dead_code))]

#[cfg(not(unix))]
use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::human::{Opts, human_readable};
#[cfg(unix)]
use coreutils::quote::os_from_bytes;
use coreutils::quote::{os_bytes, quote, quoteaf, quotef};
#[cfg(unix)]
use coreutils::stdfd::{self, Stream};
use coreutils::xnum::strtol_fatal;
use quoting::{Mb, next_mb};
use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{self, Write};
use std::process::ExitCode;

const DF: Program = Program::new("df", 1);

/// GNU's own short-option string, in GNU's own order.
///
/// The order is observable — the short letters decide nothing else, but a
/// reader comparing this file with `df.c` should not have to wonder whether a
/// difference is meaningful. `v` is here because GNU accepts and ignores it;
/// `F` is an undocumented synonym for `-t` that upstream still honours.
const SHORT_OPTIONS: &str = "aB:iF:hHklmPTt:vx:";

/// GNU's long table, in declaration order.
///
/// `block-size` sits before `inodes` and `total` after `sync`, which is not
/// alphabetical and is not a typo: it is the order `df.c`'s `long_options[]`
/// declares. The order is observable through an ambiguous abbreviation, whose
/// diagnostic lists the candidates in table order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("all", Takes::Nothing),
    ("block-size", Takes::Required),
    ("inodes", Takes::Nothing),
    ("human-readable", Takes::Nothing),
    ("si", Takes::Nothing),
    ("local", Takes::Nothing),
    ("output", Takes::Optional),
    ("portability", Takes::Nothing),
    ("print-type", Takes::Nothing),
    ("sync", Takes::Nothing),
    ("no-sync", Takes::Nothing),
    ("total", Takes::Nothing),
    ("type", Takes::Required),
    ("exclude-type", Takes::Required),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

// ----------------------------------------------------------------- fields ---

/// One column `--output` can name.
///
/// The order of the variants is the order of `field_data[]` upstream, which is
/// also the order [`ALL_ARGS`] lists and therefore the order `--output` with no
/// argument produces.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Field {
    /// `source` — the device.
    Source,
    /// `fstype` — the type, as the kernel names it.
    FsType,
    /// `itotal` — inodes, total.
    ITotal,
    /// `iused` — inodes, used.
    IUsed,
    /// `iavail` — inodes, free.
    IAvail,
    /// `ipcent` — inodes, used as a percentage.
    IPcent,
    /// `size` — blocks, total.
    Size,
    /// `used` — blocks, used.
    Used,
    /// `avail` — blocks, free to a non-root user.
    Avail,
    /// `pcent` — blocks, used as a percentage.
    Pcent,
    /// `file` — the operand that selected this row, or `-`.
    File,
    /// `target` — the mount point.
    Target,
}

/// Which of the three groups a field belongs to.
///
/// It decides two things at once: whether the field is a *number* that must be
/// scaled by the block size, and whether asking for it means the row needs a
/// `statvfs` at all. `Other` fields — `source`, `fstype`, `file`, `target` —
/// are the ones a row can print with no usage information whatsoever, which is
/// how `df --output=source,target` lists a file system that refuses to be
/// statted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    /// Counted in inodes.
    Inode,
    /// Counted in blocks.
    Block,
    /// Not counted: text.
    Other,
}

/// Which edge of its column a cell is flush with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Align {
    Left,
    Right,
}

/// A row of upstream's `field_data[]`: everything fixed about a field.
struct FieldSpec {
    field: Field,
    /// The name `--output` accepts, and `df --output=nosuch` rejects against.
    arg: &'static [u8],
    kind: Kind,
    /// The heading in the default (non-`-i`, non-`-P`, non-`-h`) mode. Four of
    /// the twelve are rewritten by [`get_header`]; the rest are used as they
    /// stand.
    caption: &'static str,
    align: Align,
}

/// `field_data[]`, in declaration order.
///
/// `size`'s caption is a placeholder: it is *always* replaced by
/// [`get_header`], because its text is derived from the block size. It is
/// spelled here as upstream spells it so that a reader comparing the two
/// tables does not have to wonder.
#[rustfmt::skip]
const FIELDS: &[FieldSpec] = &[
    FieldSpec { field: Field::Source, arg: b"source", kind: Kind::Other, caption: "Filesystem", align: Align::Left },
    FieldSpec { field: Field::FsType, arg: b"fstype", kind: Kind::Other, caption: "Type",       align: Align::Left },
    FieldSpec { field: Field::ITotal, arg: b"itotal", kind: Kind::Inode, caption: "Inodes",     align: Align::Right },
    FieldSpec { field: Field::IUsed,  arg: b"iused",  kind: Kind::Inode, caption: "IUsed",      align: Align::Right },
    FieldSpec { field: Field::IAvail, arg: b"iavail", kind: Kind::Inode, caption: "IFree",      align: Align::Right },
    FieldSpec { field: Field::IPcent, arg: b"ipcent", kind: Kind::Inode, caption: "IUse%",      align: Align::Right },
    FieldSpec { field: Field::Size,   arg: b"size",   kind: Kind::Block, caption: "blocks",     align: Align::Right },
    FieldSpec { field: Field::Used,   arg: b"used",   kind: Kind::Block, caption: "Used",       align: Align::Right },
    FieldSpec { field: Field::Avail,  arg: b"avail",  kind: Kind::Block, caption: "Available",  align: Align::Right },
    FieldSpec { field: Field::Pcent,  arg: b"pcent",  kind: Kind::Block, caption: "Use%",       align: Align::Right },
    FieldSpec { field: Field::File,   arg: b"file",   kind: Kind::Other, caption: "File",       align: Align::Left },
    FieldSpec { field: Field::Target, arg: b"target", kind: Kind::Other, caption: "Mounted on", align: Align::Left },
];

/// `all_args_string` upstream: what `--output` means with no `=FIELD_LIST`.
///
/// It is a comma-joined copy of [`FIELDS`]' `arg` column, kept as one literal
/// because it is also the text of the *diagnostic* for an unrecognised field —
/// `df --output=x` prints the whole list back — and building it from the table
/// would put an allocation in a path that has no other reason to need one. The
/// unit test `all_args_matches_table` holds the two in step.
const ALL_ARGS: &str = "source,fstype,itotal,iused,iavail,ipcent,size,used,avail,pcent,file,target";

impl Field {
    /// The [`FIELDS`] row for this field.
    ///
    /// Total by construction — [`FIELDS`] lists every variant, which
    /// `fields_table_is_complete` holds — but written as a search rather than
    /// an index so that adding a variant cannot silently shift the table.
    fn spec(self) -> &'static FieldSpec {
        FIELDS
            .iter()
            .find(|s| s.field == self)
            // Unreachable; see above. Returning the first row rather than
            // panicking keeps `clippy::unwrap_used` honest.
            .unwrap_or(&FIELDS[0])
    }

    /// Resolve one `--output` name. `None` is upstream's "not a field".
    fn from_arg(arg: &[u8]) -> Option<Self> {
        FIELDS.iter().find(|s| s.arg == arg).map(|s| s.field)
    }
}

/// A column of the table being built: a [`FieldSpec`] plus the width it has
/// grown to.
///
/// The caption is owned rather than borrowed because [`get_header`] computes
/// four of them (`1K-blocks`, `Size`, `1024-blocks`, …) at run time from the
/// block size.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Column {
    field: Field,
    kind: Kind,
    caption: String,
    /// The widest cell seen so far, in display columns. Seeded from the
    /// caption, then grown by every row.
    width: usize,
    align: Align,
}

// ------------------------------------------------------------- mount list ---

/// One row of `/proc/self/mountinfo`: gnulib's `struct mount_entry`.
///
/// Every text field is bytes, not `String`. A mount point is a path, a device
/// name is chosen by whoever formatted the disk, and neither is obliged to be
/// UTF-8 — decoding here would be the one place in `df` where an unusual
/// installation could not be listed at all.
#[derive(Clone, Debug, PartialEq, Eq)]
struct MountEntry {
    /// `me_devname` — the source: a device node, a network share, a bare
    /// keyword like `tmpfs`.
    devname: Vec<u8>,
    /// `me_mountdir` — where it is attached.
    mountdir: Vec<u8>,
    /// `me_mntroot` — which *subtree* of the source is attached, for a bind
    /// mount. `None` where the table cannot say (the `/etc/mtab` fallback),
    /// which upstream spells as a NULL pointer and treats as "not a bind".
    mntroot: Option<Vec<u8>>,
    /// `me_type` — `ext4`, `tmpfs`, `proc`…
    fstype: Vec<u8>,
    /// `me_dummy` — a pseudo file system, hidden unless `-a`.
    dummy: bool,
    /// `me_remote` — served over a network. `-l` keeps only the rows where
    /// this is false.
    remote: bool,
    /// `me_dev` — the device number the *table* claims, to be compared against
    /// the one `stat` reports.
    dev: u64,
}

/// `(dev_t) -1`: the table did not say what the device number is.
///
/// This is not a device number that could occur — it is the value gnulib
/// stores when it has none, and `df` tests for it before believing the field.
const DEV_UNKNOWN: u64 = u64::MAX;

/// `(dev_t) -2`: the table said, and `stat` disagreed.
///
/// [`Df::filter_mount_list`] writes this into an entry whose claimed device
/// number turned out to be wrong, so that a *later* entry claiming the same
/// number is not compared against a value already known to be a lie.
const DEV_BOGUS: u64 = u64::MAX - 1;

/// glibc's `gnu_dev_makedev`: pack a major/minor pair the way `st_dev` is
/// packed.
///
/// It has to be this exact packing and not the obvious `maj << 8 | min`,
/// because the result is compared for equality with the `st_dev` that
/// `stat(2)` returns. The layout is deliberately non-contiguous — the low 8
/// bits of the minor sit below the major so that the historic 16-bit encoding
/// is a prefix of the modern 64-bit one.
const fn makedev(maj: u64, min: u64) -> u64 {
    (min & 0xff) | ((maj & 0xfff) << 8) | ((min & !0xff) << 12) | ((maj & !0xfff) << 32)
}

/// gnulib's `unescape_tab`: undo the `\ooo` escaping the kernel applies to a
/// path in a mount table.
///
/// Only a three-digit octal escape whose first digit is `0`–`3` is an escape;
/// anything else — including a backslash at the very end, and `\4` — is a
/// literal backslash. That asymmetry is upstream's, and it matters: a mount
/// point named `C:\4` must come back out unchanged.
fn unescape_tab(str: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(str.len());
    let mut i = 0;
    while i < str.len() {
        // `i + 4 < len` upstream, where `len` counts the NUL — so the escape
        // may end at the last byte, but all four bytes must be present.
        let escape = str.get(i..i.saturating_add(4)).filter(|w| {
            w[0] == b'\\' && (b'0'..=b'3').contains(&w[1]) && is_octal(w[2]) && is_octal(w[3])
        });
        if let Some(w) = escape {
            // Cannot overflow: the largest is `\377` = 255.
            out.push((w[1] - b'0') * 64 + (w[2] - b'0') * 8 + (w[3] - b'0'));
            i = i.saturating_add(4);
        } else {
            out.push(str[i]);
            i = i.saturating_add(1);
        }
    }
    out
}

const fn is_octal(b: u8) -> bool {
    b >= b'0' && b <= b'7'
}

/// gnulib's `ME_DUMMY`: a file system with nothing on it worth reporting.
///
/// `bind` is the `Bind` parameter of the three-argument form, and it only ever
/// changes the answer for type `none` — which is why the `mountinfo` reader
/// passes `false` unconditionally and only the `/etc/mtab` reader looks at the
/// mount options. A bind-mounted directory shows up as type `none` in a static
/// `mtab`, and `du` needs to know those are real.
fn me_dummy(fstype: &[u8], bind: bool) -> bool {
    me_dummy_0(fstype) || (fstype == b"none" && !bind)
}

/// `ME_DUMMY_0`, verbatim from the 9.4 tarball. The comments are upstream's,
/// and record which platform each name was added for.
fn me_dummy_0(fstype: &[u8]) -> bool {
    matches!(
        fstype,
        b"autofs"
            | b"proc"
            | b"subfs"
            // for Linux 2.6/3.x
            | b"debugfs"
            | b"devpts"
            | b"fusectl"
            | b"fuse.portal"
            | b"mqueue"
            | b"rpc_pipefs"
            | b"sysfs"
            // FreeBSD, Linux 2.4
            | b"devfs"
            // for NetBSD 3.0
            | b"kernfs"
            // for Irix 6.5
            | b"ignore"
    )
}

/// gnulib's `ME_REMOTE`: served from somewhere else.
///
/// Upstream's rule, kept because the last clause is otherwise inexplicable: a
/// file system is remote if its name contains a `:`, or if it is `smbfs`,
/// `smb3` or `cifs` and its name starts with `//`, or if it is any of the
/// listed types, or if the name is exactly `-hosts` (which `autofs` uses to
/// mount remote file systems). "VM" file systems like `prl_fs` or `vboxsf` are
/// *not* considered remote here.
fn me_remote(devname: &[u8], fstype: &[u8]) -> bool {
    devname.contains(&b':')
        || (devname.starts_with(b"//") && matches!(fstype, b"smbfs" | b"smb3" | b"cifs"))
        || matches!(
            fstype,
            b"acfs"
                | b"afs"
                | b"coda"
                | b"auristorfs"
                | b"fhgfs"
                | b"gpfs"
                | b"ibrix"
                | b"ocfs2"
                | b"vxfs"
        )
        || devname == b"-hosts"
}

/// `sscanf`'s `%u` for one unsigned field: skip leading whitespace, then take
/// digits.
///
/// Returns the value and the rest of the input, or `None` if there was no
/// digit — which is `sscanf` returning fewer conversions than asked for, and
/// upstream's `continue`.
///
/// The saturating arithmetic is not defensive dressing: `/proc` is not
/// attacker-controlled, but this same shape of table can be a real file on
/// disk, and a `u64` overflow there would be a panic in a debug build.
fn scan_uint(text: &[u8]) -> Option<(u64, &[u8])> {
    let rest = text
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .map_or(&text[text.len()..], |n| &text[n..]);
    let digits = rest
        .iter()
        .position(|b| !b.is_ascii_digit())
        .unwrap_or(rest.len());
    if digits == 0 {
        return None;
    }
    let mut value: u64 = 0;
    for &b in &rest[..digits] {
        value = value.saturating_mul(10).saturating_add(u64::from(b - b'0'));
    }
    Some((value, &rest[digits..]))
}

/// gnulib's `terminate_at_blank`: split at the next **space**.
///
/// A space, not any whitespace: a tab inside a mountinfo field is escaped as
/// `\011` and so cannot appear here, and treating one as a separator would
/// split a field the kernel considers whole.
fn split_at_blank(text: &[u8]) -> Option<(&[u8], &[u8])> {
    let at = text.iter().position(|&b| b == b' ')?;
    Some((&text[..at], &text[at.saturating_add(1)..]))
}

/// The first offset at which `needle` occurs in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.len() > haystack.len() {
        return None;
    }
    (0..=haystack.len().saturating_sub(needle.len()))
        .find(|&i| haystack.get(i..i.saturating_add(needle.len())) == Some(needle))
}

/// Parse `/proc/self/mountinfo`.
///
/// The format is
/// `ID PARENT MAJ:MIN MNTROOT TARGET OPTIONS… - FSTYPE SOURCE SUPEROPTIONS`,
/// where the number of optional fields between `OPTIONS` and `-` is not fixed,
/// which is why the separator is searched for rather than counted to. A line
/// that does not match is skipped in silence — upstream's `continue` — so that
/// a future kernel adding a field cannot make `df` print nothing at all.
fn parse_mountinfo(text: &[u8]) -> Vec<MountEntry> {
    let mut list = Vec::new();
    for line in text.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        // "%*u %*u %u:%u %n" — two discarded, then the device pair.
        let Some((_, rest)) = scan_uint(line) else {
            continue;
        };
        let Some((_, rest)) = scan_uint(rest) else {
            continue;
        };
        let Some((devmaj, rest)) = scan_uint(rest) else {
            continue;
        };
        let Some(rest) = rest.strip_prefix(b":") else {
            continue;
        };
        let Some((devmin, rest)) = scan_uint(rest) else {
            continue;
        };
        // The `%n` follows a space in the format string; `scan_uint` has
        // already skipped any others.
        let rest = rest.strip_prefix(b" ").unwrap_or(rest);

        let Some((mntroot, rest)) = split_at_blank(rest) else {
            continue;
        };
        let Some((target, rest)) = split_at_blank(rest) else {
            continue;
        };
        // The optional fields, however many there are, end at " - ".
        let Some(dash) = find(rest, b" - ") else {
            continue;
        };
        let after = &rest[dash.saturating_add(3)..];
        let Some((fstype, rest)) = split_at_blank(after) else {
            continue;
        };
        // The source must be followed by a blank too — the super options —
        // even though what follows is discarded.
        let Some((source, _)) = split_at_blank(rest) else {
            continue;
        };

        let devname = unescape_tab(source);
        let fstype = unescape_tab(fstype);
        list.push(MountEntry {
            dummy: me_dummy(&fstype, false),
            remote: me_remote(&devname, &fstype),
            devname,
            mountdir: unescape_tab(target),
            mntroot: Some(unescape_tab(mntroot)),
            fstype,
            dev: makedev(devmaj, devmin),
        });
    }
    list
}

/// Parse an `/etc/mtab`-format table: glibc's `getmntent`, then gnulib's use
/// of it.
///
/// Four differences from [`parse_mountinfo`], all of them visible: fields are
/// separated by runs of spaces *or* tabs rather than one space; a `#` line is
/// a comment; the escaping is glibc's four-escape `decode_name` rather than
/// the kernel's general octal; and there is no device number at all, so every
/// row gets [`DEV_UNKNOWN`]. (Upstream's `dev_from_mount_options` is
/// `#ifndef __linux__` — Linux lets a file system define `dev=` to mean
/// whatever it likes, so the field is not to be trusted.)
fn parse_mounts(text: &[u8]) -> Vec<MountEntry> {
    let mut list = Vec::new();
    for line in text.split(|&b| b == b'\n') {
        // glibc chops the newline, and any blanks that were before it.
        let line = trim_end_blanks(line);
        let mut rest = skip_blanks(line);
        if rest.is_empty() || rest.first() == Some(&b'#') {
            continue;
        }
        let mut field = || -> Vec<u8> {
            let at = rest.iter().position(|&b| b == b' ' || b == b'\t');
            let (this, next) = match at {
                Some(at) => (&rest[..at], skip_blanks(&rest[at.saturating_add(1)..])),
                None => (rest, &rest[rest.len()..]),
            };
            rest = next;
            decode_name(this)
        };
        let devname = field();
        let mountdir = field();
        let fstype = field();
        let opts = field();

        let bind = hasmntopt(&opts, b"bind");
        list.push(MountEntry {
            dummy: me_dummy(&fstype, bind),
            remote: me_remote(&devname, &fstype),
            devname,
            mountdir,
            // The format cannot express one, so a bind mount is
            // indistinguishable from a whole-device mount here.
            mntroot: None,
            fstype,
            dev: DEV_UNKNOWN,
        });
    }
    list
}

fn skip_blanks(text: &[u8]) -> &[u8] {
    let at = text
        .iter()
        .position(|&b| b != b' ' && b != b'\t')
        .unwrap_or(text.len());
    &text[at..]
}

fn trim_end_blanks(text: &[u8]) -> &[u8] {
    let mut end = text.len();
    while end > 0 && matches!(text.get(end.saturating_sub(1)), Some(b' ' | b'\t')) {
        end = end.saturating_sub(1);
    }
    &text[..end]
}

/// glibc's `decode_name`: the four escapes `getmntent` understands.
///
/// Not the same grammar as [`unescape_tab`] — this one knows exactly four
/// sequences and passes everything else through, so `\040` is a space but
/// `\101` is the four bytes `\101` unchanged.
fn decode_name(buf: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(buf.len());
    let mut i = 0;
    while i < buf.len() {
        match (
            buf.get(i..i.saturating_add(4)),
            buf.get(i..i.saturating_add(2)),
        ) {
            (Some(b"\\040"), _) => {
                out.push(b' ');
                i = i.saturating_add(4);
            }
            (Some(b"\\011"), _) => {
                out.push(b'\t');
                i = i.saturating_add(4);
            }
            (Some(b"\\012"), _) => {
                out.push(b'\n');
                i = i.saturating_add(4);
            }
            (_, Some(b"\\\\")) => {
                out.push(b'\\');
                i = i.saturating_add(2);
            }
            _ => {
                out.push(buf[i]);
                i = i.saturating_add(1);
            }
        }
    }
    out
}

/// glibc's `hasmntopt`: is `opt` present as a whole option?
///
/// The whole-token test is the point. A plain substring search would find
/// `bind` inside `nobind` and inside `rbind`, and answering yes for those
/// would keep an entry visible that upstream hides.
fn hasmntopt(opts: &[u8], opt: &[u8]) -> bool {
    let mut rest = opts;
    let mut base = 0usize;
    while let Some(at) = find(rest, opt) {
        let p = base.saturating_add(at);
        let before_ok = p == 0 || opts.get(p.saturating_sub(1)) == Some(&b',');
        let after = opts.get(p.saturating_add(opt.len()));
        let after_ok = matches!(after, None | Some(b'=') | Some(b','));
        if before_ok && after_ok {
            return true;
        }
        // Resume after the next comma, as glibc does — not just past the
        // failed match, which for an empty `opt` would never advance.
        let Some(comma) = rest
            .get(at..)
            .and_then(|t| t.iter().position(|&b| b == b','))
        else {
            return false;
        };
        let skip = at.saturating_add(comma).saturating_add(1);
        base = base.saturating_add(skip);
        rest = &rest[skip..];
    }
    false
}

// ---------------------------------------------------------------- fs usage ---

/// gnulib's `struct fs_usage`: how full one file system is.
///
/// Every count may be **unknown**, and unknown is spelled `u64::MAX` — see
/// [`known_value`], which is deliberately more generous than that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FsUsage {
    /// `fsu_blocksize` — the size in bytes of the unit the other block counts
    /// are in. Not the file system's I/O block size; `statvfs`'s `f_frsize`.
    blocksize: u64,
    /// `fsu_blocks` — total.
    blocks: u64,
    /// `fsu_bfree` — free to a privileged user.
    bfree: u64,
    /// `fsu_bavail` — free to an unprivileged one, which can be *negative* on
    /// a file system whose reserve is overdrawn. See `bavail_top_bit_set`.
    bavail: u64,
    /// `fsu_bavail_top_bit_set` — `bavail` is a negative number stored in
    /// two's complement. Kept as a separate flag rather than making the field
    /// signed because the *magnitude* still needs the full 64 bits.
    bavail_top_bit_set: bool,
    /// `fsu_files` — inodes, total.
    files: u64,
    /// `fsu_ffree` — inodes, free.
    ffree: u64,
}

impl FsUsage {
    /// Every count unknown: what `get_dev` substitutes for a file system it
    /// was allowed to list but not to measure.
    const fn unknown() -> Self {
        Self {
            blocksize: u64::MAX,
            blocks: u64::MAX,
            bfree: u64::MAX,
            bavail: u64::MAX,
            bavail_top_bit_set: false,
            files: u64::MAX,
            ffree: u64::MAX,
        }
    }

    /// The accumulator `--total` sums into. `blocksize` is 1 because the
    /// running totals are kept in **bytes** — each row contributes
    /// `input_units * count` — while every other field starts at zero.
    const fn grand() -> Self {
        Self {
            blocksize: 1,
            blocks: 0,
            bfree: 0,
            bavail: 0,
            bavail_top_bit_set: false,
            files: 0,
            ffree: 0,
        }
    }
}

/// `struct statvfs` as x86-64 Linux lays it out, with room to spare.
///
/// Only the first seven members are read, and every libc that could be linked
/// here agrees on those seven. The oversized tail exists so that a libc whose
/// struct is *larger* than this cannot make the kernel write past the end of
/// the allocation: `statvfs(3)` fills in a caller-provided buffer and takes no
/// length, so being generous is the only defence available.
#[cfg(unix)]
#[repr(C)]
#[derive(Default)]
struct CStatvfs {
    f_bsize: u64,
    f_frsize: u64,
    f_blocks: u64,
    f_bfree: u64,
    f_bavail: u64,
    f_files: u64,
    f_ffree: u64,
    f_favail: u64,
    _tail: [u64; 16],
}

#[cfg(unix)]
unsafe extern "C" {
    fn statvfs(path: *const u8, buf: *mut CStatvfs) -> i32;
    fn sync();
}

/// Linux errno values the two callers branch on. Small enough, and stable
/// enough, that binding them by name would cost a dependency for nothing.
const ENOENT: i32 = 2;
const EIO: i32 = 5;
const EACCES: i32 = 13;
const ENOTDIR: i32 = 20;

/// `O_NOCTTY | O_NONBLOCK`: opening a device must not make it our controlling
/// terminal, and must not block on a tape waiting to be loaded.
#[cfg(unix)]
const O_NOCTTY: i32 = 0o400;
#[cfg(unix)]
const O_NONBLOCK: i32 = 0o4000;

// ------------------------------------------------------------------ system ---

/// The three things `df` asks `stat(2)` about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct StatInfo {
    /// `st_dev` — which file system the file is on. The whole of `get_point`
    /// is a search for the mount entry whose directory has this same number.
    dev: u64,
    /// A block or character device: `get_entry` tries these as *devices*
    /// first, so `df /dev/sda1` reports on what is mounted from it rather than
    /// on `/dev`.
    special: bool,
    /// A directory, which decides where [`System::find_mount_point`] starts.
    dir: bool,
}

/// Everything `df` needs from outside itself.
///
/// It is a trait for the same reason `du`'s `Tree` is: the interesting cases —
/// an over-mounted directory, a mount table naming a device that is not there,
/// a `statvfs` that fails with `EACCES` — cannot be staged on a real machine
/// from a test, but they are exactly the cases the filtering logic exists for.
trait System {
    /// The mount table, in the order the system reports it.
    ///
    /// # Errors
    ///
    /// Whatever prevented every source from being read.
    fn mounts(&self) -> io::Result<Vec<MountEntry>>;

    /// `statvfs(2)`, folded into gnulib's `get_fs_usage`.
    ///
    /// # Errors
    ///
    /// Whatever `statvfs` failed with; the caller branches on `EACCES` and
    /// `ENOENT` specifically.
    fn statvfs(&self, file: &[u8]) -> io::Result<FsUsage>;

    /// `stat(2)`.
    ///
    /// # Errors
    ///
    /// Whatever `stat` failed with.
    fn stat(&self, file: &[u8]) -> io::Result<StatInfo>;

    /// `stat(2)`, but opening the file first so that an automounter has a
    /// reason to mount it.
    ///
    /// # Errors
    ///
    /// Whatever `open` or `stat` failed with. Note the asymmetry upstream
    /// insists on: an `open` that fails with `ENOENT` or `ENOTDIR` is
    /// reported, but any *other* `open` failure falls back to a plain `stat`,
    /// because a file one may not open may still be one whose size is public.
    fn automount_stat(&self, file: &[u8]) -> io::Result<StatInfo>;

    /// `canonicalize_file_name(3)`: the absolute, symlink-free name, or `None`
    /// if it could not be resolved.
    fn canonicalize(&self, file: &[u8]) -> Option<Vec<u8>>;

    /// `sync(2)`, for `--sync`.
    fn sync(&self);

    /// Whether standard output is a terminal, which decides how an unprintable
    /// byte in a cell is replaced. See [`Df::replace_problematic_chars`].
    fn stdout_is_tty(&self) -> bool;

    /// gnulib's `find_mount_point`: the nearest ancestor of `point` that is on
    /// a different file system than its own parent.
    ///
    /// Upstream walks there with `chdir("..")` and reads `getcwd` at the end.
    /// This walks the canonical name upward instead, which reaches the same
    /// directory without moving the process's working directory — a
    /// side effect that would be invisible here but is not one a utility
    /// should have, and one that cannot be undone if the walk fails partway.
    fn find_mount_point(&self, point: &[u8], st: &StatInfo) -> Option<Vec<u8>> {
        let resolved = self.canonicalize(point)?;
        // A non-directory contributes its containing directory; the file
        // itself is never a mount point.
        let mut dir = if st.dir {
            resolved
        } else {
            parent_of(&resolved)?
        };
        let mut last = self.stat(&dir).ok()?;
        while let Some(up) = parent_of(&dir) {
            let above = self.stat(&up).ok()?;
            if above.dev != last.dev {
                break;
            }
            dir = up;
            last = above;
        }
        Some(dir)
    }
}

/// The parent directory of an absolute, canonical path: everything before the
/// last `/`, or `/` itself.
///
/// `None` for `/`, which has no parent and terminates the upward walk.
fn parent_of(path: &[u8]) -> Option<Vec<u8>> {
    if path == b"/" {
        return None;
    }
    let at = path.iter().rposition(|&b| b == b'/')?;
    if at == 0 {
        Some(b"/".to_vec())
    } else {
        Some(path[..at].to_vec())
    }
}

/// The real machine.
#[cfg(unix)]
struct RealSystem;

#[cfg(unix)]
impl System for RealSystem {
    /// `/proc/self/mountinfo`, then `/etc/mtab`, then `/proc/self/mounts`.
    ///
    /// The first two are gnulib's chain verbatim. The third is ours, and it is
    /// strictly a repair: it is reached only when both of the others could not
    /// be opened, which is a state in which GNU `df` prints `cannot read table
    /// of mounted file systems` and gives up. On a container whose `/etc/mtab`
    /// was never created and whose `/proc` is mounted with `hidepid`, that is
    /// the difference between a listing and nothing at all.
    fn mounts(&self) -> io::Result<Vec<MountEntry>> {
        match std::fs::read("/proc/self/mountinfo") {
            Ok(text) => return Ok(parse_mountinfo(&text)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        match std::fs::read("/etc/mtab") {
            Ok(text) => return Ok(parse_mounts(&text)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        std::fs::read("/proc/self/mounts").map(|text| parse_mounts(&text))
    }

    fn statvfs(&self, file: &[u8]) -> io::Result<FsUsage> {
        let mut path = file.to_vec();
        path.push(0);
        let mut buf = CStatvfs::default();
        // SAFETY: `path` is NUL-terminated and outlives the call; `buf` is a
        // live, writable allocation at least as large as any `struct statvfs`
        // this libc could define (see `CStatvfs`'s `_tail`).
        let rc = unsafe { statvfs(path.as_ptr(), &raw mut buf) };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        // gnulib's `PROPAGATE_ALL_ONES` and `PROPAGATE_TOP_BIT` are both the
        // identity on a platform where these members are already 64 bits wide,
        // which x86-64 Linux is; they exist for the ones where they are not.
        // The one thing that is *not* identity is the flag below.
        Ok(FsUsage {
            // `f_frsize` is not guaranteed to be supported; zero means "ask
            // `f_bsize` instead".
            blocksize: if buf.f_frsize != 0 {
                buf.f_frsize
            } else {
                buf.f_bsize
            },
            blocks: buf.f_blocks,
            bfree: buf.f_bfree,
            bavail: buf.f_bavail,
            bavail_top_bit_set: buf.f_bavail >> 63 != 0,
            files: buf.f_files,
            ffree: buf.f_ffree,
        })
    }

    fn stat(&self, file: &[u8]) -> io::Result<StatInfo> {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};
        let meta = std::fs::metadata(os_from_bytes(file))?;
        let kind = meta.file_type();
        Ok(StatInfo {
            dev: meta.dev(),
            special: kind.is_block_device() || kind.is_char_device(),
            dir: meta.is_dir(),
        })
    }

    fn automount_stat(&self, file: &[u8]) -> io::Result<StatInfo> {
        use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
        let opened = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(O_NOCTTY | O_NONBLOCK)
            .open(os_from_bytes(file));
        let file_handle = match opened {
            Ok(handle) => handle,
            Err(e) if matches!(e.raw_os_error(), Some(ENOENT | ENOTDIR)) => return Err(e),
            // Any other reason we could not open it — a permission we lack, a
            // device with no medium — is not a reason we cannot *stat* it.
            Err(_) => return self.stat(file),
        };
        // `fstat`, via the handle: the automount has now been triggered, and
        // re-resolving the name could race with something unmounting it.
        let meta = file_handle.metadata()?;
        let kind = meta.file_type();
        Ok(StatInfo {
            dev: meta.dev(),
            special: kind.is_block_device() || kind.is_char_device(),
            dir: meta.is_dir(),
        })
    }

    fn canonicalize(&self, file: &[u8]) -> Option<Vec<u8>> {
        coreutils::canon::canonicalize(
            &coreutils::canon::RealFs,
            file,
            coreutils::canon::Mode::Existing,
        )
        .ok()
    }

    fn sync(&self) {
        // SAFETY: `sync(2)` takes no arguments, returns nothing, and cannot
        // fail. It is the whole of what `--sync` asks for.
        unsafe { sync() }
    }

    fn stdout_is_tty(&self) -> bool {
        stdfd::is_tty(1)
    }
}

// ------------------------------------------------------------------ values ---

/// Upstream's `known_value`: is `n` a real count?
///
/// The bound is `UINTMAX_MAX - 1`, not `UINTMAX_MAX`, and upstream says why:
/// most file systems spell "unknown" as all-ones, but AIX spells it as
/// all-ones-minus-one. Excluding both costs one representable value that no
/// real file system will ever have and works on either.
const fn known_value(n: u64) -> bool {
    n < u64::MAX - 1
}

/// Upstream's `df_readable`: a count as text, or `-` when it is unknown.
///
/// `negative` is not redundant with the sign of `n`: `n` is a *magnitude*
/// stored in two's complement, so a negative available-space figure arrives as
/// a huge unsigned number with the flag set. Rendering it means negating it
/// back, formatting the magnitude, and prefixing the sign — which also
/// explains why an unknown value with the flag set is *not* `-`: the flag says
/// the number is meaningful.
fn df_readable(negative: bool, n: u64, opts: Opts, input_units: u64, output_units: u64) -> String {
    if !known_value(n) && !negative {
        return "-".to_string();
    }
    let magnitude = if negative { n.wrapping_neg() } else { n };
    let text = human_readable(magnitude, opts, input_units, output_units);
    if negative { format!("-{text}") } else { text }
}

/// Upstream's `add_uint_with_neg_flag`: add two magnitude/sign pairs.
///
/// Written out rather than done in `i128` because the magnitudes genuinely use
/// all 64 bits — this is summing byte counts over every mounted file system —
/// and because the wrapping is the representation, not an accident of it.
fn add_uint_with_neg_flag(dest: &mut u64, dest_neg: &mut bool, src: u64, src_neg: bool) {
    if *dest_neg == src_neg {
        *dest = dest.wrapping_add(src);
        return;
    }
    if *dest_neg {
        *dest = dest.wrapping_neg();
    }
    let src = if src_neg { src.wrapping_neg() } else { src };
    if src < *dest {
        *dest = dest.wrapping_sub(src);
    } else {
        *dest = src.wrapping_sub(*dest);
        *dest_neg = src_neg;
    }
    if *dest_neg {
        *dest = dest.wrapping_neg();
    }
}

/// One column group's numbers, ready to be rendered: upstream's
/// `struct field_values_t`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct FieldValues {
    /// The unit the counts are in — bytes per block, or 1 for inodes.
    input_units: u64,
    /// The unit they are to be *printed* in: the `-B` block size, or 1.
    output_units: u64,
    total: u64,
    available: u64,
    negate_available: bool,
    /// Free space including the reserve only root may use. This, not
    /// `available`, is what `used` is derived from — which is why `used +
    /// available` can be less than `total`, and why the two percentages a
    /// naive reader would compute do not agree.
    available_to_root: u64,
    used: u64,
    negate_used: bool,
}

/// Upstream's `get_field_values`: split one [`FsUsage`] into the inode group
/// and the block group.
fn get_field_values(fsu: &FsUsage, output_block_size: u64) -> (FieldValues, FieldValues) {
    let mut inode = FieldValues {
        input_units: 1,
        output_units: 1,
        total: fsu.files,
        available: fsu.ffree,
        available_to_root: fsu.ffree,
        negate_available: false,
        used: u64::MAX,
        negate_used: false,
    };
    if known_value(inode.total) && known_value(inode.available_to_root) {
        inode.used = inode.total.wrapping_sub(inode.available_to_root);
        inode.negate_used = inode.total < inode.available_to_root;
    }

    let mut block = FieldValues {
        input_units: fsu.blocksize,
        output_units: output_block_size,
        total: fsu.blocks,
        available: fsu.bavail,
        available_to_root: fsu.bfree,
        negate_available: fsu.bavail_top_bit_set && known_value(fsu.bavail),
        used: u64::MAX,
        negate_used: false,
    };
    if known_value(block.total) && known_value(block.available_to_root) {
        block.used = block.total.wrapping_sub(block.available_to_root);
        block.negate_used = block.total < block.available_to_root;
    }
    (block, inode)
}

/// Upstream's percentage: `used / (used + available)`, rounded **up**.
///
/// `None` is the `-` cell. There are two paths, and which one is taken is
/// observable to the last digit:
///
/// - Integer, when `used * 100` cannot overflow, the denominator is non-zero,
///   and the sign of the sum is the sign the flags predict. `u100 /
///   nonroot_total + (u100 % nonroot_total != 0)` is an exact ceiling.
/// - Floating point otherwise, with the ceiling done by comparison against a
///   truncated copy so that upstream need not link the math library. That
///   dance is reproduced rather than replaced by `f64::ceil` because the
///   guard `ipct - 1 < pct && pct <= ipct + 1` silently declines to round
///   values too large for the `long int` round trip, and `ceil` would not.
///
/// The result is always integral, so the caller's `{:.0}` matches `%.0f`
/// without a rounding-mode question.
#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "the lossy casts are upstream's arithmetic, and the loss is what is being reproduced"
)]
fn percent(v: &FieldValues) -> Option<f64> {
    if !known_value(v.used) || !known_value(v.available) {
        return None;
    }
    let sum = v.used.wrapping_add(v.available);
    if !v.negate_used
        && v.used <= u64::MAX / 100
        && sum != 0
        && (sum < v.used) == v.negate_available
    {
        let u100 = v.used.saturating_mul(100);
        let pct = u100 / sum + u64::from(!u100.is_multiple_of(sum));
        return Some(pct as f64);
    }
    let u = if v.negate_used {
        -(v.used.wrapping_neg() as f64)
    } else {
        v.used as f64
    };
    let a = if v.negate_available {
        -(v.available.wrapping_neg() as f64)
    } else {
        v.available as f64
    };
    let nonroot_total = u + a;
    if nonroot_total == 0.0 {
        // Upstream leaves `pct` at its initialiser of -1 here, which the
        // caller renders as `-`.
        return None;
    }
    let pct = u * 100.0 / nonroot_total;
    let ipct = (pct as i64) as f64;
    // `pct = ceil (pct)`, without libm.
    if ipct - 1.0 < pct && pct <= ipct + 1.0 {
        return Some(ipct + f64::from(u8::from(ipct < pct)));
    }
    Some(pct).filter(|p| *p >= 0.0)
}

/// Upstream's `has_uuid_suffix`: does `s` end in something shaped like a UUID?
///
/// Used for one thing only — deciding whether a device name is worth resolving
/// through its symlink. `/dev/disk/by-uuid/828fc648-…` is a name nobody wants
/// in a table; `/dev/sda1` is. The test is deliberately loose (any 36
/// hex-or-dash bytes) because being wrong costs a `canonicalize` call, not a
/// wrong answer.
fn has_uuid_suffix(s: &[u8]) -> bool {
    s.len() > 36
        && s[s.len() - 36..]
            .iter()
            .all(|b| *b == b'-' || b.is_ascii_hexdigit())
}

// ------------------------------------------------------------------- cells ---

/// Upstream's `replace_control_chars`, used when stdout is **not** a terminal.
///
/// Byte-wise, and deliberately so: upstream's comment is "since only control
/// characters are currently considered, this should work in all encodings",
/// which is true because no byte of a multi-byte UTF-8 sequence is below
/// `0x20`. An undecodable byte is left alone, because nothing is going to
/// render it.
fn replace_control_chars(cell: &mut [u8]) {
    for b in cell.iter_mut() {
        if *b < 0x20 || *b == 0x7f {
            *b = b'?';
        }
    }
}

/// Upstream's `replace_invalid_chars`, used when stdout **is** a terminal.
///
/// Character-wise, so it catches both a control character and a byte that
/// decodes to nothing at all — the second of which the byte-wise version
/// cannot see, and which would otherwise be handed to a terminal that may act
/// on it.
///
/// One byte is consumed per `?`, not one character: upstream sets `n = 1` on a
/// failed `mbrtowc` before the `memmove`, so a three-byte invalid run becomes
/// three question marks rather than one.
fn replace_invalid_chars(cell: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(cell.len());
    let mut rest = cell;
    while let Some(mb) = next_mb(rest) {
        match mb {
            Mb::Char(c, n) if !c.is_control() => {
                out.extend_from_slice(&rest[..n]);
                rest = &rest[n..];
            }
            Mb::Char(_, n) => {
                out.push(b'?');
                rest = &rest[n..];
            }
            Mb::Invalid | Mb::Incomplete => {
                out.push(b'?');
                rest = &rest[1..];
            }
        }
    }
    out
}

/// gnulib's `mbswidth (cell, 0)`: how many terminal columns a cell occupies.
///
/// Not `cell.len()` and not `chars().count()`. With flags of zero, gnulib
/// counts a non-printable character as **1** rather than rejecting the string,
/// and counts an undecodable byte as 1 as well; a wide character counts 2 and
/// a combining mark counts 0. Getting this wrong misaligns every column to the
/// right of the offending cell, and only for the users whose mount points are
/// not ASCII.
fn mbswidth(cell: &[u8]) -> usize {
    let mut width = 0usize;
    let mut rest = cell;
    while let Some(mb) = next_mb(rest) {
        match mb {
            Mb::Char(c, n) => {
                // `wcwidth` answers -1 for a non-printable character, which
                // gnulib's flags-of-zero path turns into 1 — except for a
                // control character, which it counts as 0.
                width = width.saturating_add(char_width(c).unwrap_or(usize::from(!c.is_control())));
                rest = &rest[n..];
            }
            Mb::Invalid => {
                width = width.saturating_add(1);
                rest = &rest[1..];
            }
            Mb::Incomplete => {
                width = width.saturating_add(1);
                break;
            }
        }
    }
    width
}

/// [`charwidth::char_width`], named locally so the call sites read like
/// `wcwidth`.
fn char_width(c: char) -> Option<usize> {
    charwidth::char_width(c)
}

/// gnulib's `ambsalign`, for the one shape of call `df` makes.
///
/// `no_right_pad` is `MBA_NO_RIGHT_PAD`, which the last column of every row
/// passes so that lines do not end in trailing blanks.
///
/// Two of upstream's behaviours are deliberately *not* here:
///
/// - **Truncation.** `mbsalign` truncates a cell wider than the field, but
///   `df` computes the field width as the maximum over the cells, so no cell
///   can be wider than its field. Reproducing the truncation would add a code
///   path that no input reaches.
/// - **`wc_ensure_printable`.** It replaces a non-printable character with
///   U+FFFD, but [`replace_control_chars`]/[`replace_invalid_chars`] have
///   already run over every cell, so there is none left to replace.
///
/// The failure case *is* here: when the cell does not decode, upstream's
/// `mbstowcs` returns `(size_t) -1`, `ambsalign` returns NULL, and `df` prints
/// the cell unpadded. A misaligned row is upstream's answer, and matching it
/// matters more than a tidier one would.
fn align_cell(out: &mut Vec<u8>, cell: &[u8], width: usize, align: Align, no_right_pad: bool) {
    if std::str::from_utf8(cell).is_err() {
        out.extend_from_slice(cell);
        return;
    }
    let pad = width.saturating_sub(mbswidth(cell));
    let (left, right) = match align {
        Align::Left => (0, pad),
        Align::Right => (pad, 0),
    };
    let right = if no_right_pad { 0 } else { right };
    out.extend(std::iter::repeat_n(b' ', left));
    out.extend_from_slice(cell);
    out.extend(std::iter::repeat_n(b' ', right));
}

// -------------------------------------------------------- settings/parsing ---

/// Which set of columns, and which captions, the table gets: upstream's
/// `header_mode`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum HeaderMode {
    /// No mode-setting option: `Filesystem 1K-blocks Used Available Use%
    /// Mounted on`.
    Default,
    /// `-i`.
    Inodes,
    /// Implied by any `-h`/`-H`/`-B` that autoscales — the same columns as
    /// `Default` with shorter captions, because the numbers are shorter.
    Human,
    /// `-P`, unless `--output` or `-i` got there first.
    Posix,
    /// `--output`, which names its own columns and ignores the rest.
    Output,
}

/// Everything the option loop decides.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Settings {
    show_all_fs: bool,
    show_local_fs: bool,
    print_type: bool,
    print_grand_total: bool,
    require_sync: bool,
    posix_format: bool,
    header_mode: HeaderMode,
    human_output_opts: Opts,
    output_block_size: u64,
    /// `-t`/`-F`, in list order — which is the **reverse** of the command
    /// line, because upstream prepends. The order is observable through the
    /// "both selected and excluded" diagnostic, which reports the first clash
    /// it finds.
    fs_select: Vec<Vec<u8>>,
    /// `-x`, likewise reversed.
    fs_exclude: Vec<Vec<u8>>,
    /// The columns `--output` asked for. Empty in every other mode, where
    /// [`Df::field_list`] fills them in.
    columns: Vec<Column>,
    operands: Vec<Vec<u8>>,
}

/// What the command line turned out to be asking for.
///
/// `Run` is boxed because [`Settings`] is much larger than the other two
/// variants and `clippy::large_enum_variant` is right to say so.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Request {
    Help,
    Version,
    Run(Box<Settings>),
}

/// A refusal to run: the lines to print, whether the referral follows, and the
/// status to exit with.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Refusal {
    lines: Vec<String>,
    referral: bool,
    status: i32,
}

impl Refusal {
    fn from_getopt(error: &getopt::Error) -> Self {
        Self {
            lines: vec![format!("df: {}", error.sentence)],
            referral: error.referral.is_some(),
            status: error.status,
        }
    }

    /// One sentence, a referral, and status 1 — upstream's `error (0, 0, …);
    /// usage (EXIT_FAILURE);` pair, which is how every `--output` and
    /// mutual-exclusion complaint is raised.
    fn usage(sentence: &str) -> Self {
        Self {
            lines: vec![format!("df: {sentence}")],
            referral: true,
            status: 1,
        }
    }

    /// One sentence and **no** referral: upstream's `error (EXIT_FAILURE, …)`,
    /// which prints and exits without suggesting `--help`.
    fn fatal(sentence: &str) -> Self {
        Self {
            lines: vec![format!("df: {sentence}")],
            referral: false,
            status: 1,
        }
    }

    fn print(&self, err: &mut dyn Write) {
        for line in &self.lines {
            // A diagnostic that cannot be written has nowhere left to be
            // reported, so the failure is deliberately dropped here.
            let _ = writeln!(err, "{line}");
        }
        if self.referral {
            let _ = writeln!(err, "Try 'df --help' for more information.");
        }
    }
}

/// The environment `df` reads, gathered up so parsing stays a pure function.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Environment {
    df_block_size: Option<Vec<u8>>,
    block_size: Option<Vec<u8>>,
    blocksize: Option<Vec<u8>>,
    posixly_correct: bool,
}

/// The single byte the option loop switches on for a long option.
fn long_key(name: &str) -> u8 {
    match name {
        "all" => b'a',
        "block-size" => b'B',
        "inodes" => b'i',
        "human-readable" => b'h',
        "si" => b'H',
        "local" => b'l',
        "output" => b'\x02',
        "portability" => b'P',
        "print-type" => b'T',
        "sync" => b'\x03',
        "no-sync" => b'\x04',
        "total" => b'\x05',
        "type" => b't',
        "exclude-type" => b'x',
        "help" => b'\0',
        // `--version`, and — by construction — nothing else: every name in
        // `LONG_OPTIONS` is listed above.
        _ => b'\x01',
    }
}

/// Add one column, refusing a field that is already there.
///
/// Upstream's `alloc_field` `affirm`s that the field is unused and leaves the
/// *checking* to `decode_output_arg`; the check is folded in here so that
/// there is one place a column can be created and no way to skip the guard.
fn alloc_field(columns: &mut Vec<Column>, field: Field, caption: Option<&str>) {
    let spec = field.spec();
    columns.push(Column {
        field,
        kind: spec.kind,
        caption: caption.unwrap_or(spec.caption).to_string(),
        width: 0,
        align: spec.align,
    });
}

/// Upstream's `decode_output_arg`: a comma-separated field list.
///
/// Two fields get a caption that is not their table default, and only here —
/// `size` becomes `Size` and `avail` becomes `Avail`, because `--output` is
/// expected to be read by a person rather than parsed by a script and the long
/// forms crowd the line.
///
/// An empty list is not an error: `df --output=` asks for one field whose name
/// is the empty string, and is refused as `field '' unknown` — measured, and
/// the reason the loop is `do … while` upstream rather than a `for` over
/// non-empty pieces.
fn decode_output_arg(columns: &mut Vec<Column>, arg: &[u8]) -> Result<(), Refusal> {
    for name in arg.split(|&b| b == b',') {
        let Some(field) = Field::from_arg(name) else {
            return Err(Refusal::usage(&format!(
                "option --output: field {} unknown",
                quote(name)
            )));
        };
        if columns.iter().any(|c| c.field == field) {
            // Reported against the *table's* spelling, which is the same text
            // — the lookup succeeded — but is how upstream words it.
            return Err(Refusal::usage(&format!(
                "option --output: field {} used more than once",
                quote(field.spec().arg)
            )));
        }
        let caption = match field {
            Field::Size => Some("Size"),
            Field::Avail => Some("Avail"),
            _ => None,
        };
        alloc_field(columns, field, caption);
    }
    Ok(())
}

/// The option loop.
#[expect(
    clippy::too_many_lines,
    reason = "one option per arm; splitting the match would hide the order the arms must keep"
)]
fn parse_args(argv: &[OsString], env: &Environment) -> Result<Request, Refusal> {
    const MUT_EXCL: &str = "options {} and {} are mutually exclusive";
    let mut show_all_fs = false;
    let mut show_local_fs = false;
    let mut print_type = false;
    let mut print_grand_total = false;
    let mut require_sync = false;
    let mut posix_format = false;
    let mut header_mode = HeaderMode::Default;
    // Upstream's `human_output_opts == -1` sentinel: not "no flags", but "no
    // option has spoken yet", which is a different state and decides whether
    // the environment is consulted at all.
    let mut human: Option<(Opts, u64)> = None;
    let mut fs_select: Vec<Vec<u8>> = Vec::new();
    let mut fs_exclude: Vec<Vec<u8>> = Vec::new();
    let mut columns: Vec<Column> = Vec::new();
    let mut operands: Vec<Vec<u8>> = Vec::new();

    let mut_excl = |a: &str, b: &str| -> Refusal {
        Refusal::usage(&MUT_EXCL.replacen("{}", a, 1).replacen("{}", b, 1))
    };

    for item in DF.parse(argv, SHORT_OPTIONS, LONG_OPTIONS) {
        let item = item.map_err(|error| Refusal::from_getopt(&error))?;
        let (key, value, spelling): (u8, Option<OsString>, String) = match item {
            Opt::Operand(word) => {
                operands.push(os_bytes(word).into_owned());
                continue;
            }
            Opt::Short(flag, value) => (flag, value, format!("-{}", char::from(flag))),
            Opt::Long(name, value) => (long_key(name), value, format!("--{name}")),
        };
        let raw = value.as_deref().map(|v| os_bytes(v).into_owned());
        let arg = raw.as_deref().unwrap_or_default();

        match key {
            b'\0' => return Ok(Request::Help),
            b'\x01' => return Ok(Request::Version),
            b'a' => show_all_fs = true,
            b'B' => {
                let (block_size, opts, status) =
                    coreutils::human::human_options(arg, env.posixly_correct);
                if let Some(sentence) = strtol_fatal(status, &spelling, arg) {
                    // `error (EXIT_FAILURE, …)` upstream: no referral.
                    return Err(Refusal::fatal(&sentence));
                }
                human = Some((opts, block_size));
            }
            b'i' => {
                if header_mode == HeaderMode::Output {
                    return Err(mut_excl("-i", "--output"));
                }
                header_mode = HeaderMode::Inodes;
            }
            b'h' => human = Some((Opts::AUTOSCALE | Opts::SI | Opts::BASE_1024, 1)),
            b'H' => human = Some((Opts::AUTOSCALE | Opts::SI, 1)),
            b'k' => human = Some((Opts::NONE, 1024)),
            b'l' => show_local_fs = true,
            // Obsolescent; exists for BSD compatibility.
            b'm' => human = Some((Opts::NONE, 1024 * 1024)),
            b'T' => {
                if header_mode == HeaderMode::Output {
                    return Err(mut_excl("-T", "--output"));
                }
                print_type = true;
            }
            b'P' => {
                if header_mode == HeaderMode::Output {
                    return Err(mut_excl("-P", "--output"));
                }
                posix_format = true;
            }
            b'\x03' => require_sync = true,
            b'\x04' => require_sync = false,
            // `-F` is a Solaris-compatible synonym for `-t`. Prepended, not
            // appended: see `Settings::fs_select`.
            b'F' | b't' => fs_select.insert(0, arg.to_vec()),
            b'x' => fs_exclude.insert(0, arg.to_vec()),
            // `-v` exists for SysV compatibility and does nothing.
            b'v' => {}
            b'\x02' => {
                if header_mode == HeaderMode::Inodes {
                    return Err(mut_excl("-i", "--output"));
                }
                // `posix_format` rather than `header_mode == Posix`: the
                // header mode is not resolved until after the loop, so `-P`
                // has set only the flag by this point.
                if posix_format && header_mode == HeaderMode::Default {
                    return Err(mut_excl("-P", "--output"));
                }
                if print_type {
                    return Err(mut_excl("-T", "--output"));
                }
                header_mode = HeaderMode::Output;
                if raw.is_some() {
                    decode_output_arg(&mut columns, arg)?;
                }
            }
            b'\x05' => print_grand_total = true,
            // Unreachable: every byte `long_key` and `SHORT_OPTIONS` can
            // produce has an arm above.
            _ => {
                return Err(Refusal::usage(&format!(
                    "option '{spelling}' is not implemented"
                )));
            }
        }
    }

    let (human_output_opts, output_block_size) = match human {
        Some(pair) => pair,
        None if posix_format => (
            Opts::NONE,
            coreutils::human::default_block_size(env.posixly_correct),
        ),
        None => {
            // `human_options (getenv ("DF_BLOCK_SIZE"), …)`, whose NULL case
            // falls through to `BLOCK_SIZE` and then `BLOCKSIZE` inside
            // gnulib's `humblock`. The status is discarded here — an
            // unparseable environment variable leaves the repaired default and
            // is not an error — which is exactly what `du` does with its own.
            match env
                .df_block_size
                .as_deref()
                .or(env.block_size.as_deref())
                .or(env.blocksize.as_deref())
            {
                Some(spec) => {
                    let (size, opts, _) =
                        coreutils::human::human_options(spec, env.posixly_correct);
                    (opts, size)
                }
                None => (
                    Opts::NONE,
                    coreutils::human::default_block_size(env.posixly_correct),
                ),
            }
        }
    };

    if !matches!(header_mode, HeaderMode::Inodes | HeaderMode::Output) {
        if human_output_opts.has(Opts::AUTOSCALE) {
            header_mode = HeaderMode::Human;
        } else if posix_format {
            header_mode = HeaderMode::Posix;
        }
    }

    // Fail if the same file system type was both selected and excluded. The
    // first clash found wins, and "first" is in list order — the reverse of
    // the command line.
    if let Some(name) = fs_select.iter().find(|n| fs_exclude.contains(n)) {
        // `error (0, 0, …)` and then a plain `return EXIT_FAILURE`: this one
        // does not go through `usage`, so there is no referral.
        return Err(Refusal::fatal(&format!(
            "file system type {} both selected and excluded",
            quote(name)
        )));
    }

    Ok(Request::Run(Box::new(Settings {
        show_all_fs,
        show_local_fs,
        print_type,
        print_grand_total,
        require_sync,
        posix_format,
        header_mode,
        human_output_opts,
        output_block_size,
        fs_select,
        fs_exclude,
        columns,
        operands,
    })))
}

/// GNU's `--help`, minus the four-line ancillary block.
///
/// The omission is this project's house style and is shared by every converted
/// utility: the block points at `gnu.org` and at an `info` page that is not
/// installed here, so reproducing it would be an instruction the user cannot
/// follow. Everything above it is byte-for-byte what coreutils 9.4 prints,
/// including the two-space indent that does not line up on the `-B` line.
fn help_text() -> String {
    "\
Usage: df [OPTION]... [FILE]...
Show information about the file system on which each FILE resides,
or all file systems by default.

Mandatory arguments to long options are mandatory for short options too.
  -a, --all             include pseudo, duplicate, inaccessible file systems
  -B, --block-size=SIZE  scale sizes by SIZE before printing them; e.g.,
                           '-BM' prints sizes in units of 1,048,576 bytes;
                           see SIZE format below
  -h, --human-readable  print sizes in powers of 1024 (e.g., 1023M)
  -H, --si              print sizes in powers of 1000 (e.g., 1.1G)
  -i, --inodes          list inode information instead of block usage
  -k                    like --block-size=1K
  -l, --local           limit listing to local file systems
      --no-sync         do not invoke sync before getting usage info (default)
      --output[=FIELD_LIST]  use the output format defined by FIELD_LIST,
                               or print all fields if FIELD_LIST is omitted.
  -P, --portability     use the POSIX output format
      --sync            invoke sync before getting usage info
      --total           elide all entries insignificant to available space,
                          and produce a grand total
  -t, --type=TYPE       limit listing to file systems of type TYPE
  -T, --print-type      print file system type
  -x, --exclude-type=TYPE   limit listing to file systems not of type TYPE
  -v                    (ignored)
      --help        display this help and exit
      --version     output version information and exit

Display values are in units of the first available SIZE from --block-size,
and the DF_BLOCK_SIZE, BLOCK_SIZE and BLOCKSIZE environment variables.
Otherwise, units default to 1024 bytes (or 512 if POSIXLY_CORRECT is set).

The SIZE argument is an integer and optional unit (example: 10K is 10*1024).
Units are K,M,G,T,P,E,Z,Y,R,Q (powers of 1024) or KB,MB,... (powers of 1000).
Binary prefixes can be used, too: KiB=K, MiB=M, and so on.

FIELD_LIST is a comma-separated list of columns to be included.  Valid
field names are: 'source', 'fstype', 'itotal', 'iused', 'iavail', 'ipcent',
'size', 'used', 'avail', 'pcent', 'file' and 'target' (see info page).
"
    .to_string()
}

// ------------------------------------------------------------------ driver ---

/// The nine parameters of upstream's `get_dev`, as one struct.
///
/// Nine positional arguments — five of them nullable strings, two of them
/// adjacent booleans — is a call signature in which a transposition is
/// invisible. Naming them costs a struct and removes the whole class of
/// mistake; `clippy::too_many_arguments` would object to the alternative in
/// any case.
struct DevArgs<'a> {
    /// The source. `None` is upstream's null device, printed as `-`: the file
    /// system is mounted but nothing in the table says from what.
    device: Option<&'a [u8]>,
    /// Where it is mounted. `None` means *not* mounted, and the row then
    /// describes whichever file system the special file itself lives on.
    mount_point: Option<&'a [u8]>,
    /// The operand that produced this row, for the `file` column. `None` is
    /// printed as `-`.
    file: Option<&'a [u8]>,
    /// The name to measure. `None` falls back to the mount point, then to the
    /// device — upstream's `stat_file = mount_point ? mount_point : device`.
    /// Naming a file *inside* the file system gives better diagnostics, and on
    /// some network file systems a different answer.
    stat_file: Option<&'a [u8]>,
    /// The type, `None` when unknown; printed as `-`.
    fstype: Option<&'a [u8]>,
    me_dummy: bool,
    me_remote: bool,
    /// Usage figures to print instead of measuring any: the `--total` row, and
    /// the only case in which nothing is added to the grand total.
    force_fsu: Option<&'a FsUsage>,
    /// Whether this row came from walking the whole mount table rather than
    /// from an operand. It relaxes two diagnostics into placeholder rows — a
    /// file system that vanished between reading the table and measuring it is
    /// not the user's fault when the user did not name it.
    process_all: bool,
}

/// The run: the table being built, and everything it is being built from.
///
/// `err` is deliberately *not* a field. Every method that can diagnose takes it
/// as a parameter instead, because the alternative — holding `&mut dyn Write`
/// in the struct — makes `&mut self` methods conflict with the `&self` reads of
/// `self.mounts` that surround every one of those diagnostics.
struct Df<'a, S: System + ?Sized> {
    cfg: &'a Settings,
    sys: &'a S,
    /// The columns, with their widths grown as cells are added.
    columns: Vec<Column>,
    /// The table: the header row, then one row per file system.
    rows: Vec<Vec<Vec<u8>>>,
    /// The mount table, filtered by [`Df::filter_mount_list`] unless `-a`.
    mounts: Vec<MountEntry>,
    /// `devlist_table`, populated **only** in `df -a`.
    ///
    /// Upstream frees the table at the end of `filter_mount_list` unless
    /// `devices_only`, so `me_for_dev` answers null in every other mode — and
    /// the over-mount check in `get_dev` that consults it is guarded by
    /// `show_all_fs` for exactly that reason.
    devlist: HashMap<u64, MountEntry>,
    /// `grand_fsu`: the running total, in bytes. See [`FsUsage::grand`].
    grand: FsUsage,
    /// `exit_status`. Diagnostics set it; nothing resets it.
    exit_status: i32,
    /// `file_systems_processed`: whether any row was produced. Its absence is
    /// the `no file systems processed` diagnostic.
    processed: bool,
    /// `show_listed_fs`: set once operands are known to exist, and it overrides
    /// both the dummy-file-system filter and the empty-file-system filter. A
    /// file system the user named explicitly is listed even when `df` would
    /// never have volunteered it.
    show_listed_fs: bool,
    /// `isatty (STDOUT_FILENO)`, read once. It selects between the two
    /// unprintable-byte replacements.
    tty_out: bool,
}

impl<'a, S: System + ?Sized> Df<'a, S> {
    fn new(cfg: &'a Settings, sys: &'a S) -> Self {
        Self {
            cfg,
            sys,
            columns: Vec::new(),
            rows: Vec::new(),
            mounts: Vec::new(),
            devlist: HashMap::new(),
            grand: FsUsage::grand(),
            exit_status: 0,
            processed: false,
            show_listed_fs: false,
            tty_out: sys.stdout_is_tty(),
        }
    }

    /// Upstream's `selected_fstype`: is this type on the `-t` list?
    ///
    /// An empty list selects everything, and so does an *unknown* type — a
    /// mount entry with no type is never filtered out by a type filter.
    fn selected_fstype(&self, fstype: Option<&[u8]>) -> bool {
        match fstype {
            None => true,
            Some(t) => self.cfg.fs_select.is_empty() || self.cfg.fs_select.iter().any(|n| n == t),
        }
    }

    /// Upstream's `excluded_fstype`: is this type on the `-x` list?
    fn excluded_fstype(&self, fstype: Option<&[u8]>) -> bool {
        match fstype {
            None => false,
            Some(t) => self.cfg.fs_exclude.iter().any(|n| n == t),
        }
    }

    /// Upstream's `replace_problematic_chars`.
    fn scrub(&self, cell: Vec<u8>) -> Vec<u8> {
        if self.tty_out {
            replace_invalid_chars(&cell)
        } else {
            let mut cell = cell;
            replace_control_chars(&mut cell);
            cell
        }
    }

    /// Add a finished cell to the row being built, growing its column.
    fn push_cell(&mut self, row: &mut Vec<Vec<u8>>, index: usize, cell: Vec<u8>) {
        let cell = self.scrub(cell);
        let width = mbswidth(&cell);
        if let Some(col) = self.columns.get_mut(index) {
            col.width = col.width.max(width);
        }
        row.push(cell);
    }

    /// Upstream's `get_field_list`: which columns this mode has.
    ///
    /// The four modes differ in captions more than in membership — only
    /// `Inodes` swaps the block group for the inode group — but the captions
    /// are the visible part, so they are spelled out rather than derived.
    fn field_list(&mut self) {
        let mut columns = Vec::new();
        let type_column = |columns: &mut Vec<Column>, print_type: bool| {
            if print_type {
                alloc_field(columns, Field::FsType, None);
            }
        };
        match self.cfg.header_mode {
            HeaderMode::Default => {
                alloc_field(&mut columns, Field::Source, None);
                type_column(&mut columns, self.cfg.print_type);
                alloc_field(&mut columns, Field::Size, None);
                alloc_field(&mut columns, Field::Used, None);
                alloc_field(&mut columns, Field::Avail, None);
                alloc_field(&mut columns, Field::Pcent, None);
                alloc_field(&mut columns, Field::Target, None);
            }
            HeaderMode::Human => {
                alloc_field(&mut columns, Field::Source, None);
                type_column(&mut columns, self.cfg.print_type);
                alloc_field(&mut columns, Field::Size, Some("Size"));
                alloc_field(&mut columns, Field::Used, None);
                alloc_field(&mut columns, Field::Avail, Some("Avail"));
                alloc_field(&mut columns, Field::Pcent, None);
                alloc_field(&mut columns, Field::Target, None);
            }
            HeaderMode::Inodes => {
                alloc_field(&mut columns, Field::Source, None);
                type_column(&mut columns, self.cfg.print_type);
                alloc_field(&mut columns, Field::ITotal, None);
                alloc_field(&mut columns, Field::IUsed, None);
                alloc_field(&mut columns, Field::IAvail, None);
                alloc_field(&mut columns, Field::IPcent, None);
                alloc_field(&mut columns, Field::Target, None);
            }
            HeaderMode::Posix => {
                alloc_field(&mut columns, Field::Source, None);
                type_column(&mut columns, self.cfg.print_type);
                alloc_field(&mut columns, Field::Size, None);
                alloc_field(&mut columns, Field::Used, None);
                alloc_field(&mut columns, Field::Avail, None);
                alloc_field(&mut columns, Field::Pcent, Some("Capacity"));
                alloc_field(&mut columns, Field::Target, None);
            }
            HeaderMode::Output => {
                columns = self.cfg.columns.clone();
                if columns.is_empty() {
                    // `--output` with no field list. The argument cannot be
                    // rejected — it is our own literal — so the error is
                    // discarded rather than plumbed through a caller that has
                    // nowhere to put it.
                    drop(decode_output_arg(&mut columns, ALL_ARGS.as_bytes()));
                }
            }
        }
        self.columns = columns;
    }

    /// Upstream's `get_header`: the first row, and the widths it seeds.
    ///
    /// Only the `size` column is computed; every other caption is used as it
    /// stands. The two computed spellings are `1K-blocks` (the size *scaled*,
    /// so `-B 1000000` reads `1MB-blocks`) and, under `-P`, `1024-blocks` (the
    /// size in full).
    fn get_header(&mut self) {
        let mut row = Vec::new();
        for index in 0..self.columns.len() {
            let Some(col) = self.columns.get(index) else {
                continue;
            };
            let is_size = col.field == Field::Size;
            let scaled = is_size
                && (self.cfg.header_mode == HeaderMode::Default
                    || (self.cfg.header_mode == HeaderMode::Output
                        && !self.cfg.human_output_opts.has(Opts::AUTOSCALE)));
            let cell = if scaled {
                // The caption is *replaced*, not appended to: in `--output`
                // mode the column's caption is `Size`, and the header is still
                // `1K-blocks`.
                format!("{}-blocks", self.scaled_block_size()).into_bytes()
            } else if self.cfg.header_mode == HeaderMode::Posix && is_size {
                format!("{}-{}", self.cfg.output_block_size, col.caption).into_bytes()
            } else {
                col.caption.clone().into_bytes()
            };
            self.push_cell(&mut row, index, cell);
        }
        self.rows.push(row);
    }

    /// The block size as it appears in the `1K-blocks` header.
    ///
    /// The base is chosen by *exactness*, not by the option that set the size:
    /// both bases are divided out until one of them leaves a remainder, and
    /// whichever survived longer wins. `1048576` is `1M-blocks` because it is a
    /// power of 1024; `1000000` is `1MB-blocks` because it is a power of 1000;
    /// a size that is neither, like `1234`, keeps whatever base the options
    /// asked for. The `B` suffix is added whenever the base ends up as 1000,
    /// which is what distinguishes `1MB` from `1M`.
    fn scaled_block_size(&self) -> String {
        let mut opts = Opts::SUPPRESS_POINT_ZERO | Opts::AUTOSCALE | Opts::SI;
        if self.cfg.human_output_opts.has(Opts::GROUP_DIGITS) {
            opts = opts | Opts::GROUP_DIGITS;
        }
        let mut base_1024 = self.cfg.human_output_opts.has(Opts::BASE_1024);
        let mut suffix_b = self.cfg.human_output_opts.has(Opts::B);

        let mut q1000 = self.cfg.output_block_size;
        let mut q1024 = self.cfg.output_block_size;
        let (mut by_1000, mut by_1024);
        loop {
            by_1000 = q1000.is_multiple_of(1000);
            q1000 /= 1000;
            by_1024 = q1024.is_multiple_of(1024);
            q1024 /= 1024;
            if !(by_1000 && by_1024) {
                break;
            }
        }
        if !by_1000 && by_1024 {
            base_1024 = true;
        }
        if !by_1024 && by_1000 {
            base_1024 = false;
        }
        if !base_1024 {
            suffix_b = true;
        }
        if base_1024 {
            opts = opts | Opts::BASE_1024;
        }
        if suffix_b {
            opts = opts | Opts::B;
        }
        human_readable(self.cfg.output_block_size, opts, 1, 1)
    }

    /// Upstream's `add_to_grand_total`.
    ///
    /// The block counts are multiplied up to **bytes** before being summed,
    /// because the file systems being added together do not share a block size.
    /// The inode counts are summed as they are.
    fn add_to_grand_total(&mut self, block: &FieldValues, inode: &FieldValues) {
        if known_value(inode.total) {
            self.grand.files = self.grand.files.wrapping_add(inode.total);
        }
        if known_value(inode.available) {
            self.grand.ffree = self.grand.ffree.wrapping_add(inode.available);
        }
        if known_value(block.total) {
            self.grand.blocks = self
                .grand
                .blocks
                .wrapping_add(block.input_units.wrapping_mul(block.total));
        }
        if known_value(block.available_to_root) {
            self.grand.bfree = self
                .grand
                .bfree
                .wrapping_add(block.input_units.wrapping_mul(block.available_to_root));
        }
        if known_value(block.available) {
            let mut bavail = self.grand.bavail;
            let mut negative = self.grand.bavail_top_bit_set;
            add_uint_with_neg_flag(
                &mut bavail,
                &mut negative,
                block.input_units.wrapping_mul(block.available),
                block.negate_available,
            );
            self.grand.bavail = bavail;
            self.grand.bavail_top_bit_set = negative;
        }
    }

    /// Upstream's `get_dev`: measure one file system and append its row.
    ///
    /// Every early return is a row that is *not* printed, and each has its own
    /// reason: the type was filtered out, the mount point is relative, the file
    /// system is empty, or measuring it failed in a way that was reported
    /// instead.
    fn get_dev(&mut self, args: &DevArgs, err: &mut dyn Write) {
        if args.me_remote && self.cfg.show_local_fs {
            return;
        }
        if args.me_dummy && !self.cfg.show_all_fs && !self.show_listed_fs {
            return;
        }
        if !self.selected_fstype(args.fstype) || self.excluded_fstype(args.fstype) {
            return;
        }
        // A relative mount point is not a place: Linux writes them into
        // `/proc/mounts` for file systems mounted in another namespace, where
        // the path we can see is not the path they are attached at.
        if args.force_fsu.is_none() && args.mount_point.is_some_and(|mp| !mp.starts_with(b"/")) {
            return;
        }

        // `device` can only be `None` when `mount_point` is not, so the final
        // fallback is unreachable; it is spelled out rather than unwrapped.
        let stat_file: &[u8] = args
            .stat_file
            .or(args.mount_point)
            .or(args.device)
            .unwrap_or(b"");

        let mut fstype = args.fstype;
        let fsu = match args.force_fsu {
            Some(forced) => *forced,
            None => match self.sys.statvfs(stat_file) {
                Ok(usage) => self.check_over_mount(args, stat_file, usage, &mut fstype),
                Err(error) => {
                    let code = error.raw_os_error().unwrap_or(0);
                    if args.process_all && (code == EACCES || code == ENOENT) {
                        // The table said it was there and it was not, or it was
                        // there and we may not look. Neither is worth failing
                        // over when the user asked for *everything*; the row
                        // becomes placeholders, and only `-a` shows it at all.
                        if !self.cfg.show_all_fs {
                            return;
                        }
                        fstype = Some(b"-");
                        FsUsage::unknown()
                    } else {
                        let _ = writeln!(err, "df: {}: {}", quotef(stat_file), strerror(&error));
                        self.exit_status = 1;
                        return;
                    }
                }
            },
        };

        // An empty file system is one of the pseudo-file-systems in all but
        // name, and is hidden by the same rule.
        if fsu.blocks == 0 && !self.cfg.show_all_fs && !self.show_listed_fs {
            return;
        }
        if args.force_fsu.is_none() {
            self.processed = true;
        }

        let device = args.device.unwrap_or(b"-");
        let file = args.file.unwrap_or(b"-");
        let mount_point = args.mount_point.unwrap_or(b"-");
        let fstype = fstype.unwrap_or(b"-");

        // `/dev/disk/by-uuid/828fc648-…` is a name, but not one anybody wants
        // in a table; the symlink it points at is `/dev/sda1`. Only done when
        // walking the whole table, because a device the user *named* is printed
        // as the user named it.
        let mut dev_name = device.to_vec();
        if args.process_all
            && has_uuid_suffix(&dev_name)
            && let Some(resolved) = self.sys.canonicalize(&dev_name)
        {
            dev_name = resolved;
        }

        let (block, inode) = get_field_values(&fsu, self.cfg.output_block_size);
        // The total row is not added to the total.
        if self.cfg.print_grand_total && args.force_fsu.is_none() {
            self.add_to_grand_total(&block, &inode);
        }

        let opts = self.cfg.human_output_opts;
        let mut row = Vec::with_capacity(self.columns.len());
        for index in 0..self.columns.len() {
            let Some(col) = self.columns.get(index) else {
                continue;
            };
            let values = match col.kind {
                Kind::Block => Some(&block),
                Kind::Inode => Some(&inode),
                Kind::Other => None,
            };
            let cell: Vec<u8> = match col.field {
                Field::Source => dev_name.clone(),
                Field::FsType => fstype.to_vec(),
                Field::File => file.to_vec(),
                Field::Target => mount_point.to_vec(),
                Field::Size | Field::ITotal => number(values, |v| {
                    df_readable(false, v.total, opts, v.input_units, v.output_units)
                }),
                Field::Used | Field::IUsed => number(values, |v| {
                    df_readable(v.negate_used, v.used, opts, v.input_units, v.output_units)
                }),
                Field::Avail | Field::IAvail => number(values, |v| {
                    df_readable(
                        v.negate_available,
                        v.available,
                        opts,
                        v.input_units,
                        v.output_units,
                    )
                }),
                Field::Pcent | Field::IPcent => number(values, |v| {
                    percent(v).map_or_else(|| "-".to_string(), |pct| format!("{pct:.0}%"))
                }),
            };
            self.push_cell(&mut row, index, cell);
        }
        self.rows.push(row);
    }

    /// The over-mount check `get_dev` runs on a *successful* measurement.
    ///
    /// `df -a` lists every entry in the table, including one whose mount point
    /// has since had something else mounted over it. Measuring that mount point
    /// measures the *newer* file system, and printing those numbers under the
    /// older one's name would be simply wrong — so when the device the mount
    /// point actually resolves to is not the device this row is about, the
    /// numbers are dropped and the row prints placeholders.
    ///
    /// Two remote file systems are exempt: the same export can legitimately be
    /// reached under two host names, and neither is the over-mount of the other.
    fn check_over_mount<'b>(
        &self,
        args: &DevArgs<'b>,
        stat_file: &[u8],
        usage: FsUsage,
        fstype: &mut Option<&'b [u8]>,
    ) -> FsUsage {
        if !(args.process_all && self.cfg.show_all_fs) {
            return usage;
        }
        let Ok(sb) = self.sys.stat(stat_file) else {
            return usage;
        };
        let Some(dev_me) = self.devlist.get(&sb.dev) else {
            return usage;
        };
        if Some(dev_me.devname.as_slice()) != args.device && (!dev_me.remote || !args.me_remote) {
            *fstype = Some(b"-");
            return FsUsage::unknown();
        }
        usage
    }

    /// Upstream's `last_device_for_mount`: the source of the **last** entry in
    /// the table naming this mount point.
    ///
    /// Last, not first, because that is the one currently on top: the mount
    /// table is in mount order, so a later entry for the same directory is an
    /// over-mount of the earlier one.
    fn last_device_for_mount(&self, mount: &[u8]) -> Option<Vec<u8>> {
        let entry = self.mounts.iter().rev().find(|me| me.mountdir == mount)?;
        match self.sys.canonicalize(&entry.devname) {
            Some(canon) if canon.starts_with(b"/") => Some(canon),
            _ => Some(entry.devname.clone()),
        }
    }

    /// Upstream's `get_device`: `df /dev/sda1`.
    ///
    /// Returns whether the name was handled *as a device* — false means the
    /// caller should fall back to treating it as an ordinary file. Both
    /// outcomes of "handled" are true: a row was produced, or the device turned
    /// out to be over-mounted and was diagnosed.
    fn get_device(&mut self, device: &[u8], err: &mut dyn Write) -> bool {
        // The operand, as spelled, is what the `file` column shows.
        let file = device;
        let resolved = self
            .sys
            .canonicalize(device)
            .filter(|r| r.starts_with(b"/"));
        let device: &[u8] = resolved.as_deref().unwrap_or(device);

        let mut best: Option<usize> = None;
        let mut best_len = usize::MAX;
        let mut best_accessible = false;
        let mut eclipsed = false;

        for index in 0..self.mounts.len() {
            let Some(me) = self.mounts.get(index) else {
                continue;
            };
            let canon = self
                .sys
                .canonicalize(&me.devname)
                .filter(|c| c.starts_with(b"/"));
            let devname: &[u8] = canon.as_deref().unwrap_or(&me.devname);
            if device != devname {
                continue;
            }
            // Is something else now mounted where this entry says it is? If so
            // this entry is stale and cannot be reported on.
            eclipsed = self
                .last_device_for_mount(&me.mountdir)
                .is_some_and(|last| last != devname);
            let len = me.mountdir.len();
            if eclipsed || (best_accessible && len >= best_len) {
                continue;
            }
            let accessible = self.sys.stat(&me.mountdir).is_ok();
            if accessible {
                best_accessible = true;
            }
            // An inaccessible match can only win while *no* accessible one has
            // been found: the flag just set is read back deliberately.
            if accessible || (!best_accessible && len < best_len) {
                best = Some(index);
                if len == 1 {
                    // The root file system: nothing can be nearer the root, so
                    // no later entry can beat it.
                    break;
                }
                best_len = len;
            }
        }

        if let Some(entry) = best.and_then(|index| self.mounts.get(index)).cloned() {
            self.get_dev(
                &DevArgs {
                    device: Some(&entry.devname),
                    mount_point: Some(&entry.mountdir),
                    file: Some(file),
                    stat_file: None,
                    fstype: Some(&entry.fstype),
                    me_dummy: entry.dummy,
                    me_remote: entry.remote,
                    force_fsu: None,
                    process_all: false,
                },
                err,
            );
            return true;
        }
        if eclipsed {
            let _ = writeln!(
                err,
                "df: cannot access {}: over-mounted by another device",
                quoteaf(file)
            );
            self.exit_status = 1;
            return true;
        }
        false
    }

    /// Upstream's `get_point`: which file system is this file on?
    ///
    /// Two searches, in order. The first is textual — the longest mount point
    /// that is a prefix of the resolved name — and is preferred because it
    /// touches nothing: statting an unreachable network mount point can hang,
    /// and `df /tmp` has no business hanging on a dead NFS server elsewhere in
    /// the table. It is confirmed with a single `stat` of the winner, and only
    /// if that disagrees does the second search run, which stats mount points
    /// until it finds the matching device number.
    fn get_point(&mut self, point: &[u8], st: &StatInfo, err: &mut dyn Write) {
        let mut best: Option<usize> = None;

        if let Some(resolved) = self.sys.canonicalize(point).filter(|r| r.starts_with(b"/")) {
            let mut best_len = 0usize;
            for index in 0..self.mounts.len() {
                let Some(me) = self.mounts.get(index) else {
                    continue;
                };
                if me.fstype == b"lofs" || !self.beats_best(best, me) {
                    continue;
                }
                let len = me.mountdir.len();
                // A prefix, and a prefix that ends on a path component
                // boundary: `/var` is not a mount point of `/variable`. The
                // root is special-cased because it ends on no boundary at all.
                let prefix = len == 1
                    || ((len == resolved.len() || resolved.get(len) == Some(&b'/'))
                        && resolved.get(..len) == Some(me.mountdir.as_slice()));
                // `<=` and not `<`: a later entry with the same length wins,
                // because it is mounted later and therefore on top.
                if best_len <= len && len <= resolved.len() && prefix {
                    best = Some(index);
                    best_len = len;
                }
            }
        }

        // Confirm the textual winner really is the file system the file is on.
        if let Some(dir) = best
            .and_then(|i| self.mounts.get(i))
            .map(|m| m.mountdir.clone())
            && self.sys.stat(&dir).ok().map(|s| s.dev) != Some(st.dev)
        {
            best = None;
        }

        if best.is_none() {
            best = self.search_by_dev(st, err);
        }

        if let Some(entry) = best.and_then(|index| self.mounts.get(index)).cloned() {
            self.get_dev(
                &DevArgs {
                    device: Some(&entry.devname),
                    mount_point: Some(&entry.mountdir),
                    file: Some(point),
                    // The operand itself, not the mount point: on a network
                    // file system the answer can depend on where you ask.
                    stat_file: Some(point),
                    fstype: Some(&entry.fstype),
                    me_dummy: entry.dummy,
                    me_remote: entry.remote,
                    force_fsu: None,
                    process_all: false,
                },
                err,
            );
        } else if let Some(mount_point) = self.sys.find_mount_point(point, st) {
            // No entry in the table describes this file system — it is mounted
            // in a way the table does not record. Print what can still be
            // learned: the mount point found by walking up, and the usage
            // figures, with no source and no type.
            self.get_dev(
                &DevArgs {
                    device: None,
                    mount_point: Some(&mount_point),
                    file: Some(point),
                    stat_file: None,
                    fstype: None,
                    me_dummy: false,
                    me_remote: false,
                    force_fsu: None,
                    process_all: false,
                },
                err,
            );
        }
    }

    /// `get_point`'s tie-break: a real file system beats a dummy one, and
    /// anything beats nothing.
    fn beats_best(&self, best: Option<usize>, me: &MountEntry) -> bool {
        match best.and_then(|i| self.mounts.get(i)) {
            None => true,
            Some(current) => current.dummy || !me.dummy,
        }
    }

    /// `get_point`'s second search: stat mount points until one has the device
    /// number we are looking for.
    ///
    /// Each entry's device number is resolved at most once. A failure is
    /// remembered as [`DEV_BOGUS`] so that a mount point which cannot be
    /// statted is not statted again for the next operand — and only `EIO` is
    /// reported, because every other failure is consistent with the mount point
    /// being shadowed, which merely proves the file is not there.
    fn search_by_dev(&mut self, st: &StatInfo, err: &mut dyn Write) -> Option<usize> {
        let mut best: Option<usize> = None;
        for index in 0..self.mounts.len() {
            let Some((unknown, mountdir)) = self
                .mounts
                .get(index)
                .map(|m| (m.dev == DEV_UNKNOWN, m.mountdir.clone()))
            else {
                continue;
            };
            if unknown {
                let dev = match self.sys.stat(&mountdir) {
                    Ok(found) => found.dev,
                    Err(error) => {
                        if error.raw_os_error() == Some(EIO) {
                            let _ =
                                writeln!(err, "df: {}: {}", quotef(&mountdir), strerror(&error));
                            self.exit_status = 1;
                        }
                        DEV_BOGUS
                    }
                };
                if let Some(me) = self.mounts.get_mut(index) {
                    me.dev = dev;
                }
            }

            let Some(me) = self.mounts.get(index) else {
                continue;
            };
            if st.dev != me.dev || me.fstype == b"lofs" || !self.beats_best(best, me) {
                continue;
            }
            // The entry claims this device number; check that it still holds.
            // An `/etc/mtab` that outlived its mount says otherwise.
            let dev = me.dev;
            if self.sys.stat(&mountdir).ok().map(|s| s.dev) == Some(dev) {
                best = Some(index);
            } else if let Some(me) = self.mounts.get_mut(index) {
                me.dev = DEV_BOGUS;
            }
        }
        best
    }

    /// Upstream's `get_entry`: one operand.
    ///
    /// A block or character device is tried as a *device* first, so `df
    /// /dev/sda1` reports on what is mounted from it rather than on `/dev`.
    fn get_entry(&mut self, name: &[u8], st: &StatInfo, err: &mut dyn Write) {
        if st.special && self.get_device(name, err) {
            return;
        }
        self.get_point(name, st, err);
    }

    /// Upstream's `get_all_entries`: `df` with no operands.
    fn get_all_entries(&mut self, err: &mut dyn Write) {
        self.filter_mount_list(self.cfg.show_all_fs);
        // Cloned because `get_dev` needs `&mut self` for the table it is
        // appending to, and because `get_dev` can itself change `self.mounts`
        // through nothing — but the borrow checker cannot know that, and a
        // mount table is tens of entries.
        for entry in self.mounts.clone() {
            self.get_dev(
                &DevArgs {
                    device: Some(&entry.devname),
                    mount_point: Some(&entry.mountdir),
                    file: None,
                    stat_file: None,
                    fstype: Some(&entry.fstype),
                    me_dummy: entry.dummy,
                    me_remote: entry.remote,
                    force_fsu: None,
                    process_all: true,
                },
                err,
            );
        }
    }

    /// Upstream's `filter_mount_list`: one row per device.
    ///
    /// A device can appear in the table many times — a bind mount, a mount
    /// point that was mounted over, the same file system mounted twice — and
    /// printing its free space once per appearance is both noise and, under
    /// `--total`, wrong. The rules for which appearance survives, in the order
    /// they are tried:
    ///
    /// 1. Two *different* remote sources for one device are both kept: the same
    ///    export reached under two host names is two things the user asked for.
    ///    Not done under `--total`, where double-counting is the worse error.
    /// 2. A source with a `/` in it beats one without: `/dev/sda1` is a device,
    ///    `tmpfs` is a word.
    /// 3. The mount point nearer the root wins, unless the *source* subtree of
    ///    the contender is nearer the root of its own device.
    /// 4. A different source on the same mount point wins — that is an
    ///    over-mount, and the newer one is what is there now.
    ///
    /// With `devices_only` — `df -a`, where nothing is filtered out — only the
    /// device-to-entry map is built, for `get_dev`'s over-mount check.
    fn filter_mount_list(&mut self, devices_only: bool) {
        // The entry kept for each device, in table order, with the device
        // number it was filed under.
        let mut kept: Vec<(MountEntry, u64)> = Vec::new();
        // Device number to its index in `kept` — upstream's `seen_last`, which
        // is why a later entry replaces an earlier one rather than adding to it.
        let mut seen: HashMap<u64, usize> = HashMap::new();

        let list = std::mem::take(&mut self.mounts);
        // With `devices_only` the table is *not* filtered — `df -a` prints
        // every entry — so the original list is put straight back and only the
        // device map is a product of the loop.
        if devices_only {
            self.mounts.clone_from(&list);
        }

        for me in list {
            // A file system this run will not print is not statted: statting a
            // remote mount point can hang, and `-l` exists precisely to avoid
            // that. Such an entry is still carried, so that a later diagnostic
            // can name it.
            let filtered = (me.remote && self.cfg.show_local_fs)
                || (me.dummy && !self.cfg.show_all_fs && !self.show_listed_fs)
                || !self.selected_fstype(Some(&me.fstype))
                || self.excluded_fstype(Some(&me.fstype));
            let stat_dev = if filtered {
                None
            } else {
                self.sys.stat(&me.mountdir).ok().map(|s| s.dev)
            };

            // `Some((index, true))` replaces the entry at `index`;
            // `Some((index, false))` discards this one.
            let mut verdict: Option<(usize, bool)> = None;
            if let Some(dev) = stat_dev
                && let Some((index, existing)) = seen
                    .get(&dev)
                    .and_then(|&i| kept.get(i).map(|(e, _)| (i, e)))
            {
                let both_remote_elsewhere = !self.cfg.print_grand_total
                    && me.remote
                    && existing.remote
                    && existing.devname != me.devname;
                if !both_remote_elsewhere {
                    let target_nearer_root = existing.mountdir.len() > me.mountdir.len();
                    let source_below_root = match (&existing.mntroot, &me.mntroot) {
                        (Some(seen_root), Some(new_root)) => seen_root.len() < new_root.len(),
                        _ => false,
                    };
                    let replace = (me.devname.contains(&b'/') && !existing.devname.contains(&b'/'))
                        || (target_nearer_root && !source_below_root)
                        || (existing.devname != me.devname && me.mountdir == existing.mountdir);
                    verdict = Some((index, replace));
                }
            }

            match verdict {
                Some((index, true)) => {
                    if let Some(slot) = kept.get_mut(index) {
                        slot.0 = me;
                    }
                }
                Some((_, false)) => {}
                None => {
                    let dev = stat_dev.unwrap_or(me.dev);
                    kept.push((me, dev));
                    // Not `entry().or_insert`: a *later* entry for the same
                    // device is the one subsequent lookups must find.
                    seen.insert(dev, kept.len().saturating_sub(1));
                }
            }
        }

        if devices_only {
            self.devlist = seen
                .into_iter()
                .filter_map(|(dev, index)| kept.get(index).map(|(me, _)| (dev, me.clone())))
                .collect();
        } else {
            self.mounts = kept.into_iter().map(|(me, _)| me).collect();
        }
    }

    /// Upstream's `print_table`: every row, padded to the widths the cells grew.
    ///
    /// # Errors
    ///
    /// Whatever writing to `out` failed with.
    fn print_table(&self, out: &mut dyn Write) -> io::Result<()> {
        let last = self.columns.len().saturating_sub(1);
        let mut line = Vec::new();
        for row in &self.rows {
            line.clear();
            for (index, cell) in row.iter().enumerate() {
                if index != 0 {
                    line.push(b' ');
                }
                let Some(col) = self.columns.get(index) else {
                    continue;
                };
                align_cell(&mut line, cell, col.width, col.align, index == last);
            }
            line.push(b'\n');
            out.write_all(&line)?;
        }
        Ok(())
    }
}

/// A numeric cell: the value group is `Some` for every field that has one, so
/// the `None` arm is unreachable and returns the same `-` an unknown value
/// would.
fn number(values: Option<&FieldValues>, render: impl Fn(&FieldValues) -> String) -> Vec<u8> {
    values.map_or_else(|| b"-".to_vec(), |v| render(v).into_bytes())
}

/// Upstream's `main` from the end of the option loop onwards, with the system
/// behind a trait so that the whole of it is testable.
///
/// Returns the exit status. The order of the first three steps is upstream's
/// and is not arbitrary: the operands are statted **before** the mount table is
/// read, because statting a path under an automounter is what causes it to be
/// mounted, and a file system mounted after the table was read is not in it.
fn run<S: System + ?Sized>(
    cfg: &Settings,
    sys: &S,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let mut df = Df::new(cfg, sys);

    let mut targets: Vec<(Vec<u8>, StatInfo)> = Vec::new();
    for name in &cfg.operands {
        match sys.automount_stat(name) {
            Ok(st) => targets.push((name.clone(), st)),
            Err(error) => {
                let _ = writeln!(err, "df: {}: {}", quotef(name), strerror(&error));
                df.exit_status = 1;
            }
        }
    }

    // Reading the table is fatal when `df` was given nothing else to go on, or
    // when an option was given that only means anything against the table. With
    // operands and no such option there is still a useful answer, so it is a
    // warning and the run continues.
    //
    // One divergence: gnulib cannot distinguish an empty table from a failed
    // read, because both are a null list. Here a successful read of an empty
    // table falls through to `no file systems processed`, which is the more
    // accurate of the two diagnostics for a case neither of us can reach.
    match sys.mounts() {
        Ok(mounts) => df.mounts = mounts,
        Err(error) => {
            let fatal = cfg.operands.is_empty()
                || cfg.show_all_fs
                || cfg.show_local_fs
                || !cfg.fs_select.is_empty()
                || !cfg.fs_exclude.is_empty();
            let warning = if fatal { "" } else { "Warning: " };
            let _ = writeln!(
                err,
                "df: {warning}cannot read table of mounted file systems: {}",
                strerror(&error)
            );
            if fatal {
                return 1;
            }
        }
    }

    if cfg.require_sync {
        sys.sync();
    }

    df.field_list();
    df.get_header();

    if cfg.operands.is_empty() {
        df.get_all_entries(err);
    } else {
        // Named file systems are listed even when empty, and even when they are
        // the pseudo-file-systems `df` otherwise hides. Note this is reached
        // even when *every* operand failed to stat, which is why a run of only
        // bad operands prints a header and no rows rather than the whole table.
        df.show_listed_fs = true;
        for (name, st) in &targets {
            df.get_entry(name, st, err);
        }
    }

    if df.processed {
        if cfg.print_grand_total {
            // The word `total` goes in the source column when there is one, and
            // in the mount-point column when there is not — so it is visible in
            // every set of columns that has either.
            let has_source = df.columns.iter().any(|c| c.field == Field::Source);
            let mount_point: &[u8] = if has_source { b"-" } else { b"total" };
            let grand = df.grand;
            df.get_dev(
                &DevArgs {
                    device: Some(b"total"),
                    mount_point: Some(mount_point),
                    file: None,
                    stat_file: None,
                    fstype: None,
                    me_dummy: false,
                    me_remote: false,
                    force_fsu: Some(&grand),
                    process_all: false,
                },
                err,
            );
        }
        if df.print_table(out).is_err() {
            return 1;
        }
    } else if df.exit_status == 0 {
        // Only when nothing else was said: if every file system was excluded by
        // an option the user gave, that is the answer, but if one of them failed
        // then the failure has already been reported and this would bury it.
        let _ = writeln!(err, "df: no file systems processed");
        return 1;
    }

    df.exit_status
}

// ------------------------------------------------------------------- main ---

#[cfg(not(unix))]
fn main() -> ExitCode {
    diag!("df: unix-only utility; not supported on this platform");
    ExitCode::from(1)
}

/// The funnel. A diagnostic that could not be written turns the earned status
/// into failure, which is what upstream's `atexit (close_stdout)` does on every
/// exit path at once. See [`stdfd::close_stderr`].
#[cfg(unix)]
fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

#[cfg(unix)]
fn run_main() -> ExitCode {
    let argv: Vec<OsString> = std::env::args_os().skip(1).collect();
    let env = Environment {
        df_block_size: std::env::var_os("DF_BLOCK_SIZE").map(|v| os_bytes(&v).into_owned()),
        block_size: std::env::var_os("BLOCK_SIZE").map(|v| os_bytes(&v).into_owned()),
        blocksize: std::env::var_os("BLOCKSIZE").map(|v| os_bytes(&v).into_owned()),
        posixly_correct: std::env::var_os("POSIXLY_CORRECT").is_some(),
    };

    let request = match parse_args(&argv, &env) {
        Ok(request) => request,
        Err(refusal) => {
            // `Stream` and not `io::stderr()`, whose failures the runtime hides:
            // a diagnostic that never arrived has to reach `close_stderr`'s flag.
            refusal.print(&mut Stream::stderr());
            return ExitCode::from(u8::try_from(refusal.status).unwrap_or(1));
        }
    };

    let cfg = match request {
        Request::Help => {
            print!("{}", help_text());
            return ExitCode::SUCCESS;
        }
        Request::Version => {
            println!("df (SlateOS coreutils) 0.1.0");
            return ExitCode::SUCCESS;
        }
        Request::Run(cfg) => cfg,
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut err = Stream::stderr();
    let status = run(&cfg, &RealSystem, &mut out, &mut err);
    if out.flush().is_err() {
        return ExitCode::from(1);
    }
    ExitCode::from(u8::try_from(status).unwrap_or(1))
}

// ------------------------------------------------------------------ tests ---

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "panicking on bad data is the point of a test"
)]
mod tests {
    use super::*;
    use std::cell::Cell;

    // ------------------------------------------------------------- fakes ---

    /// A machine assembled for one test.
    #[derive(Default)]
    struct FakeSystem {
        mounts: Vec<MountEntry>,
        /// Make [`System::mounts`] fail, for the two diagnostics that only
        /// appear when the table cannot be read.
        mounts_fail: bool,
        usage: HashMap<Vec<u8>, FsUsage>,
        stats: HashMap<Vec<u8>, StatInfo>,
        /// Symlink targets, for `canonicalize`. A name that is not here
        /// resolves to itself if it is absolute.
        links: HashMap<Vec<u8>, Vec<u8>>,
        tty: bool,
        synced: Cell<bool>,
    }

    /// Is `file` on the file system mounted at `dir`, ignoring any mount
    /// nested between them? Component-boundary aware, so `/var` does not
    /// contain `/variable`.
    fn under(dir: &[u8], file: &[u8]) -> bool {
        dir == b"/"
            || (file.get(..dir.len()) == Some(dir)
                && (file.len() == dir.len() || file.get(dir.len()) == Some(&b'/')))
    }

    impl System for FakeSystem {
        fn mounts(&self) -> io::Result<Vec<MountEntry>> {
            if self.mounts_fail {
                return Err(io::Error::from_raw_os_error(ENOENT));
            }
            Ok(self.mounts.clone())
        }

        fn statvfs(&self, file: &[u8]) -> io::Result<FsUsage> {
            if let Some(usage) = self.usage.get(file) {
                return Ok(*usage);
            }
            // A real `statvfs` answers for *any* file on the file system, not
            // just its mount point — which is what `get_point` relies on, since
            // it asks about the operand rather than about the mount. A file the
            // fake has never heard of is still absent.
            if self.stats.contains_key(file)
                && let Some(usage) = self
                    .mounts
                    .iter()
                    .filter(|m| under(&m.mountdir, file))
                    .max_by_key(|m| m.mountdir.len())
                    .and_then(|m| self.usage.get(&m.mountdir))
            {
                return Ok(*usage);
            }
            Err(io::Error::from_raw_os_error(ENOENT))
        }

        fn stat(&self, file: &[u8]) -> io::Result<StatInfo> {
            self.stats
                .get(file)
                .copied()
                .ok_or_else(|| io::Error::from_raw_os_error(ENOENT))
        }

        fn automount_stat(&self, file: &[u8]) -> io::Result<StatInfo> {
            self.stat(file)
        }

        fn canonicalize(&self, file: &[u8]) -> Option<Vec<u8>> {
            if let Some(target) = self.links.get(file) {
                return Some(target.clone());
            }
            file.starts_with(b"/").then(|| file.to_vec())
        }

        fn sync(&self) {
            self.synced.set(true);
        }

        fn stdout_is_tty(&self) -> bool {
            self.tty
        }
    }

    impl FakeSystem {
        /// A mount, its usage figures and the `stat` of its mount point, in one
        /// call: `dev` doubles as the mount point's `st_dev`.
        #[expect(clippy::too_many_arguments, reason = "a whole file system in one call")]
        fn mount(
            &mut self,
            source: &str,
            dir: &str,
            fstype: &str,
            dev: u64,
            blocks: u64,
            bfree: u64,
            bavail: u64,
        ) -> &mut Self {
            self.mounts.push(MountEntry {
                devname: source.as_bytes().to_vec(),
                mountdir: dir.as_bytes().to_vec(),
                mntroot: Some(b"/".to_vec()),
                fstype: fstype.as_bytes().to_vec(),
                dummy: me_dummy(fstype.as_bytes(), false),
                remote: me_remote(source.as_bytes(), fstype.as_bytes()),
                dev: DEV_UNKNOWN,
            });
            self.usage.insert(
                dir.as_bytes().to_vec(),
                FsUsage {
                    blocksize: 1024,
                    blocks,
                    bfree,
                    bavail,
                    bavail_top_bit_set: false,
                    files: 100,
                    ffree: 40,
                },
            );
            self.stats.insert(
                dir.as_bytes().to_vec(),
                StatInfo {
                    dev,
                    special: false,
                    dir: true,
                },
            );
            self
        }
    }

    /// An environment with nothing set but `POSIXLY_CORRECT`.
    fn env(posixly_correct: bool) -> Environment {
        Environment {
            df_block_size: None,
            block_size: None,
            blocksize: None,
            posixly_correct,
        }
    }

    /// Parse a command line the way `run_main` does, with an empty environment.
    fn settings(args: &[&str]) -> Settings {
        settings_in(args, &env(false))
    }

    /// The same, with `POSIXLY_CORRECT` set.
    fn settings_posix(args: &[&str]) -> Settings {
        settings_in(args, &env(true))
    }

    fn settings_in(args: &[&str], env: &Environment) -> Settings {
        let argv: Vec<OsString> = args.iter().map(OsString::from).collect();
        match parse_args(&argv, env) {
            Ok(Request::Run(cfg)) => *cfg,
            Ok(other) => panic!("expected a run, got {other:?}"),
            Err(refusal) => panic!("expected a run, got {refusal:?}"),
        }
    }

    /// The refusal a command line earns.
    fn refusal(args: &[&str]) -> Refusal {
        let argv: Vec<OsString> = args.iter().map(OsString::from).collect();
        match parse_args(&argv, &env(false)) {
            Err(refusal) => refusal,
            Ok(other) => panic!("expected a refusal, got {other:?}"),
        }
    }

    /// A whole run: stdout, stderr, status.
    fn go(args: &[&str], sys: &FakeSystem) -> (String, String, i32) {
        go_with(&settings(args), sys)
    }

    /// The same, with `POSIXLY_CORRECT` set.
    fn go_posix(args: &[&str], sys: &FakeSystem) -> (String, String, i32) {
        go_with(&settings_posix(args), sys)
    }

    fn go_with(cfg: &Settings, sys: &FakeSystem) -> (String, String, i32) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let status = run(cfg, sys, &mut out, &mut err);
        (
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
            status,
        )
    }

    /// One file system on `/`, with round numbers.
    fn one_disk() -> FakeSystem {
        let mut sys = FakeSystem::default();
        sys.mount("/dev/sda1", "/", "ext4", 2049, 1000, 400, 300);
        sys
    }

    // ------------------------------------------------------------ tables ---

    #[test]
    fn fields_table_is_complete() {
        // Every variant has a row, and no row is a duplicate.
        for spec in FIELDS {
            assert_eq!(spec.field.spec().arg, spec.arg);
            assert_eq!(Field::from_arg(spec.arg), Some(spec.field));
        }
        assert_eq!(FIELDS.len(), 12);
    }

    #[test]
    fn all_args_matches_table() {
        let joined = FIELDS
            .iter()
            .map(|s| String::from_utf8_lossy(s.arg).into_owned())
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(joined, ALL_ARGS);
    }

    // ------------------------------------------------------- mount tables ---

    #[test]
    fn mountinfo_is_parsed() {
        let text = b"22 28 0:21 / /sys rw,nosuid - sysfs sysfs rw\n\
                     31 22 0:26 / /home/a\\040b rw,relatime - ext4 /dev/sda2 rw\n\
                     36 22 8:1 /sub /mnt rw - ext4 /dev/sda1 rw\n";
        let list = parse_mountinfo(text);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].fstype, b"sysfs");
        assert!(list[0].dummy, "sysfs is a dummy file system");
        assert_eq!(list[0].dev, makedev(0, 21));
        // The octal escape is the kernel's, and a space in a mount point is not
        // a reason to lose the entry.
        assert_eq!(list[1].mountdir, b"/home/a b");
        assert_eq!(list[1].devname, b"/dev/sda2");
        assert_eq!(list[1].dev, makedev(0, 26));
        assert_eq!(list[2].mntroot.as_deref(), Some(&b"/sub"[..]));
        assert_eq!(list[2].dev, makedev(8, 1));
    }

    #[test]
    fn mountinfo_survives_an_undecodable_mount_point() {
        // \377 is not valid UTF-8 anywhere. The entry must still be listed.
        let text = b"31 22 0:26 / /mnt/\\377 rw - ext4 /dev/sdb1 rw\n";
        let list = parse_mountinfo(text);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].mountdir, b"/mnt/\xff");
    }

    #[test]
    fn mtab_is_parsed() {
        let text = b"/dev/sda1 / ext4 rw,relatime 0 1\n\
                     # a comment\n\
                     server:/export /net nfs rw 0 0\n\
                     /dev/sdb1 /a\\040b ext4 rw 0 0\n";
        let list = parse_mounts(text);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].mountdir, b"/");
        assert!(!list[0].remote);
        assert!(list[1].remote, "a host:path source is remote");
        assert_eq!(list[2].mountdir, b"/a b");
        // No device numbers are available from this format.
        assert!(list.iter().all(|m| m.dev == DEV_UNKNOWN));
    }

    #[test]
    fn bind_mounts_escape_the_dummy_rule_only_in_mtab() {
        // A static `mtab` writes a bind mount as type `none`, so the `bind`
        // mount option is the only thing that saves it from the dummy rule.
        assert!(me_dummy(b"none", false), "the 'none' type is a dummy");
        assert!(
            !me_dummy(b"none", true),
            "unless it says it is a bind mount"
        );
        // `mountinfo` names the real type, so it passes `bind` as false and the
        // flag can never change the answer there.
        assert!(!me_dummy(b"ext4", false));
        assert!(!me_dummy(b"ext4", true));
        assert!(me_dummy(b"proc", false));
    }

    #[test]
    fn remote_file_systems_are_recognised() {
        assert!(me_remote(b"server:/export", b"nfs"));
        assert!(me_remote(b"//server/share", b"cifs"));
        assert!(
            !me_remote(b"//server/share", b"ext4"),
            "the type matters too"
        );
        assert!(me_remote(b"-hosts", b"autofs"));
        assert!(!me_remote(b"/dev/sda1", b"ext4"));
    }

    #[test]
    fn octal_escapes_are_decoded() {
        assert_eq!(unescape_tab(b"a\\040b"), b"a b");
        assert_eq!(unescape_tab(b"\\134"), b"\\");
        // Not an octal escape: the backslash and the digits stand.
        assert_eq!(unescape_tab(b"\\9"), b"\\9");
        assert_eq!(unescape_tab(b"\\4x"), b"\\4x");
        // glibc's grammar knows only four escapes.
        assert_eq!(decode_name(b"a\\040b\\011c"), b"a b\tc");
        assert_eq!(decode_name(b"\\055"), b"\\055");
    }

    #[test]
    fn device_numbers_are_packed_the_way_glibc_packs_them() {
        assert_eq!(makedev(8, 1), 0x801);
        assert_eq!(makedev(0, 21), 0x15);
        // A minor above 255 spills past the major.
        assert_eq!(
            makedev(1, 300),
            (300 & 0xff) | (1 << 8) | ((300 & !0xff) << 12)
        );
    }

    // ------------------------------------------------------------ numbers ---

    #[test]
    fn unknown_values_render_as_a_dash() {
        assert_eq!(df_readable(false, u64::MAX, Opts::NONE, 1, 1), "-");
        assert_eq!(df_readable(false, u64::MAX - 1, Opts::NONE, 1, 1), "-");
        assert_eq!(
            df_readable(false, u64::MAX - 2, Opts::NONE, 1, 1),
            "18446744073709551613"
        );
    }

    #[test]
    fn negative_available_space_keeps_its_sign() {
        // Twelve blocks overdrawn: the magnitude is stored negated.
        let text = df_readable(true, 12u64.wrapping_neg(), Opts::NONE, 1024, 1024);
        assert_eq!(text, "-12");
    }

    #[test]
    fn percentages_round_up() {
        let v = FieldValues {
            input_units: 1,
            output_units: 1,
            total: 900,
            available: 300,
            negate_available: false,
            available_to_root: 300,
            used: 600,
            negate_used: false,
        };
        // 600 / 900 is 66.67, and df never rounds a used percentage down.
        assert_eq!(percent(&v), Some(67.0));
    }

    #[test]
    fn an_empty_file_system_has_no_percentage() {
        let v = FieldValues {
            used: 0,
            available: 0,
            ..FieldValues::default()
        };
        assert_eq!(percent(&v), None);
    }

    #[test]
    fn an_unknown_count_has_no_percentage() {
        let v = FieldValues {
            used: u64::MAX,
            available: 10,
            ..FieldValues::default()
        };
        assert_eq!(percent(&v), None);
    }

    #[test]
    fn uuid_suffixes_are_recognised() {
        assert!(has_uuid_suffix(
            b"/dev/disk/by-uuid/828fc648-9f30-43d8-a0b1-f7196a2edb66"
        ));
        assert!(!has_uuid_suffix(b"/dev/sda1"));
    }

    // -------------------------------------------------------------- cells ---

    #[test]
    fn control_characters_are_replaced() {
        let mut cell = b"a\tb\nc\x7f".to_vec();
        replace_control_chars(&mut cell);
        assert_eq!(cell, b"a?b?c?");
    }

    #[test]
    fn undecodable_bytes_become_one_question_mark_each() {
        // Three bad bytes, three question marks — not one per character.
        assert_eq!(replace_invalid_chars(b"a\xff\xfe\xfdb"), b"a???b");
        assert_eq!(replace_invalid_chars("aé".as_bytes()), "aé".as_bytes());
    }

    #[test]
    fn width_is_measured_in_columns() {
        assert_eq!(mbswidth(b"abc"), 3);
        // A wide character is two columns, and a combining mark is none.
        assert_eq!(mbswidth("\u{4e00}".as_bytes()), 2);
        assert_eq!(mbswidth("e\u{301}".as_bytes()), 1);
        // An undecodable byte still occupies the terminal.
        assert_eq!(mbswidth(b"a\xffb"), 3);
    }

    #[test]
    fn cells_are_padded_to_their_column() {
        let mut out = Vec::new();
        align_cell(&mut out, b"ab", 5, Align::Right, false);
        assert_eq!(out, b"   ab");
        out.clear();
        align_cell(&mut out, b"ab", 5, Align::Left, false);
        assert_eq!(out, b"ab   ");
        out.clear();
        align_cell(&mut out, b"ab", 5, Align::Left, true);
        assert_eq!(out, b"ab", "the last column is not right-padded");
        out.clear();
        // A cell that does not decode is printed as it is, unpadded, which is
        // what gnulib's ambsalign does when mbstowcs fails.
        align_cell(&mut out, b"a\xffb", 8, Align::Right, false);
        assert_eq!(out, b"a\xffb");
    }

    // ------------------------------------------------------------ parsing ---

    #[test]
    fn the_default_block_size_is_1024() {
        let cfg = settings(&[]);
        assert_eq!(cfg.output_block_size, 1024);
        assert_eq!(cfg.header_mode, HeaderMode::Default);
    }

    #[test]
    fn autoscaling_selects_the_human_header() {
        assert_eq!(settings(&["-h"]).header_mode, HeaderMode::Human);
        assert_eq!(settings(&["-H"]).header_mode, HeaderMode::Human);
        assert_eq!(settings(&["--si"]).header_mode, HeaderMode::Human);
        // A fixed block size does not.
        assert_eq!(settings(&["-B", "1M"]).header_mode, HeaderMode::Default);
        assert_eq!(settings(&["-k"]).output_block_size, 1024);
        assert_eq!(settings(&["-m"]).output_block_size, 1024 * 1024);
    }

    #[test]
    fn inodes_and_output_beat_posix() {
        assert_eq!(settings(&["-P"]).header_mode, HeaderMode::Posix);
        assert_eq!(settings(&["-i", "-P"]).header_mode, HeaderMode::Inodes);
    }

    #[test]
    fn posix_output_is_512_byte_blocks_only_under_posixly_correct() {
        assert_eq!(settings(&["-P"]).output_block_size, 1024);
        assert_eq!(settings_posix(&["-P"]).output_block_size, 512);
        // An explicit block size wins over both.
        assert_eq!(
            settings_posix(&["-P", "-B", "1M"]).output_block_size,
            1024 * 1024
        );
    }

    #[test]
    fn output_is_exclusive_with_the_mode_options_either_way_round() {
        for pair in [
            ["-i", "--output"],
            ["--output", "-i"],
            ["-T", "--output"],
            ["--output", "-T"],
            ["-P", "--output"],
            ["--output", "-P"],
        ] {
            let text = refusal(&pair).lines.join("\n");
            assert!(
                text.contains("mutually exclusive"),
                "{pair:?} produced {text}"
            );
        }
    }

    #[test]
    fn output_rejects_an_unknown_or_repeated_field() {
        let text = refusal(&["--output=nosuch"]).lines.join("\n");
        assert!(
            text.contains("field \u{2018}nosuch\u{2019} unknown"),
            "{text}"
        );
        let text = refusal(&["--output=size,size"]).lines.join("\n");
        assert!(text.contains("used more than once"), "{text}");
    }

    #[test]
    fn a_type_cannot_be_both_selected_and_excluded() {
        let refusal = refusal(&["-t", "ext4", "-x", "ext4"]);
        assert!(
            refusal.lines[0].contains("both selected and excluded"),
            "{:?}",
            refusal.lines
        );
        assert!(!refusal.referral, "this one does not go through usage");
    }

    #[test]
    fn operands_are_kept_as_bytes() {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let argv = vec![OsString::from_vec(b"/mnt/\xff".to_vec())];
            let env = Environment {
                df_block_size: None,
                block_size: None,
                blocksize: None,
                posixly_correct: false,
            };
            match parse_args(&argv, &env) {
                Ok(Request::Run(cfg)) => assert_eq!(cfg.operands, vec![b"/mnt/\xff".to_vec()]),
                other => panic!("expected a run, got {other:?}"),
            }
        }
    }

    // -------------------------------------------------------------- table ---

    #[test]
    fn the_default_table_is_six_columns() {
        let (out, err, status) = go(&[], &one_disk());
        assert_eq!(err, "");
        assert_eq!(status, 0);
        assert_eq!(
            out,
            "Filesystem 1K-blocks Used Available Use% Mounted on\n\
             /dev/sda1       1000  600       300  67% /\n"
        );
    }

    #[test]
    fn the_block_size_names_the_column() {
        let (out, _, _) = go(&["-B", "1M"], &one_disk());
        assert!(out.starts_with("Filesystem 1M-blocks"), "{out}");
        let (out, _, _) = go(&["-B", "1MB"], &one_disk());
        assert!(out.starts_with("Filesystem 1MB-blocks"), "{out}");
        // `-B K` is a *unit*, not a count: the header is 1K-blocks either way,
        // but `-P` prints the size in full.
        let (out, _, _) = go(&["-B", "K"], &one_disk());
        assert!(out.starts_with("Filesystem 1K-blocks"), "{out}");
        // POSIX mode names its block size in full — 1024 bytes here, and 512
        // only when POSIXLY_CORRECT insists on the older unit.
        let (out, _, _) = go(&["-P"], &one_disk());
        assert!(out.starts_with("Filesystem 1024-blocks"), "{out}");
        let (out, _, _) = go_posix(&["-P"], &one_disk());
        assert!(out.starts_with("Filesystem 512-blocks"), "{out}");
    }

    #[test]
    fn posix_renames_the_percentage_column() {
        let (out, _, _) = go(&["-P"], &one_disk());
        assert!(out.contains("Capacity"), "{out}");
    }

    #[test]
    fn the_type_column_is_opt_in() {
        let (out, _, _) = go(&["-T"], &one_disk());
        assert!(out.starts_with("Filesystem Type"), "{out}");
        assert!(out.contains("ext4"), "{out}");
    }

    #[test]
    fn inodes_replace_the_block_columns() {
        let (out, _, _) = go(&["-i"], &one_disk());
        assert_eq!(
            out,
            "Filesystem Inodes IUsed IFree IUse% Mounted on\n\
             /dev/sda1     100    60    40   60% /\n"
        );
    }

    #[test]
    fn output_names_its_own_columns_in_its_own_order() {
        let (out, _, _) = go(&["--output=target,source"], &one_disk());
        assert_eq!(
            out,
            "Mounted on Filesystem\n\
             /          /dev/sda1\n"
        );
    }

    #[test]
    fn output_with_no_list_is_every_column() {
        let (out, _, _) = go(&["--output"], &one_disk());
        let header = out.lines().next().unwrap();
        for caption in [
            "Filesystem",
            "Type",
            "Inodes",
            "IUsed",
            "IFree",
            "IUse%",
            "1K-blocks",
            "Used",
            "Avail",
            "Use%",
            "File",
            "Mounted on",
        ] {
            assert!(header.contains(caption), "{caption} missing from {header}");
        }
    }

    #[test]
    fn the_file_column_shows_the_operand_as_given() {
        let mut sys = one_disk();
        sys.stats.insert(
            b"/home".to_vec(),
            StatInfo {
                dev: 2049,
                special: false,
                dir: true,
            },
        );
        let (out, err, status) = go(&["--output=file,target", "/home"], &sys);
        assert_eq!(err, "");
        assert_eq!(status, 0);
        assert_eq!(out, "File  Mounted on\n/home /\n");
    }

    #[test]
    fn a_total_row_is_added_last() {
        let mut sys = FakeSystem::default();
        sys.mount("/dev/sda1", "/", "ext4", 2049, 1000, 400, 300);
        sys.mount("/dev/sdb1", "/data", "ext4", 2050, 2000, 900, 800);
        let (out, _, _) = go(&["--total"], &sys);
        let last = out.lines().last().unwrap();
        assert!(last.starts_with("total"), "{out}");
        // 1000 + 2000 blocks, 600 + 1100 used, 300 + 800 available.
        assert!(last.contains("3000"), "{out}");
        assert!(last.contains("1700"), "{out}");
        assert!(last.contains("1100"), "{out}");
    }

    #[test]
    fn the_total_row_moves_when_there_is_no_source_column() {
        let (out, _, _) = go(&["--total", "--output=target,size"], &one_disk());
        assert!(out.lines().last().unwrap().starts_with("total"), "{out}");
    }

    // ---------------------------------------------------------- filtering ---

    #[test]
    fn pseudo_file_systems_are_hidden_unless_asked_for() {
        let mut sys = FakeSystem::default();
        sys.mount("/dev/sda1", "/", "ext4", 2049, 1000, 400, 300);
        sys.mount("proc", "/proc", "proc", 4, 0, 0, 0);
        let (out, _, _) = go(&[], &sys);
        assert!(!out.contains("/proc"), "{out}");
        let (out, _, _) = go(&["-a"], &sys);
        assert!(out.contains("/proc"), "{out}");
    }

    #[test]
    fn an_empty_file_system_is_hidden_unless_named() {
        let mut sys = FakeSystem::default();
        sys.mount("/dev/sda1", "/", "ext4", 2049, 1000, 400, 300);
        sys.mount("/dev/sdb1", "/empty", "ext4", 2050, 0, 0, 0);
        let (out, _, _) = go(&[], &sys);
        assert!(!out.contains("/empty"), "{out}");
        // Named explicitly, it is listed.
        let (out, err, status) = go(&["/empty"], &sys);
        assert_eq!((err.as_str(), status), ("", 0));
        assert!(out.contains("/empty"), "{out}");
    }

    #[test]
    fn types_can_be_selected_and_excluded() {
        let mut sys = FakeSystem::default();
        sys.mount("/dev/sda1", "/", "ext4", 2049, 1000, 400, 300);
        sys.mount("/dev/sdb1", "/data", "xfs", 2050, 2000, 900, 800);
        let (out, _, _) = go(&["-t", "xfs"], &sys);
        assert!(out.contains("/data") && !out.contains("/dev/sda1"), "{out}");
        let (out, _, _) = go(&["-x", "xfs"], &sys);
        assert!(!out.contains("/data") && out.contains("/dev/sda1"), "{out}");
    }

    #[test]
    fn remote_file_systems_are_hidden_by_local() {
        let mut sys = FakeSystem::default();
        sys.mount("/dev/sda1", "/", "ext4", 2049, 1000, 400, 300);
        sys.mount("server:/export", "/net", "nfs", 2051, 500, 100, 100);
        let (out, _, _) = go(&[], &sys);
        assert!(out.contains("/net"), "{out}");
        let (out, _, _) = go(&["-l"], &sys);
        assert!(!out.contains("/net"), "{out}");
    }

    #[test]
    fn one_device_produces_one_row() {
        // The same device mounted twice: the entry with a real source, and then
        // the one nearer the root, wins.
        let mut sys = FakeSystem::default();
        sys.mount("tmpfs", "/run", "tmpfs", 30, 100, 50, 50);
        sys.mount("/dev/sda1", "/run", "ext4", 30, 100, 50, 50);
        let (out, _, _) = go(&[], &sys);
        assert_eq!(out.lines().count(), 2, "one header and one row: {out}");
        assert!(out.contains("/dev/sda1"), "{out}");
    }

    #[test]
    fn distinct_remote_sources_are_both_kept() {
        let mut sys = FakeSystem::default();
        sys.mount("a:/export", "/net", "nfs", 40, 500, 100, 100);
        sys.mount("b:/export", "/net2", "nfs", 40, 500, 100, 100);
        sys.usage
            .insert(b"/net2".to_vec(), *sys.usage.get(&b"/net"[..]).unwrap());
        let (out, _, _) = go(&[], &sys);
        assert_eq!(out.lines().count(), 3, "both exports listed: {out}");
        // Under --total they collapse, so the space is not counted twice.
        let (out, _, _) = go(&["--total"], &sys);
        assert_eq!(out.lines().count(), 3, "one row and one total: {out}");
    }

    // -------------------------------------------------------- diagnostics ---

    #[test]
    fn a_missing_operand_is_reported_and_the_rest_still_run() {
        let mut sys = one_disk();
        sys.stats.insert(
            b"/home".to_vec(),
            StatInfo {
                dev: 2049,
                special: false,
                dir: true,
            },
        );
        let (out, err, status) = go(&["/nope", "/home"], &sys);
        // `quotef` adds no quotes to a name that needs none.
        assert_eq!(err, "df: /nope: No such file or directory\n");
        assert_eq!(status, 1);
        assert!(out.contains("/dev/sda1"), "{out}");
    }

    #[test]
    fn nothing_processed_is_itself_a_failure() {
        let sys = FakeSystem::default();
        let (out, err, status) = go(&[], &sys);
        assert_eq!(out, "");
        assert_eq!(err, "df: no file systems processed\n");
        assert_eq!(status, 1);
    }

    #[test]
    fn an_unreadable_table_is_fatal_without_operands() {
        let sys = FakeSystem {
            mounts_fail: true,
            ..FakeSystem::default()
        };
        let (_, err, status) = go(&[], &sys);
        assert_eq!(
            err,
            "df: cannot read table of mounted file systems: No such file or directory\n"
        );
        assert_eq!(status, 1);
    }

    #[test]
    fn an_unreadable_table_is_only_a_warning_with_an_operand() {
        let mut sys = FakeSystem {
            mounts_fail: true,
            ..FakeSystem::default()
        };
        sys.stats.insert(
            b"/home".to_vec(),
            StatInfo {
                dev: 2049,
                special: false,
                dir: true,
            },
        );
        sys.stats.insert(
            b"/".to_vec(),
            StatInfo {
                dev: 1,
                special: false,
                dir: true,
            },
        );
        sys.usage.insert(
            b"/home".to_vec(),
            FsUsage {
                blocksize: 1024,
                blocks: 10,
                bfree: 5,
                bavail: 5,
                bavail_top_bit_set: false,
                files: 4,
                ffree: 2,
            },
        );
        let (out, err, status) = go(&["/home"], &sys);
        assert!(err.starts_with("df: Warning: cannot read table"), "{err}");
        assert_eq!(status, 0);
        // With no table there is no source and no type, but the usage figures
        // are still available through the mount point found by walking up.
        assert!(out.contains("/home"), "{out}");
    }

    #[test]
    fn sync_is_requested_only_when_asked() {
        let sys = one_disk();
        let _ = go(&[], &sys);
        assert!(!sys.synced.get());
        let _ = go(&["--sync"], &sys);
        assert!(sys.synced.get());
    }

    #[test]
    fn an_undecodable_mount_point_is_still_listed() {
        let mut sys = FakeSystem::default();
        sys.mount("/dev/sda1", "/", "ext4", 2049, 1000, 400, 300);
        sys.mounts.push(MountEntry {
            devname: b"/dev/sdb1".to_vec(),
            mountdir: b"/mnt/\xff".to_vec(),
            mntroot: Some(b"/".to_vec()),
            fstype: b"ext4".to_vec(),
            dummy: false,
            remote: false,
            dev: DEV_UNKNOWN,
        });
        sys.usage.insert(
            b"/mnt/\xff".to_vec(),
            FsUsage {
                blocksize: 1024,
                blocks: 50,
                bfree: 20,
                bavail: 20,
                bavail_top_bit_set: false,
                files: 8,
                ffree: 4,
            },
        );
        sys.stats.insert(
            b"/mnt/\xff".to_vec(),
            StatInfo {
                dev: 2050,
                special: false,
                dir: true,
            },
        );
        let cfg = settings(&[]);
        let mut out = Vec::new();
        let mut err = Vec::new();
        let status = run(&cfg, &sys, &mut out, &mut err);
        assert_eq!(status, 0);
        assert_eq!(err, b"");
        // Not a terminal, so only the control bytes are replaced — the
        // undecodable byte reaches the pipe intact, as GNU's does.
        assert!(
            out.windows(6).any(|w| w == b"/mnt/\xff"),
            "{}",
            String::from_utf8_lossy(&out)
        );
    }

    #[test]
    fn a_terminal_gets_question_marks_instead_of_raw_bytes() {
        let mut sys = FakeSystem {
            tty: true,
            ..FakeSystem::default()
        };
        sys.mount("/dev/sda1", "/", "ext4", 2049, 1000, 400, 300);
        sys.mounts.push(MountEntry {
            devname: b"/dev/sdb1".to_vec(),
            mountdir: b"/mnt/\xff".to_vec(),
            mntroot: Some(b"/".to_vec()),
            fstype: b"ext4".to_vec(),
            dummy: false,
            remote: false,
            dev: DEV_UNKNOWN,
        });
        sys.usage.insert(
            b"/mnt/\xff".to_vec(),
            FsUsage {
                blocksize: 1024,
                blocks: 50,
                bfree: 20,
                bavail: 20,
                bavail_top_bit_set: false,
                files: 8,
                ffree: 4,
            },
        );
        sys.stats.insert(
            b"/mnt/\xff".to_vec(),
            StatInfo {
                dev: 2050,
                special: false,
                dir: true,
            },
        );
        let (out, _, _) = go(&[], &sys);
        assert!(out.contains("/mnt/?"), "{out}");
    }

    #[test]
    fn a_device_operand_reports_on_what_is_mounted_from_it() {
        let mut sys = one_disk();
        sys.stats.insert(
            b"/dev/sda1".to_vec(),
            StatInfo {
                dev: 6,
                special: true,
                dir: false,
            },
        );
        let (out, err, status) = go(&["/dev/sda1"], &sys);
        assert_eq!((err.as_str(), status), ("", 0));
        assert!(out.contains("/dev/sda1") && out.contains(" /\n"), "{out}");
    }

    #[test]
    fn an_over_mounted_device_is_diagnosed() {
        let mut sys = FakeSystem::default();
        sys.mount("/dev/sda1", "/mnt", "ext4", 2049, 1000, 400, 300);
        sys.mount("/dev/sdb1", "/mnt", "ext4", 2050, 500, 200, 200);
        sys.stats.insert(
            b"/dev/sda1".to_vec(),
            StatInfo {
                dev: 6,
                special: true,
                dir: false,
            },
        );
        let (_, err, status) = go(&["/dev/sda1"], &sys);
        assert_eq!(
            err,
            "df: cannot access '/dev/sda1': over-mounted by another device\n"
        );
        assert_eq!(status, 1);
    }

    #[test]
    fn a_uuid_device_name_is_resolved_when_walking_the_table() {
        let mut sys = FakeSystem::default();
        sys.mount(
            "/dev/disk/by-uuid/828fc648-9f30-43d8-a0b1-f7196a2edb66",
            "/",
            "ext4",
            2049,
            1000,
            400,
            300,
        );
        sys.links.insert(
            b"/dev/disk/by-uuid/828fc648-9f30-43d8-a0b1-f7196a2edb66".to_vec(),
            b"/dev/sda1".to_vec(),
        );
        let (out, _, _) = go(&[], &sys);
        assert!(out.contains("/dev/sda1"), "{out}");
        assert!(!out.contains("by-uuid"), "{out}");
    }
}
