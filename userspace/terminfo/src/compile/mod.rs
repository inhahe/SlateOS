//! The terminfo compiler: ncurses 6.4's `tic` library -- what `tic`,
//! `infocmp` and `toe` share -- turning terminfo or termcap *source* into
//! terminals and terminals back into source.
//!
//! - [`scan`]: the scanner (`comp_scan.c`), with the error context its
//!   warnings name (`comp_error.c`): `"FILE", line N, col M, terminal 'T':`.
//! - [`parse`]: one entry read (`parse_entry.c`), its names and
//!   capabilities, extended ones made where `-x` allows, and the termcap
//!   and terminfo post-processing; and the [`parse::Compiler`] that holds
//!   what upstream keeps in globals, and the hook `tic -v` checks through.
//! - [`comp_parse`]: a source read entry by entry, and `use=` resolved --
//!   from the source, else the database -- and merged (`comp_parse.c`).
//! - [`entry`]: an entry's string table and merging (`alloc_entry.c`).
//! - [`captoinfo`]: capability strings translated termcap to terminfo and
//!   back (`captoinfo.c`).
//! - [`expand`]: a string written back as source (`_nc_tic_expand`).
//! - [`dump`]: an entry written back as source, sorted, wrapped and
//!   trimmed to fit (`progs/dump_entry.c`), and compared (`infocmp`).
//! - [`write`]: an entry compiled to the binary format (`write_entry.c`).
//! - [`tables`] and [`caps`]: the capability names and the indices they
//!   look up (`comp_hash.c`, `name_match.c`).
//!
//! Everything here is a transcription of upstream's file of the same name,
//! its quirks kept; where ncurses keeps state in globals, it is a field of
//! the struct the function is a method of.
//!
//! # Diagnostics
//!
//! Upstream writes its warnings to `stderr` itself. Here they go to a
//! [`Diagnostics`] the caller supplies -- the programs write them to
//! standard error, unbuffered, as upstream's land; a test collects them.
//! An error that ends the program (`_nc_err_abort`) is the [`Abort`] it is
//! returned as, its message already written.

pub mod caps;
pub mod captoinfo;
pub mod comp_parse;
pub mod dump;
pub mod entry;
pub mod expand;
pub mod parse;
pub mod scan;
pub mod tables;
pub mod write;

/// Where the compiler's warnings and errors go: upstream's `stderr`,
/// unbuffered.
pub trait Diagnostics {
    /// Write `bytes` as one `fprintf` would.
    fn emit(&mut self, bytes: &[u8]);
}

/// Collected, for a caller that shows them later or not at all.
impl Diagnostics for Vec<u8> {
    fn emit(&mut self, bytes: &[u8]) {
        self.extend_from_slice(bytes);
    }
}

/// What ends a compile: `_nc_err_abort` or `_nc_syserr_abort`, whose message
/// has been written. Upstream exits with `EXIT_FAILURE` then; the caller
/// does, after its own `exit` would have flushed what it holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Abort;

/// `MAX_ENTRY_SIZE`: the largest compiled entry, and the scanner's token
/// buffer -- the extended-numbers format's, which the reference's build has.
pub const MAX_ENTRY_SIZE: usize = 32768;

/// `MAX_NAME_SIZE`: the longest name field.
pub const MAX_NAME_SIZE: usize = 512;
