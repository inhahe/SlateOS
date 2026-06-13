//! Syntactic structure tree for the SlateOS text editor.
//!
//! Provides a hierarchical view of source-code scopes derived from balanced
//! brace / paren / bracket pairs, with awareness of string and comment
//! tokens so that delimiters inside strings or comments do not open or close
//! scopes. The tree supports three operations the editor needs:
//!
//! 1. [`SyntaxTree::enclosing`] — given a buffer position, return the deepest
//!    node whose byte range contains it. Used for "expand selection to
//!    enclosing scope" (a hallmark tree-sitter UX feature: Ctrl+Shift+A).
//! 2. [`SyntaxTree::outline`] — depth-first traversal returning each multi-
//!    line node with its depth and a header label (the source text of the
//!    line containing the opening delimiter, trimmed). Used for an outline /
//!    document-symbol panel.
//! 3. [`SyntaxTree::fold_ranges`] — line-pair ranges for every multi-line
//!    node, sorted by start line. Used to render fold markers in the gutter
//!    and to drive collapse / expand operations.
//!
//! ## Why not upstream tree-sitter?
//!
//! Upstream tree-sitter is a C library with Rust bindings; pulling it in
//! would require sysroot stubs, a C cross-compile, and per-language grammar
//! crates. For the editor's needs (structural selection, outline, folding)
//! a brace-scope tree is sufficient, native to Rust, and `no_std`-friendly.
//! When upstream tree-sitter becomes available in the SlateOS toolchain the
//! editor can swap this module for a tree-sitter-backed one behind the
//! same API; the rest of the editor will not need to change.
//!
//! ## Language awareness
//!
//! The parser knows, per [`Language`], which characters open block scopes,
//! which sequences start line and block comments, and which characters
//! delimit strings. Languages without a brace-based syntax (Python, YAML,
//! Markdown, plain text) still parse correctly but produce a tree whose
//! interesting structure lives mostly under parens and brackets — which is
//! still useful for argument-list selection and outline of list literals.
//!
//! ## Robustness
//!
//! The parser never panics on malformed input. Unclosed scopes (missing
//! `}`, unterminated string, unterminated block comment) are closed at
//! end-of-buffer with `end = (lines.len(), 0)` and the resulting tree is
//! still traversable. This matches the editor's expectation that it must
//! work on files mid-edit.

use crate::Language;

// ============================================================================
// Public types
// ============================================================================

/// A byte position in the document: 0-based line index and byte column.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

impl Pos {
    pub const fn new(line: usize, col: usize) -> Self {
        Self { line, col }
    }
}

/// Kind of syntactic node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    /// Synthetic root spanning the entire buffer.
    Root,
    /// Brace-delimited block: `{ ... }`.
    Block,
    /// Parenthesised group: `( ... )`.
    Paren,
    /// Bracket group: `[ ... ]`.
    Bracket,
}

/// A node in the syntax tree.
#[derive(Clone, Debug)]
pub struct Node {
    pub kind: NodeKind,
    /// Position of the opening delimiter (or `(0,0)` for the root).
    pub start: Pos,
    /// Position one past the closing delimiter (or end-of-buffer for the root
    /// and for unclosed nodes).
    pub end: Pos,
    /// Index of the parent node, or `None` for the root.
    pub parent: Option<usize>,
    /// Indices of direct children, in source order.
    pub children: Vec<usize>,
    /// Header line text (the line containing [`Node::start`]) trimmed and
    /// truncated, used as a label in outline views. Empty for the root.
    pub header: String,
}

impl Node {
    /// Whether the node spans more than one line (and is therefore foldable).
    pub fn is_multiline(&self) -> bool {
        self.end.line > self.start.line
    }

    /// Whether the node's byte range contains the given position. The range is
    /// inclusive on the start and exclusive on the end, matching tree-sitter.
    pub fn contains(&self, pos: Pos) -> bool {
        pos >= self.start && pos < self.end
    }
}

/// A parsed syntactic structure tree.
pub struct SyntaxTree {
    /// All nodes. Index 0 is always the root.
    pub nodes: Vec<Node>,
    /// Source language used to drive tokenisation.
    pub language: Language,
}

impl SyntaxTree {
    /// Build a syntax tree from the document lines and language.
    pub fn build(lines: &[String], language: Language) -> Self {
        let dialect = Dialect::for_language(language);
        let mut parser = Parser::new(lines, dialect);
        parser.run();
        Self {
            nodes: parser.nodes,
            language,
        }
    }

    /// Returns the index of the deepest node whose range contains `pos`.
    /// Falls back to the root (index 0) when no inner node matches.
    pub fn enclosing(&self, pos: Pos) -> usize {
        self.enclosing_from(0, pos)
    }

    fn enclosing_from(&self, idx: usize, pos: Pos) -> usize {
        let node = &self.nodes[idx];
        for &child in &node.children {
            if self.nodes[child].contains(pos) {
                return self.enclosing_from(child, pos);
            }
        }
        idx
    }

    /// Returns the smallest node whose range covers the closed interval
    /// `[start, end]` (i.e. contains both `start` and the position one byte
    /// before `end`). Used by selection expansion.
    pub fn enclosing_range(&self, start: Pos, end: Pos) -> usize {
        if end <= start {
            return self.enclosing(start);
        }
        self.enclosing_range_from(0, start, end)
    }

    fn enclosing_range_from(&self, idx: usize, start: Pos, end: Pos) -> usize {
        for &child in &self.nodes[idx].children {
            let c = &self.nodes[child];
            if c.start <= start && end <= c.end {
                return self.enclosing_range_from(child, start, end);
            }
        }
        idx
    }

    /// Depth-first outline of multi-line nodes, returning `(depth, header)`.
    /// Depth 0 is the root's direct children.
    pub fn outline(&self) -> Vec<(usize, String)> {
        let mut out = Vec::new();
        self.outline_walk(0, 0, &mut out);
        out
    }

    fn outline_walk(&self, idx: usize, depth: usize, out: &mut Vec<(usize, String)>) {
        let node = &self.nodes[idx];
        // Skip the synthetic root in the listing.
        if idx != 0 && node.is_multiline() {
            out.push((depth.saturating_sub(1), node.header.clone()));
        }
        for &child in &node.children {
            let child_depth = if idx == 0 { 1 } else { depth + 1 };
            self.outline_walk(child, child_depth, out);
        }
    }

    /// Returns `(start_line, end_line)` for every multi-line node, sorted by
    /// start line. `end_line` is the line containing the closing delimiter.
    pub fn fold_ranges(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for (i, n) in self.nodes.iter().enumerate() {
            if i == 0 {
                continue;
            }
            if n.is_multiline() {
                out.push((n.start.line, n.end.line));
            }
        }
        out.sort();
        out
    }
}

// ============================================================================
// Language dialect
// ============================================================================

#[derive(Clone, Copy)]
struct Dialect {
    /// Line-comment prefix (e.g. `"//"`, `"#"`), or empty if none.
    line_comment: &'static str,
    /// Block-comment open/close pair, or empty strings if none.
    block_comment: (&'static str, &'static str),
    /// String delimiters; each entry is `(open, close, multiline)`.
    /// `multiline = false` means the string terminates at newline (the parser
    /// treats unterminated single-line strings as ending at newline anyway).
    strings: &'static [(char, char, bool)],
    /// Whether `{`, `(`, `[` open scopes. Always true for languages we care
    /// about; kept as a field for future plain-text dialects.
    braces: bool,
}

impl Dialect {
    fn for_language(lang: Language) -> Self {
        match lang {
            Language::Rust => Self {
                line_comment: "//",
                block_comment: ("/*", "*/"),
                strings: &[('"', '"', false), ('\'', '\'', false)],
                braces: true,
            },
            Language::C => Self {
                line_comment: "//",
                block_comment: ("/*", "*/"),
                strings: &[('"', '"', false), ('\'', '\'', false)],
                braces: true,
            },
            Language::JavaScript => Self {
                line_comment: "//",
                block_comment: ("/*", "*/"),
                strings: &[('"', '"', false), ('\'', '\'', false), ('`', '`', true)],
                braces: true,
            },
            Language::Css => Self {
                line_comment: "",
                block_comment: ("/*", "*/"),
                strings: &[('"', '"', false), ('\'', '\'', false)],
                braces: true,
            },
            Language::Python => Self {
                line_comment: "#",
                block_comment: ("", ""),
                strings: &[('"', '"', false), ('\'', '\'', false)],
                braces: true,
            },
            Language::Shell => Self {
                line_comment: "#",
                block_comment: ("", ""),
                strings: &[('"', '"', false), ('\'', '\'', false)],
                braces: true,
            },
            Language::Toml => Self {
                line_comment: "#",
                block_comment: ("", ""),
                strings: &[('"', '"', false), ('\'', '\'', false)],
                braces: true,
            },
            Language::Yaml => Self {
                line_comment: "#",
                block_comment: ("", ""),
                strings: &[('"', '"', false), ('\'', '\'', false)],
                braces: true,
            },
            Language::Json => Self {
                line_comment: "",
                block_comment: ("", ""),
                strings: &[('"', '"', false)],
                braces: true,
            },
            Language::Html => Self {
                line_comment: "",
                block_comment: ("<!--", "-->"),
                strings: &[('"', '"', false), ('\'', '\'', false)],
                braces: false,
            },
            Language::Markdown | Language::Plain => Self {
                line_comment: "",
                block_comment: ("", ""),
                strings: &[],
                braces: false,
            },
        }
    }
}

// ============================================================================
// Parser
// ============================================================================

struct Parser<'a> {
    lines: &'a [String],
    dialect: Dialect,
    nodes: Vec<Node>,
    /// Stack of currently-open node indices (root is always nodes[0] and is
    /// not pushed on the stack — the stack holds child scopes).
    stack: Vec<usize>,
    in_block_comment: bool,
}

impl<'a> Parser<'a> {
    fn new(lines: &'a [String], dialect: Dialect) -> Self {
        let root = Node {
            kind: NodeKind::Root,
            start: Pos::new(0, 0),
            end: Pos::new(lines.len(), 0),
            parent: None,
            children: Vec::new(),
            header: String::new(),
        };
        Self {
            lines,
            dialect,
            nodes: vec![root],
            stack: Vec::new(),
            in_block_comment: false,
        }
    }

    fn run(&mut self) {
        for (line_idx, line) in self.lines.iter().enumerate() {
            self.scan_line(line_idx, line);
        }
        // Close any unclosed scopes at end-of-buffer.
        let eof = Pos::new(self.lines.len(), 0);
        while let Some(idx) = self.stack.pop() {
            self.nodes[idx].end = eof;
        }
        // Set root's end to end-of-buffer for consistency.
        self.nodes[0].end = eof;
    }

    fn current_parent(&self) -> usize {
        self.stack.last().copied().unwrap_or(0)
    }

    fn scan_line(&mut self, line_idx: usize, line: &str) {
        let bytes = line.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            if self.in_block_comment {
                let (close_open, close_close) = self.dialect.block_comment;
                let _ = close_open;
                if !close_close.is_empty() && bytes_starts_with(bytes, i, close_close) {
                    i += close_close.len();
                    self.in_block_comment = false;
                    continue;
                }
                i += utf8_step(bytes, i);
                continue;
            }

            // Line comment: skip rest of line.
            if !self.dialect.line_comment.is_empty()
                && bytes_starts_with(bytes, i, self.dialect.line_comment)
            {
                break;
            }

            // Block comment start.
            let (bo, _bc) = self.dialect.block_comment;
            if !bo.is_empty() && bytes_starts_with(bytes, i, bo) {
                i += bo.len();
                self.in_block_comment = true;
                continue;
            }

            // String literal: scan until terminating quote on same line. We
            // intentionally drop multi-line strings on the floor — they are
            // rare in the languages we support without triple-quote handling.
            let b = bytes[i];
            if let Some(&(_, close, _ml)) = self
                .dialect
                .strings
                .iter()
                .find(|(open, _, _)| (*open as u32) == b as u32 && b < 0x80)
            {
                i += 1;
                while i < bytes.len() {
                    let ch = bytes[i];
                    if ch == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                        continue;
                    }
                    if (close as u32) < 0x80 && ch == close as u8 {
                        i += 1;
                        break;
                    }
                    i += utf8_step(bytes, i);
                }
                continue;
            }

            // Scope open / close.
            if self.dialect.braces {
                match b {
                    b'{' | b'(' | b'[' => {
                        let kind = match b {
                            b'{' => NodeKind::Block,
                            b'(' => NodeKind::Paren,
                            _ => NodeKind::Bracket,
                        };
                        let header = make_header(line);
                        let parent = self.current_parent();
                        let new_idx = self.nodes.len();
                        self.nodes.push(Node {
                            kind,
                            start: Pos::new(line_idx, i),
                            end: Pos::new(self.lines.len(), 0),
                            parent: Some(parent),
                            children: Vec::new(),
                            header,
                        });
                        self.nodes[parent].children.push(new_idx);
                        self.stack.push(new_idx);
                        i += 1;
                        continue;
                    }
                    b'}' | b')' | b']' => {
                        let want = match b {
                            b'}' => NodeKind::Block,
                            b')' => NodeKind::Paren,
                            _ => NodeKind::Bracket,
                        };
                        if let Some(&top) = self.stack.last() {
                            if self.nodes[top].kind == want {
                                self.stack.pop();
                                // Close at position one past the delimiter.
                                self.nodes[top].end = Pos::new(line_idx, i + 1);
                            } else {
                                // Mismatched close: tolerate by skipping; the
                                // open scope stays open until EOF.
                            }
                        }
                        i += 1;
                        continue;
                    }
                    _ => {}
                }
            }

            i += utf8_step(bytes, i);
        }
    }
}

fn bytes_starts_with(haystack: &[u8], at: usize, needle: &str) -> bool {
    let n = needle.as_bytes();
    if at + n.len() > haystack.len() {
        return false;
    }
    &haystack[at..at + n.len()] == n
}

/// Step forward by the UTF-8 byte length of the codepoint at `at`, or by 1
/// for invalid input. We always advance at least one byte to guarantee
/// progress.
fn utf8_step(bytes: &[u8], at: usize) -> usize {
    let b = bytes[at];
    if b < 0x80 {
        1
    } else if b < 0xC0 {
        // Continuation byte in the middle of a malformed sequence — advance
        // one byte to make progress without panicking.
        1
    } else if b < 0xE0 {
        2
    } else if b < 0xF0 {
        3
    } else {
        4
    }
}

fn make_header(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.len() <= 80 {
        trimmed.to_string()
    } else {
        let mut s = String::with_capacity(83);
        // Take 77 chars (not bytes) to avoid slicing inside a codepoint.
        let cutoff: String = trimmed.chars().take(77).collect();
        s.push_str(&cutoff);
        s.push_str("...");
        s
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn build(lang: Language, src: &str) -> SyntaxTree {
        let lines: Vec<String> = src.lines().map(str::to_string).collect();
        SyntaxTree::build(&lines, lang)
    }

    #[test]
    fn empty_input_has_only_root() {
        let t = build(Language::Rust, "");
        assert_eq!(t.nodes.len(), 1);
        assert_eq!(t.nodes[0].kind, NodeKind::Root);
        assert!(t.nodes[0].children.is_empty());
    }

    #[test]
    fn single_block_is_one_child_of_root() {
        let t = build(Language::Rust, "fn f() {}\n");
        // Paren for () and Block for {} both children of root.
        let root_kids: Vec<NodeKind> = t.nodes[0]
            .children
            .iter()
            .map(|&i| t.nodes[i].kind)
            .collect();
        assert_eq!(root_kids, vec![NodeKind::Paren, NodeKind::Block]);
    }

    #[test]
    fn nested_blocks_form_a_tree() {
        let src = "fn outer() {\n    fn inner() {\n        1;\n    }\n}\n";
        let t = build(Language::Rust, src);
        // outer's Block (last top-level Block in root children)
        let outer_block = *t.nodes[0]
            .children
            .iter()
            .rev()
            .find(|&&i| t.nodes[i].kind == NodeKind::Block)
            .expect("outer block");
        assert!(t.nodes[outer_block].is_multiline());
        // outer block should contain inner Paren + inner Block.
        let inner_block = *t.nodes[outer_block]
            .children
            .iter()
            .rev()
            .find(|&&i| t.nodes[i].kind == NodeKind::Block)
            .expect("inner block");
        assert_eq!(t.nodes[inner_block].parent, Some(outer_block));
        assert!(t.nodes[inner_block].is_multiline());
        // enclosing of a position inside inner returns inner_block.
        let inside_inner = Pos::new(2, 10);
        assert_eq!(t.enclosing(inside_inner), inner_block);
    }

    #[test]
    fn brace_inside_string_is_ignored() {
        let src = "let s = \"{not a block}\";\n";
        let t = build(Language::Rust, src);
        // Only the outer Paren-less code: no Block nodes.
        assert!(
            t.nodes
                .iter()
                .skip(1)
                .all(|n| n.kind != NodeKind::Block),
            "found a stray block: {:?}",
            t.nodes
        );
    }

    #[test]
    fn brace_inside_line_comment_is_ignored() {
        let src = "// { not a block\nlet x = 1;\n";
        let t = build(Language::Rust, src);
        assert!(t.nodes.iter().skip(1).all(|n| n.kind != NodeKind::Block));
    }

    #[test]
    fn brace_inside_block_comment_is_ignored() {
        let src = "/* { still not\n   a block } */\nlet x = 1;\n";
        let t = build(Language::Rust, src);
        assert!(t.nodes.iter().skip(1).all(|n| n.kind != NodeKind::Block));
    }

    #[test]
    fn unclosed_block_extends_to_eof() {
        let src = "fn f() {\n    let x = 1;\n";
        let t = build(Language::Rust, src);
        let block = t
            .nodes
            .iter()
            .skip(1)
            .find(|n| n.kind == NodeKind::Block)
            .expect("block");
        assert_eq!(block.end.line, 2); // lines.len() (lines has 2 entries)
    }

    #[test]
    fn mismatched_close_does_not_panic() {
        let src = "fn f() {\n    )\n}\n";
        // Stray ')': just don't crash.
        let _t = build(Language::Rust, src);
    }

    #[test]
    fn outline_lists_multiline_nodes_with_depth() {
        let src = "fn outer() {\n    fn inner() {\n        1;\n    }\n}\n";
        let t = build(Language::Rust, src);
        let outline = t.outline();
        // Expect at least the two Block headers (the Parens are single-line
        // so not included).
        let multiline_blocks: Vec<_> = outline.iter().collect();
        assert!(
            multiline_blocks.len() >= 2,
            "outline = {:?}",
            outline
        );
        // The inner one should have greater depth than the outer one.
        let max_depth = outline.iter().map(|(d, _)| *d).max().unwrap();
        let min_depth = outline.iter().map(|(d, _)| *d).min().unwrap();
        assert!(max_depth > min_depth);
    }

    #[test]
    fn fold_ranges_match_multiline_blocks() {
        let src = "fn f() {\n    1\n}\n";
        let t = build(Language::Rust, src);
        let folds = t.fold_ranges();
        assert!(folds.contains(&(0, 2)), "folds = {:?}", folds);
    }

    #[test]
    fn enclosing_range_grows_outward() {
        let src = "fn f() {\n    {\n        let x = 1;\n    }\n}\n";
        let t = build(Language::Rust, src);
        // A position inside the inner-most block should enclose to the inner
        // block; a range covering both lines 1..=3 should enclose to the
        // outer block.
        let inner = t.enclosing(Pos::new(2, 12));
        assert_eq!(t.nodes[inner].kind, NodeKind::Block);
        let outer = t.enclosing_range(Pos::new(1, 0), Pos::new(3, 5));
        assert_eq!(t.nodes[outer].kind, NodeKind::Block);
        // outer must be a (transitive) parent of inner.
        let mut cur = Some(inner);
        let mut found = false;
        while let Some(idx) = cur {
            if idx == outer {
                found = true;
                break;
            }
            cur = t.nodes[idx].parent;
        }
        assert!(found, "outer is not an ancestor of inner");
    }

    #[test]
    fn python_uses_hash_comments_and_no_block_braces() {
        let src = "def f():\n    x = [1, 2, 3]  # bracket here\n    return x\n";
        let t = build(Language::Python, src);
        // Bracket should be present, no Block (Python uses indentation).
        assert!(t.nodes.iter().skip(1).any(|n| n.kind == NodeKind::Bracket));
        assert!(t.nodes.iter().skip(1).all(|n| n.kind != NodeKind::Block));
    }

    #[test]
    fn json_treats_braces_as_blocks() {
        let src = "{\n  \"a\": 1\n}\n";
        let t = build(Language::Json, src);
        let blocks: Vec<_> = t
            .nodes
            .iter()
            .skip(1)
            .filter(|n| n.kind == NodeKind::Block)
            .collect();
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].is_multiline());
    }

    #[test]
    fn markdown_has_only_root() {
        let src = "# Heading\n\nSome text with { not a block } in it.\n";
        let t = build(Language::Markdown, src);
        // Markdown dialect disables braces and has no comment/string syntax;
        // the result should be just the root.
        assert_eq!(t.nodes.len(), 1);
    }

    #[test]
    fn utf8_in_source_does_not_panic() {
        let src = "// 日本語コメント {\nfn f() { let s = \"héllo\"; }\n";
        let t = build(Language::Rust, src);
        // We should still find a top-level block.
        assert!(t.nodes.iter().skip(1).any(|n| n.kind == NodeKind::Block));
    }

    #[test]
    fn header_truncates_long_lines() {
        let long = format!("fn very_long_name_{} () {{", "x".repeat(200));
        let src = format!("{long}\n}}\n");
        let t = build(Language::Rust, &src);
        let block = t
            .nodes
            .iter()
            .skip(1)
            .find(|n| n.kind == NodeKind::Block)
            .expect("block");
        assert!(block.header.ends_with("..."));
        assert!(block.header.chars().count() <= 80);
    }

    #[test]
    fn enclosing_falls_back_to_root_for_top_level_position() {
        let src = "fn a() {}\nfn b() {}\n";
        let t = build(Language::Rust, src);
        // Position between the two functions: the newline at end of line 0.
        // No node should contain it except the root.
        let between = Pos::new(0, 9); // past `fn a() {}`
        assert_eq!(t.enclosing(between), 0);
    }
}
