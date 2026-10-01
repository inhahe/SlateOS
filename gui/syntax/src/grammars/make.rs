//! Makefiles: tree-sitter-make 1.1.1 (MIT, Alexandre A. Muller and the
//! tree-sitter-grammars contributors) -- GNU make's language, which the
//! toolchains the OS ports are built with. `grammars/make/` holds its
//! `parser.c`, its highlight query and its test corpus as published; it has no
//! external scanner.

super::generated!("make");

/// The highlight query, as published.
pub(crate) const HIGHLIGHTS: &str = include_str!("../../grammars/make/highlights.scm");
