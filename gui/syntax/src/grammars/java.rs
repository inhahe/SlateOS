//! Java: tree-sitter-java 0.23.5 (MIT, Ayman Nadeem and the tree-sitter
//! contributors). `grammars/java/` holds its `parser.c`, its highlight query,
//! its test corpus and highlight tests as published; it has no external
//! scanner.

super::generated!("java");

/// The highlight query, as published.
pub(crate) const HIGHLIGHTS: &str = include_str!("../../grammars/java/highlights.scm");
