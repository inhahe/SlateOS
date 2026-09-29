//! Ada: tree-sitter-ada (MIT, Emmanuel Briot), the language of the OS's
//! safety-critical drivers. `grammars/ada/` holds its `parser.c`, its
//! highlight query and its test corpus as published at the commit
//! `grammars/ada/SOURCE` names -- the grammar has no release tags. It has no
//! external scanner.
//!
//! Its `locals.scm` is not used: it names its captures as Neovim once did
//! (`@scope`, `@definition.var`, `@reference`), which tree-sitter's
//! highlighter does not read, and the package's `tree-sitter.json` lists
//! only the highlight query.

super::generated!("ada");

/// The highlight query, as published.
pub(crate) const HIGHLIGHTS: &str = include_str!("../../grammars/ada/highlights.scm");
