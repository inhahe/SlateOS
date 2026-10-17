//! glibc 2.39's `tsearch` and `twalk` (`misc/tsearch.c`): the red-black
//! tree `hardlink` keeps its files in, transcribed so that it takes the same
//! shape -- the same splits and rotations at the same insertions -- and so
//! is walked in the same order. That order is the order `hardlink` links
//! files in, and `-v` prints it.
//!
//! The nodes are an arena, and where glibc passes a pointer to a link (the
//! tree's root, or a node's left or right pointer) this passes a [`Slot`].
//! glibc keeps a node's colour in the low bit of its left pointer, and
//! writing a link through `SETNODEPTR` keeps that bit; here the colour is a
//! field of its own, which no link write touches -- the same thing.

use std::cmp::Ordering;

/// Where `twalk` is in its visit of a node: `VISIT`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visit {
    /// Before its left subtree.
    Preorder,
    /// Between its subtrees.
    Postorder,
    /// After both.
    Endorder,
    /// A node with no children, visited once.
    Leaf,
}

/// One node: its key, its two links, and its colour.
struct Node<K> {
    key: K,
    left: Option<usize>,
    right: Option<usize>,
    red: bool,
}

/// A place a node's index is kept: the tree's root, or a node's left or
/// right link -- what glibc's `node *` points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slot {
    Root,
    Left(usize),
    Right(usize),
}

/// A tree of keys, as `tsearch` builds it.
pub struct Tree<K> {
    nodes: Vec<Node<K>>,
    root: Option<usize>,
}

impl<K> Default for Tree<K> {
    fn default() -> Self {
        Tree {
            nodes: Vec::new(),
            root: None,
        }
    }
}

/// `CMP`'s sign as glibc's comparison functions return it.
fn sign(o: Ordering) -> i32 {
    match o {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

impl<K> Tree<K> {
    /// `DEREFNODEPTR (np)`.
    fn get(&self, s: Slot) -> Option<usize> {
        match s {
            Slot::Root => self.root,
            Slot::Left(n) => self.nodes.get(n).and_then(|x| x.left),
            Slot::Right(n) => self.nodes.get(n).and_then(|x| x.right),
        }
    }

    /// `SETNODEPTR (np, p)`.
    fn set(&mut self, s: Slot, p: Option<usize>) {
        match s {
            Slot::Root => self.root = p,
            Slot::Left(n) => {
                if let Some(x) = self.nodes.get_mut(n) {
                    x.left = p;
                }
            }
            Slot::Right(n) => {
                if let Some(x) = self.nodes.get_mut(n) {
                    x.right = p;
                }
            }
        }
    }

    /// `RED (n)`, for a link that may be empty.
    fn red(&self, n: Option<usize>) -> bool {
        n.and_then(|n| self.nodes.get(n)).is_some_and(|x| x.red)
    }

    fn set_red(&mut self, n: usize, red: bool) {
        if let Some(x) = self.nodes.get_mut(n) {
            x.red = red;
        }
    }

    fn left(&self, n: usize) -> Option<usize> {
        self.nodes.get(n).and_then(|x| x.left)
    }

    fn right(&self, n: usize) -> Option<usize> {
        self.nodes.get(n).and_then(|x| x.right)
    }

    fn set_left(&mut self, n: usize, l: Option<usize>) {
        self.set(Slot::Left(n), l);
    }

    fn set_right(&mut self, n: usize, r: Option<usize>) {
        self.set(Slot::Right(n), r);
    }

    /// `maybe_split_for_insert`: split the node in `rootp` if both its
    /// children are red (or, with `mode` 1, the new node just linked there),
    /// rotating where that leaves two red links in a row. `p_r` and `gp_r`
    /// are the comparisons that led down from the grandparent and parent.
    #[allow(
        clippy::similar_names,
        reason = "glibc's own names: rp/lp, rpn/lpn, p_r/gp_r"
    )]
    fn maybe_split_for_insert(
        &mut self,
        rootp: Slot,
        parentp: Option<Slot>,
        gparentp: Option<Slot>,
        p_r: i32,
        gp_r: i32,
        mode: i32,
    ) {
        let Some(root) = self.get(rootp) else {
            return;
        };
        let rp = Slot::Right(root);
        let rpn = self.right(root);
        let lp = Slot::Left(root);
        let lpn = self.left(root);
        if !(mode == 1 || (rpn.is_some() && lpn.is_some() && self.red(rpn) && self.red(lpn))) {
            return;
        }
        // "This node becomes red, its successors black."
        self.set_red(root, true);
        if let Some(r) = rpn {
            self.set_red(r, false);
        }
        if let Some(l) = lpn {
            self.set_red(l, false);
        }
        // "If the parent of this node is also red, we have to do rotations."
        let Some(parentp) = parentp else {
            return;
        };
        if !self.red(self.get(parentp)) {
            return;
        }
        // A red parent is never the root, so there is a grandparent.
        let (Some(gparentp), Some(p)) = (gparentp, self.get(parentp)) else {
            return;
        };
        let Some(gp) = self.get(gparentp) else {
            return;
        };
        if (p_r > 0) != (gp_r > 0) {
            // "Put the child at the top of the tree, with its parent and
            // grandparent as successors."
            self.set_red(p, true);
            self.set_red(gp, true);
            self.set_red(root, false);
            if p_r < 0 {
                // "Child is left of parent."
                self.set_left(p, rpn);
                self.set(rp, Some(p));
                self.set_right(gp, lpn);
                self.set(lp, Some(gp));
            } else {
                // "Child is right of parent."
                self.set_right(p, lpn);
                self.set(lp, Some(p));
                self.set_left(gp, rpn);
                self.set(rp, Some(gp));
            }
            self.set(gparentp, Some(root));
        } else {
            self.set(gparentp, Some(p));
            // "Parent becomes the top of the tree, grandparent and child are
            // its successors."
            self.set_red(p, false);
            self.set_red(gp, true);
            if p_r < 0 {
                // "Left edges."
                let pr = self.right(p);
                self.set_left(gp, pr);
                self.set_right(p, Some(gp));
            } else {
                // "Right edges."
                let pl = self.left(p);
                self.set_right(gp, pl);
                self.set_left(p, Some(gp));
            }
        }
    }

    /// `tsearch (key, &root, compar)`: the node whose key compares equal to
    /// `key` -- and `false` -- or, when there is none, a new node holding
    /// `key`, and `true`. `cmp` is handed the new key first, as `compar`
    /// is.
    pub fn search(&mut self, key: K, mut cmp: impl FnMut(&K, &K) -> Ordering) -> (usize, bool) {
        let mut rootp = Slot::Root;
        let mut parentp: Option<Slot> = None;
        let mut gparentp: Option<Slot> = None;
        let (mut r, mut p_r, mut gp_r) = (0, 0, 0);
        // "This saves some additional tests below."
        if let Some(root) = self.root {
            self.set_red(root, false);
        }
        let mut nextp = rootp;
        while self.get(nextp).is_some() {
            let Some(root) = self.get(rootp) else {
                break;
            };
            let Some(node) = self.nodes.get(root) else {
                break;
            };
            r = sign(cmp(&key, &node.key));
            if r == 0 {
                return (root, false);
            }
            self.maybe_split_for_insert(rootp, parentp, gparentp, p_r, gp_r, 0);
            // "If that did any rotations, parentp and gparentp are now
            // garbage. That doesn't matter, because the values they contain
            // are never used again in that case."
            nextp = if r < 0 {
                Slot::Left(root)
            } else {
                Slot::Right(root)
            };
            if self.get(nextp).is_none() {
                break;
            }
            gparentp = parentp;
            parentp = Some(rootp);
            rootp = nextp;
            gp_r = p_r;
            p_r = r;
        }
        let q = self.nodes.len();
        self.nodes.push(Node {
            key,
            left: None,
            right: None,
            red: true,
        });
        self.set(nextp, Some(q));
        if nextp != rootp {
            // "There may be two red edges in a row now, which we must avoid
            // by rotating the tree."
            self.maybe_split_for_insert(nextp, Some(rootp), parentp, r, p_r, 1);
        }
        (q, true)
    }

    /// The key of node `n`.
    pub fn key(&self, n: usize) -> Option<&K> {
        self.nodes.get(n).map(|x| &x.key)
    }

    /// The key of node `n`, to be replaced -- as `*node = fil` replaces the
    /// key `tsearch`'s answer points at.
    pub fn key_mut(&mut self, n: usize) -> Option<&mut K> {
        self.nodes.get_mut(n).map(|x| &mut x.key)
    }

    /// `twalk (root, action)`: `action` on every node, as `trecurse` visits
    /// them -- a leaf once, any other node before, between and after its
    /// subtrees -- with the node's depth. `action` answering `false` stops
    /// the walk (where upstream's visitor calls `exit`).
    pub fn walk(&self, mut action: impl FnMut(&K, Visit, usize) -> bool) -> bool {
        match self.root {
            Some(root) => self.trecurse(root, &mut action, 0),
            None => true,
        }
    }

    /// `trecurse (root, action, level)`.
    fn trecurse(
        &self,
        n: usize,
        action: &mut impl FnMut(&K, Visit, usize) -> bool,
        level: usize,
    ) -> bool {
        let Some(node) = self.nodes.get(n) else {
            return true;
        };
        if node.left.is_none() && node.right.is_none() {
            return action(&node.key, Visit::Leaf, level);
        }
        if !action(&node.key, Visit::Preorder, level) {
            return false;
        }
        if let Some(l) = node.left
            && !self.trecurse(l, action, level.saturating_add(1))
        {
            return false;
        }
        if !action(&node.key, Visit::Postorder, level) {
            return false;
        }
        if let Some(r) = node.right
            && !self.trecurse(r, action, level.saturating_add(1))
        {
            return false;
        }
        action(&node.key, Visit::Endorder, level)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// The keys in the order `hardlink`'s visitor sees them: each node at
    /// its leaf or end-order visit.
    fn post_order(t: &Tree<i32>) -> Vec<i32> {
        let mut out = Vec::new();
        t.walk(|k, v, _| {
            if matches!(v, Visit::Leaf | Visit::Endorder) {
                out.push(*k);
            }
            true
        });
        out
    }

    fn built(keys: &[i32]) -> Tree<i32> {
        let mut t = Tree::default();
        for &k in keys {
            t.search(k, Ord::cmp);
        }
        t
    }

    #[test]
    fn a_key_found_is_not_inserted_again() {
        let mut t = Tree::default();
        let (a, new_a) = t.search(5, Ord::cmp);
        let (b, new_b) = t.search(5, Ord::cmp);
        assert!(new_a && !new_b);
        assert_eq!(a, b);
        *t.key_mut(a).unwrap() = 7;
        assert_eq!(t.key(a), Some(&7));
    }

    /// The shapes glibc's tree takes: each post-order measured with glibc
    /// 2.39's own `tsearch` and `twalk` on the same insertions.
    #[test]
    fn the_walk_is_glibcs() {
        assert_eq!(post_order(&built(&[1, 2, 3])), [1, 3, 2]);
        assert_eq!(
            post_order(&built(&[1, 2, 3, 4, 5, 6, 7])),
            [1, 3, 5, 7, 6, 4, 2]
        );
        assert_eq!(
            post_order(&built(&[7, 6, 5, 4, 3, 2, 1])),
            [1, 3, 2, 5, 4, 7, 6]
        );
        assert_eq!(
            post_order(&built(&[4, 2, 6, 1, 3, 5, 7])),
            [1, 3, 2, 5, 7, 6, 4]
        );
        assert_eq!(post_order(&built(&[1, 3, 2])), [1, 3, 2]);
        assert_eq!(post_order(&built(&[3, 1, 2])), [1, 3, 2]);
        assert_eq!(
            post_order(&built(&[5, 9, 2, 8, 1, 7, 3, 6, 4, 10, 15, 11, 14, 12, 13])),
            [1, 4, 3, 2, 6, 7, 9, 8, 11, 13, 12, 15, 14, 10, 5]
        );
        assert_eq!(post_order(&built(&[])), Vec::<i32>::new());
    }

    #[test]
    fn a_false_from_the_action_stops_the_walk() {
        let t = built(&[1, 2, 3, 4, 5]);
        let mut seen = 0;
        assert!(!t.walk(|_, _, _| {
            seen += 1;
            seen < 3
        }));
        assert_eq!(seen, 3);
    }
}
