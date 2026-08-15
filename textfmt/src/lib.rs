//! Escaping and parsing for the line-oriented text formats SlateOS reads and
//! writes.
//!
//! Every module here answers one question about one format: *can a value
//! written into this file re-punctuate the file?* That question has a
//! different answer per format — a comma is structure in CSV and data in a
//! `key = value` line; a `<` is structure in markup and data everywhere else
//! — so there is one escaper per grammar rather than one general-purpose
//! `sanitize`. Picking the wrong one is the bug this crate exists to make
//! hard to write.
//!
//! # Why this is not part of the GUI toolkit
//!
//! It was, and that was the mistake. These primitives grew inside
//! `guitk` because the first callers were GUI applications, but the
//! components with the *strongest* need for them are the ones that must not
//! depend on a widget library: the file indexer, the backup tool and the
//! installer all run headless. Unable to reach the shared escapers, each of
//! them wrote its own — and each got it wrong in one of the two ways this
//! crate documents at length in [`kv`]: decoding with a chain of
//! `str::replace`, and escaping a trailing space as backslash-space.
//!
//! The indexer's version of the bug was not cosmetic. `exclude_paths` was
//! comma-joined, so excluding `/home/u/Private, Ltd` wrote two entries,
//! neither of which named the directory the user meant, and its contents
//! were indexed.
//!
//! So the layering is deliberate: a crate with no dependencies at all, which
//! `guitk` re-exports for the 137 applications that reach these modules
//! through it, and which the four headless ones depend on directly.
//!
//! # Choosing a module
//!
//! | Writing into… | Use |
//! |---|---|
//! | a spreadsheet-readable table | [`csv`] |
//! | HTML, XML or another markup document | [`escape`] |
//! | JSON | [`escape::json_string`] |
//! | one line of a `key = value` config file | [`kv`] |
//! | a report a person reads, with no parser | [`fold`] |
//!
//! The last row is the odd one out, and the difference is worth stating
//! plainly: [`fold`] does not escape, it destroys. Which of the two a caller
//! wants is decided by who reads the output. Where a parser will read the
//! value back, escape — the value must survive. Where a person will read it,
//! fold — a literal `\n` in the middle of a sentence is noise to a reader,
//! and the only thing that actually needs preventing is a value drawing its
//! own section header.
//!
//! If a format is not listed, it does not follow that no escaping is needed
//! — it follows that nobody has written the escaper yet. Add it here rather
//! than inline at the call site, which is how the duplication above started.

#![no_std]

extern crate alloc;

pub mod csv;
pub mod escape;
pub mod fold;
pub mod kv;
