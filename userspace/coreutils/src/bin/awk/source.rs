//! Where the program text came from, for diagnostics that say so.
//!
//! gawk prefixes what it reports with where it was in the program: `awk: cmd.
//! line:1: warning: ...` for the program given as an operand, `awk:
//! prog.awk:3: fatal: ...` for one read with `-f`, each source numbering its
//! own lines from 1. The parser sees one joined text -- the `-f` files run
//! together -- so [`SourceMap`] is what turns a byte offset in it back into a
//! source and a line.

use crate::ast::Loc;
use crate::value::Str;

/// The joined program text's sources, and where its lines begin.
#[derive(Debug, Clone)]
pub struct SourceMap {
    /// Each source's first byte in the joined text and its name -- `None` for
    /// the program operand, gawk's `cmd. line` -- in order.
    spans: Vec<(usize, Option<Str>)>,
    /// The offset at which each line of the joined text begins.
    line_starts: Vec<usize>,
}

impl SourceMap {
    /// A map of `text`, whose sources begin where `spans` says.
    #[must_use]
    pub fn new(text: &[u8], spans: Vec<(usize, Option<Str>)>) -> SourceMap {
        let mut line_starts = vec![0];
        line_starts.extend(
            text.iter()
                .enumerate()
                .filter(|&(_, &b)| b == b'\n')
                .map(|(i, _)| i.saturating_add(1)),
        );
        SourceMap { spans, line_starts }
    }

    /// The map of a program given as an operand: one source, unnamed.
    #[must_use]
    pub fn operand(text: &[u8]) -> SourceMap {
        SourceMap::new(text, vec![(0, None)])
    }

    /// Which source holds byte `offset`, and the line it is on there.
    #[must_use]
    pub fn loc(&self, offset: usize) -> Loc {
        let source = self
            .spans
            .partition_point(|&(start, _)| start <= offset)
            .saturating_sub(1);
        let start = self.spans.get(source).map_or(0, |&(start, _)| start);
        let line_of = |at: usize| self.line_starts.partition_point(|&s| s <= at);
        Loc {
            source,
            line: line_of(offset)
                .saturating_sub(line_of(start))
                .saturating_add(1),
        }
    }

    /// The sources' names, which a [`Loc`]'s `source` indexes.
    #[must_use]
    pub fn names(&self) -> Vec<Option<Str>> {
        self.spans.iter().map(|(_, name)| name.clone()).collect()
    }
}

/// gawk's location prefix, `cmd. line:3: ` or `prog.awk:3: `, for `loc` in a
/// program whose sources are `names`.
#[must_use]
pub fn prefix(names: &[Option<Str>], loc: Loc) -> Str {
    let mut out = match names.get(loc.source) {
        Some(Some(name)) => name.clone(),
        _ => b"cmd. line".to_vec(),
    };
    out.extend_from_slice(format!(":{}: ", loc.line).as_bytes());
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn an_operand_program_numbers_its_lines_from_one() {
        let text = b"BEGIN {\n  x = 1\n}\n";
        let map = SourceMap::operand(text);
        assert_eq!(map.loc(0), Loc { source: 0, line: 1 });
        assert_eq!(map.loc(7), Loc { source: 0, line: 1 });
        assert_eq!(map.loc(8), Loc { source: 0, line: 2 });
        assert_eq!(map.loc(text.len()), Loc { source: 0, line: 4 });
        assert_eq!(prefix(&map.names(), map.loc(10)), b"cmd. line:2: ");
    }

    #[test]
    fn each_f_file_numbers_its_own_lines() {
        // Two files joined: `a.awk` is two lines, `b.awk` starts at offset 8.
        let text = b"x = 1\ny\nz = 2\nw\n";
        let map = SourceMap::new(
            text,
            vec![(0, Some(b"a.awk".to_vec())), (8, Some(b"b.awk".to_vec()))],
        );
        assert_eq!(map.loc(6), Loc { source: 0, line: 2 });
        assert_eq!(map.loc(8), Loc { source: 1, line: 1 });
        assert_eq!(map.loc(14), Loc { source: 1, line: 2 });
        assert_eq!(prefix(&map.names(), map.loc(14)), b"b.awk:2: ");
    }
}
