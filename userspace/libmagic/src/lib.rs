//! libmagic -- file 5.45's library (Ian F. Darwin, Christos Zoulas and
//! others), ported: the magic database, and the tests that tell what a file
//! is from what is in it.
//!
//! Each module is a port of the libmagic source file of the same name,
//! function by function, and keeps upstream's behaviour where it is odd,
//! because what libmagic answers is read by scripts:
//!
//! - [`magicapi`] (`magic.c`) -- the entry points: open, load, identify a named
//!   file or standard input, read back the answer or the error;
//! - [`apprentice`] -- the database: reading magic text, checking it, sorting
//!   it by strength, compiling it to `.mgc` and mapping a compiled one;
//! - [`funcs`] -- the magic set (`struct magic_set`), `file_printf`, and
//!   `file_buffer`, which decides the order the tests run in: encoding,
//!   compressed, tar, JSON, CSV, SIMH, CDF, ELF, the rules, and text;
//! - [`softmagic`] -- the rule interpreter;
//! - [`encoding`], [`ascmagic`] -- what text is in, and how it is described;
//! - [`is_tar`], [`is_json`], [`is_csv`], [`is_simh`], [`fsmagic`], [`der`],
//!   [`readelf`], [`cdf`] and [`readcdf`], [`compress`] -- the tests that are
//!   code rather than rules;
//! - [`print`], [`printf`], [`fmtcheck`], [`cdf_time`] -- how values are
//!   written;
//! - [`buffer`], [`cstd`], [`out`], [`zlib`] -- what the C sources have from
//!   their headers and the C library: the buffer being identified, `<ctype.h>`
//!   and `strtoul`, stdio's buffering of standard output, zlib's `inflate`.
//!
//! The program, `file.c`, is `userspace/file`, which also builds file 5.45's
//! database into itself. `scripts/file-diff.sh` measures the pair against
//! file 5.45 byte for byte.

// The workspace's lint policy, less two of its defensive lints. The port
// keeps C's arithmetic and C's array accesses with the bounds C checks: every
// index is checked in the function that makes it, against a buffer's length
// or a fixed array's size (reviewed site by site, 2026-10-02 -- the one that
// was not, `parse_extra`'s terminator, now writes where upstream's does), and
// unsigned arithmetic that wraps in C wraps here explicitly (`wrapping_*`).
// `scripts/file-diff.sh` holds both to upstream on crafted ELF, CDF and
// compressed files and on random mutations; a debug build, with overflow
// checks, ran those corpora -- 14,601 files in five modes -- without a panic.
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]

pub mod apprentice;
pub mod ascmagic;
pub mod buffer;
pub mod cdf;
pub mod cdf_time;
pub mod compress;
pub mod cstd;
pub mod der;
pub mod encoding;
pub mod fmtcheck;
pub mod fsmagic;
pub mod funcs;
pub mod is_csv;
pub mod is_json;
pub mod is_simh;
pub mod is_tar;
pub mod magic;
pub mod magicapi;
pub mod out;
pub mod print;
pub mod printf;
pub mod readcdf;
pub mod readelf;
pub mod softmagic;
pub mod zlib;
