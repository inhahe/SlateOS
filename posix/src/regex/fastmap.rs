//! The fastmap: which bytes a match can begin with, as glibc's
//! `re_compile_fastmap` works it out -- for `re_search` to pass over the
//! places no match can begin, and for a program to read.
//!
//! glibc's is the union, over the four contexts a match can begin in -- after
//! an ordinary byte, after a word byte, after a newline, at the string's
//! start -- of the first nodes the automaton's initial state for that
//! context holds: a literal's or a bracket expression's bytes, and every
//! byte for a `.` or for an end of the pattern, which a pattern that can
//! match nothing reaches at once (and which sets `can_be_null` too). An
//! anchor a context does not satisfy stops what follows it there; one it
//! does, and a back-reference -- which a match can begin with only when its
//! group matched nothing -- are passed through.
//!
//! So here, over the tree: bottom up, which of the four contexts each node
//! lets through while matching nothing; then top down, which contexts can
//! reach each node before anything has been matched. A set reached at all
//! is in the fastmap.
//!
//! glibc's pattern under RE_ICASE is upper case when the fastmap is made,
//! and every byte of a set and that byte's lower case go in: a negated
//! bracket expression, `[^a]`, so puts in every lower-case letter, `a`
//! among them, though none can begin a match. The parser keeps that view
//! of each such set (`Tree::views`), so that the fastmap a program reads is
//! glibc's.

use super::parse::{Assert, ByteSet, NONE, Node, Tree};
use crate::list::{List, NoMem};

/// The four contexts, a bit each.
const ORDINARY: u8 = 1;
const WORD: u8 = 2;
const NEWLINE: u8 = 4;
const START: u8 = 8;
const ALL: u8 = ORDINARY | WORD | NEWLINE | START;

/// The contexts in which `a` lets a match go on before anything is matched:
/// what glibc's anchors ask of the byte before.
fn passes(a: Assert) -> u8 {
    match a {
        // After a newline, or at the start (glibc's begin-buffer context is
        // a newline's too).
        Assert::Bol => NEWLINE | START,
        Assert::BufStart => START,
        // No word byte before.
        Assert::WordStart => ORDINARY | NEWLINE | START,
        // A word byte before.
        Assert::WordEnd => WORD,
        // glibc makes each of these two alternatives, one for a word byte
        // before and one for none; and `$` and `\'` ask only of the byte
        // after.
        Assert::WordBoundary | Assert::NotWordBoundary | Assert::Eol | Assert::BufEnd => ALL,
    }
}

/// The fastmap of `tree`, and glibc's `can_be_null`: whether the pattern can
/// match the empty string (in some context), which makes every byte one a
/// match can begin with.
pub(super) fn fastmap(tree: &Tree) -> Result<(ByteSet, bool), NoMem> {
    let n = tree.nodes.len();
    let node = |id: usize| tree.nodes.get(id).copied().unwrap_or(Node::Empty);
    let kids = |first: u32, len: u32| {
        let a = first as usize;
        tree.kids
            .get(a..a.saturating_add(len as usize))
            .unwrap_or(&[])
    };
    // Bottom up -- children come before their parents -- the contexts each
    // node lets through matching nothing.
    let mut pass: List<u8> = List::filled(n, 0)?;
    for id in 0..n {
        let get = |k: u32, pass: &List<u8>| pass.get(k as usize).copied().unwrap_or(0);
        let p = match node(id) {
            Node::Empty | Node::BackRef(_) => ALL,
            Node::Set(_) => 0,
            Node::Assert(a) => passes(a),
            Node::Group { body, .. } => {
                if body == NONE {
                    ALL
                } else {
                    get(body, &pass)
                }
            }
            Node::Cat { first, len } => {
                kids(first, len).iter().fold(ALL, |m, &k| m & get(k, &pass))
            }
            Node::Alt { first, len } => kids(first, len).iter().fold(0, |m, &k| m | get(k, &pass)),
            Node::Rep { body, min, .. } => {
                if min == 0 {
                    ALL
                } else {
                    get(body, &pass)
                }
            }
        };
        if let Some(slot) = pass.get_mut(id) {
            *slot = p;
        }
    }
    // Top down -- parents have the larger ids -- the contexts in which each
    // node can be reached with nothing matched yet.
    let mut reach: List<u8> = List::filled(n, 0)?;
    if let Some(r) = reach.get_mut(tree.root as usize) {
        *r = ALL;
    }
    let mut map = ByteSet::EMPTY;
    let mut every = false;
    for id in (0..n).rev() {
        let r = reach.get(id).copied().unwrap_or(0);
        if r == 0 {
            continue;
        }
        let give = |k: u32, m: u8, reach: &mut List<u8>| {
            if let Some(slot) = reach.get_mut(k as usize) {
                *slot |= m;
            }
        };
        match node(id) {
            Node::Set(s) => {
                if s == tree.period {
                    every = true;
                } else {
                    let set = view(tree, s)
                        .or_else(|| tree.sets.get(s as usize).copied())
                        .unwrap_or(ByteSet::EMPTY);
                    map.union(&set);
                }
            }
            Node::Group { body, .. } => {
                if body != NONE {
                    give(body, r, &mut reach);
                }
            }
            Node::Rep { body, .. } => give(body, r, &mut reach),
            Node::Cat { first, len } => {
                let mut m = r;
                for &k in kids(first, len) {
                    if m == 0 {
                        break;
                    }
                    give(k, m, &mut reach);
                    m &= pass.get(k as usize).copied().unwrap_or(0);
                }
            }
            Node::Alt { first, len } => {
                for &k in kids(first, len) {
                    give(k, r, &mut reach);
                }
            }
            Node::Empty | Node::Assert(_) | Node::BackRef(_) => {}
        }
    }
    let can_be_null = pass.get(tree.root as usize).copied().unwrap_or(0) != 0;
    if every || can_be_null {
        map = ByteSet([u64::MAX; 4]);
    }
    Ok((map, can_be_null))
}

/// glibc's view of set `s`, if the parser kept one.
fn view(tree: &Tree, s: u32) -> Option<ByteSet> {
    let v = tree.views.as_slice();
    v.binary_search_by_key(&s, |&(id, _)| id)
        .ok()
        .and_then(|i| v.get(i))
        .map(|&(_, set)| set)
}
