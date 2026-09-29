//! Go: tree-sitter-go 0.25.0 (MIT, Max Brunsfeld and the tree-sitter
//! contributors). `grammars/go/` holds its `parser.c`, its highlight query
//! and its test corpus as published; it has no external scanner.
//!
//! The query read is not the published one but the same patterns with its
//! general ones first (`highlights.general-first.scm`, design-decisions
//! §1442): the published query puts `(identifier) @variable` after the
//! patterns that make a function's name a function, and of one node's
//! captures the last pattern's wins.

super::generated!("go");

/// The highlight query, its general patterns first.
pub(crate) const HIGHLIGHTS: &str = include_str!("../../grammars/go/highlights.general-first.scm");

#[cfg(test)]
mod tests {
    use super::*;

    /// **The query read is the published one, its general patterns
    /// first**: every pattern of it, no more, the Identifiers block moved to
    /// the top -- so an update of the grammar that changes the query shows
    /// here, rather than going unread.
    #[test]
    fn the_query_read_is_the_published_one_general_first() {
        let published = include_str!("../../grammars/go/highlights.scm");
        let block = "; Identifiers\n\n(type_identifier) @type\n(field_identifier) @property\n(identifier) @variable\n\n";
        assert_eq!(published.matches(block).count(), 1);
        let rest = published.replacen(block, "", 1);
        let body = HIGHLIGHTS
            .split_once("\n\n")
            .map(|(_, body)| body)
            .unwrap_or_default();
        assert!(HIGHLIGHTS.starts_with("; Go's highlight query"));
        assert_eq!(body, format!("{block}{rest}"));
    }
}
