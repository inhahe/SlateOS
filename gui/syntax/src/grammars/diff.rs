//! Diffs: tree-sitter-diff 0.2.0 (MIT, Michael Davis and the
//! tree-sitter-grammars contributors) -- a unified or context diff, a patch.
//! `grammars/diff/` holds its `parser.c`, its highlight query and its test
//! corpus as published; it has no external scanner. Its lines are coloured as
//! the toolkit's kinds for a change's (`Inserted`, `Deleted`, `Changed`).
//!
//! A line's `+` or `-` is drawn as punctuation, the rest of the line as the
//! change: the query's `(#set! priority 95)` on the markers, which would put
//! them under the line's colour, is Neovim's, and this highlighter -- like
//! tree-sitter's own -- does not read it.
//!
//! Its injection query -- each hunk's lines in the language its file's name
//! says, their `+`, `-` or blank left out -- is not read: it needs the
//! language named by a file (`@injection.filename`) and a capture moved by
//! `#offset!`, which this highlighter does not do yet, and the package's
//! `tree-sitter.json` does not list it.

super::generated!("diff");

/// The highlight query, as published.
pub(crate) const HIGHLIGHTS: &str = include_str!("../../grammars/diff/highlights.scm");
