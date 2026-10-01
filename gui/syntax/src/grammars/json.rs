//! JSON: tree-sitter-json 0.24.8 (MIT, Max Brunsfeld), which has no external
//! scanner. `grammars/json/` holds its `parser.c`, its highlight query and its
//! test corpus as published.

super::generated!("json");

/// The highlight query, as published.
pub(crate) const HIGHLIGHTS: &str = include_str!("../../grammars/json/highlights.scm");
