//! C: tree-sitter-c 0.24.2 (MIT, Max Brunsfeld and the tree-sitter
//! contributors), which has no external scanner. `grammars/c/` holds its
//! `parser.c`, its highlight query and its test corpus as published.

super::generated!("c");

/// The highlight query, as published.
pub(crate) const HIGHLIGHTS: &str = include_str!("../../grammars/c/highlights.scm");
