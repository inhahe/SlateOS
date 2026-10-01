//! Regular expressions: tree-sitter-regex 0.25.0 (MIT, Max Brunsfeld and the
//! tree-sitter contributors) -- JavaScript's flavour, which JavaScript's and
//! TypeScript's injection queries hand it for every `/.../` literal.
//! `grammars/regex/` holds its `parser.c`, its highlight query and its test
//! corpus as published; it has no external scanner.

super::generated!("regex");

/// The highlight query, as published: groups, escapes, quantifiers,
/// classes.
pub(crate) const HIGHLIGHTS: &str = include_str!("../../grammars/regex/highlights.scm");
