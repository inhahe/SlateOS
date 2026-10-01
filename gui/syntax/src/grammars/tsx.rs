//! TSX: tree-sitter-typescript 0.23.2's TSX grammar (MIT, Max Brunsfeld and
//! the tree-sitter contributors) -- TypeScript with JSX. `grammars/tsx/` holds
//! its `parser.c` and its two-line `scanner.c` as published; the scanner the
//! `scanner.c` includes is TypeScript's, ported in `typescript.rs` and used
//! here, and the package's queries and test corpus are vendored beside
//! TypeScript's grammar.

super::generated!("tsx", super::super::typescript::Scanner);

/// The highlight queries: JavaScript's, its JSX query, then TypeScript's.
/// `tree-sitter.json` lists TypeScript's first and the JSX query before
/// JavaScript's main one; but the last pattern wins (§1438), so JavaScript's
/// `(identifier) @variable` would paint every tag a variable -- they are in
/// the order JavaScript gives its own, general first, and TypeScript's last
/// (§1441).
pub(crate) const HIGHLIGHTS: &str = concat!(
    include_str!("../../grammars/javascript/highlights.scm"),
    "\n",
    include_str!("../../grammars/javascript/highlights-jsx.scm"),
    "\n",
    include_str!("../../grammars/typescript/highlights.scm"),
);

/// The locals query, as `tree-sitter.json` lists it for TSX: JavaScript's,
/// whose `(pattern/identifier)` declares a parameter's name as TypeScript's
/// own locals query does.
pub(crate) const LOCALS: &str = include_str!("../../grammars/javascript/locals.scm");
