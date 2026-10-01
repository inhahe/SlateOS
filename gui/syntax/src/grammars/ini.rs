//! INI files: tree-sitter-ini 1.4.0 (Apache-2.0, Justin M. Keyes) -- the
//! settings files of many programs, and the freedesktop formats built on them:
//! a desktop entry, a systemd unit. `grammars/ini/` holds its `parser.c`, its
//! highlight query and its test corpus as published; it has no external
//! scanner.

super::generated!("ini");

/// The highlight query, as published.
pub(crate) const HIGHLIGHTS: &str = include_str!("../../grammars/ini/highlights.scm");
