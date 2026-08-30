//! dd — convert and copy a file, with the operand syntax POSIX gave it in 1970
//! and the conversions GNU has accumulated since.
//!
//! This is a transcription of `coreutils-9.4/src/dd.c`, not a fresh design. The
//! reason is that `dd`'s observable behaviour is almost entirely edge cases —
//! how a partial read is counted, whether `conv=block` pads before or after
//! translating, which of two mutually exclusive symbols wins, whether a failed
//! `lseek` is fatal — and every one of those was settled by upstream decades
//! ago in a way that scripts now depend on. Reimplementing from the prose in
//! POSIX would reproduce the shape and get the corners wrong.
//!
//! ## What this replaces
//!
//! The previous version of this file implemented six operands (`if`, `of`,
//! `bs`, `count`, `skip`, `seek`) and *rejected* everything else, on the stated
//! principle that "a `dd` that accepts `conv=notrunc` and truncates anyway is
//! worse than one that refuses the request, because the caller has no way to
//! find out". That principle still holds and is why the flags that cannot be
//! honoured here are still rejected (see below) rather than ignored. What
//! changed is that the honourable set is now nearly all of it.
//!
//! Two defects in that version are fixed here, both found by measuring GNU
//! rather than by reading it:
//!
//! * **Truncation happened at the wrong offset.** The old code passed
//!   `.truncate(seek == 0)`, so `dd of=big seek=10 bs=1` left `big` at its
//!   original length instead of cutting it to 12 bytes. GNU truncates *at the
//!   seek offset* — `seek_records * obs + seek_bytes` — and only skips the
//!   truncation entirely for `conv=notrunc`. Measured: a 100-byte file written
//!   with `seek=10 bs=1` and two bytes of input ends at 12 bytes.
//! * **`skip=` past the end of the input was fatal.** GNU reports
//!   `cannot skip to specified offset`, prints `0+0 records in`, and exits
//!   **0**; it is a warning, not an error. The old code exited 1.
//!
//! (History: `known-issues.md` →
//! `B-dd-DESTROYS-THE-OUTPUT-FILE-WHEN-seek-IS-GIVEN`.)
//!
//! ## Flags that are rejected
//!
//! `iflag=`/`oflag=` names that map to an `O_*` bit this platform cannot honour
//! — `binary`, `cio`, `direct`, `directory`, `dsync`, `noatime`, `nocache`,
//! `noctty`, `nofollow`, `nolinks`, `nonblock`, `sync`, `text` — are carried in
//! the symbol table with value **0**, and a zero-valued entry fails
//! [`parse_symbols`]'s match exactly as an unknown name does:
//!
//! ```text
//! dd: invalid input flag: ‘direct’
//! Try 'dd --help' for more information.
//! ```
//!
//! That is not a divergence dressed up as a feature. Upstream's loop condition
//! is `!(operand_matches (...) && entry->value)`, so GNU itself rejects these
//! names on any platform where the corresponding macro is 0 — which is why
//! `iflag=binary`, `iflag=text`, `iflag=cio` and `iflag=nolinks` are all
//! rejected by stock GNU `dd` on Linux today. The accepted set is `append`,
//! `fullblock`, `count_bytes`, `skip_bytes` and `seek_bytes`.
//!
//! ## Signals
//!
//! GNU `dd` prints its statistics on `SIGUSR1` and re-prints them on a fatal
//! signal. Neither is here, and the corresponding paragraph is absent from
//! `--help`. `design.txt` rules out Unix signals for process control on this
//! system — that is a requirement, not a shortcut — so there is no signal to
//! catch. `status=progress` covers the reason people reach for `SIGUSR1`.

// `userspace/coreutils/Cargo.toml` carries no `[lints]` table, so `lib.rs`'s
// crate-level deny does not reach a `src/bin/*.rs` — each binary is its own
// crate root. Stated here so `dd` is held to the project standard.
#![deny(clippy::all)]
#![warn(clippy::pedantic)]

use coreutils::errmsg;
use coreutils::extfloat::{self, ExtF80, Spec};
use coreutils::filekind;
use coreutils::getopt::{self, Program, Takes};
use coreutils::human::{self, Opts};
use coreutils::quote::{os_bytes, os_from_bytes, quote, quoteaf_os, quotef_os};
use coreutils::stdfd::{self, Stream};
use coreutils::xnum::{self, Status};
use std::ffi::{OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::mem::ManuallyDrop;
use std::process::ExitCode;
use std::time::Instant;

coreutils::guard_std_fds!();

const DD: Program = Program::new("dd", 1);
const SHORT_OPTIONS: &str = "";
const LONG_OPTIONS: &[(&str, Takes)] = &[("help", Takes::Nothing), ("version", Takes::Nothing)];

// ---------------------------------------------------------------------------
// Conversion and flag bits
// ---------------------------------------------------------------------------

/// EBCDIC to ASCII.
const C_ASCII: u32 = 0o1;
/// ASCII to EBCDIC.
const C_EBCDIC: u32 = 0o2;
/// A different ASCII to EBCDIC.
const C_IBM: u32 = 0o4;
/// Variable-length to fixed-length records.
const C_BLOCK: u32 = 0o10;
/// Fixed-length to variable-length records.
const C_UNBLOCK: u32 = 0o20;
const C_LCASE: u32 = 0o40;
const C_UCASE: u32 = 0o100;
const C_SWAB: u32 = 0o200;
const C_NOERROR: u32 = 0o400;
const C_NOTRUNC: u32 = 0o1000;
/// Pad every input record to `ibs` with NULs.
const C_SYNC: u32 = 0o2000;
/// Use separate input and output buffers, and combine partial input blocks.
/// Set by every conversion that needs to see whole records, and by the mere
/// absence of `bs=`.
const C_TWOBUFS: u32 = 0o4000;
const C_NOCREAT: u32 = 0o10000;
const C_EXCL: u32 = 0o20000;
const C_FDATASYNC: u32 = 0o40000;
// These two are single bits, not magnitudes: `0o100_000` splits the ladder in
// the one place where the digit that matters moves, and the value stops being
// comparable at a glance with the `0o40000` above it.
#[allow(clippy::unreadable_literal)]
const C_FSYNC: u32 = 0o100000;
#[allow(clippy::unreadable_literal)]
const C_SPARSE: u32 = 0o200000;

/// `oflag=append` — the one real `O_*` bit that survives translation here.
const F_APPEND: u32 = 0o1;
/// `iflag=fullblock` — accumulate a whole block from short reads.
const F_FULLBLOCK: u32 = 0o2;
/// `iflag=count_bytes` — `count=` counts bytes, not records.
const F_COUNT_BYTES: u32 = 0o4;
/// `iflag=skip_bytes` — `skip=` counts bytes, not records.
const F_SKIP_BYTES: u32 = 0o10;
/// `oflag=seek_bytes` — `seek=` counts bytes, not records.
const F_SEEK_BYTES: u32 = 0o20;

/// How much `dd` says when it is done, and whether it says anything meanwhile.
///
/// Ordered, because upstream compares `status_level` against `STATUS_NOXFER`
/// and `STATUS_NONE` with `<` and `>=`; the discriminants are upstream's.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(u8)]
enum StatusLevel {
    /// Error messages only.
    None = 1,
    /// Record counts, but no `N bytes copied` line.
    Noxfer = 2,
    /// Record counts and the transfer line.
    Default = 3,
    /// The above, plus a periodic line on standard error while copying.
    Progress = 4,
}

// ---------------------------------------------------------------------------
// Symbol tables
// ---------------------------------------------------------------------------

/// `conv=` symbols. Several imply `C_TWOBUFS` because they cannot work on a
/// short read: `conv=block` has to see a whole record to know where to pad.
const CONVERSIONS: &[(&str, u32)] = &[
    ("ascii", C_ASCII | C_UNBLOCK | C_TWOBUFS),
    ("ebcdic", C_EBCDIC | C_BLOCK | C_TWOBUFS),
    ("ibm", C_IBM | C_BLOCK | C_TWOBUFS),
    ("block", C_BLOCK | C_TWOBUFS),
    ("unblock", C_UNBLOCK | C_TWOBUFS),
    ("lcase", C_LCASE | C_TWOBUFS),
    ("ucase", C_UCASE | C_TWOBUFS),
    ("sparse", C_SPARSE),
    ("swab", C_SWAB | C_TWOBUFS),
    ("noerror", C_NOERROR),
    ("nocreat", C_NOCREAT),
    ("excl", C_EXCL),
    ("notrunc", C_NOTRUNC),
    ("sync", C_SYNC),
    ("fdatasync", C_FDATASYNC),
    ("fsync", C_FSYNC),
];

/// `iflag=`/`oflag=` symbols, in upstream's order.
///
/// A **0** value means "this platform has no such bit", and
/// [`parse_symbols`] treats that exactly as it treats an unknown name — which
/// is upstream's own behaviour, since its match is
/// `operand_matches (...) && entry->value`. Keeping the names in the table
/// rather than deleting them is what makes the diagnostic say
/// `invalid input flag: ‘direct’` here and on a GNU host that lacks
/// `O_DIRECT`, instead of the two differing.
const FLAGS: &[(&str, u32)] = &[
    ("append", F_APPEND),
    ("binary", 0),
    ("cio", 0),
    ("direct", 0),
    ("directory", 0),
    ("dsync", 0),
    ("noatime", 0),
    ("nocache", 0),
    ("noctty", 0),
    ("nofollow", 0),
    ("nolinks", 0),
    ("nonblock", 0),
    ("sync", 0),
    ("text", 0),
    ("fullblock", F_FULLBLOCK),
    ("count_bytes", F_COUNT_BYTES),
    ("skip_bytes", F_SKIP_BYTES),
    ("seek_bytes", F_SEEK_BYTES),
];

/// `status=` symbols. Unlike `conv=` these do not accumulate: the parse is
/// exclusive and the last one named wins, so `status=none,progress` is
/// `progress`.
const STATUSES: &[(&str, u32)] = &[
    ("none", StatusLevel::None as u32),
    ("noxfer", StatusLevel::Noxfer as u32),
    ("progress", StatusLevel::Progress as u32),
];

/// The multiplicative suffixes `xstrtoumax` accepts for a `dd` operand. The
/// trailing `0` is `xnum`'s marker enabling the `B`/`iB` second suffix.
const SIZE_SUFFIXES: &[u8] = b"bcEGkKMPQRTwYZ0";

// ---------------------------------------------------------------------------
// Translation tables
// ---------------------------------------------------------------------------
//
// Taken from POSIX 1003.1-2013 by way of GNU, and then verified against the
// shipped binary byte for byte: every one of the 256 inputs was pushed through
// `dd conv=ebcdic`, `conv=ibm` and `conv=ascii` in WSL and compared. The
// warning in upstream's source — "beware of imitations; there are lots of
// ASCII<->EBCDIC tables floating around the net" — is the reason this was
// measured rather than copied from a reference.

/// ASCII to EBCDIC, for `conv=ebcdic`.
#[rustfmt::skip]
const ASCII_TO_EBCDIC: [u8; 256] = [
    0x00, 0x01, 0x02, 0x03, 0x37, 0x2d, 0x2e, 0x2f, 0x16, 0x05, 0x25, 0x0b,
    0x0c, 0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x3c, 0x3d, 0x32, 0x26,
    0x18, 0x19, 0x3f, 0x27, 0x1c, 0x1d, 0x1e, 0x1f, 0x40, 0x5a, 0x7f, 0x7b,
    0x5b, 0x6c, 0x50, 0x7d, 0x4d, 0x5d, 0x5c, 0x4e, 0x6b, 0x60, 0x4b, 0x61,
    0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0x7a, 0x5e,
    0x4c, 0x7e, 0x6e, 0x6f, 0x7c, 0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7,
    0xc8, 0xc9, 0xd1, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xe2,
    0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xad, 0xe0, 0xbd, 0x9a, 0x6d,
    0x79, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x91, 0x92,
    0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6,
    0xa7, 0xa8, 0xa9, 0xc0, 0x4f, 0xd0, 0x5f, 0x07, 0x20, 0x21, 0x22, 0x23,
    0x24, 0x15, 0x06, 0x17, 0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x09, 0x0a, 0x1b,
    0x30, 0x31, 0x1a, 0x33, 0x34, 0x35, 0x36, 0x08, 0x38, 0x39, 0x3a, 0x3b,
    0x04, 0x14, 0x3e, 0xe1, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48,
    0x49, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x62, 0x63,
    0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x70, 0x71, 0x72, 0x73, 0x74, 0x75,
    0x76, 0x77, 0x78, 0x80, 0x8a, 0x8b, 0x8c, 0x8d, 0x8e, 0x8f, 0x90, 0x6a,
    0x9b, 0x9c, 0x9d, 0x9e, 0x9f, 0xa0, 0xaa, 0xab, 0xac, 0x4a, 0xae, 0xaf,
    0xb0, 0xb1, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xbb,
    0xbc, 0xa1, 0xbe, 0xbf, 0xca, 0xcb, 0xcc, 0xcd, 0xce, 0xcf, 0xda, 0xdb,
    0xdc, 0xdd, 0xde, 0xdf, 0xea, 0xeb, 0xec, 0xed, 0xee, 0xef, 0xfa, 0xfb,
    0xfc, 0xfd, 0xfe, 0xff,
];

/// ASCII to the alternate EBCDIC, for `conv=ibm`.
///
/// It differs from [`ASCII_TO_EBCDIC`] in exactly four places — `~`, `[`, `]`
/// and `\` — which is the whole reason both tables exist.
#[rustfmt::skip]
const ASCII_TO_IBM: [u8; 256] = [
    0x00, 0x01, 0x02, 0x03, 0x37, 0x2d, 0x2e, 0x2f, 0x16, 0x05, 0x25, 0x0b,
    0x0c, 0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x3c, 0x3d, 0x32, 0x26,
    0x18, 0x19, 0x3f, 0x27, 0x1c, 0x1d, 0x1e, 0x1f, 0x40, 0x5a, 0x7f, 0x7b,
    0x5b, 0x6c, 0x50, 0x7d, 0x4d, 0x5d, 0x5c, 0x4e, 0x6b, 0x60, 0x4b, 0x61,
    0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0x7a, 0x5e,
    0x4c, 0x7e, 0x6e, 0x6f, 0x7c, 0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7,
    0xc8, 0xc9, 0xd1, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xe2,
    0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xad, 0xe0, 0xbd, 0x5f, 0x6d,
    0x79, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x91, 0x92,
    0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6,
    0xa7, 0xa8, 0xa9, 0xc0, 0x4f, 0xd0, 0xa1, 0x07, 0x20, 0x21, 0x22, 0x23,
    0x24, 0x15, 0x06, 0x17, 0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x09, 0x0a, 0x1b,
    0x30, 0x31, 0x1a, 0x33, 0x34, 0x35, 0x36, 0x08, 0x38, 0x39, 0x3a, 0x3b,
    0x04, 0x14, 0x3e, 0xe1, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48,
    0x49, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x62, 0x63,
    0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x70, 0x71, 0x72, 0x73, 0x74, 0x75,
    0x76, 0x77, 0x78, 0x80, 0x8a, 0x8b, 0x8c, 0x8d, 0x8e, 0x8f, 0x90, 0x9a,
    0x9b, 0x9c, 0x9d, 0x9e, 0x9f, 0xa0, 0xaa, 0xab, 0xac, 0xad, 0xae, 0xaf,
    0xb0, 0xb1, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xbb,
    0xbc, 0xbd, 0xbe, 0xbf, 0xca, 0xcb, 0xcc, 0xcd, 0xce, 0xcf, 0xda, 0xdb,
    0xdc, 0xdd, 0xde, 0xdf, 0xea, 0xeb, 0xec, 0xed, 0xee, 0xef, 0xfa, 0xfb,
    0xfc, 0xfd, 0xfe, 0xff,
];

/// EBCDIC to ASCII, for `conv=ascii`.
#[rustfmt::skip]
const EBCDIC_TO_ASCII: [u8; 256] = [
    0x00, 0x01, 0x02, 0x03, 0x9c, 0x09, 0x86, 0x7f, 0x97, 0x8d, 0x8e, 0x0b,
    0x0c, 0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x9d, 0x85, 0x08, 0x87,
    0x18, 0x19, 0x92, 0x8f, 0x1c, 0x1d, 0x1e, 0x1f, 0x80, 0x81, 0x82, 0x83,
    0x84, 0x0a, 0x17, 0x1b, 0x88, 0x89, 0x8a, 0x8b, 0x8c, 0x05, 0x06, 0x07,
    0x90, 0x91, 0x16, 0x93, 0x94, 0x95, 0x96, 0x04, 0x98, 0x99, 0x9a, 0x9b,
    0x14, 0x15, 0x9e, 0x1a, 0x20, 0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6,
    0xa7, 0xa8, 0xd5, 0x2e, 0x3c, 0x28, 0x2b, 0x7c, 0x26, 0xa9, 0xaa, 0xab,
    0xac, 0xad, 0xae, 0xaf, 0xb0, 0xb1, 0x21, 0x24, 0x2a, 0x29, 0x3b, 0x7e,
    0x2d, 0x2f, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xcb, 0x2c,
    0x25, 0x5f, 0x3e, 0x3f, 0xba, 0xbb, 0xbc, 0xbd, 0xbe, 0xbf, 0xc0, 0xc1,
    0xc2, 0x60, 0x3a, 0x23, 0x40, 0x27, 0x3d, 0x22, 0xc3, 0x61, 0x62, 0x63,
    0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9,
    0xca, 0x6a, 0x6b, 0x6c, 0x6d, 0x6e, 0x6f, 0x70, 0x71, 0x72, 0x5e, 0xcc,
    0xcd, 0xce, 0xcf, 0xd0, 0xd1, 0xe5, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78,
    0x79, 0x7a, 0xd2, 0xd3, 0xd4, 0x5b, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xdb,
    0xdc, 0xdd, 0xde, 0xdf, 0xe0, 0xe1, 0xe2, 0xe3, 0xe4, 0x5d, 0xe6, 0xe7,
    0x7b, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0xe8, 0xe9,
    0xea, 0xeb, 0xec, 0xed, 0x7d, 0x4a, 0x4b, 0x4c, 0x4d, 0x4e, 0x4f, 0x50,
    0x51, 0x52, 0xee, 0xef, 0xf0, 0xf1, 0xf2, 0xf3, 0x5c, 0x9f, 0x53, 0x54,
    0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9,
    0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0xfa, 0xfb,
    0xfc, 0xfd, 0xfe, 0xff,
];

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

/// A fatal diagnostic, and whether it is followed by the `Try 'dd --help'`
/// line.
///
/// The two flavours are upstream's two exit paths and they are not
/// interchangeable: `usage (EXIT_FAILURE)` — a *syntax* error, such as an
/// unknown `conv=` symbol — prints the referral, while `error (EXIT_FAILURE,
/// ...)` — a *value* error, such as `bs=0` — does not. Measured:
///
/// ```text
/// dd: invalid status level: ‘bogus’      dd: invalid number: '0'
/// Try 'dd --help' for more information.  (nothing further)
/// ```
#[derive(Debug, PartialEq, Eq)]
struct Fatal {
    message: String,
    referral: bool,
}

impl Fatal {
    /// An `error (EXIT_FAILURE, ...)` diagnostic — no referral line.
    fn plain(message: String) -> Self {
        Self {
            message,
            referral: false,
        }
    }

    /// A `usage (EXIT_FAILURE)` diagnostic — the referral line follows.
    fn usage(message: String) -> Self {
        Self {
            message,
            referral: true,
        }
    }

    /// Emit on standard error, in upstream's order: the sentence, then the
    /// referral.
    fn report(&self) {
        stdfd::diag_line(&format!("dd: {}", self.message));
        if self.referral {
            stdfd::diag_line("Try 'dd --help' for more information.");
        }
    }
}

// ---------------------------------------------------------------------------
// Operand parsing
// ---------------------------------------------------------------------------

/// True when `text` is `pattern`, or `pattern` followed by `delim` and
/// anything. Upstream's `operand_matches`.
fn operand_matches(text: &[u8], pattern: &[u8], delim: u8) -> bool {
    match text.split_at_checked(pattern.len()) {
        Some((head, rest)) if head == pattern => rest.first().is_none_or(|c| *c == delim),
        _ => false,
    }
}

/// Interpret one comma-separated symbol list against `table`.
///
/// `exclusive` distinguishes `status=` (each symbol *replaces* the value, so
/// the last one named wins — measured: `status=none,progress` is `progress`)
/// from `conv=` and `iflag=` (each symbol *adds* a bit).
///
/// A table entry whose value is 0 does not match, which is upstream's
/// `operand_matches (...) && entry->value` and is how a flag the platform
/// cannot honour reports itself as invalid rather than being silently dropped.
fn parse_symbols(
    text: &[u8],
    table: &[(&str, u32)],
    exclusive: bool,
    error_msgid: &str,
) -> Result<u32, Fatal> {
    let mut value = 0;
    let mut rest = text;
    loop {
        let comma = rest.iter().position(|c| *c == b',');
        let found = table
            .iter()
            .find(|(sym, v)| *v != 0 && operand_matches(rest, sym.as_bytes(), b','));
        let Some((_, v)) = found else {
            // The name reported is just this component, not the tail: GNU
            // quotes `str` with an explicit length of `strcomma - str`.
            let end = comma.unwrap_or(rest.len());
            // The bytes go to `quote` unconverted rather than through
            // `from_utf8_lossy`: a `conv=` name is argv, so it can hold any
            // byte, and lossy conversion would report `conv=<U+FFFD>` for
            // every one of them alike.
            let bad = quote(&rest[..end]);
            return Err(Fatal::usage(format!("{error_msgid}: {bad}")));
        };
        if exclusive {
            value = *v;
        } else {
            value |= *v;
        }
        match comma {
            None => return Ok(value),
            Some(at) => rest = &rest[at + 1..],
        }
    }
}

/// Whether `status` is `INVALID_SUFFIX_CHAR` with the overflow bit masked off
/// — upstream's `(e & ~LONGINT_OVERFLOW) == LONGINT_INVALID_SUFFIX_CHAR`.
fn is_suffix_char(status: Status) -> bool {
    matches!(
        status,
        Status::InvalidSuffix | Status::InvalidSuffixWithOverflow
    )
}

/// Whether `status` carries the overflow bit.
fn has_overflow(status: Status) -> bool {
    matches!(status, Status::Overflow | Status::InvalidSuffixWithOverflow)
}

/// `e &= ~LONGINT_INVALID_SUFFIX_CHAR` — clearing the suffix complaint while
/// keeping any overflow.
fn clear_suffix_char(status: Status) -> Status {
    match status {
        Status::InvalidSuffix => Status::Ok,
        Status::InvalidSuffixWithOverflow => Status::Overflow,
        other => other,
    }
}

/// A non-negative decimal integer with `dd`'s multiplicative suffixes, its
/// optional trailing `B`, and its `x` products.
///
/// Returns `i64::MAX` on overflow and an indeterminate value on any other
/// error, exactly as upstream does; the caller must test the status before
/// using the number.
fn parse_integer(text: &[u8]) -> (i64, Status) {
    let (n, mut e, mut suffix) = xnum::xstrtoumax_end(text, 10, Some(SIZE_SUFFIXES));
    let result;

    // A trailing `B` means "bytes, not blocks", and is not in the suffix list,
    // so it arrives here as a complaint to be withdrawn. The two guards keep
    // `B` alone (`bs=B`) and a doubled `B` (`bs=1kBB`) invalid.
    if is_suffix_char(e)
        && text.get(suffix) == Some(&b'B')
        && suffix > 0
        && text.get(suffix - 1) != Some(&b'B')
    {
        suffix += 1;
        if suffix == text.len() {
            e = clear_suffix_char(e);
        }
    }

    if is_suffix_char(e) && text.get(suffix) == Some(&b'x') {
        let (o, f) = parse_integer(&text[suffix + 1..]);
        if !matches!(f, Status::Ok | Status::Overflow) {
            e = f;
            result = 0;
        } else if let Some(product) = i64::try_from(n).ok().and_then(|a| a.checked_mul(o))
            && !(product != 0 && (has_overflow(e) || has_overflow(f)))
        {
            if product == 0 && text.starts_with(b"0x") {
                // `dd bs=0x1` is almost always a hexadecimal literal typed by
                // someone who did not know `x` is dd's multiplication sign.
                // Zero blocks is a legal request, so this is a warning.
                stdfd::diag_line(&format!(
                    "dd: warning: {} is a zero multiplier; use {} if that is intended",
                    quote(b"0x"),
                    quote(b"00x")
                ));
            }
            e = Status::Ok;
            result = product;
        } else {
            e = Status::Overflow;
            result = i64::MAX;
        }
    } else if n <= i64::MAX.cast_unsigned() {
        result = n.cast_signed();
    } else {
        e = Status::Overflow;
        result = i64::MAX;
    }

    (result, e)
}

/// Everything the operands say, after cross-checking.
#[derive(Debug)]
struct Settings {
    input_file: Option<OsString>,
    output_file: Option<OsString>,
    conversions: u32,
    input_flags: u32,
    output_flags: u32,
    status_level: StatusLevel,
    ibs: i64,
    obs: i64,
    cbs: i64,
    skip_records: i64,
    skip_bytes: i64,
    seek_records: i64,
    seek_bytes: i64,
    max_records: i64,
    max_bytes: i64,
    warn_partial_read: bool,
    fullblock: bool,
}

/// The default for both `ibs=` and `obs=`, and therefore for `bs=`.
const DEFAULT_BLOCKSIZE: i64 = 512;

/// Upstream's `MIN (IDX_MAX - 1, MIN (SSIZE_MAX, OFF_T_MAX))`. The `- 1` is so
/// that `conv=swab`'s extra byte still fits. Nothing this large can actually
/// be allocated; the request is refused later, by name, with `memory
/// exhausted`.
const MAX_BLOCKSIZE: i64 = i64::MAX - 1;

/// True when more than one bit of `i` is set.
const fn multiple_bits_set(i: u32) -> bool {
    i & i.wrapping_sub(1) != 0
}

/// Turn the operand list into [`Settings`], applying every cross-operand rule
/// upstream applies and in the same order — the order matters, because
/// `cbs=0` clears `C_BLOCK` *before* the "cannot combine block and unblock"
/// test is made, which is why `dd conv=block,unblock` with no `cbs=` copies
/// happily instead of failing.
#[allow(clippy::too_many_lines)]
fn scan_args(operands: &[OsString]) -> Result<Settings, Fatal> {
    let mut input_file = None;
    let mut output_file = None;
    let mut conversions = 0u32;
    let mut input_flags = 0u32;
    let mut output_flags = 0u32;
    let mut status_level = StatusLevel::Default;
    let mut ibs = 0i64;
    let mut obs = 0i64;
    let mut cbs = 0i64;

    let mut skip_records = 0i64;
    let mut skip_bytes = 0i64;
    let mut seek_records = 0i64;
    let mut seek_bytes = 0i64;
    let mut max_records = i64::MAX;
    let mut max_bytes = 0i64;

    let mut blocksize = 0i64;
    let mut count = i64::MAX;
    let mut skip = 0i64;
    let mut seek = 0i64;
    let (mut count_b, mut skip_b, mut seek_b) = (false, false, false);

    for operand in operands {
        let raw = os_bytes(operand);
        let Some(eq) = raw.iter().position(|c| *c == b'=') else {
            return Err(Fatal::usage(format!(
                "unrecognized operand {}",
                quoteaf_os(operand)
            )));
        };
        let (key, val) = (&raw[..eq], &raw[eq + 1..]);

        if key == b"if" {
            input_file = Some(os_from_bytes(val));
        } else if key == b"of" {
            output_file = Some(os_from_bytes(val));
        } else if key == b"conv" {
            conversions |= parse_symbols(val, CONVERSIONS, false, "invalid conversion")?;
        } else if key == b"iflag" {
            input_flags |= parse_symbols(val, FLAGS, false, "invalid input flag")?;
        } else if key == b"oflag" {
            output_flags |= parse_symbols(val, FLAGS, false, "invalid output flag")?;
        } else if key == b"status" {
            status_level = match parse_symbols(val, STATUSES, true, "invalid status level")? {
                1 => StatusLevel::None,
                2 => StatusLevel::Noxfer,
                4 => StatusLevel::Progress,
                _ => StatusLevel::Default,
            };
        } else {
            // Every remaining operand is a number, and upstream parses it
            // *before* deciding whether the name is one it knows — which is
            // why `dd bogus=0x1` can print the zero-multiplier warning on its
            // way to rejecting the operand.
            let (n, mut invalid) = parse_integer(val);
            let has_b = val.contains(&b'B');
            let mut n_min = 0i64;
            let mut n_max = i64::MAX;
            let mut converted: Option<&mut i64> = None;

            if key == b"ibs" {
                (n_min, n_max, converted) = (1, MAX_BLOCKSIZE, Some(&mut ibs));
            } else if key == b"obs" {
                (n_min, n_max, converted) = (1, MAX_BLOCKSIZE, Some(&mut obs));
            } else if key == b"bs" {
                (n_min, n_max, converted) = (1, MAX_BLOCKSIZE, Some(&mut blocksize));
            } else if key == b"cbs" {
                (n_min, n_max, converted) = (1, i64::MAX, Some(&mut cbs));
            } else if key == b"skip" || key == b"iseek" {
                skip = n;
                skip_b = has_b;
            } else if key == b"seek" || key == b"oseek" {
                seek = n;
                seek_b = has_b;
            } else if key == b"count" {
                count = n;
                count_b = has_b;
            } else {
                return Err(Fatal::usage(format!(
                    "unrecognized operand {}",
                    quoteaf_os(operand)
                )));
            }

            if n < n_min {
                invalid = Status::Invalid;
            } else if n_max < n {
                invalid = Status::Overflow;
            }

            match invalid {
                Status::Ok => {
                    if let Some(slot) = converted {
                        *slot = n;
                    }
                }
                // EOVERFLOW's text is appended by `error`'s errno argument;
                // every other error passes 0 and so says nothing further.
                other => {
                    let quoted = quoteaf_os(os_from_bytes(val));
                    let tail = if has_overflow(other) {
                        ": Value too large for defined data type"
                    } else {
                        ""
                    };
                    return Err(Fatal::plain(format!("invalid number: {quoted}{tail}")));
                }
            }
        }
    }

    if blocksize != 0 {
        ibs = blocksize;
        obs = blocksize;
    } else {
        // POSIX says dd aggregates partial reads into obs when bs= is absent.
        // This is the difference measured as `0+2 records in / 0+2 records
        // out` for `bs=4096` against `0+2 / 0+1` for `ibs=4096 obs=4096`.
        conversions |= C_TWOBUFS;
    }

    if ibs == 0 {
        ibs = DEFAULT_BLOCKSIZE;
    }
    if obs == 0 {
        obs = DEFAULT_BLOCKSIZE;
    }
    if cbs == 0 {
        conversions &= !(C_BLOCK | C_UNBLOCK);
    }

    // Upstream widens `iflag=dsync`/`sync` to `O_RSYNC` here. Neither bit is
    // honourable on this platform, so both are 0-valued in FLAGS and rejected
    // at parse time; there is nothing left to widen.

    if output_flags & F_FULLBLOCK != 0 {
        return Err(Fatal::usage(format!(
            "invalid output flag: {}",
            quote(b"fullblock")
        )));
    }

    if skip_b {
        input_flags |= F_SKIP_BYTES;
    }
    if input_flags & F_SKIP_BYTES != 0 && skip != 0 {
        (skip_records, skip_bytes) = (skip / ibs, skip % ibs);
    } else if skip != 0 {
        skip_records = skip;
    }

    if count_b {
        input_flags |= F_COUNT_BYTES;
    }
    if input_flags & F_COUNT_BYTES != 0 && count != i64::MAX {
        (max_records, max_bytes) = (count / ibs, count % ibs);
    } else if count != i64::MAX {
        max_records = count;
    }

    if seek_b {
        output_flags |= F_SEEK_BYTES;
    }
    if output_flags & F_SEEK_BYTES != 0 && seek != 0 {
        (seek_records, seek_bytes) = (seek / obs, seek % obs);
    } else if seek != 0 {
        seek_records = seek;
    }

    // Diagnose a short read only where one would silently miscount: with a
    // single buffer (so nothing re-aggregates it), no `iflag=fullblock`, and a
    // skip or a bounded count to be thrown off.
    let warn_partial_read = conversions & C_TWOBUFS == 0
        && input_flags & F_FULLBLOCK == 0
        && (skip_records != 0 || (0 < max_records && max_records < i64::MAX));

    let fullblock = input_flags & F_FULLBLOCK != 0;
    input_flags &= !F_FULLBLOCK;

    if multiple_bits_set(conversions & (C_ASCII | C_EBCDIC | C_IBM)) {
        return Err(Fatal::plain(
            "cannot combine any two of {ascii,ebcdic,ibm}".to_string(),
        ));
    }
    if multiple_bits_set(conversions & (C_BLOCK | C_UNBLOCK)) {
        return Err(Fatal::plain("cannot combine block and unblock".to_string()));
    }
    if multiple_bits_set(conversions & (C_LCASE | C_UCASE)) {
        return Err(Fatal::plain("cannot combine lcase and ucase".to_string()));
    }
    if multiple_bits_set(conversions & (C_EXCL | C_NOCREAT)) {
        return Err(Fatal::plain("cannot combine excl and nocreat".to_string()));
    }
    // `cannot combine direct and nocache` has no reachable form here: both
    // names are 0-valued in FLAGS and so never survive parsing.

    Ok(Settings {
        input_file,
        output_file,
        conversions,
        input_flags,
        output_flags,
        status_level,
        ibs,
        obs,
        cbs,
        skip_records,
        skip_bytes,
        seek_records,
        seek_bytes,
        max_records,
        max_bytes,
        warn_partial_read,
        fullblock,
    })
}

/// The byte-for-byte translation the conversions add up to, plus the two
/// characters `conv=block`/`unblock` emit, which move when the output charset
/// does.
struct Translation {
    table: [u8; 256],
    needed: bool,
    newline_character: u8,
    space_character: u8,
}

/// Compose the translation table, in upstream's order: charset in, then case,
/// then charset out.
///
/// The order is observable. `conv=ucase,ebcdic` upcases *before* converting,
/// so `abc` becomes EBCDIC `ABC` (`0xc1 0xc2 0xc3`); reversing the two steps
/// would upcase bytes that are no longer letters and leave `abc` alone.
/// Measured both ways round — `conv=ucase,ebcdic` and `conv=ebcdic,ucase`
/// give the same answer, because `conv=` is a set, not a sequence.
fn apply_translations(conversions: u32) -> Translation {
    let mut table = [0u8; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        *slot = u8::try_from(i).unwrap_or(0);
    }
    let mut needed = false;

    let translate_charset = |table: &mut [u8; 256], new_trans: &[u8; 256]| {
        for slot in table.iter_mut() {
            *slot = new_trans[usize::from(*slot)];
        }
    };

    if conversions & C_ASCII != 0 {
        translate_charset(&mut table, &EBCDIC_TO_ASCII);
        needed = true;
    }

    if conversions & C_UCASE != 0 {
        for slot in &mut table {
            *slot = slot.to_ascii_uppercase();
        }
        needed = true;
    } else if conversions & C_LCASE != 0 {
        for slot in &mut table {
            *slot = slot.to_ascii_lowercase();
        }
        needed = true;
    }

    let (mut newline_character, mut space_character) = (b'\n', b' ');
    if conversions & C_EBCDIC != 0 {
        translate_charset(&mut table, &ASCII_TO_EBCDIC);
        needed = true;
        newline_character = ASCII_TO_EBCDIC[usize::from(b'\n')];
        space_character = ASCII_TO_EBCDIC[usize::from(b' ')];
    } else if conversions & C_IBM != 0 {
        translate_charset(&mut table, &ASCII_TO_IBM);
        needed = true;
        newline_character = ASCII_TO_IBM[usize::from(b'\n')];
        space_character = ASCII_TO_IBM[usize::from(b' ')];
    }

    Translation {
        table,
        needed,
        newline_character,
        space_character,
    }
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// The four counters `dd` prints, plus the two it needs to print them.
#[derive(Default, Clone, Copy)]
struct Stats {
    /// Whole input records read.
    r_full: i64,
    /// Short input records read — the `+1` in `19+1 records in`.
    r_partial: i64,
    /// Whole output records written.
    w_full: i64,
    /// Short output records written.
    w_partial: i64,
    /// Records `conv=block` had to cut because they exceeded `cbs`.
    r_truncate: i64,
    /// Bytes written, including those "written" by a `conv=sparse` seek.
    w_bytes: i64,
}

/// `human_readable` flags for every size `dd` prints. `SI` with `AUTOSCALE`
/// gives `10 kB`; adding `BASE_1024` to the same set gives `9.8 KiB`.
///
/// A function rather than a `const` because [`Opts`]'s combinator is
/// `std::ops::BitOr`, and an operator trait cannot be called in a constant.
fn human_opts() -> Opts {
    Opts::AUTOSCALE | Opts::ROUND_TO_NEAREST | Opts::SPACE_BEFORE_UNIT | Opts::SI | Opts::B
}

/// Whether a `human_readable` result has no multiplier letter — that is,
/// whether it ends in a bare `B` rather than `kB` or `KiB`.
///
/// Upstream tests the *second to last* byte for a space, which is exactly the
/// difference between `"3 B"` and `"1.0 kB"`. It is doing so because the
/// three summary forms below must not repeat a number that carries no new
/// information: printing `3 bytes (3 B) copied` would be noise.
fn abbreviation_lacks_prefix(message: &str) -> bool {
    let b = message.as_bytes();
    b.len() >= 2 && b[b.len() - 2] == b' '
}

/// Everything about *when* statistics are printed, and what was printed last.
struct Reporter {
    level: StatusLevel,
    start: Instant,
    /// When the next `status=progress` line is due, measured from `start`.
    next_time: std::time::Duration,
    /// Length of the progress line currently on screen, or 0 if there is
    /// none. Kept so the next line can blank the tail of a longer one.
    progress_len: i64,
    /// `w_bytes` as of the last transfer line, or negative if never reported.
    reported_w_bytes: i64,
}

impl Reporter {
    fn new(level: StatusLevel) -> Self {
        Self {
            level,
            start: Instant::now(),
            next_time: std::time::Duration::from_secs(1),
            progress_len: 0,
            reported_w_bytes: -1,
        }
    }

    /// Emit a progress line if one is due. Called from the copy loop.
    fn tick(&mut self, stats: &Stats) {
        if self.level != StatusLevel::Progress {
            return;
        }
        let now = self.start.elapsed();
        if self.next_time <= now {
            self.xfer_stats(stats, Some(now));
            self.next_time += std::time::Duration::from_secs(1);
        }
    }

    /// The `N bytes (…) copied, T s, R/s` line.
    ///
    /// `progress` is `Some` when this is an interim line, which changes three
    /// things: the line is prefixed with a carriage return rather than
    /// terminated with a newline, the elapsed time is rounded to whole seconds
    /// (there is no point showing sub-second jitter on a report emitted once a
    /// second), and the tail of any longer previous line is blanked.
    fn xfer_stats(&mut self, stats: &Stats, progress: Option<std::time::Duration>) {
        let now = progress.unwrap_or_else(|| self.start.elapsed());
        let written = stats.w_bytes.max(0).cast_unsigned();
        let si = human::human_readable(written, human_opts(), 1, 1);
        let iec = human::human_readable(written, human_opts() | Opts::BASE_1024, 1, 1);

        // The rate is computed by `human_readable` itself, as
        // `written * 10^9 / elapsed_ns`, rather than in floating point. That
        // is upstream's choice and it is what makes the SI abbreviations come
        // out consistent with the byte counts beside them.
        let delta_ns = u64::try_from(now.as_nanos()).unwrap_or(u64::MAX);
        let (elapsed, rate) = if delta_ns == 0 {
            (0.0, "Infinity B/s".to_string())
        } else {
            let per_s = human::human_readable(written, human_opts(), 1_000_000_000, delta_ns);
            (now.as_secs_f64(), format!("{per_s}/s"))
        };

        let spec = if progress.is_some() {
            Spec::fixed(0)
        } else {
            Spec::general()
        };
        let elapsed_text = format!("{} s", extfloat::render(&spec, ExtF80::from_f64(elapsed)));

        let count = stats.w_bytes;
        let body = if abbreviation_lacks_prefix(&si) {
            let noun = if count == 1 { "byte" } else { "bytes" };
            format!("{count} {noun} copied, {elapsed_text}, {rate}")
        } else if abbreviation_lacks_prefix(&iec) {
            format!("{count} bytes ({si}) copied, {elapsed_text}, {rate}")
        } else {
            format!("{count} bytes ({si}, {iec}) copied, {elapsed_text}, {rate}")
        };

        let mut line = String::new();
        if progress.is_some() {
            line.push('\r');
        }
        line.push_str(&body);
        if progress.is_some() {
            // The measured length is the body's, not the line's: upstream's
            // `\r` goes out through a separate `fputc` and is not counted by
            // the `fprintf` whose return value becomes `stats_len`.
            let stats_len = i64::try_from(body.len()).unwrap_or(i64::MAX);
            if stats_len < self.progress_len {
                let pad = usize::try_from(self.progress_len - stats_len).unwrap_or(0);
                line.extend(std::iter::repeat_n(' ', pad));
            }
            self.progress_len = stats_len;
        } else {
            line.push('\n');
        }
        stdfd::diag_bytes(line.as_bytes());

        self.reported_w_bytes = stats.w_bytes;
    }

    /// The `records in` / `records out` block, and — unless `status=noxfer` —
    /// the transfer line after it.
    fn print_stats(&mut self, stats: &Stats) {
        if self.level == StatusLevel::None {
            return;
        }
        if 0 < self.progress_len {
            stdfd::diag_bytes(b"\n");
            self.progress_len = 0;
        }
        stdfd::diag_bytes(
            format!(
                "{}+{} records in\n{}+{} records out\n",
                stats.r_full, stats.r_partial, stats.w_full, stats.w_partial
            )
            .as_bytes(),
        );
        if stats.r_truncate != 0 {
            let noun = if stats.r_truncate == 1 {
                "record"
            } else {
                "records"
            };
            stdfd::diag_bytes(format!("{} truncated {noun}\n", stats.r_truncate).as_bytes());
        }
        if self.level == StatusLevel::Noxfer {
            return;
        }
        self.xfer_stats(stats, None);
    }
}

// ---------------------------------------------------------------------------
// The two ends of the copy
// ---------------------------------------------------------------------------

/// An open file that `dd` may or may not own.
///
/// `dd` with no `if=`/`of=` reads descriptor 0 and writes descriptor 1, and it
/// must treat them exactly as it treats an opened file — `seek=` and
/// `conv=sparse` and the closing `ftruncate` all apply to `dd seek=10 > img`
/// just as they do to `dd seek=10 of=img`. A borrowed handle gets that for
/// free, provided it never closes what it borrowed, which is what the
/// [`ManuallyDrop`] is for.
enum Handle {
    Owned(File),
    Std(ManuallyDrop<File>),
}

impl Handle {
    fn file(&mut self) -> &mut File {
        match self {
            Handle::Owned(f) => f,
            Handle::Std(f) => f,
        }
    }
}

/// Turn on `O_APPEND` on an already-open descriptor.
///
/// Needed only for `oflag=append` without `of=`, where there is no `open` to
/// pass the flag to. Upstream's `set_fd_flags` does the same `fcntl` dance.
#[cfg(unix)]
fn set_append(fd: i32) -> io::Result<()> {
    use std::ffi::c_int;

    const F_GETFL: c_int = 3;
    const F_SETFL: c_int = 4;
    const O_APPEND: c_int = 0o2000;

    unsafe extern "C" {
        fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
    }

    // SAFETY: `fd` is an open descriptor owned by this process, and both
    // commands are the standard ones for reading and writing its status
    // flags. Neither reads nor writes memory.
    let old = unsafe { fcntl(fd, F_GETFL) };
    if old < 0 {
        return Err(io::Error::last_os_error());
    }
    if old & O_APPEND != 0 {
        return Ok(());
    }
    // SAFETY: as above.
    if unsafe { fcntl(fd, F_SETFL, old | O_APPEND) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(unix))]
fn set_append(fd: i32) -> io::Result<()> {
    let _ = fd;
    // Reported rather than silently ignored, for the reason in the module
    // documentation: a flag that is accepted and not honoured is worse than
    // one that is refused.
    Err(io::Error::other("Function not implemented"))
}

/// The input side: the descriptor, what is known about seeking it, and where
/// `dd` believes the file offset to be.
///
/// The four `bool`s are four *independent* facts — a seekable input can be
/// `fullblock` or not, quiet or not — so the state-machine or two-variant-enum
/// refactor `struct_excessive_bools` asks for would replace four honest flags
/// with a sixteen-state enum that no code ever matches on exhaustively.
#[allow(clippy::struct_excessive_bools)]
struct Reader {
    handle: Handle,
    name: OsString,
    /// Whether the initial `lseek` succeeded.
    seekable: bool,
    /// The error from that initial `lseek`, kept so a later failure can be
    /// reported with the original reason — and so a pipe (`ESPIPE`) can be
    /// recognised and not complained about twice.
    seek_error: Option<io::Error>,
    /// Byte offset, or negative once it has overflowed.
    offset: i64,
    fullblock: bool,
    warn_partial_read: bool,
    /// Size of the previous read, or negative after an error. Upstream keeps
    /// this in a function-local `static`; it is per-input either way, because
    /// there is only ever one input.
    prev_nread: i64,
    quiet: bool,
}

impl Reader {
    /// Add `nbytes` to the recorded offset, latching it negative on overflow.
    fn advance(&mut self, nbytes: i64) {
        if 0 <= self.offset {
            self.offset = self.offset.checked_add(nbytes).unwrap_or(-1);
        }
    }

    /// One `read(2)`, retried on `EINTR`, with the partial-read warning.
    fn iread(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let size = buf.len();
        let nread = loop {
            match self.handle.file().read(buf) {
                Ok(n) => break n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => {
                    self.prev_nread = -1;
                    return Err(e);
                }
            }
        };

        // The warning is about the read *before* this one, and fires only
        // once. Reporting it a read late is what lets `dd` stay quiet about
        // the final short read at end of file, which is not a miscount.
        if 0 < nread && self.warn_partial_read {
            let prev = self.prev_nread;
            if 0 < prev && prev < i64::try_from(size).unwrap_or(i64::MAX) {
                if !self.quiet {
                    let noun = if prev == 1 { "byte" } else { "bytes" };
                    stdfd::diag_line(&format!(
                        "dd: warning: partial read ({prev} {noun}); suggest iflag=fullblock"
                    ));
                }
                self.warn_partial_read = false;
            }
        }

        self.prev_nread = i64::try_from(nread).unwrap_or(i64::MAX);
        Ok(nread)
    }

    /// `iflag=fullblock`: keep reading until the buffer is full or the input
    /// ends. Without it a short read from a pipe becomes a short *record*,
    /// which is the difference measured as `3 bytes copied` against
    /// `6 bytes copied` for the same slow writer.
    fn iread_fullblock(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut nread = 0;
        while nread < buf.len() {
            let ncurr = self.iread(&mut buf[nread..])?;
            if ncurr == 0 {
                break;
            }
            nread += ncurr;
        }
        Ok(nread)
    }

    /// Read one block, honouring `iflag=fullblock`.
    fn read_block(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.fullblock {
            self.iread_fullblock(buf)
        } else {
            self.iread(buf)
        }
    }

    /// Put the file offset just past a record that failed to read, so that
    /// `conv=noerror` can carry on with the next one.
    ///
    /// Returns false when the position could not be re-established, which the
    /// caller turns into a non-zero exit status *and* a suppression of further
    /// complaints — otherwise a bad region produces one diagnostic per block.
    fn advance_after_read_error(&mut self, nbytes: i64) -> bool {
        if self.seekable {
            self.advance(nbytes);
            if self.offset < 0 {
                stdfd::diag_line(&format!(
                    "dd: offset overflow while reading file {}",
                    quoteaf_os(&self.name)
                ));
                return false;
            }
            if let Ok(offset) = self.handle.file().stream_position() {
                let offset = i64::try_from(offset).unwrap_or(i64::MAX);
                if offset == self.offset {
                    return true;
                }
                let diff = self.offset - offset;
                if !self.quiet && !(0..=nbytes).contains(&diff) {
                    stdfd::diag_line("dd: warning: invalid file offset after failed read");
                }
                if self.handle.file().seek(SeekFrom::Current(diff)).is_ok() {
                    return true;
                }
            }
        } else if self
            .seek_error
            .as_ref()
            .is_some_and(|e| e.kind() == io::ErrorKind::NotSeekable)
        {
            // A pipe cannot be repositioned and never needed to be.
            return true;
        }

        let reason = self
            .seek_error
            .as_ref()
            .map_or_else(|| "Illegal seek".to_string(), errmsg::strerror);
        stdfd::diag_line(&format!(
            "dd: {}: cannot seek: {reason}",
            quotef_os(&self.name)
        ));
        false
    }
}

/// The output side.
struct Writer {
    handle: Handle,
    name: OsString,
    /// `conv=sparse`, cleared the first time a seek is refused so that the
    /// advisory request degrades to ordinary writing without a diagnostic.
    sparse: bool,
    /// Whether the most recent output operation was a `conv=sparse` seek
    /// rather than a write. If the copy ends that way the file has to be
    /// extended explicitly, since a seek past the end alone does not lengthen
    /// it.
    final_op_was_seek: bool,
    /// The reason the last short write was short.
    error: Option<io::Error>,
}

impl Writer {
    /// Write the whole of `buf`, retrying partial writes. Returns the number
    /// of bytes written, which is less than `buf.len()` only on failure —
    /// with the reason left in [`Writer::error`].
    fn iwrite(&mut self, buf: &[u8]) -> usize {
        let size = buf.len();
        let mut total = 0;
        self.error = None;

        while total < size {
            self.final_op_was_seek = false;
            let mut nwritten = 0;

            if self.sparse && buf.iter().all(|b| *b == 0) {
                let by = i64::try_from(size).unwrap_or(i64::MAX);
                if self.handle.file().seek(SeekFrom::Current(by)).is_ok() {
                    self.final_op_was_seek = true;
                    nwritten = size;
                } else {
                    // Advisory: the caller asked for a hole and the file
                    // cannot hold one, so write the zeros instead. GNU does
                    // not warn, and neither does this.
                    self.sparse = false;
                }
            }

            if nwritten == 0 {
                match self.handle.file().write(&buf[total..]) {
                    // Some drivers return 0 rather than an error when written
                    // past the end of a device. Upstream substitutes ENOSPC so
                    // the diagnostic says something usable.
                    Ok(0) => {
                        self.error = Some(io::Error::from(io::ErrorKind::StorageFull));
                        break;
                    }
                    Ok(n) => nwritten = n,
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => {
                        self.error = Some(e);
                        break;
                    }
                }
            }

            total += nwritten;
        }

        total
    }

    /// The text of the last failure, for a `writing to %s` diagnostic.
    fn reason(&self) -> String {
        self.error
            .as_ref()
            .map_or_else(|| "Success".to_string(), errmsg::strerror)
    }
}

// ---------------------------------------------------------------------------
// The copy engine
// ---------------------------------------------------------------------------

/// Allocate one of the two buffers, reporting the size by name if it cannot
/// be had.
///
/// Failing to allocate must be a *diagnostic*, not an abort: `dd bs=99999999999999`
/// is a plausible typo and GNU answers it with
/// `memory exhausted by input buffer of size 99999999999999 bytes (91 TiB)`
/// and exit status 1. The reported size excludes `conv=swab`'s extra byte,
/// as upstream's does.
///
/// ## Why the allocation is made twice
///
/// The size has to be *asked for* fallibly and then *obtained* lazily, and no
/// single call does both.
///
/// `try_reserve_exact` is the fallible half. It cannot be the whole answer,
/// because a `Vec` reserved that way has length zero, and the only safe way to
/// give it a length is `resize`, which writes a zero into every byte. Writing
/// them is what makes the memory real: `dd bs=9999999999` asks for ten
/// gigabytes, which upstream's `malloc` hands back as untouched address space
/// and never faults in — the buffer is a *read target*, so on `if=/dev/null`
/// not one page of it is ever used. Zeroing it would fault in all ten
/// gigabytes and reach the OOM killer on a copy GNU completes instantly.
///
/// `vec![0u8; n]` is the lazy half: it is specialised to `alloc_zeroed`, so the
/// pages come from the kernel already zero and stay untouched until read. But
/// it aborts the process when the allocator says no, which is exactly the
/// diagnostic being avoided.
///
/// So the reservation runs first as a *probe* and is dropped, and the buffer
/// that survives is the zeroed one. The probe costs an anonymous mapping and
/// its immediate unmapping — it is never written to either — and it is what
/// turns an abort into the sentence above.
fn alloc_buffer(size: i64, extra: usize, what: &str) -> Result<Vec<u8>, Fatal> {
    if let Ok(n) = usize::try_from(size)
        && let Some(n) = n.checked_add(extra)
        && {
            let mut probe: Vec<u8> = Vec::new();
            probe.try_reserve_exact(n).is_ok()
        }
    {
        return Ok(vec![0u8; n]);
    }
    let pretty = human::human_readable(
        size.max(0).cast_unsigned(),
        human_opts() | Opts::BASE_1024,
        1,
        1,
    );
    Err(Fatal::plain(format!(
        "memory exhausted by {what} buffer of size {size} bytes ({pretty})"
    )))
}

/// Apply the composed translation table to a buffer in place.
fn translate_buffer(table: &[u8; 256], buf: &mut [u8]) {
    for b in buf {
        *b = table[usize::from(*b)];
    }
}

/// `conv=swab`: exchange every pair of bytes, carrying an odd byte over to the
/// next call.
///
/// `buf` must have one byte of room past `*nread`, because the swap is done by
/// moving every other byte two positions toward the end — which touches
/// `buf[*nread]`. Returns the index the data now starts at, either 0 or 1.
///
/// The carry is what makes `conv=swab` independent of the block size: the same
/// input swaps identically at `bs=2`, `bs=3` and `bs=4`, which was measured
/// (`abcde` → `badce` for all three) and is not what a naive per-block swap
/// would do.
fn swab_buffer(buf: &mut [u8], nread: &mut usize, saved_byte: &mut i32) -> usize {
    if *nread == 0 {
        return 0;
    }

    let prev_saved = *saved_byte;
    if (prev_saved < 0) == (*nread % 2 == 1) {
        *nread -= 1;
        *saved_byte = i32::from(buf[*nread]);
    } else {
        *saved_byte = -1;
    }

    let mut i = *nread;
    while 1 < i {
        buf[i] = buf[i - 2];
        i -= 2;
    }

    if prev_saved < 0 {
        return 1;
    }
    buf[1] = u8::try_from(prev_saved).unwrap_or(0);
    *nread += 1;
    0
}

/// The copy stopped early and its diagnostic has already been printed.
///
/// Upstream spells this `quit (EXIT_FAILURE)`, which prints the statistics
/// and exits from wherever it is called. Returning instead keeps the control
/// flow visible at every `?`, and the statistics are printed by the one
/// caller that owns them.
struct Aborted;

/// The output half of the copy, plus everything the conversions need to
/// remember between blocks.
struct Dd {
    conversions: u32,
    /// `ibs=` and `obs=`, kept as the signed counts the operands were parsed
    /// into rather than as `usize`, because a size too large to allocate must
    /// still be *printable* — `memory exhausted by input buffer of size
    /// 99999999999999 bytes` names a number that would not survive the
    /// conversion.
    ibs: i64,
    obs: i64,
    /// `cbs=`, as a signed count so it can be compared with [`Dd::col`].
    cbs: i64,
    trans: Translation,
    max_records: i64,
    max_bytes: i64,
    /// Whether input and output use separate buffers, so that partial reads
    /// are re-aggregated into whole output records.
    two_bufs: bool,
    out: Writer,
    obuf: Vec<u8>,
    /// Bytes currently in `obuf`.
    oc: usize,
    /// Position within the current `cbs`-sized record, for `conv=block` and
    /// `conv=unblock`.
    col: i64,
    /// Spaces seen by `conv=unblock` that have not yet been decided about:
    /// they are dropped if the record ends, emitted if anything else follows.
    pending_spaces: i64,
    stats: Stats,
    rep: Reporter,
}

impl Dd {
    /// Write, then empty, the output buffer.
    ///
    /// The buffer's length *is* `obs=`: this and the two callers below run
    /// only when `two_bufs` holds, and [`Dd::ensure_obuf`] allocates exactly
    /// `obs` bytes in that case. Taking the size from the buffer rather than
    /// from `self.obs` keeps the indexing provably in range.
    fn write_output(&mut self) -> Result<(), Aborted> {
        let obs = self.obuf.len();
        let nwritten = self.out.iwrite(&self.obuf);
        self.stats.w_bytes += i64::try_from(nwritten).unwrap_or(i64::MAX);
        self.oc = 0;
        if nwritten == obs {
            self.stats.w_full += 1;
            return Ok(());
        }
        if nwritten != 0 {
            self.stats.w_partial += 1;
        }
        stdfd::diag_line(&format!(
            "dd: writing to {}: {}",
            quoteaf_os(&self.out.name),
            self.out.reason()
        ));
        Err(Aborted)
    }

    /// Append one byte to the output buffer, flushing when it fills.
    fn output_char(&mut self, c: u8) -> Result<(), Aborted> {
        self.obuf[self.oc] = c;
        self.oc += 1;
        if self.oc >= self.obuf.len() {
            self.write_output()?;
        }
        Ok(())
    }

    /// No conversion: move bytes into the output buffer, flushing whenever it
    /// fills. This is what re-aggregates short reads into whole records.
    fn copy_simple(&mut self, buf: &[u8]) -> Result<(), Aborted> {
        let mut start = 0;
        let mut nread = buf.len();
        loop {
            let nfree = nread.min(self.obuf.len() - self.oc);
            self.obuf[self.oc..self.oc + nfree].copy_from_slice(&buf[start..start + nfree]);
            nread -= nfree;
            start += nfree;
            self.oc += nfree;
            if self.oc >= self.obuf.len() {
                self.write_output()?;
            }
            if nread == 0 {
                return Ok(());
            }
        }
    }

    /// `conv=block`: pad each newline-terminated record out to `cbs` with
    /// spaces, dropping the newline. A record longer than `cbs` is cut, and
    /// counted in the `N truncated records` line.
    fn copy_with_block(&mut self, buf: &[u8]) -> Result<(), Aborted> {
        for &b in buf {
            if b == self.trans.newline_character {
                for _ in self.col..self.cbs {
                    self.output_char(self.trans.space_character)?;
                }
                self.col = 0;
            } else {
                if self.col == self.cbs {
                    self.stats.r_truncate += 1;
                } else if self.col < self.cbs {
                    self.output_char(b)?;
                }
                self.col += 1;
            }
        }
        Ok(())
    }

    /// `conv=unblock`: replace the trailing spaces of each `cbs`-sized record
    /// with a newline.
    ///
    /// The record boundary is detected one byte late — the byte that would be
    /// the `cbs + 1`th is pushed back and re-read after the newline goes out —
    /// which is why the loop advances `i` on every path but one.
    fn copy_with_unblock(&mut self, buf: &[u8]) -> Result<(), Aborted> {
        let mut i = 0;
        while i < buf.len() {
            let c = buf[i];
            let col = self.col;
            self.col += 1;

            if col >= self.cbs {
                self.col = 0;
                self.pending_spaces = 0;
                self.output_char(self.trans.newline_character)?;
                continue;
            }

            if c == self.trans.space_character {
                self.pending_spaces += 1;
            } else {
                // A run of spaces that turned out not to be at the end of the
                // record after all; they are real data and must go out.
                while 0 < self.pending_spaces {
                    self.output_char(self.trans.space_character)?;
                    self.pending_spaces -= 1;
                }
                self.output_char(c)?;
            }
            i += 1;
        }
        Ok(())
    }

    /// Dispatch a converted block to whichever of the three copiers applies.
    fn copy_block(&mut self, buf: &[u8]) -> Result<(), Aborted> {
        if self.conversions & C_BLOCK != 0 {
            self.copy_with_block(buf)
        } else if self.conversions & C_UNBLOCK != 0 {
            self.copy_with_unblock(buf)
        } else {
            self.copy_simple(buf)
        }
    }

    /// `conv=fdatasync` / `conv=fsync`, run once however many times it is
    /// called. Returns the exit status.
    fn synchronize_output(&mut self) -> u8 {
        let mask = self.conversions;
        self.conversions &= !(C_FDATASYNC | C_FSYNC);

        let mut status = 0;
        let mut want_fsync = mask & C_FSYNC != 0;

        if mask & C_FDATASYNC != 0
            && let Err(e) = self.out.handle.file().sync_data()
        {
            // A device that cannot be synced is not a failed copy.
            if !matches!(
                e.kind(),
                io::ErrorKind::Unsupported | io::ErrorKind::InvalidInput
            ) {
                stdfd::diag_line(&format!(
                    "dd: fdatasync failed for {}: {}",
                    quoteaf_os(&self.out.name),
                    errmsg::strerror(&e)
                ));
                status = 1;
            }
            want_fsync = true;
        }

        if want_fsync && let Err(e) = self.out.handle.file().sync_all() {
            stdfd::diag_line(&format!(
                "dd: fsync failed for {}: {}",
                quoteaf_os(&self.out.name),
                errmsg::strerror(&e)
            ));
            return 1;
        }

        status
    }
}

/// Why the copy stopped early.
///
/// The distinction is whether the statistics are printed on the way out.
/// Upstream's `quit (EXIT_FAILURE)` prints them — a failed write still copied
/// something, and the counts are the only record of how much — while its
/// `error (EXIT_FAILURE, ...)`, used for a buffer that could not be allocated,
/// exits before any copying has happened and prints nothing. Measured:
/// `dd bs=99999999999999` emits the `memory exhausted` line and no counts.
enum DdError {
    /// Report the sentence and exit 1, without statistics.
    Fatal(Fatal),
    /// The diagnostic is already out; print statistics and exit 1.
    Aborted,
}

impl From<Aborted> for DdError {
    fn from(_: Aborted) -> Self {
        DdError::Aborted
    }
}

impl From<Fatal> for DdError {
    fn from(fatal: Fatal) -> Self {
        DdError::Fatal(fatal)
    }
}

impl Dd {
    /// Allocate the output buffer if it has not been allocated already.
    ///
    /// Deliberately lazy, and deliberately not done at the top of
    /// [`dd_copy`]: `dd if=/dev/null bs=99999999999999 count=0` exits **0**
    /// under GNU, because a copy of zero records never reaches the allocation.
    /// Allocating eagerly would turn that into `memory exhausted`.
    ///
    /// Without `conv=` and without `ibs=`/`obs=` there is only one buffer —
    /// upstream aliases `obuf` to `ibuf` — so what is allocated here is an
    /// *input*-sized buffer, and it says so if it cannot be had.
    fn ensure_obuf(&mut self) -> Result<(), Fatal> {
        if !self.obuf.is_empty() {
            return Ok(());
        }
        self.obuf = if self.two_bufs {
            alloc_buffer(self.obs, 0, "output")?
        } else {
            alloc_buffer(
                self.ibs,
                usize::from(self.conversions & C_SWAB != 0),
                "input",
            )?
        };
        Ok(())
    }
}

/// How far a stream was moved, and how it was moved.
///
/// [`skip_input`] and [`seek_output`] share this because upstream shares one
/// `skip` between them, keyed on the descriptor number. The two differ in
/// three details — which buffer the read fallback uses, whether the input
/// offset is advanced, and whether the diagnostic says `cannot skip` or
/// `cannot seek` — and are written out separately here rather than reproducing
/// the `fdesc == STDIN_FILENO` tests.
struct Skipped {
    /// Whole blocks that could *not* be skipped, because the stream ended.
    records: i64,
}

/// Advance the input past `records` blocks plus `*bytes` bytes.
///
/// Prefers one `lseek`; falls back to reading and discarding when the stream
/// cannot seek, which is the only thing that works on a pipe. `*bytes` is left
/// as whatever remains unskipped of the byte tail.
///
/// A skip that runs off the end of the input is **not** an error: the count of
/// blocks left over comes back in [`Skipped::records`] and the caller turns it
/// into the `cannot skip to specified offset` warning. Measured:
/// `printf abc | dd bs=1 skip=10` prints that warning, `0+0 records in`, and
/// exits 0.
fn skip_input(
    dd: &mut Dd,
    reader: &mut Reader,
    records: i64,
    bytes: &mut i64,
    ibuf: &mut Vec<u8>,
) -> Result<Skipped, DdError> {
    let blocksize = dd.ibs;
    let mut records = records;
    let offset = records
        .checked_mul(blocksize)
        .and_then(|o| o.checked_add(*bytes));

    // `None` means the product did not fit, so no seek is attempted at all —
    // which upstream distinguishes from a seek that was attempted and failed
    // by leaving `errno` zero, and reports as `EOVERFLOW` further down.
    let seek_error = match offset {
        Some(offset) => match reader.handle.file().seek(SeekFrom::Current(offset)) {
            Ok(_) => {
                let md = match reader.handle.file().metadata() {
                    Ok(md) => md,
                    Err(e) => {
                        return Err(DdError::Fatal(Fatal::plain(format!(
                            "cannot fstat {}: {}",
                            quoteaf_os(&reader.name),
                            errmsg::strerror(&e)
                        ))));
                    }
                };
                let mut moved = offset;
                let size = i64::try_from(md.len()).unwrap_or(i64::MAX);
                // Only a file whose size means something can say in advance
                // that the skip overran it; for anything else the shortfall
                // shows up later, as a read that returns nothing.
                if filekind::is_regular(reader.handle.file())
                    && 0 <= reader.offset
                    && size.saturating_sub(reader.offset) < offset
                {
                    records = (offset - size) / blocksize;
                    moved = size - reader.offset;
                } else {
                    records = 0;
                }
                reader.advance(moved);
                return Ok(Skipped { records });
            }
            Err(e) => Some(e),
        },
        None => None,
    };

    // The seek may have failed because the target is past the end of a device
    // rather than because the stream cannot seek at all. Asking for the end
    // tells the two apart, and if the stream *can* seek there is nothing to be
    // gained by reading forwards to a place it has already refused.
    if reader.handle.file().seek(SeekFrom::End(0)).is_ok() {
        let reason = seek_error.as_ref().map_or_else(
            || "Value too large for defined data type".to_string(),
            errmsg::strerror,
        );
        stdfd::diag_line(&format!(
            "dd: {}: cannot skip: {reason}",
            quotef_os(&reader.name)
        ));
        return Err(DdError::Aborted);
    }

    if ibuf.is_empty() {
        *ibuf = alloc_buffer(dd.ibs, usize::from(dd.conversions & C_SWAB != 0), "input")?;
    }
    let capacity = ibuf
        .len()
        .saturating_sub(usize::from(dd.conversions & C_SWAB != 0));

    loop {
        let want = if records != 0 { blocksize } else { *bytes };
        let want = usize::try_from(want).unwrap_or(usize::MAX).min(capacity);
        match reader.read_block(&mut ibuf[..want]) {
            Err(e) => {
                stdfd::diag_line(&format!(
                    "dd: error reading {}: {}",
                    quoteaf_os(&reader.name),
                    errmsg::strerror(&e)
                ));
                // `conv=noerror` asked for the copy to survive read errors, so
                // the counts so far are worth having even though this one is
                // fatal anyway.
                if dd.conversions & C_NOERROR != 0 {
                    let stats = dd.stats;
                    dd.rep.print_stats(&stats);
                }
                return Err(DdError::Aborted);
            }
            Ok(0) => break,
            Ok(n) => reader.advance(i64::try_from(n).unwrap_or(i64::MAX)),
        }

        if records != 0 {
            records -= 1;
        } else {
            *bytes = 0;
        }
        if records == 0 && *bytes == 0 {
            break;
        }
    }

    Ok(Skipped { records })
}

/// [`skip_input`] for the output side.
///
/// The read fallback reads from the *output* descriptor, which looks wrong and
/// is upstream's own code: it is how a seekable-but-not-lseekable output would
/// be advanced. In practice it is what produces the diagnostic for a pipe —
/// `printf abc | dd seek=1` reports
/// `dd: 'standard output': cannot seek: Illegal seek`, because the read fails
/// and the reason printed is the *original* `lseek` errno, not the read's.
fn seek_output(dd: &mut Dd, records: i64, bytes: &mut i64) -> Result<Skipped, DdError> {
    let blocksize = dd.obs;
    let mut records = records;
    let offset = records
        .checked_mul(blocksize)
        .and_then(|o| o.checked_add(*bytes));

    let seek_error = match offset {
        Some(offset) => match dd.out.handle.file().seek(SeekFrom::Current(offset)) {
            Ok(_) => {
                // Nothing is left over: unlike the input side there is no
                // end-of-file to run into, since a write past the end extends.
                *bytes = 0;
                return Ok(Skipped { records: 0 });
            }
            Err(e) => Some(e),
        },
        None => None,
    };

    if dd.out.handle.file().seek(SeekFrom::End(0)).is_ok() {
        let reason = seek_error.as_ref().map_or_else(
            || "Value too large for defined data type".to_string(),
            errmsg::strerror,
        );
        stdfd::diag_line(&format!(
            "dd: {}: cannot seek: {reason}",
            quotef_os(&dd.out.name)
        ));
        return Err(DdError::Aborted);
    }

    dd.ensure_obuf()?;
    let reason = seek_error.as_ref().map_or_else(
        || "Value too large for defined data type".to_string(),
        errmsg::strerror,
    );

    loop {
        let want = if records != 0 { blocksize } else { *bytes };
        let want = usize::try_from(want)
            .unwrap_or(usize::MAX)
            .min(dd.obuf.len());
        let read = loop {
            match dd.out.handle.file().read(&mut dd.obuf[..want]) {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                other => break other,
            }
        };
        match read {
            Err(_) => {
                stdfd::diag_line(&format!(
                    "dd: {}: cannot seek: {reason}",
                    quotef_os(&dd.out.name)
                ));
                return Err(DdError::Aborted);
            }
            Ok(0) => break,
            Ok(_) => {}
        }

        if records != 0 {
            records -= 1;
        } else {
            *bytes = 0;
        }
        if records == 0 && *bytes == 0 {
            break;
        }
    }

    Ok(Skipped { records })
}

/// The copy itself: skip, seek, then read-convert-write until the count runs
/// out or the input ends.
///
/// Returns the exit status. `Err` means the same thing with the statistics
/// still to print — see [`DdError`].
#[allow(clippy::too_many_lines)]
fn dd_copy(dd: &mut Dd, reader: &mut Reader, set: &Settings) -> Result<u8, DdError> {
    let mut exit_status = 0u8;
    // Size of the previous read if it was short, else 0. What makes
    // `conv=noerror,sync` replace only the part of a record that is missing.
    let mut partread = 0usize;
    let swab = dd.conversions & C_SWAB != 0;
    let mut ibuf: Vec<u8> = Vec::new();

    if set.skip_records != 0 || set.skip_bytes != 0 {
        let mut bytes = set.skip_bytes;
        let requested = set
            .skip_records
            .checked_mul(dd.ibs)
            .and_then(|v| v.checked_add(set.skip_bytes));
        let offset0 = reader.offset;
        let unskipped = skip_input(dd, reader, set.skip_records, &mut bytes, &mut ibuf)?;

        // Three ways to come up short: the file is smaller than the skip, a
        // pipe held less than the skip, or the reads that did the skipping
        // were partial. POSIX does not say what to do about any of them;
        // upstream warns and carries on, and so does this.
        let short =
            0 <= reader.offset && requested.is_none_or(|want| want != reader.offset - offset0);
        if (unskipped.records != 0 || short) && dd.rep.level != StatusLevel::None {
            stdfd::diag_line(&format!(
                "dd: {}: cannot skip to specified offset",
                quotef_os(&reader.name)
            ));
        }
    }

    if set.seek_records != 0 || set.seek_bytes != 0 {
        let mut bytes = set.seek_bytes;
        let mut write_records = seek_output(dd, set.seek_records, &mut bytes)?.records;

        // Whatever the seek could not cover is covered by writing zeros. Note
        // that these bytes are *not* added to `w_bytes`: upstream does not
        // count them, and `dd seek=1 of=pipe` accordingly reports only what
        // the copy proper wrote.
        if write_records != 0 || bytes != 0 {
            let fill = if write_records != 0 { dd.obs } else { bytes };
            let fill = usize::try_from(fill)
                .unwrap_or(usize::MAX)
                .min(dd.obuf.len());
            dd.obuf[..fill].fill(0);
            loop {
                let size = if write_records != 0 { dd.obs } else { bytes };
                let size = usize::try_from(size)
                    .unwrap_or(usize::MAX)
                    .min(dd.obuf.len());
                if dd.out.iwrite(&dd.obuf[..size]) != size {
                    stdfd::diag_line(&format!(
                        "dd: writing to {}: {}",
                        quoteaf_os(&dd.out.name),
                        dd.out.reason()
                    ));
                    return Err(DdError::Aborted);
                }
                if write_records != 0 {
                    write_records -= 1;
                } else {
                    bytes = 0;
                }
                if write_records == 0 && bytes == 0 {
                    break;
                }
            }
        }
    }

    // `count=0`. Returning here rather than falling through is what keeps
    // `dd bs=99999999999999 count=0` from allocating — see [`Dd::ensure_obuf`].
    if dd.max_records == 0 && dd.max_bytes == 0 {
        return Ok(exit_status);
    }

    if ibuf.is_empty() {
        ibuf = alloc_buffer(dd.ibs, usize::from(swab), "input")?;
    }
    dd.ensure_obuf()?;
    let ibs = ibuf.len() - usize::from(swab);
    let mut saved_byte: i32 = -1;

    loop {
        let stats = dd.stats;
        dd.rep.tick(&stats);

        if dd.stats.r_partial + dd.stats.r_full
            >= dd.max_records.saturating_add(i64::from(dd.max_bytes != 0))
        {
            break;
        }

        // Zeroed before the read, not after, so that a read error leaves the
        // bytes it did manage followed by padding rather than by whatever the
        // last record held.
        if dd.conversions & C_SYNC != 0 && dd.conversions & C_NOERROR != 0 {
            let pad = if dd.conversions & (C_BLOCK | C_UNBLOCK) != 0 {
                b' '
            } else {
                0
            };
            ibuf[..ibs].fill(pad);
        }

        // The last record of a `count=` given in bytes is short by design.
        let want = if dd.stats.r_partial + dd.stats.r_full >= dd.max_records {
            usize::try_from(dd.max_bytes).unwrap_or(0).min(ibs)
        } else {
            ibs
        };

        let mut n_bytes_read = match reader.read_block(&mut ibuf[..want]) {
            Ok(0) => break,
            Ok(n) => {
                reader.advance(i64::try_from(n).unwrap_or(i64::MAX));
                n
            }
            Err(e) => {
                if dd.conversions & C_NOERROR == 0 || dd.rep.level != StatusLevel::None {
                    stdfd::diag_line(&format!(
                        "dd: error reading {}: {}",
                        quoteaf_os(&reader.name),
                        errmsg::strerror(&e)
                    ));
                }
                if dd.conversions & C_NOERROR == 0 {
                    // Whatever is already in the output buffer still goes out;
                    // that is what breaking rather than returning is for.
                    exit_status = 1;
                    break;
                }
                let stats = dd.stats;
                dd.rep.print_stats(&stats);
                let bad_portion = dd.ibs - i64::try_from(partread).unwrap_or(0);
                if !reader.advance_after_read_error(bad_portion) {
                    exit_status = 1;
                    // One diagnostic for a bad region, not one per block.
                    reader.seekable = false;
                    reader.seek_error = Some(io::Error::from(io::ErrorKind::NotSeekable));
                }
                if dd.conversions & C_SYNC != 0 && partread == 0 {
                    // The buffer was zeroed above, so an empty record is a
                    // full record of padding.
                    0
                } else {
                    continue;
                }
            }
        };

        if n_bytes_read < ibs {
            dd.stats.r_partial += 1;
            partread = n_bytes_read;
            if dd.conversions & C_SYNC != 0 {
                if dd.conversions & C_NOERROR == 0 {
                    let pad = if dd.conversions & (C_BLOCK | C_UNBLOCK) != 0 {
                        b' '
                    } else {
                        0
                    };
                    ibuf[n_bytes_read..ibs].fill(pad);
                }
                n_bytes_read = ibs;
            }
        } else {
            dd.stats.r_full += 1;
            partread = 0;
        }

        // One buffer: the record read is the record written, short reads and
        // all. This is the `bs=` shape, and the reason `bs=4096` on a pipe
        // reports `0+2 records out` where `ibs=4096 obs=4096` reports `0+1`.
        if !dd.two_bufs {
            let nwritten = dd.out.iwrite(&ibuf[..n_bytes_read]);
            dd.stats.w_bytes += i64::try_from(nwritten).unwrap_or(i64::MAX);
            if nwritten != n_bytes_read {
                stdfd::diag_line(&format!(
                    "dd: error writing {}: {}",
                    quoteaf_os(&dd.out.name),
                    dd.out.reason()
                ));
                return Ok(1);
            }
            if n_bytes_read == ibs {
                dd.stats.w_full += 1;
            } else {
                dd.stats.w_partial += 1;
            }
            continue;
        }

        if dd.trans.needed {
            translate_buffer(&dd.trans.table, &mut ibuf[..n_bytes_read]);
        }
        let start = if swab {
            swab_buffer(&mut ibuf, &mut n_bytes_read, &mut saved_byte)
        } else {
            0
        };
        dd.copy_block(&ibuf[start..start + n_bytes_read])?;
    }

    // `conv=swab` holds a byte back whenever it has an odd one; at the end of
    // the input there is nothing left to pair it with, so it goes out alone.
    if 0 <= saved_byte {
        let c = u8::try_from(saved_byte).unwrap_or(0);
        if dd.conversions & C_BLOCK != 0 {
            dd.copy_with_block(&[c])?;
        } else if dd.conversions & C_UNBLOCK != 0 {
            dd.copy_with_unblock(&[c])?;
        } else {
            dd.output_char(c)?;
        }
    }

    // An input whose last line had no newline still owes a padded record.
    if dd.conversions & C_BLOCK != 0 && 0 < dd.col {
        for _ in dd.col..dd.cbs {
            dd.output_char(dd.trans.space_character)?;
        }
    }

    if dd.col != 0 && dd.conversions & C_UNBLOCK != 0 {
        dd.output_char(dd.trans.newline_character)?;
    }

    if dd.oc != 0 {
        let oc = dd.oc;
        let nwritten = dd.out.iwrite(&dd.obuf[..oc]);
        dd.stats.w_bytes += i64::try_from(nwritten).unwrap_or(i64::MAX);
        if nwritten != 0 {
            dd.stats.w_partial += 1;
        }
        if nwritten != oc {
            stdfd::diag_line(&format!(
                "dd: error writing {}: {}",
                quoteaf_os(&dd.out.name),
                dd.out.reason()
            ));
            return Ok(1);
        }
    }

    // A `conv=sparse` copy that ended on a seek has moved the file pointer
    // past the end without lengthening the file. Only an explicit truncation
    // makes the hole real, and only for something that has a length at all.
    if dd.out.final_op_was_seek {
        let md = match dd.out.handle.file().metadata() {
            Ok(md) => md,
            Err(e) => {
                stdfd::diag_line(&format!(
                    "dd: cannot fstat {}: {}",
                    quoteaf_os(&dd.out.name),
                    errmsg::strerror(&e)
                ));
                return Ok(1);
            }
        };
        if filekind::is_regular(dd.out.handle.file())
            && let Ok(at) = dd.out.handle.file().stream_position()
            && md.len() < at
            && let Err(e) = dd.out.handle.file().set_len(at)
        {
            stdfd::diag_line(&format!(
                "dd: failed to truncate to {at} bytes in output file {}: {}",
                quoteaf_os(&dd.out.name),
                errmsg::strerror(&e)
            ));
            return Ok(1);
        }
    }

    // `fsync` can take long enough that a progress display which stopped
    // updating looks like a hang. One last line before it starts.
    if dd.conversions & (C_FDATASYNC | C_FSYNC) != 0
        && dd.rep.level == StatusLevel::Progress
        && 0 <= dd.rep.reported_w_bytes
        && dd.rep.reported_w_bytes < dd.stats.w_bytes
    {
        let stats = dd.stats;
        dd.rep.xfer_stats(&stats, None);
    }

    Ok(exit_status)
}

// ---------------------------------------------------------------------------
// The command line
// ---------------------------------------------------------------------------

/// `--help`, which is upstream's with two edits.
///
/// The `FLAG` list drops every name whose `O_*` value is zero here — they are
/// rejected, so advertising them would be a lie — and gains the three that
/// upstream supports but only documents in `info`: `count_bytes`,
/// `skip_bytes` and `seek_bytes`. What is left is exactly the set
/// [`parse_symbols`] accepts, which is the point.
///
/// The `USR1` paragraph is gone; see the module documentation.
///
/// The odd two-space gaps after `fdatasync` and `fullblock` are upstream's:
/// those names are one column wider than the table they sit in.
fn help_text() -> String {
    "\
Usage: dd [OPERAND]...
  or:  dd OPTION
Copy a file, converting and formatting according to the operands.

  bs=BYTES        read and write up to BYTES bytes at a time (default: 512);
                  overrides ibs and obs
  cbs=BYTES       convert BYTES bytes at a time
  conv=CONVS      convert the file as per the comma separated symbol list
  count=N         copy only N input blocks
  ibs=BYTES       read up to BYTES bytes at a time (default: 512)
  if=FILE         read from FILE instead of stdin
  iflag=FLAGS     read as per the comma separated symbol list
  obs=BYTES       write BYTES bytes at a time (default: 512)
  of=FILE         write to FILE instead of stdout
  oflag=FLAGS     write as per the comma separated symbol list
  seek=N          (or oseek=N) skip N obs-sized output blocks
  skip=N          (or iseek=N) skip N ibs-sized input blocks
  status=LEVEL    The LEVEL of information to print to stderr;
                  'none' suppresses everything but error messages,
                  'noxfer' suppresses the final transfer statistics,
                  'progress' shows periodic transfer statistics

N and BYTES may be followed by the following multiplicative suffixes:
c=1, w=2, b=512, kB=1000, K=1024, MB=1000*1000, M=1024*1024, xM=M,
GB=1000*1000*1000, G=1024*1024*1024, and so on for T, P, E, Z, Y, R, Q.
Binary prefixes can be used, too: KiB=K, MiB=M, and so on.
If N ends in 'B', it counts bytes not blocks.

Each CONV symbol may be:

  ascii     from EBCDIC to ASCII
  ebcdic    from ASCII to EBCDIC
  ibm       from ASCII to alternate EBCDIC
  block     pad newline-terminated records with spaces to cbs-size
  unblock   replace trailing spaces in cbs-size records with newline
  lcase     change upper case to lower case
  ucase     change lower case to upper case
  sparse    try to seek rather than write all-NUL output blocks
  swab      swap every pair of input bytes
  sync      pad every input block with NULs to ibs-size; when used
            with block or unblock, pad with spaces rather than NULs
  excl      fail if the output file already exists
  nocreat   do not create the output file
  notrunc   do not truncate the output file
  noerror   continue after read errors
  fdatasync  physically write output file data before finishing
  fsync     likewise, but also write metadata

Each FLAG symbol may be:

  append    append mode (makes sense only for output; conv=notrunc suggested)
  fullblock  accumulate full blocks of input (iflag only)
  count_bytes  treat 'count=N' as a byte count (iflag only)
  skip_bytes  treat 'skip=N' as a byte count (iflag only)
  seek_bytes  treat 'seek=N' as a byte count (oflag only)

Options are:

      --help        display this help and exit
      --version     output version information and exit
"
    .to_string()
}

fn version_text() -> String {
    "dd (SlateOS coreutils) 0.1.0\n".to_string()
}

/// What a successful scan of `argv` asks for.
#[derive(Debug)]
enum Request {
    Help,
    Version,
    Run(Box<Settings>),
}

/// The two long options, then every operand.
///
/// Upstream calls `parse_gnu_standard_options_only` before `scanargs`, so the
/// options are recognised wherever they appear and `dd --version if=/dev/null`
/// prints the version — measured, and the reason this is a getopt pass rather
/// than a scan of `argv[1]`.
fn parse_args(argv: &[OsString]) -> Result<Request, Fatal> {
    let mut operands: Vec<OsString> = Vec::new();

    for item in DD.parse(argv, SHORT_OPTIONS, LONG_OPTIONS) {
        match item {
            Ok(getopt::Opt::Long("help", _)) => return Ok(Request::Help),
            Ok(getopt::Opt::Long("version", _)) => return Ok(Request::Version),
            Ok(getopt::Opt::Operand(value)) => operands.push(value.clone()),
            // `SHORT_OPTIONS` is empty and `LONG_OPTIONS` has two entries, so
            // there is nothing else a successful parse can yield.
            Ok(_) => {}
            // getopt's sentence carries the referral, as `usage (EXIT_FAILURE)`
            // does: `dd -x` prints `invalid option -- 'x'` and then
            // `Try 'dd --help' for more information.`
            Err(e) => return Err(Fatal::usage(e.sentence)),
        }
    }

    Ok(Request::Run(Box::new(scan_args(&operands)?)))
}

// ---------------------------------------------------------------------------
// Wiring
// ---------------------------------------------------------------------------

/// `set_fd_flags` for the one flag that survives: `oflag=append` on a
/// descriptor that was inherited rather than opened.
fn apply_fd_flags(fd: i32, flags: u32, name: &OsStr) -> Result<(), Fatal> {
    if flags & F_APPEND == 0 {
        return Ok(());
    }
    set_append(fd).map_err(|e| {
        Fatal::plain(format!(
            "setting flags for {}: {}",
            quoteaf_os(name),
            errmsg::strerror(&e)
        ))
    })
}

/// `lseek (fd, 0, SEEK_CUR)`, and the reason it failed.
///
/// Not simply [`Seek::stream_position`], because on the host the differential
/// tests run on a pipe answers that call successfully and moves nothing (see
/// [`coreutils::filekind`]). Off Unix the answer is therefore believed only
/// for a handle that is genuinely a file.
fn probe_offset(file: &mut File) -> (i64, Option<io::Error>) {
    if !cfg!(unix) && filekind::regular(file) != Some(true) {
        return (0, Some(io::Error::from(io::ErrorKind::NotSeekable)));
    }
    match file.stream_position() {
        Ok(at) => (i64::try_from(at).unwrap_or(i64::MAX), None),
        Err(e) => (0, Some(e)),
    }
}

/// Open the input, or borrow descriptor 0.
fn open_input(set: &Settings) -> Result<Reader, Fatal> {
    // `iflag=append` is passed to `open` by upstream and is a no-op for a
    // descriptor that is only ever read; it is dropped here because Rust's
    // `OpenOptions::append` implies write access, which would turn
    // `dd if=read-only-file iflag=append` from a working command into
    // `Permission denied`.
    let (handle, name) = if let Some(path) = &set.input_file {
        let file = File::open(path).map_err(|e| {
            Fatal::plain(format!(
                "failed to open {}: {}",
                quoteaf_os(path),
                errmsg::strerror(&e)
            ))
        })?;
        (Handle::Owned(file), path.clone())
    } else {
        let name = OsString::from("standard input");
        apply_fd_flags(0, set.input_flags, &name)?;
        let file = filekind::borrowed(0)
            .ok_or_else(|| Fatal::plain("standard input is not available".to_string()))?;
        (Handle::Std(file), name)
    };

    let mut handle = handle;
    let (offset, seek_error) = probe_offset(handle.file());
    Ok(Reader {
        handle,
        name,
        seekable: seek_error.is_none(),
        seek_error,
        offset,
        fullblock: set.fullblock,
        warn_partial_read: set.warn_partial_read,
        prev_nread: 0,
        quiet: set.status_level == StatusLevel::None,
    })
}

/// Open the output, or borrow descriptor 1, and truncate it where upstream's
/// `O_TRUNC` and `ftruncate` would.
///
/// Returns the writer and the exit status so far, which is 1 when a
/// truncation at the seek offset was refused for something that should have
/// accepted it.
fn open_output(set: &Settings) -> Result<(Writer, u8), Fatal> {
    let mut status = 0u8;
    let notrunc = set.conversions & C_NOTRUNC != 0;
    let append = set.output_flags & F_APPEND != 0;

    let (handle, name) = match &set.output_file {
        None => {
            let name = OsString::from("standard output");
            apply_fd_flags(1, set.output_flags, &name)?;
            let file = filekind::borrowed(1)
                .ok_or_else(|| Fatal::plain("standard output is not available".to_string()))?;
            (Handle::Std(file), name)
        }
        Some(path) => {
            // The size a `seek=` asks the file to be cut to. `None` means the
            // multiplication overflowed, which is only an error if a
            // truncation was going to happen at all.
            let size = set
                .seek_records
                .checked_mul(set.obs)
                .and_then(|s| set.seek_bytes.checked_add(s));
            if size.is_none() && !notrunc {
                return Err(Fatal::plain(format!(
                    "offset too large: cannot truncate to a length of seek={} ({}-byte) blocks",
                    set.seek_records, set.obs
                )));
            }

            // `O_TRUNC` applies only when the copy starts at offset zero:
            // `dd of=big seek=10` cuts the file at ten blocks, not at nothing,
            // and that is done below with `set_len`.
            let truncate = set.seek_records == 0 && !notrunc;
            let mut opts = OpenOptions::new();
            opts.write(true);
            if append {
                opts.append(true);
            }
            if set.conversions & C_EXCL != 0 {
                opts.create_new(true);
            } else if set.conversions & C_NOCREAT == 0 {
                opts.create(true);
            }
            // `append` and `truncate` cannot be asked for together through
            // `OpenOptions`, though `O_APPEND | O_TRUNC` is a legal open; the
            // truncation is done by hand in that case.
            if truncate && !append {
                opts.truncate(true);
            }

            // Read access only if a `seek=` might have to be satisfied by
            // reading. If the file cannot be read, write-only might still work.
            let mut file = None;
            if set.seek_records != 0 {
                file = opts.read(true).open(path).ok();
            }
            let file = match file {
                Some(file) => file,
                None => opts.read(false).open(path).map_err(|e| {
                    Fatal::plain(format!(
                        "failed to open {}: {}",
                        quoteaf_os(path),
                        errmsg::strerror(&e)
                    ))
                })?,
            };

            if truncate && append && filekind::is_regular(&file) {
                file.set_len(0).map_err(|e| {
                    Fatal::plain(format!(
                        "failed to open {}: {}",
                        quoteaf_os(path),
                        errmsg::strerror(&e)
                    ))
                })?;
            }

            if set.seek_records != 0
                && !notrunc
                && let Some(size) = size
                && let Err(e) = file.set_len(size.max(0).cast_unsigned())
            {
                // POSIX defines `ftruncate` only for these, so a refusal from
                // anything else — a tape, a terminal — is not a failure.
                let regular = filekind::regular(&file);
                let directory = file.metadata().is_ok_and(|m| m.is_dir());
                if regular == Some(true) || directory {
                    stdfd::diag_line(&format!(
                        "dd: failed to truncate to {size} bytes in output file {}: {}",
                        quoteaf_os(path),
                        errmsg::strerror(&e)
                    ));
                    status = 1;
                }
            }

            (Handle::Owned(file), path.clone())
        }
    };

    Ok((
        Writer {
            handle,
            name,
            sparse: set.conversions & C_SPARSE != 0,
            final_op_was_seek: false,
            error: None,
        },
        status,
    ))
}

/// Close a descriptor the way upstream's `iclose` does, so that a write error
/// only reported at close time is still reported.
///
/// On Unix the descriptor is closed by number, because that is the only way to
/// see the result: `File`'s `Drop` closes it and discards the error. Off Unix
/// a borrowed standard descriptor is left alone — there its handle belongs to
/// `std::io::Stdout`, which will close it again, and a double close is a worse
/// bug than a missed diagnostic.
///
/// The `Result` is always `Ok` off Unix, which is the whole of what
/// `unnecessary_wraps` sees when it compiles for Windows; on the target — the
/// only platform whose behaviour is being specified — it is the `close(2)`
/// result and the reason the function exists.
#[allow(clippy::unnecessary_wraps)]
fn close_handle(handle: Handle, fd: i32) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::ffi::c_int;
        use std::os::fd::IntoRawFd;

        unsafe extern "C" {
            fn close(fd: c_int) -> c_int;
        }

        let fd = match handle {
            Handle::Owned(file) => file.into_raw_fd(),
            Handle::Std(_) => fd,
        };
        // SAFETY: `fd` is an open descriptor this process owns, and ownership
        // of an `Owned` handle has just been given up by `into_raw_fd`, so
        // nothing will close it a second time.
        if unsafe { close(fd) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = fd;
        if let Handle::Owned(file) = handle {
            drop(file);
        }
        Ok(())
    }
}

/// Everything after the operands have been understood.
fn copy(set: &Settings) -> Result<u8, Fatal> {
    let mut reader = open_input(set)?;
    let (out, open_status) = open_output(set)?;

    let mut dd = Dd {
        conversions: set.conversions,
        ibs: set.ibs,
        obs: set.obs,
        cbs: set.cbs,
        trans: apply_translations(set.conversions),
        max_records: set.max_records,
        max_bytes: set.max_bytes,
        two_bufs: set.conversions & C_TWOBUFS != 0,
        out,
        obuf: Vec::new(),
        oc: 0,
        col: 0,
        pending_spaces: 0,
        stats: Stats::default(),
        // The clock starts here, after the opens, as upstream's does.
        rep: Reporter::new(set.status_level),
    };

    // Upstream overwrites the status from a refused `ftruncate` with
    // `dd_copy`'s rather than combining them, so a copy that then succeeds
    // exits 0. Transcribed rather than corrected: it is the observable
    // behaviour, and `dd` is not the place to be creative.
    let _ = open_status;

    let (status, aborted) = match dd_copy(&mut dd, &mut reader, set) {
        Ok(status) => (status, false),
        Err(DdError::Aborted) => (1, true),
        // `error (EXIT_FAILURE, ...)` exits on the spot: no synchronisation,
        // no statistics, no close diagnostics.
        Err(DdError::Fatal(fatal)) => return Err(fatal),
    };

    let sync_status = dd.synchronize_output();
    if sync_status != 0 {
        if aborted {
            // `cleanup` exits with the synchronisation status before it can
            // reach `print_stats`.
            return Ok(sync_status);
        }
        return Ok(finish(dd, reader, sync_status));
    }

    Ok(finish(dd, reader, status))
}

/// `finish_up`: close both descriptors, then print the statistics.
///
/// Both structures are consumed rather than borrowed, because closing a
/// descriptor means giving up the [`Handle`] that owns it, and a `Dd` with its
/// output handle moved out is not a `Dd` any more. Destructuring says that in
/// the type system; the alternative — swapping a placeholder in — would need a
/// `File` conjured from nothing.
fn finish(dd: Dd, reader: Reader, status: u8) -> u8 {
    let Dd {
        out,
        stats,
        mut rep,
        ..
    } = dd;
    let Reader {
        handle: in_handle,
        name: in_name,
        ..
    } = reader;
    let Writer {
        handle: out_handle,
        name: out_name,
        ..
    } = out;

    if let Err(e) = close_handle(in_handle, 0) {
        stdfd::diag_line(&format!(
            "dd: closing input file {}: {}",
            quoteaf_os(&in_name),
            errmsg::strerror(&e)
        ));
        return 1;
    }
    if let Err(e) = close_handle(out_handle, 1) {
        stdfd::diag_line(&format!(
            "dd: closing output file {}: {}",
            quoteaf_os(&out_name),
            errmsg::strerror(&e)
        ));
        return 1;
    }

    rep.print_stats(&stats);
    status
}

fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    stdfd::restore();

    let argv: Vec<OsString> = std::env::args_os().skip(1).collect();
    let request = match parse_args(&argv) {
        Ok(request) => request,
        Err(fatal) => {
            fatal.report();
            return ExitCode::from(1);
        }
    };

    match request {
        Request::Help | Request::Version => {
            let mut out = Stream::stdout();
            let text = if matches!(request, Request::Help) {
                help_text()
            } else {
                version_text()
            };
            let _ = out.write_all(text.as_bytes());
            stdfd::close_stdout("dd", out, ExitCode::SUCCESS)
        }
        Request::Run(set) => match copy(&set) {
            Ok(status) => ExitCode::from(status),
            Err(fatal) => {
                fatal.report();
                ExitCode::from(1)
            }
        },
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::pedantic)]
mod tests {
    use super::{
        C_ASCII, C_BLOCK, C_EBCDIC, C_LCASE, C_NOERROR, C_SPARSE, C_SWAB, C_SYNC, C_TWOBUFS,
        C_UCASE, C_UNBLOCK, CONVERSIONS, F_APPEND, F_COUNT_BYTES, F_SEEK_BYTES, F_SKIP_BYTES,
        FLAGS, Fatal, Request, STATUSES, Settings, StatusLevel, abbreviation_lacks_prefix,
        apply_translations, help_text, multiple_bits_set, operand_matches, parse_args,
        parse_integer, parse_symbols, scan_args, swab_buffer, version_text,
    };
    use coreutils::xnum::Status;
    use std::ffi::OsString;

    /// The whole point of the rewrite: an operand value is `argv`, which on
    /// this OS may hold any byte but `/` and NUL, so a path is carried as
    /// bytes and never decoded.
    fn os(bytes: &[u8]) -> OsString {
        coreutils::quote::os_from_bytes(bytes)
    }

    fn settings(words: &[&str]) -> Settings {
        match scan_args(&words.iter().map(|w| OsString::from(*w)).collect::<Vec<_>>()) {
            Ok(set) => set,
            Err(e) => panic!("scan_args({words:?}) failed: {}", e.message),
        }
    }

    fn scan_err(words: &[&str]) -> Fatal {
        match scan_args(&words.iter().map(|w| OsString::from(*w)).collect::<Vec<_>>()) {
            Ok(_) => panic!("scan_args({words:?}) unexpectedly succeeded"),
            Err(e) => e,
        }
    }

    // -----------------------------------------------------------------------
    // Non-UTF-8 argv — the defect this rewrite exists to fix
    // -----------------------------------------------------------------------

    /// `if=` and `of=` values reach [`Settings`] as the exact bytes given.
    /// Before the rewrite the operand scan went through `String`, so
    /// `dd if=$'a\xffb'` aborted on a filename that this OS considers legal.
    ///
    /// Unix-only, and not because `dd` is: `coreutils::quote::os_from_bytes`
    /// documents that its round trip is exact only where an `OsStr` *is* its
    /// bytes, which is the target and is not a Windows host. There a byte
    /// sequence that is not UTF-8 cannot be put into an `OsString` at all, so
    /// the property has no way to be stated, let alone broken.
    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_survive_the_operand_scan() {
        let mut infile = OsString::from("if=");
        infile.push(os(b"a\xffb"));
        let mut outfile = OsString::from("of=");
        outfile.push(os(b"\xfe\xfe"));

        let set = scan_args(&[infile, outfile]).expect("scan");
        assert_eq!(
            coreutils::quote::os_bytes(set.input_file.as_ref().unwrap()).as_ref(),
            b"a\xffb"
        );
        assert_eq!(
            coreutils::quote::os_bytes(set.output_file.as_ref().unwrap()).as_ref(),
            b"\xfe\xfe"
        );
    }

    /// The same bytes through the real front door, `parse_args`, which is what
    /// `run_main` calls with `args_os`. Unix-only for the reason above.
    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_survive_parse_args() {
        let mut infile = OsString::from("if=");
        infile.push(os(b"\x80\x81"));
        match parse_args(&[infile]) {
            Ok(Request::Run(set)) => assert_eq!(
                coreutils::quote::os_bytes(set.input_file.as_ref().unwrap()).as_ref(),
                b"\x80\x81"
            ),
            Ok(_) => panic!("expected a copy request"),
            Err(e) => panic!("parse_args failed: {}", e.message),
        }
    }

    /// A bad *operand name* made of arbitrary bytes is quoted, not decoded —
    /// and the diagnostic still names the whole operand.
    #[test]
    fn a_non_utf8_operand_is_rejected_without_a_panic() {
        let err = scan_args(&[os(b"nosuch\xff=1")]).expect_err("rejected");
        assert!(
            err.message.starts_with("unrecognized operand"),
            "got {}",
            err.message
        );
        assert!(err.referral, "an unrecognized operand takes the usage path");
    }

    /// A `conv=` symbol is argv too. Upstream quotes only the failing
    /// component, up to the comma.
    #[test]
    fn a_non_utf8_conversion_name_names_only_its_own_component() {
        let err = parse_symbols(b"\xffz,sync", CONVERSIONS, false, "invalid conversion")
            .expect_err("rejected");
        assert!(
            err.message.starts_with("invalid conversion: "),
            "got {}",
            err.message
        );
        // The tail after the comma must not appear.
        assert!(!err.message.contains("sync"), "got {}", err.message);
    }

    // -----------------------------------------------------------------------
    // `operand_matches`
    // -----------------------------------------------------------------------

    #[test]
    fn operand_matches_wants_the_whole_name_or_a_delimiter() {
        assert!(operand_matches(b"sync", b"sync", b','));
        assert!(operand_matches(b"sync,fsync", b"sync", b','));
        assert!(!operand_matches(b"syncx", b"sync", b','));
        assert!(!operand_matches(b"syn", b"sync", b','));
        // The prefix relation is not symmetric: `fsync` does not match `sync`.
        assert!(!operand_matches(b"fsync", b"sync", b','));
    }

    // -----------------------------------------------------------------------
    // `parse_integer`
    // -----------------------------------------------------------------------

    #[test]
    fn a_plain_number_parses() {
        assert_eq!(parse_integer(b"512"), (512, Status::Ok));
        assert_eq!(parse_integer(b"0"), (0, Status::Ok));
    }

    /// The block suffixes, measured against GNU: `k` is 1024 and `kB` is 1000,
    /// which is the opposite of the SI convention and is why they are tested.
    #[test]
    fn the_size_suffixes_are_the_ones_gnu_uses() {
        assert_eq!(parse_integer(b"1k"), (1024, Status::Ok));
        assert_eq!(parse_integer(b"1K"), (1024, Status::Ok));
        assert_eq!(parse_integer(b"1kB"), (1000, Status::Ok));
        assert_eq!(parse_integer(b"1M"), (1024 * 1024, Status::Ok));
        assert_eq!(parse_integer(b"1MB"), (1_000_000, Status::Ok));
        assert_eq!(parse_integer(b"1b"), (512, Status::Ok));
        assert_eq!(parse_integer(b"2w"), (4, Status::Ok));
        assert_eq!(parse_integer(b"3c"), (3, Status::Ok));
    }

    /// `B` alone is not a number, and a doubled `B` is not a suffix. Both are
    /// guards in `parse_integer` and both are one character away from valid.
    #[test]
    fn a_bare_or_doubled_b_is_invalid() {
        assert_ne!(parse_integer(b"B").1, Status::Ok);
        assert_ne!(parse_integer(b"1kBB").1, Status::Ok);
    }

    /// `x` multiplies, and recursively.
    #[test]
    fn x_multiplies() {
        assert_eq!(parse_integer(b"2x3"), (6, Status::Ok));
        assert_eq!(parse_integer(b"2x3x4"), (24, Status::Ok));
        assert_eq!(parse_integer(b"2kx2"), (4096, Status::Ok));
    }

    /// A value past `i64` is `Overflow` with the count clamped, not a wrap.
    #[test]
    fn an_enormous_number_overflows_rather_than_wrapping() {
        let (n, status) = parse_integer(b"99999999999999999999999");
        assert_eq!(n, i64::MAX);
        assert!(super::has_overflow(status), "got {status:?}");
    }

    #[test]
    fn a_non_number_is_invalid() {
        assert_ne!(parse_integer(b"zzz").1, Status::Ok);
        assert_ne!(parse_integer(b"").1, Status::Ok);
    }

    // -----------------------------------------------------------------------
    // `parse_symbols`
    // -----------------------------------------------------------------------

    /// `conv=` accumulates; `status=` replaces. Measured:
    /// `status=none,progress` behaves as `progress`.
    #[test]
    fn conv_accumulates_and_status_replaces() {
        let both = parse_symbols(b"noerror,sparse", CONVERSIONS, false, "x").expect("conv");
        assert_eq!(both, C_NOERROR | C_SPARSE);

        let last = parse_symbols(b"none,progress", STATUSES, true, "x").expect("status");
        assert_eq!(last, StatusLevel::Progress as u32);
        let last = parse_symbols(b"progress,none", STATUSES, true, "x").expect("status");
        assert_eq!(last, StatusLevel::None as u32);
    }

    /// A flag this platform has no bit for is refused by name rather than
    /// accepted and quietly dropped — the module's stated policy, and also
    /// upstream's own (`operand_matches (...) && entry->value`).
    #[test]
    fn a_zero_valued_flag_is_rejected_like_an_unknown_one() {
        for name in [
            "direct",
            "dsync",
            "sync",
            "noatime",
            "nocache",
            "noctty",
            "nofollow",
            "nolinks",
            "nonblock",
            "binary",
            "text",
            "cio",
            "directory",
        ] {
            let err =
                parse_symbols(name.as_bytes(), FLAGS, false, "invalid input flag").expect_err(name);
            assert_eq!(err.message, format!("invalid input flag: ‘{name}’"));
        }
    }

    /// The flags that *are* honoured, so the test above cannot pass by
    /// rejecting everything.
    #[test]
    fn the_honoured_flags_are_accepted() {
        assert_eq!(parse_symbols(b"append", FLAGS, false, "x"), Ok(F_APPEND));
        assert_eq!(
            parse_symbols(b"count_bytes,skip_bytes,seek_bytes", FLAGS, false, "x"),
            Ok(F_COUNT_BYTES | F_SKIP_BYTES | F_SEEK_BYTES)
        );
    }

    /// Measured on GNU 9.4: every symbol-table error takes the `usage` path,
    /// so the referral line follows the sentence.
    ///
    /// ```text
    /// dd: invalid conversion: ‘zzz’
    /// Try 'dd --help' for more information.
    /// ```
    #[test]
    fn an_unknown_symbol_is_quoted_and_takes_the_usage_path() {
        let err = parse_symbols(b"zzz", CONVERSIONS, false, "invalid conversion").expect_err("no");
        assert_eq!(err.message, "invalid conversion: ‘zzz’");
        assert!(err.referral);

        let err = parse_symbols(b"zzz", STATUSES, true, "invalid status level").expect_err("no");
        assert_eq!(err.message, "invalid status level: ‘zzz’");
        assert!(err.referral);
    }

    // -----------------------------------------------------------------------
    // `scan_args` — the cross-operand rules
    // -----------------------------------------------------------------------

    #[test]
    fn the_default_block_size_is_512_both_ways() {
        let set = settings(&[]);
        assert_eq!((set.ibs, set.obs), (512, 512));
        assert_eq!(set.max_records, i64::MAX);
    }

    /// `bs=` sets both sizes *and* suppresses the second buffer, which is the
    /// measured difference between `0+2 records out` for `bs=4096` on a pipe
    /// and `0+1` for `ibs=4096 obs=4096`.
    #[test]
    fn bs_sets_both_sizes_and_suppresses_the_second_buffer() {
        let set = settings(&["bs=4096"]);
        assert_eq!((set.ibs, set.obs), (4096, 4096));
        assert_eq!(set.conversions & C_TWOBUFS, 0);

        let set = settings(&["ibs=4096", "obs=4096"]);
        assert_ne!(set.conversions & C_TWOBUFS, 0);
    }

    /// Measured: `dd bs=1000 bs=2000` is accepted and the last one wins.
    #[test]
    fn a_repeated_operand_takes_the_last_value() {
        let set = settings(&["bs=1000", "bs=2000"]);
        assert_eq!((set.ibs, set.obs), (2000, 2000));
        let set = settings(&["if=a", "if=b"]);
        assert_eq!(
            set.input_file.as_deref(),
            Some(OsString::from("b").as_os_str())
        );
    }

    /// `conv=block` with no `cbs=` is *not* an error: `cbs == 0` clears the
    /// bit before the mutual-exclusion test runs, which is why the measured
    /// `dd conv=block,unblock` (no cbs) copies happily.
    #[test]
    fn cbs_zero_clears_block_before_the_exclusion_test() {
        let set = settings(&["conv=block"]);
        assert_eq!(set.conversions & C_BLOCK, 0);

        let set = settings(&["conv=block,unblock"]);
        assert_eq!(set.conversions & (C_BLOCK | C_UNBLOCK), 0);

        // With a cbs the same pair *is* refused.
        let err = scan_err(&["cbs=4", "conv=block,unblock"]);
        assert_eq!(err.message, "cannot combine block and unblock");
    }

    /// `conv=ascii` implies `unblock`, `conv=ebcdic` and `conv=ibm` imply
    /// `block`. Measured: `conv=ascii` turns EBCDIC into lines, `conv=ebcdic`
    /// pads to `cbs`.
    #[test]
    fn the_charset_conversions_imply_a_record_conversion() {
        let set = settings(&["cbs=4", "conv=ascii"]);
        assert_ne!(set.conversions & C_ASCII, 0);
        assert_ne!(set.conversions & C_UNBLOCK, 0);

        let set = settings(&["cbs=4", "conv=ebcdic"]);
        assert_ne!(set.conversions & C_EBCDIC, 0);
        assert_ne!(set.conversions & C_BLOCK, 0);
    }

    /// The mutual exclusions, with the wording measured from GNU 9.4 — and
    /// with no referral line, which is the measured difference between these
    /// (`error`) and an unrecognized operand (`usage`).
    #[test]
    fn the_mutual_exclusions_are_worded_as_gnu_words_them() {
        let err = scan_err(&["conv=ascii,ebcdic"]);
        assert_eq!(err.message, "cannot combine any two of {ascii,ebcdic,ibm}");
        assert!(!err.referral);
        assert_eq!(
            scan_err(&["conv=lcase,ucase"]).message,
            "cannot combine lcase and ucase"
        );
        assert_eq!(
            scan_err(&["conv=excl,nocreat"]).message,
            "cannot combine excl and nocreat"
        );
    }

    /// `fullblock` is an input-only flag; upstream rejects it on the output
    /// side by name.
    #[test]
    fn oflag_fullblock_is_refused() {
        let err = scan_err(&["oflag=fullblock"]);
        assert_eq!(err.message, "invalid output flag: ‘fullblock’");
    }

    /// A `B` suffix on `skip=`/`count=`/`seek=` implies the matching
    /// `_bytes` flag, and the count is then split into whole records plus a
    /// remainder against the *relevant* block size — `ibs` for skip and
    /// count, `obs` for seek.
    #[test]
    fn a_b_suffix_splits_the_count_against_the_right_block_size() {
        let set = settings(&["ibs=100", "obs=10", "skip=250B", "count=1050B", "seek=35B"]);
        assert_eq!((set.skip_records, set.skip_bytes), (2, 50));
        assert_eq!((set.max_records, set.max_bytes), (10, 50));
        assert_eq!((set.seek_records, set.seek_bytes), (3, 5));
    }

    /// Without the suffix (or the flag) the counts are records, undivided.
    #[test]
    fn without_a_b_suffix_the_counts_are_records() {
        let set = settings(&["bs=100", "skip=2", "count=3", "seek=4"]);
        assert_eq!((set.skip_records, set.skip_bytes), (2, 0));
        assert_eq!((set.max_records, set.max_bytes), (3, 0));
        assert_eq!((set.seek_records, set.seek_bytes), (4, 0));
    }

    /// The explicit flags do the same job as the suffix.
    #[test]
    fn the_bytes_flags_do_what_the_suffix_does() {
        let set = settings(&[
            "ibs=100",
            "iflag=skip_bytes,count_bytes",
            "skip=250",
            "count=1050",
        ]);
        assert_eq!((set.skip_records, set.skip_bytes), (2, 50));
        assert_eq!((set.max_records, set.max_bytes), (10, 50));
    }

    /// `iflag=fullblock` is lifted out of the flag word — it is not an `O_*`
    /// bit and must not reach `open`.
    #[test]
    fn fullblock_leaves_the_flag_word() {
        let set = settings(&["iflag=fullblock"]);
        assert!(set.fullblock);
        assert_eq!(set.input_flags, 0);
    }

    /// The short-read warning is armed only where a short read would silently
    /// miscount: one buffer, no `fullblock`, and a skip or bounded count.
    #[test]
    fn the_partial_read_warning_is_armed_only_where_it_can_bite() {
        assert!(settings(&["bs=512", "count=1"]).warn_partial_read);
        assert!(settings(&["bs=512", "skip=1"]).warn_partial_read);
        // Two buffers re-aggregate, so nothing is miscounted.
        assert!(!settings(&["ibs=512", "obs=512", "count=1"]).warn_partial_read);
        // `fullblock` makes the short read impossible.
        assert!(!settings(&["bs=512", "count=1", "iflag=fullblock"]).warn_partial_read);
        // Nothing to throw off.
        assert!(!settings(&["bs=512"]).warn_partial_read);
    }

    #[test]
    fn the_status_levels_map_to_the_enum() {
        assert_eq!(settings(&[]).status_level, StatusLevel::Default);
        assert_eq!(settings(&["status=none"]).status_level, StatusLevel::None);
        assert_eq!(
            settings(&["status=noxfer"]).status_level,
            StatusLevel::Noxfer
        );
        assert_eq!(
            settings(&["status=progress"]).status_level,
            StatusLevel::Progress
        );
    }

    /// `iseek`/`oseek` are the historical spellings of `skip`/`seek`.
    #[test]
    fn iseek_and_oseek_are_accepted() {
        let set = settings(&["bs=1", "iseek=3", "oseek=4"]);
        assert_eq!(set.skip_records, 3);
        assert_eq!(set.seek_records, 4);
    }

    /// A block size of 0 is refused — `n_min` is 1 — and, measured, with no
    /// referral line: `dd bs=0` prints `dd: invalid number: '0'` and stops.
    #[test]
    fn a_zero_block_size_is_invalid() {
        let err = scan_err(&["bs=0"]);
        // `quoteaf`, not `quote`: measured in the same shell, GNU prints
        // `invalid number: '0'` with plain apostrophes but
        // `invalid conversion: ‘zzz’` with directional ones, because the two
        // diagnostics reach for different gnulib quoting styles.
        assert_eq!(err.message, "invalid number: '0'");
        assert!(!err.referral);
        assert!(scan_err(&["ibs=0"]).message.starts_with("invalid number"));
        assert!(scan_err(&["cbs=0"]).message.starts_with("invalid number"));
    }

    /// An overflowing operand gets the errno text appended; a merely invalid
    /// one does not. Measured on GNU 9.4.
    #[test]
    fn an_overflowing_operand_names_the_errno() {
        let err = scan_err(&["bs=99999999999999999999999"]);
        assert!(
            err.message
                .ends_with(": Value too large for defined data type"),
            "got {}",
            err.message
        );
        let err = scan_err(&["bs=zzz"]);
        assert!(
            !err.message.contains("Value too large"),
            "got {}",
            err.message
        );
    }

    /// An operand with no `=` is not an operand at all.
    #[test]
    fn a_bare_word_is_not_an_operand() {
        let err = scan_err(&["hello"]);
        assert!(err.message.starts_with("unrecognized operand"));
    }

    // -----------------------------------------------------------------------
    // `apply_translations`
    // -----------------------------------------------------------------------

    /// Measured: `conv=ucase,ebcdic` and `conv=ebcdic,ucase` give the same
    /// bytes, because `conv=` is a *set* and the case fold is always applied
    /// after the charset map. `abc` becomes EBCDIC `ABC` = 0o301 0o302 0o303
    /// either way, where `conv=ebcdic` alone gives 0o201 0o202 0o203.
    #[test]
    fn the_case_fold_follows_the_charset_map_whatever_the_order() {
        let plain = apply_translations(C_EBCDIC);
        assert_eq!(
            [plain.table[b'a' as usize], plain.table[b'b' as usize]],
            [0o201, 0o202]
        );

        let folded = apply_translations(C_EBCDIC | C_UCASE);
        assert_eq!(
            [folded.table[b'a' as usize], folded.table[b'b' as usize]],
            [0o301, 0o302]
        );
    }

    #[test]
    fn lcase_and_ucase_alone_fold_ascii() {
        let up = apply_translations(C_UCASE);
        assert_eq!(up.table[b'a' as usize], b'A');
        assert_eq!(up.table[b'1' as usize], b'1');

        let down = apply_translations(C_LCASE);
        assert_eq!(down.table[b'Z' as usize], b'z');
    }

    /// With no conversion at all the table is the identity and `needed` says
    /// so, which is what lets the copy skip the translation pass entirely.
    #[test]
    fn no_conversion_means_no_translation_pass() {
        let t = apply_translations(0);
        assert!(!t.needed);
        for (i, b) in t.table.iter().enumerate() {
            assert_eq!(usize::from(*b), i);
        }
        assert_eq!(t.newline_character, b'\n');
        assert_eq!(t.space_character, b' ');
    }

    /// `conv=ebcdic` moves the two characters `block`/`unblock` emit, because
    /// they are emitted *after* the table is applied and so must already be in
    /// the output charset. GNU's `ascii_to_ebcdic` sends `\n` to 0x25 and
    /// space to 0x40.
    #[test]
    fn the_block_characters_move_with_the_output_charset() {
        let t = apply_translations(C_EBCDIC);
        assert_eq!(t.newline_character, 0x25);
        assert_eq!(t.space_character, 0x40);
    }

    // -----------------------------------------------------------------------
    // `swab_buffer`
    // -----------------------------------------------------------------------

    /// `conv=swab` on an odd-length read holds the last byte back for the next
    /// one, which is why the measured `printf abcde | dd conv=swab` is
    /// `badce` and not `badc` followed by a lost `e`.
    #[test]
    fn swab_carries_an_odd_byte_into_the_next_block() {
        // One extra byte, as `alloc_buffer` reserves for exactly this.
        let mut buf = b"abcde\0".to_vec();
        let mut nread = 5usize;
        let mut saved = -1i32;

        let start = swab_buffer(&mut buf, &mut nread, &mut saved);
        assert_eq!(start, 1, "the swapped bytes begin one byte in");
        assert_eq!(nread, 4);
        assert_eq!(&buf[start..start + nread], b"badc");
        assert_eq!(saved, i32::from(b'e'));

        // The next read picks the carried byte back up.
        let mut buf = b"fg\0".to_vec();
        let mut nread = 2usize;
        let start = swab_buffer(&mut buf, &mut nread, &mut saved);
        assert_eq!(start, 0);
        assert_eq!(nread, 2);
        assert_eq!(&buf[start..start + nread], b"fe");
        assert_eq!(saved, i32::from(b'g'));
    }

    /// An even-length block with nothing carried in is a plain pairwise swap —
    /// but it still comes back at offset 1, because the swap is done by
    /// shifting every second byte two places toward the end (half the moves of
    /// a pairwise exchange) and that shift needs the slot in front. Only a
    /// block that has a carried byte to put *into* that slot returns 0.
    #[test]
    fn swab_of_an_even_block_swaps_in_place() {
        let mut buf = b"abcd\0".to_vec();
        let mut nread = 4usize;
        let mut saved = -1i32;
        let start = swab_buffer(&mut buf, &mut nread, &mut saved);
        assert_eq!(start, 1);
        assert_eq!(nread, 4);
        assert_eq!(&buf[start..start + nread], b"badc");
        assert_eq!(saved, -1, "nothing is held back from an even block");
    }

    #[test]
    fn swab_of_nothing_does_nothing() {
        let mut buf = vec![0u8; 4];
        let mut nread = 0usize;
        let mut saved = -1i32;
        assert_eq!(swab_buffer(&mut buf, &mut nread, &mut saved), 0);
        assert_eq!(nread, 0);
    }

    // -----------------------------------------------------------------------
    // Small helpers
    // -----------------------------------------------------------------------

    #[test]
    fn multiple_bits_set_means_more_than_one() {
        assert!(!multiple_bits_set(0));
        assert!(!multiple_bits_set(C_ASCII));
        assert!(!multiple_bits_set(C_SWAB));
        assert!(multiple_bits_set(C_ASCII | C_EBCDIC));
        assert!(multiple_bits_set(C_SYNC | C_SWAB | C_ASCII));
    }

    /// The test upstream uses to decide whether a parenthetical would repeat
    /// the byte count: `3 B` has a space second-from-last, `1.0 kB` does not.
    #[test]
    fn a_bare_b_abbreviation_is_recognised() {
        assert!(abbreviation_lacks_prefix("3 B"));
        assert!(!abbreviation_lacks_prefix("1.0 kB"));
        assert!(!abbreviation_lacks_prefix("9.8 KiB"));
        assert!(!abbreviation_lacks_prefix(""));
    }

    // -----------------------------------------------------------------------
    // `--help` and `--version`
    // -----------------------------------------------------------------------

    #[test]
    fn help_names_every_operand_and_ends_with_version() {
        let help = help_text();
        assert!(help.starts_with("Usage: dd [OPERAND]...\n"));
        for operand in [
            "bs=BYTES",
            "cbs=BYTES",
            "conv=CONVS",
            "count=N",
            "ibs=BYTES",
            "if=FILE",
            "iflag=FLAGS",
            "obs=BYTES",
            "of=FILE",
            "oflag=FLAGS",
            "seek=N",
            "skip=N",
            "status=LEVEL",
        ] {
            assert!(help.contains(operand), "help omits {operand}");
        }
        // The text ends after the `--version` line. Upstream follows it with a
        // block of GNU URLs; this build does not carry those.
        assert_eq!(
            help.lines().last(),
            Some("      --version     output version information and exit")
        );
    }

    /// Every FLAG name `--help` advertises must actually parse, and every name
    /// that parses must be advertised. This is the invariant that keeps the
    /// 0-valued entries out of the help text — an accepted-looking flag that
    /// is refused is exactly the confusion the module set out to avoid.
    #[test]
    fn the_help_text_advertises_exactly_the_flags_that_parse() {
        let help = help_text();
        // Only the FLAG section — `sync` is a CONV name too, and the CONV
        // list is not what this is about.
        let section = help
            .split_once("Each FLAG symbol may be:\n")
            .expect("the help text has a FLAG section")
            .1;
        let section = section
            .split_once("\nOptions are:")
            .map_or(section, |x| x.0);
        let advertised: Vec<&str> = section
            .lines()
            .filter_map(|line| line.strip_prefix("  "))
            .filter_map(|line| line.split_whitespace().next())
            .collect();
        assert!(!advertised.is_empty(), "the FLAG section parsed as empty");

        for (name, value) in FLAGS {
            let listed = advertised.contains(name);
            assert_eq!(
                listed,
                *value != 0,
                "flag {name} is {}listed but {}accepted",
                if listed { "" } else { "not " },
                if *value == 0 { "not " } else { "" }
            );
        }
    }

    #[test]
    fn version_is_one_line_naming_the_program() {
        let v = version_text();
        assert_eq!(v.lines().count(), 1);
        assert!(v.starts_with("dd ("), "got {v}");
        assert!(v.ends_with('\n'));
    }

    #[test]
    fn help_and_version_are_recognised_anywhere_in_argv() {
        assert!(matches!(
            parse_args(&[OsString::from("if=/dev/null"), OsString::from("--help")]),
            Ok(Request::Help)
        ));
        assert!(matches!(
            parse_args(&[OsString::from("--version"), OsString::from("if=/dev/null")]),
            Ok(Request::Version)
        ));
    }

    #[test]
    fn an_unknown_option_takes_the_usage_path() {
        let err = parse_args(&[OsString::from("--bogus")]).expect_err("rejected");
        assert!(err.referral, "an option error prints the referral line");
    }
}
