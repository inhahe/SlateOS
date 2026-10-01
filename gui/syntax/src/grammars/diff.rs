//! Diffs: tree-sitter-diff 0.2.0 (MIT, Michael Davis and the
//! tree-sitter-grammars contributors) -- a unified or context diff, a patch.
//! `grammars/diff/` holds its `parser.c`, its highlight query and its test
//! corpus as published; it has no external scanner. Its lines are coloured as
//! the toolkit's kinds for a change's (`Inserted`, `Deleted`, `Changed`).
//!
//! A line is drawn in the change's colour, its `+` or `-` too: the query's
//! `(#set! priority 95)` -- Neovim's, which this highlighter reads
//! (design-decisions §1443) -- puts the markers' punctuation under their
//! line's colour.
//!
//! Its injection query colours each hunk's lines in the language its file's
//! name says -- a `.rs` file's as Rust -- the new file's lines (those it
//! keeps and those it adds) as one document and the old file's as another,
//! each line's marker left out (`#offset!`). The package's
//! `tree-sitter.json` does not list the query, so `tree-sitter highlight`
//! leaves it out; it is Neovim's, whose reading of both is followed here.
//!
//! That query finds hunks only under a `diff` line -- git's `diff --git`,
//! `diff -r`'s -- which the grammar makes a block of. `diff -u` of two
//! files, and `svn diff`, have none: the grammar leaves their lines flat.
//! `injections.flat.scm`, this crate's own, reads those: each hunk in the
//! language of the file named last before it (§1444).

super::generated!("diff");

/// The highlight query, as published.
pub(crate) const HIGHLIGHTS: &str = include_str!("../../grammars/diff/highlights.scm");

/// The injection query: a hunk's lines in their file's language -- as
/// published, then the same for a diff with no `diff` line.
pub(crate) const INJECTIONS: &str = concat!(
    include_str!("../../grammars/diff/injections.scm"),
    "\n",
    include_str!("../../grammars/diff/injections.flat.scm"),
);
