//! Tree-sitter queries: tree-sitter-query 0.8.0 (Apache-2.0, the
//! tree-sitter-grammars organisation, commit 8e9e223), which has no external
//! scanner. `grammars/query/` holds its `parser.c`, its highlight and
//! injection queries and its test corpus as published.
//!
//! The language every grammar's highlight query here is written in. A
//! `.scm` file opens in it: every `.scm` file in the tree is a query, and
//! there is no Scheme grammar to say otherwise -- a Scheme program would be
//! read as a query too, until there is one to tell the two apart.

super::generated!("query");

/// The highlight query, as published -- Neovim's, whose `#lua-match?`
/// patterns `luapat` reads as `#match?`.
pub(crate) const HIGHLIGHTS: &str = include_str!("../../grammars/query/highlights.scm");

/// The injection query, as published: a `#match?`'s pattern in the regular
/// expression grammar. The Lua patterns and comments it names languages for
/// have no grammar here, and stay as they are.
pub(crate) const INJECTIONS: &str = include_str!("../../grammars/query/injections.scm");
